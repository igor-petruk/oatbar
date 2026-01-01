#![allow(dead_code)]
use anyhow::Context;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex, RwLock},
};
use xcb::{x, xinput, Xid};

use crate::{
    bar::{self, BarUpdates, BlockUpdates},
    config, drawing,
    engine::Engine,
    notify, parse, popup_visibility, state, thread, wmready, xutils,
};
use tracing::*;

pub struct Window {
    pub conn: Arc<xcb::Connection>,
    pub id: x::Window,
    pub name: String,
    pub width: u16,
    pub height: u16,
    back_buffer_context: drawing::Context,
    back_buffer_surface: cairo::XCBSurface,
    back_buffer_pixmap: x::Pixmap,
    shape_buffer_context: drawing::Context,
    shape_buffer_surface: cairo::XCBSurface,
    shape_buffer_pixmap: x::Pixmap,
    swap_gc: x::Gcontext,
    bar: bar::Bar,
    // bar_index: usize,
    bar_config: config::Bar<parse::Placeholder>,
    state: Arc<RwLock<state::State>>,
    screen: x::ScreenBuf,
    state_update_tx: crossbeam_channel::Sender<state::Update>,
    popup_manager_mutex: Arc<Mutex<popup_visibility::PopupManager>>,
    update_tx: crossbeam_channel::Sender<state::Update>,
    visible: bool,
}

