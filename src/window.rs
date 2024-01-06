// Copyright 2023 Oatbar Authors
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use anyhow::Context;
use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex, RwLock},
    time::{Duration, SystemTime},
};
use xcb::{x, xinput, Xid};

use crate::{bar, config, drawing, parse, state, timer, wmready, xutils};
use tracing::*;

pub struct VisibilityControl {
    name: String,
    conn: Arc<xcb::Connection>,
    window_id: x::Window,
    timer: Option<timer::Timer>,
    show_only: Option<HashMap<config::PopupMode, HashSet<String>>>,
    visible: bool,
    default_visibility: bool,
    popped_up: bool,
}

impl VisibilityControl {
    fn is_poping_up(&self) -> bool {
        self.timer.is_some()
    }

    fn set_visible(&mut self, visible: bool) -> anyhow::Result<()> {
        if self.visible == visible {
            return Ok(());
        }
        if visible {
            xutils::send(
                &self.conn,
                &x::MapWindow {
                    window: self.window_id,
                },
            )?;
        } else {
            xutils::send(
                &self.conn,
                &x::UnmapWindow {
                    window: self.window_id,
                },
            )?;
        }
        self.conn.flush()?;
        self.visible = visible;
        Ok(())
    }

    fn extend_show_only(&mut self, extra_show_only: HashMap<config::PopupMode, HashSet<String>>) {
        if extra_show_only.is_empty() {
            return;
        }

        let show_only = self.show_only.get_or_insert_with(Default::default);

        for (k, v) in extra_show_only.into_iter() {
            show_only.entry(k).or_default().extend(v.into_iter());
        }
    }

    fn set_default_visibility(&mut self, value: bool) -> anyhow::Result<()> {
        if value == self.default_visibility {
            return Ok(());
        }
        self.default_visibility = value;
        if !self.popped_up {
            self.set_visible(self.default_visibility)?;
        }
        Ok(())
    }

    fn reset_show_only(&mut self) {
        self.show_only = None
    }

    fn show_or_prolong_popup(
        visibility_control_lock: &Arc<RwLock<VisibilityControl>>,
    ) -> anyhow::Result<()> {
        let reset_timer_at = SystemTime::now()
            .checked_add(Duration::from_secs(1))
            .unwrap();
        let mut visibility_control = visibility_control_lock.write().unwrap();
        match &visibility_control.timer {
            Some(timer) => {
                timer.set_at(reset_timer_at);
            }
            None => {
                let timer = {
                    visibility_control.set_visible(true)?;
                    visibility_control.popped_up = true;
                    let visibility_control_lock = visibility_control_lock.clone();
                    timer::Timer::new(
                        &format!("pptimer-{}", visibility_control.name),
                        reset_timer_at,
                        move || {
                            let mut visibility_control = visibility_control_lock.write().unwrap();
                            visibility_control.timer = None;
                            visibility_control.reset_show_only();
                            let default_visibility = visibility_control.default_visibility;
                            visibility_control.popped_up = false;
                            if let Err(e) = visibility_control.set_visible(default_visibility) {
                                tracing::error!("Failed to show window: {:?}", e);
                            }
                        },
                    )?
                };
                visibility_control.timer = Some(timer);
            }
        }
        Ok(())
    }
}

pub struct Window {
    pub conn: Arc<xcb::Connection>,
    pub id: x::Window,
    pub width: u16,
    pub height: u16,
    back_buffer_context: drawing::Context,
    shape_buffer_context: drawing::Context,
    swap_gc: x::Gcontext,
    bar: bar::Bar,
    bar_index: usize,
    bar_config: config::Bar<parse::Placeholder>,
    state: Arc<RwLock<state::State>>,
    screen: x::ScreenBuf,
    window_height: u16,
    visibility_control: Arc<RwLock<VisibilityControl>>,
}