impl Window {
    #[allow(clippy::too_many_arguments)]
    pub fn create_and_show(
        name: String,
        // bar_index: usize,
        config: &config::Config<parse::Placeholder>,
        bar_config: config::Bar<parse::Placeholder>,
        conn: Arc<xcb::Connection>,
        state: Arc<RwLock<state::State>>,
        state_update_tx: crossbeam_channel::Sender<state::Update>,
        wm_info: &wmready::WMInfo,
        notifier: notify::Notifier,
        popup_manager_mutex: Arc<Mutex<popup_visibility::PopupManager>>,
        update_tx: crossbeam_channel::Sender<state::Update>,
    ) -> anyhow::Result<Self> {
        info!("Loading bar {:?}", name);
        let screen = {
            let setup = conn.get_setup();
            setup.roots().next().unwrap()
        }
        .to_owned();

        let mut vis32 = match_visual(&screen, 32).unwrap();

        let margin = &bar_config.margin;

        let height = bar_config.height;

        let monitor = crate::xrandr::get_monitor(&conn, screen.root(), &bar_config.monitor)?
            .unwrap_or_else(|| crate::xrandr::Monitor {
                name: "default".into(),
                primary: true,
                x: 0,
                y: 0,
                width: screen.width_in_pixels(),
                height: screen.height_in_pixels(),
            });

        let window_width = monitor.width;
        let window_height = height + margin.top + margin.bottom;

        let cid = conn.generate_id();
        xutils::send(
            &conn,
            &x::CreateColormap {
                mid: cid,
                window: screen.root(),
                visual: vis32.visual_id(),
                alloc: x::ColormapAlloc::None,
            },
        )?;

        let id: x::Window = conn.generate_id();
        let y = match bar_config.position {
            config::BarPosition::Top => 0,
            config::BarPosition::Center => (monitor.height as i16 - window_height as i16) / 2,
            config::BarPosition::Bottom => monitor.height as i16 - window_height as i16,
        };
        let x = monitor.x as i16;

        info!(
            "Placing the bar at x: {}, y: {}, width: {}, height: {}",
            x, y, window_width, window_height
        );
        conn.send_request(&x::CreateWindow {
            depth: 32,
            wid: id,
            parent: screen.root(),
            x,
            y,
            width: window_width,
            height: window_height,
            border_width: 0,
            class: x::WindowClass::InputOutput,
            visual: vis32.visual_id(),
            value_list: &[
                x::Cw::BorderPixel(screen.white_pixel()),
                x::Cw::OverrideRedirect(
                    bar_config.popup || bar_config.position == config::BarPosition::Center,
                ),
                //x::Cw::OverrideRedirect(true),
                x::Cw::EventMask(
                    x::EventMask::EXPOSURE
                        | x::EventMask::KEY_PRESS
                        | x::EventMask::BUTTON_PRESS
                        | x::EventMask::LEAVE_WINDOW
                        | x::EventMask::POINTER_MOTION,
                ),
                x::Cw::Colormap(cid),
            ],
        });

        if bar_config.popup && bar_config.popup_at_edge {
            let raw_motion_mask_buf =
                xinput::EventMaskBuf::new(xinput::Device::All, &[xinput::XiEventMask::RAW_MOTION]);

            xutils::send(
                &conn,
                &xinput::XiSelectEvents {
                    window: screen.root(),
                    masks: &[raw_motion_mask_buf],
                },
            )?;
        }

        let app_name = "oatbar".as_bytes();
        xutils::replace_property_atom(&conn, id, x::ATOM_WM_NAME, x::ATOM_STRING, app_name)?;
        xutils::replace_property_atom(&conn, id, x::ATOM_WM_CLASS, x::ATOM_STRING, app_name)?;
        if let Err(e) = xutils::replace_atom_property(
            &conn,
            id,
            "_NET_WM_WINDOW_TYPE",
            &["_NET_WM_WINDOW_TYPE_DOCK"],
        ) {
            warn!("Unable to set window property: {:?}", e);
        }
        xutils::replace_atom_property(
            &conn,
            id,
            "_NET_WM_STATE",
            &["_NET_WM_STATE_STICKY", "_NET_WM_STATE_ABOVE"],
        )?;

        if !bar_config.popup && bar_config.position != config::BarPosition::Center {
            let top = bar_config.position == config::BarPosition::Top;
            let sp_result = xutils::replace_property(
                &conn,
                id,
                "_NET_WM_STRUT_PARTIAL",
                x::ATOM_CARDINAL,
                &[
                    0_u32,
                    0,
                    if top { window_height.into() } else { 0 },
                    if top { 0 } else { window_height.into() },
                    0,
                    0,
                    0,
                    0,
                    0,
                    if top { window_width.into() } else { 0 },
                    0,
                    if top { 0 } else { window_width.into() },
                ],
            )
            .context("_NET_WM_STRUT_PARTIAL");
            if let Err(e) = sp_result {
                debug!("Unable to set _NET_WM_STRUT_PARTIAL: {:?}", e);
            }
            let s_result = xutils::replace_property(
                &conn,
                id,
                "_NET_WM_STRUT",
                x::ATOM_CARDINAL,
                &[
                    0_u32,
                    0,
                    if top { window_height.into() } else { 0 },
                    if top { 0 } else { window_height.into() },
                ],
            )
            .context("_NET_WM_STRUT");
            if let Err(e) = s_result {
                debug!("Unable to set _NET_WM_STRUT: {:?}", e);
            }
        }
        let back_buffer_pixmap: x::Pixmap = conn.generate_id();
        xutils::send(
            &conn,
            &x::CreatePixmap {
                depth: 32,
                pid: back_buffer_pixmap,
                drawable: xcb::x::Drawable::Window(id),
                width: window_width,
                height: window_height,
            },
        )?;

        let font_cache = Arc::new(Mutex::new(drawing::FontCache::new()));
        #[cfg(feature = "image")]
        let image_loader = drawing::ImageLoader::new();

        let back_buffer_surface = make_pixmap_surface(
            &conn,
            &back_buffer_pixmap,
            &mut vis32,
            window_width,
            window_height,
        )?;
        let context = cairo::Context::new(back_buffer_surface.clone())?;
        let back_buffer_context = drawing::Context::new(
            context,
            font_cache.clone(),
            #[cfg(feature = "image")]
            image_loader.clone(),
            drawing::Mode::Full,
        )?;

        let shape_buffer_pixmap: x::Pixmap = conn.generate_id();
        xutils::send(
            &conn,
            &x::CreatePixmap {
                depth: 1,
                pid: shape_buffer_pixmap,
                drawable: xcb::x::Drawable::Window(id),
                width: window_width,
                height: window_height,
            },
        )?;
        let shape_buffer_surface = make_pixmap_surface_for_bitmap(
            &conn,
            &shape_buffer_pixmap,
            &screen,
            window_width,
            window_height,
        )?;
        let context = cairo::Context::new(shape_buffer_surface.clone())?;
        let shape_buffer_context = drawing::Context::new(
            context,
            font_cache,
            #[cfg(feature = "image")]
            image_loader,
            drawing::Mode::Shape,
        )?;

        let swap_gc: x::Gcontext = conn.generate_id();
        xutils::send(
            &conn,
            &x::CreateGc {
                cid: swap_gc,
                drawable: x::Drawable::Window(id),
                value_list: &[x::Gc::GraphicsExposures(false)],
            },
        )?;
        conn.flush()?;

        let mut config_value_list =
            vec![x::ConfigWindow::X(x.into()), x::ConfigWindow::Y(y.into())];
        xutils::send(
            &conn,
            &x::ConfigureWindow {
                window: id,
                value_list: &config_value_list,
            },
        )?;
        conn.flush()?;

        let initially_visible = !bar_config.popup;
        if initially_visible {
            xutils::send(&conn, &x::MapWindow { window: id })?;
        }
        if !bar_config.popup && bar_config.position != config::BarPosition::Center {
            config_value_list.extend_from_slice(&[
                x::ConfigWindow::Sibling(wm_info.support),
                x::ConfigWindow::StackMode(x::StackMode::Below),
            ]);
        }

        if let Err(e) = xutils::send(
            &conn,
            &x::ConfigureWindow {
                window: id,
                value_list: &config_value_list,
            },
        ) {
            tracing::error!("Failed to restack: {:?}", e);
        }
        conn.flush()?;

        let bar = bar::Bar::new(config, bar_config.clone(), notifier.clone())?;

        Ok(Self {
            conn: conn.clone(),
            id,
            name: name.clone(),
            width: window_width,
            height: window_height,
            back_buffer_context,
            back_buffer_surface,
            back_buffer_pixmap,
            shape_buffer_context,
            shape_buffer_surface,
            shape_buffer_pixmap,
            swap_gc,
            // bar_index,
            bar,
            state,
            state_update_tx,
            screen,
            bar_config,
            popup_manager_mutex,
            update_tx,
            visible: initially_visible,
        })
    }

    fn render_bar(&mut self, redraw: &bar::RedrawScope) -> anyhow::Result<()> {
        self.bar.render(&self.back_buffer_context, redraw)?;
        self.bar.render(&self.shape_buffer_context, redraw)?;

        self.swap_buffers()?;
        self.apply_shape()?;
        self.conn.flush()?;
        Ok(())
    }

    pub fn render(
        &mut self,
        loop_handle: &mut Option<calloop::LoopHandle<'static, XOrgEngine>>,
    ) -> anyhow::Result<()> {
        let state = self.state.clone();
        let state = state.read().unwrap();
        let pointer_position = state.pointer_position.get(&self.name).copied();
        let mut error = state.build_error_msg();

        let updates =
            match self
                .bar
                .update(&mut self.back_buffer_context, &state.vars, pointer_position)
            {
                Ok(updates) => updates,
                Err(e) => {
                    error = Some(state::ErrorMessage {
                        source: "bar_update".into(),
                        message: format!("Error: {:?}", e),
                    });
                    BarUpdates {
                        block_updates: BlockUpdates {
                            redraw: bar::RedrawScope::All,
                            popup: Default::default(),
                        },
                        visible_from_vars: None,
                    }
                }
            };

        self.bar
            .set_error(&mut self.back_buffer_context, error.clone());

        for popup in updates.block_updates.popup.values() {
            for block in popup {
                popup_visibility::PopupManager::trigger_popup(
                    &self.popup_manager_mutex,
                    loop_handle,
                    self.update_tx.clone(),
                    block.clone(),
                );
            }
        }
        if self.bar_config.popup {
            if let Some(visible) = updates.visible_from_vars {
                if visible != self.visible {
                    self.visible = visible;
                    if visible {
                        xutils::send(&self.conn, &x::MapWindow { window: self.id })?;
                    } else {
                        xutils::send(&self.conn, &x::UnmapWindow { window: self.id })?;
                    }
                }
            }
        }
        let mut redraw = updates.block_updates.redraw;
        let layout_changed = self.bar.layout_groups(self.width as f64);
        if layout_changed {
            redraw = bar::RedrawScope::All;
        }

        self.render_bar(&redraw)?;
        Ok(())
    }

    pub fn handle_button_press(
        &mut self,
        x: i16,
        y: i16,
        button: bar::Button,
    ) -> anyhow::Result<()> {
        self.bar.handle_button_press(x, y, button)
    }

    pub fn handle_raw_motion(&mut self, x: i16, y: i16) -> anyhow::Result<()> {
        self.handle_motion_popup(x, y)?;
        Ok(())
    }