impl Window {
    pub fn create_and_show(
        name: String,
        bar_index: usize,
        bar_config: config::Bar<parse::Placeholder>,
        conn: Arc<xcb::Connection>,
        state: Arc<RwLock<state::State>>,
        wm_info: &wmready::WMInfo,
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
                x::Cw::OverrideRedirect(bar_config.popup),
                //x::Cw::OverrideRedirect(true),
                x::Cw::EventMask(
                    x::EventMask::EXPOSURE | x::EventMask::KEY_PRESS | x::EventMask::BUTTON_PRESS,
                ),
                x::Cw::Colormap(cid),
            ],
        });

        let raw_motion_mask_buf =
            xinput::EventMaskBuf::new(xinput::Device::All, &[xinput::XiEventMask::RAW_MOTION]);

        xutils::send(
            &conn,
            &xinput::XiSelectEvents {
                window: screen.root(),
                masks: &[raw_motion_mask_buf],
            },
        )?;

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

        if !bar_config.popup {
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
        let back_buffer: x::Pixmap = conn.generate_id();
        xutils::send(
            &conn,
            &x::CreatePixmap {
                depth: 32,
                pid: back_buffer,
                drawable: xcb::x::Drawable::Window(id),
                width: window_width,
                height: window_height,
            },
        )?;

        let font_cache = Arc::new(Mutex::new(drawing::FontCache::new()));

        let back_buffer_surface =
            make_pixmap_surface(&conn, &back_buffer, &mut vis32, window_width, window_height)?;
        let back_buffer_context = drawing::Context::new(
            font_cache.clone(),
            back_buffer,
            back_buffer_surface,
            window_width.into(),
            window_height.into(),
            drawing::Mode::Full,
        )?;

        let shape_buffer: x::Pixmap = conn.generate_id();
        xutils::send(
            &conn,
            &x::CreatePixmap {
                depth: 1,
                pid: shape_buffer,
                drawable: xcb::x::Drawable::Window(id),
                width: window_width,
                height: window_height,
            },
        )?;
        let shape_buffer_surface = make_pixmap_surface_for_bitmap(
            &conn,
            &shape_buffer,
            &screen,
            window_width,
            window_height,
        )?;
        let shape_buffer_context = drawing::Context::new(
            font_cache,
            shape_buffer,
            shape_buffer_surface,
            window_width.into(),
            window_height.into(),
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

        if !bar_config.popup {
            xutils::send(&conn, &x::MapWindow { window: id })?;
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

        let bar = bar::Bar::new(&bar_config)?;

        let visible = !bar_config.popup;

        Ok(Self {
            conn: conn.clone(),
            id,
            width: window_width,
            height: window_height,
            back_buffer_context,
            shape_buffer_context,
            swap_gc,
            bar_index,
            bar,
            state,
            screen,
            bar_config,
            window_height,
            visibility_control: Arc::new(RwLock::new(VisibilityControl {
                name,
                window_id: id,
                timer: None,
                conn,
                show_only: None,
                visible,
                default_visibility: visible,
                popped_up: false,
            })),
        })
    }

    fn render_bar(&self, redraw: bar::RedrawScope) -> anyhow::Result<()> {
        self.bar.render(&self.back_buffer_context, &redraw)?;
        self.bar.render(&self.shape_buffer_context, &redraw)?;

        self.swap_buffers()?;
        self.apply_shape()?;
        self.conn.flush()?;
        Ok(())
    }

    pub fn render(&mut self, from_os: bool) -> anyhow::Result<()> {
        let state = self.state.read().unwrap();
        let bar_config = match state.bars.get(self.bar_index) {
            Some(bar_config) => bar_config,
            None => {
                return Ok(());
            }
        };
        let mut updates = self.bar.update(
            &self.back_buffer_context,
            bar_config,
            &state.blocks,
            &state.build_error_msg(),
        );

        if from_os {
            updates.redraw = bar::RedrawScope::All;
        }

        if self.bar_config.popup && !updates.popup.is_empty() {
            if let Err(e) = VisibilityControl::show_or_prolong_popup(&self.visibility_control) {
                tracing::error!("Showing popup failed: {:?}", e);
            }
        }
        let (visible, show_only, redraw) = {
            let mut visibility_control = self.visibility_control.write().unwrap();
            if let Some(visible_from_vars) = updates.visible_from_vars {
                visibility_control.set_default_visibility(visible_from_vars)?;
            }
            if self.bar_config.popup {
                visibility_control.extend_show_only(updates.popup);
                let redraw_mode = if visibility_control.show_only.is_some() {
                    bar::RedrawScope::All
                } else {
                    updates.redraw
                };
                // Maybe there is a race condition between visibility and rendering.
                (
                    visibility_control.visible,
                    visibility_control.show_only.clone(),
                    redraw_mode,
                )
            } else {
                (visibility_control.visible, None, updates.redraw)
            }
        };

        if visible && redraw != bar::RedrawScope::None {
            self.bar
                .layout_blocks(self.back_buffer_context.width, &show_only);

            self.render_bar(redraw)?;
        }
        Ok(())
    }

    pub fn handle_button_press(&self, x: i16, y: i16) -> anyhow::Result<()> {
        self.bar.handle_button_press(x, y)
    }

    pub fn handle_raw_motion(&self, _x: i16, y: i16) -> anyhow::Result<()> {
        if !self.bar_config.popup || !self.bar_config.popup_at_edge {
            return Ok(());
        }
        let edge_size: i16 = 3;
        let screen_height: i16 = self.screen.height_in_pixels() as i16;
        let over_window = match self.bar_config.position {
            config::BarPosition::Top => y < self.window_height as i16,
            config::BarPosition::Bottom => y > screen_height - self.window_height as i16,
            config::BarPosition::Center => false,
        };
        let over_edge = match self.bar_config.position {
            config::BarPosition::Top => y < edge_size,
            config::BarPosition::Bottom => y > screen_height - edge_size,
            config::BarPosition::Center => false,
        };

        let mut visibility_control = self.visibility_control.write().unwrap();
        if !visibility_control.is_poping_up() {
            if !visibility_control.visible && over_edge {
                visibility_control.set_visible(true)?;
            } else if visibility_control.visible && !over_window {
                visibility_control.set_visible(false)?;
                visibility_control.reset_show_only();
            }
        }
        Ok(())
    }

    fn apply_shape(&self) -> anyhow::Result<()> {
        self.shape_buffer_context.buffer_surface.flush();
        xutils::send(
            &self.conn,
            &xcb::shape::Mask {
                operation: xcb::shape::So::Set,
                destination_kind: xcb::shape::Sk::Bounding,
                destination_window: self.id,
                x_offset: 0,
                y_offset: 0,
                source_bitmap: self.shape_buffer_context.buffer,
            },
        )?;
        Ok(())
    }

    fn swap_buffers(&self) -> anyhow::Result<()> {
        self.back_buffer_context.buffer_surface.flush();
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
                src_drawable: xcb::x::Drawable::Pixmap(self.back_buffer_context.buffer),
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
    let cairo_xcb_connection =
        unsafe { cairo::XCBConnection::from_raw_none(std::mem::transmute(conn.get_raw_conn())) };
    let cairo_xcb_visual =
        unsafe { cairo::XCBVisualType::from_raw_none(std::mem::transmute(visual as *mut _)) };

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
    let cairo_xcb_connection =
        unsafe { cairo::XCBConnection::from_raw_none(std::mem::transmute(conn.get_raw_conn())) };
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