    pub fn handle_motion(&self, x: i16, y: i16) -> anyhow::Result<()> {
        self.state_update_tx
            .send(state::Update::MotionUpdate(state::MotionUpdate {
                window_name: self.name.clone(),
                position: Some((x, y)),
            }))?;
        Ok(())
    }

    pub fn handle_motion_leave(&self) -> anyhow::Result<()> {
        self.state_update_tx
            .send(state::Update::MotionUpdate(state::MotionUpdate {
                window_name: self.name.clone(),
                position: None,
            }))?;
        Ok(())
    }

    pub fn handle_motion_popup(&mut self, _x: i16, y: i16) -> anyhow::Result<()> {
        if !self.bar_config.popup_at_edge {
            return Ok(());
        }
        let edge_size: i16 = 3;
        let screen_height: i16 = self.screen.height_in_pixels() as i16;
        let over_window = match self.bar_config.position {
            config::BarPosition::Top => y < self.height as i16,
            config::BarPosition::Bottom => y > screen_height - self.height as i16,
            config::BarPosition::Center => false,
        };
        let over_edge = match self.bar_config.position {
            config::BarPosition::Top => y < edge_size,
            config::BarPosition::Bottom => y > screen_height - edge_size,
            config::BarPosition::Center => false,
        };

        if over_window || over_edge {
            if !self.visible {
                self.visible = true;
                xutils::send(&self.conn, &x::MapWindow { window: self.id })?;
            }
        } else {
            if self.visible {
                self.visible = false;
                xutils::send(&self.conn, &x::UnmapWindow { window: self.id })?;
            }
        }

        Ok(())
    }

    fn apply_shape(&self) -> anyhow::Result<()> {
        self.shape_buffer_surface.flush();
        xutils::send(
            &self.conn,
            &xcb::shape::Mask {
                operation: xcb::shape::So::Set,
                destination_kind: xcb::shape::Sk::Bounding,
                destination_window: self.id,
                x_offset: 0,
                y_offset: 0,
                source_bitmap: self.shape_buffer_pixmap,
            },
        )?;
        Ok(())
    }

    fn swap_buffers(&self) -> anyhow::Result<()> {
        self.back_buffer_surface.flush();
        xutils::send(
            &self.conn,
            &xcb::x::ClearArea {
                window: self.id,
                x: 0,
                y: 0,
                height: self.height,
                width: self.width,
                exposures: false,
            },
        )?;
        self.conn.flush()?;
        xutils::send(
            &self.conn,
            &xcb::x::CopyArea {
                src_drawable: xcb::x::Drawable::Pixmap(self.back_buffer_pixmap),
                dst_drawable: xcb::x::Drawable::Window(self.id),
                src_x: 0,
                src_y: 0,
                dst_x: 0,
                dst_y: 0,
                width: self.width,
                height: self.height,
                gc: self.swap_gc,
            },
        )?;
        Ok(())
    }
}

fn match_visual(screen: &xcb::x::Screen, depth: u8) -> Option<xcb::x::Visualtype> {
    let d_iter: xcb::x::DepthIterator = screen.allowed_depths();
    for allowed_depth in d_iter {
        if allowed_depth.depth() != depth {
            continue;
        }
        for vis in allowed_depth.visuals() {
            if vis.class() == xcb::x::VisualClass::TrueColor {
                return Some(*vis);
            }
        }
    }
    None
}

fn make_pixmap_surface(
    conn: &xcb::Connection,
    pixmap: &x::Pixmap,
    visual: &mut x::Visualtype,
    width: u16,
    height: u16,
) -> anyhow::Result<cairo::XCBSurface> {
    let cairo_xcb_connection = unsafe {
        cairo::XCBConnection::from_raw_none(std::mem::transmute::<
            *mut xcb::ffi::xcb_connection_t,
            *mut cairo::ffi::xcb_connection_t,
        >(conn.get_raw_conn()))
    };
    let cairo_xcb_visual = unsafe {
        cairo::XCBVisualType::from_raw_none(std::mem::transmute::<
            *mut xcb::x::Visualtype,
            *mut cairo::ffi::xcb_visualtype_t,
        >(visual as *mut _))
    };

    let pixmap_surface = cairo::XCBSurface::create(
        &cairo_xcb_connection,
        &cairo::XCBDrawable(pixmap.resource_id()),
        &cairo_xcb_visual,
        width.into(),
        height.into(),
    )?;

    conn.flush()?;

    Ok(pixmap_surface)
}

fn make_pixmap_surface_for_bitmap(
    conn: &xcb::Connection,
    pixmap: &x::Pixmap,
    screen: &x::Screen,
    width: u16,
    height: u16,
) -> anyhow::Result<cairo::XCBSurface> {
    let cairo_xcb_connection = unsafe {
        cairo::XCBConnection::from_raw_none(std::mem::transmute::<
            *mut xcb::ffi::xcb_connection_t,
            *mut cairo::ffi::xcb_connection_t,
        >(conn.get_raw_conn()))
    };
    let cairo_xcb_screen = unsafe {
        cairo::XCBScreen::from_raw_none(
            screen as *const _ as *mut x::Screen as *mut cairo::ffi::xcb_screen_t,
        )
    };
    let cairo_xcb_pixmap = cairo::XCBPixmap(pixmap.resource_id());

    let pixmap_surface = cairo::XCBSurface::create_for_bitmap(
        &cairo_xcb_connection,
        &cairo_xcb_screen,
        &cairo_xcb_pixmap,
        width.into(),
        height.into(),
    )?;

    conn.flush()?;

    Ok(pixmap_surface)
}

pub struct XOrgEngine {
    windows: HashMap<x::Window, Window>,
    window_ids: Vec<x::Window>,
    state: Arc<RwLock<state::State>>,
    conn: Arc<xcb::Connection>,
    screen: x::ScreenBuf,
    pub update_tx: crossbeam_channel::Sender<state::Update>,
    update_rx: Option<crossbeam_channel::Receiver<state::Update>>,
    popup_manager: std::sync::Arc<std::sync::Mutex<popup_visibility::PopupManager>>,
    // Set during run().
    loop_handle: Option<calloop::LoopHandle<'static, Self>>,
}

impl XOrgEngine {
    pub fn new(
        config: config::Config<parse::Placeholder>,
        initial_state: state::State,
        notifier: notify::Notifier,
    ) -> anyhow::Result<Self> {
        let state = Arc::new(RwLock::new(initial_state));
        let (update_tx, update_rx) = crossbeam_channel::unbounded();

        let (conn, _) = xcb::Connection::connect_with_xlib_display_and_extensions(
            &[
                xcb::Extension::Input,
                xcb::Extension::Shape,
                xcb::Extension::RandR,
            ],
            &[],
        )
        .unwrap();
        let conn = Arc::new(conn);
        let popup_manager = Arc::new(Mutex::new(popup_visibility::PopupManager::new()));

        let wm_info = wmready::wait().context("Unable to connect to WM")?;

        let screen = {
            let setup = conn.get_setup();
            setup.roots().next().unwrap()
        }
        .to_owned();

        tracing::info!(
            "XInput init: {:?}",
            xutils::query(
                &conn,
                &xinput::XiQueryVersion {
                    major_version: 2,
                    minor_version: 0,
                },
            )
            .context("init xinput 2.0 extension")?
        );

        let mut windows = HashMap::new();

        for (index, bar) in config.bar.iter().enumerate() {
            let window = Window::create_and_show(
                format!("bar{}", index),
                // index,
                &config,
                bar.clone(),
                conn.clone(),
                state.clone(),
                update_tx.clone(),
                &wm_info,
                notifier.clone(),
                popup_manager.clone(),
                update_tx.clone(),
            )?;
            windows.insert(window.id, window);
        }

        let window_ids = windows.keys().cloned().collect();

        Ok(Self {
            windows,
            window_ids,
            state,
            conn,
            screen,
            update_tx,
            update_rx: Some(update_rx),
            loop_handle: None,
            popup_manager,
        })
    }

    fn handle_event(&mut self, event: &xcb::Event) -> anyhow::Result<()> {
        match event {
            xcb::Event::X(x::Event::Expose(event)) => {
                if let Some(window) = self.windows.get_mut(&event.window()) {
                    // Hack for now to distinguish on-demand expose.
                    if let Err(e) = window.render(&mut self.loop_handle) {
                        tracing::error!("Failed to render bar {:?}", e);
                    }
                }
            }
            xcb::Event::Input(xinput::Event::RawMotion(_event)) => {
                let pointer = xutils::query(
                    &self.conn,
                    &x::QueryPointer {
                        window: self.screen.root(),
                    },
                )?;
                for window in self.windows.values_mut() {
                    window.handle_raw_motion(pointer.root_x(), pointer.root_y())?;
                }
            }
            xcb::Event::X(x::Event::MotionNotify(event)) => {
                if let Some(window) = self.windows.get(&event.event()) {
                    window.handle_motion(event.event_x(), event.event_y())?;
                }
            }
            xcb::Event::X(x::Event::LeaveNotify(event)) => {
                if let Some(window) = self.windows.get(&event.event()) {
                    window.handle_motion_leave()?;
                }
            }
            xcb::Event::X(x::Event::ButtonPress(event)) => {
                for window in self.windows.values_mut() {
                    if window.id == event.event() {
                        tracing::trace!(
                            "Button press: X={}, Y={}, button={}",
                            event.event_x(),
                            event.event_y(),
                            event.detail()
                        );
                        let button = match event.detail() {
                            1 => Some(bar::Button::Left),
                            2 => Some(bar::Button::Middle),
                            3 => Some(bar::Button::Right),
                            4 => Some(bar::Button::ScrollUp),
                            5 => Some(bar::Button::ScrollDown),
                            _ => None,
                        };
                        if let Some(button) = button {
                            window.handle_button_press(event.event_x(), event.event_y(), button)?;
                        }
                    }
                }
            }
            _ => {
                tracing::debug!("Unhandled XCB event: {:?}", event);
            }
        }
        Ok(())
    }

    fn pipe_xevents(
        &self,
        calloop_tx: calloop::channel::Sender<EngineMessage>,
    ) -> anyhow::Result<()> {
        let conn = self.conn.clone();
        let calloop_tx_xevent = calloop_tx.clone();
        thread::spawn("eng-xevent", move || loop {
            while let Ok(event) = xutils::get_event(&conn) {
                if let Some(event) = event {
                    calloop_tx_xevent
                        .send(EngineMessage::XEvent(event))
                        .unwrap();
                } else {
                    break;
                }
            }
        })
        .context("engine xevent")
    }

    fn pipe_state_updates(
        &mut self,
        calloop_tx: calloop::channel::Sender<EngineMessage>,
    ) -> anyhow::Result<()> {
        let state_update_rx = self.update_rx.take().unwrap();
        let calloop_tx_state = calloop_tx.clone();
        thread::spawn("eng-state", move || loop {
            while let Ok(state_update) = state_update_rx.recv() {
                calloop_tx_state
                    .send(EngineMessage::Update(state_update))
                    .unwrap();
            }
        })
        .context("engine state update")
    }
}

enum EngineMessage {
    XEvent(xcb::Event),
    Update(state::Update),
}

impl Engine for XOrgEngine {
    fn run(&mut self) -> anyhow::Result<()> {
        let mut event_loop: calloop::EventLoop<Self> =
            calloop::EventLoop::try_new().expect("Failed to create event loop");
        let loop_handle = event_loop.handle();
        self.loop_handle = Some(loop_handle.clone());

        let (calloop_tx, calloop_rx) = calloop::channel::channel();

        self.pipe_state_updates(calloop_tx.clone())
            .context("engine pipe state updates")?;

        self.pipe_xevents(calloop_tx.clone())
            .context("engine pipe xevents")?;

        loop_handle
            .insert_source(calloop_rx, move |evt, _, engine| match evt {
                calloop::channel::Event::Msg(msg) => match msg {
                    EngineMessage::XEvent(event) => {
                        engine.handle_event(&event).unwrap();
                    }
                    EngineMessage::Update(state_update) => {
                        {
                            let mut state = engine.state.write().unwrap();
                            state.handle_state_update(state_update);
                        }
                        for window in engine.windows.values_mut() {
                            if let Err(e) = window.render(&mut engine.loop_handle) {
                                tracing::error!("Failed to render bar {:?}", e);
                            }
                        }
                    }
                },
                calloop::channel::Event::Closed => {}
            })
            .expect("Failed to insert X11 source");

        loop {
            event_loop.dispatch(None, self).context("engine dispatch")?;
        }
    }

    fn update_tx(&self) -> crossbeam_channel::Sender<state::Update> {
        self.update_tx.clone()
    }
}
