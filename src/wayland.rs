#![allow(dead_code)]
use anyhow::Context;
use std::sync::{Arc, Mutex, RwLock};

use crate::{
    bar::{self, BarUpdates, BlockUpdates},
    config, drawing,
    engine::Engine,
    notify, parse,
    popup_visibility::PopupManager,
    state, thread,
};
use sct::reexports::client as smithay_client;
use sct::shell::WaylandSurface;
use sct::{
    delegate_compositor, delegate_layer, delegate_output, delegate_pointer, delegate_registry,
    delegate_seat, delegate_shm, registry_handlers,
};
use smithay_client_toolkit::{
    self as sct,
    seat::pointer::{PointerEvent, PointerEventKind, BTN_LEFT, BTN_MIDDLE, BTN_RIGHT},
};
use wayland_client::protocol::{wl_pointer, wl_seat};

pub struct WaylandWindow {
    name: String,
    state: Arc<RwLock<state::State>>,
    bar: bar::Bar,
    font_cache: Arc<Mutex<drawing::FontCache>>,
    #[cfg(feature = "image")]
    image_loader: drawing::ImageLoader,
    _surface: wayland_client::protocol::wl_surface::WlSurface, // Keep surface alive
    layer_surface: sct::shell::wlr_layer::LayerSurface,
    pool: Option<sct::shm::slot::SlotPool>,
    popup_manager_mutex: Arc<Mutex<PopupManager>>,
    update_tx: crossbeam_channel::Sender<state::Update>,
    width: u32,
    height: u32,
}

impl WaylandWindow {
    #[allow(clippy::too_many_arguments)]
    pub fn create_and_show(
        name: String,
        config: &config::Config<parse::Placeholder>,
        bar_config: config::Bar<parse::Placeholder>,
        state: Arc<RwLock<state::State>>,

        update_tx: crossbeam_channel::Sender<state::Update>,
        notifier: notify::Notifier,
        qh: &smithay_client::QueueHandle<WaylandEngine>,
        compositor_state: &sct::compositor::CompositorState,
        layer_shell: &sct::shell::wlr_layer::LayerShell,
        output: Option<&smithay_client::protocol::wl_output::WlOutput>,
        popup_manager_mutex: Arc<Mutex<PopupManager>>,
    ) -> anyhow::Result<Self> {
        let surface = compositor_state.create_surface(qh);

        let layer_surface = layer_shell.create_layer_surface(
            qh,
            surface.clone(),
            sct::shell::wlr_layer::Layer::Top,
            Some(&name),
            output,
        );

        let margin = &bar_config.margin;
        let height = bar_config.height;
        let window_height = height + margin.top + margin.bottom;

        let anchor = match bar_config.position {
            config::BarPosition::Top => sct::shell::wlr_layer::Anchor::TOP,
            config::BarPosition::Bottom => sct::shell::wlr_layer::Anchor::BOTTOM,
            config::BarPosition::Center => sct::shell::wlr_layer::Anchor::empty(),
        };

        // For center position, only anchor horizontally (LEFT+RIGHT) to let compositor center vertically
        // For top/bottom, anchor to all three sides (LEFT+RIGHT+TOP or LEFT+RIGHT+BOTTOM)
        let horizontal_anchor =
            sct::shell::wlr_layer::Anchor::LEFT | sct::shell::wlr_layer::Anchor::RIGHT;
        layer_surface.set_anchor(horizontal_anchor | anchor);

        layer_surface.set_size(0, window_height as u32);

        // For center position, use exclusive_zone = -1 to float above windows without affecting layout
        // For top/bottom, use positive exclusive zone to push windows
        let exclusive_zone = if bar_config.popup {
            -1
        } else {
            match bar_config.position {
                config::BarPosition::Center => -1,
                _ => window_height as i32,
            }
        };
        layer_surface.set_exclusive_zone(exclusive_zone);
        layer_surface.set_keyboard_interactivity(
            smithay_client_toolkit::shell::wlr_layer::KeyboardInteractivity::None,
        );
        layer_surface.commit();
        let bar = bar::Bar::new(config, bar_config.clone(), notifier.clone())?;

        let font_cache = Arc::new(Mutex::new(drawing::FontCache::new()));
        #[cfg(feature = "image")]
        let image_loader = drawing::ImageLoader::new();

        Ok(Self {
            name,
            state,
            bar,
            font_cache,
            #[cfg(feature = "image")]
            image_loader,
            _surface: surface,
            layer_surface,
            pool: None,

            width: 0,
            height: 0,
            update_tx,
            popup_manager_mutex,
        })
    }

    pub fn draw(
        &mut self,
        _qh: &smithay_client::QueueHandle<WaylandEngine>,
        shm: &sct::shm::Shm,
        compositor_state: &sct::compositor::CompositorState,
        loop_handle: &mut Option<calloop::LoopHandle<'static, WaylandEngine>>,
    ) -> anyhow::Result<()> {
        let width = self.width;
        let height = self.height;

        // Don't draw if we haven't received configure event yet
        if width == 0 || height == 0 {
            tracing::trace!(
                "Skipping draw: window not yet configured ({}x{})",
                width,
                height
            );
            return Ok(());
        }

        let stride = width as i32 * 4;
        let size = (width * height * 4) as usize;
        tracing::trace!(
            "Drawing window {}, width: {}, height: {}",
            self.name,
            width,
            height
        );
        let pool = self.pool.get_or_insert_with(|| {
            sct::shm::slot::SlotPool::new(size * 2, shm).expect("Failed to create pool")
        });

        if pool.len() < size * 2 {
            pool.resize(size * 2).expect("Failed to resize pool");
        }

        let (buffer, canvas) = pool
            .create_buffer(
                self.width as i32,
                self.height as i32,
                stride,
                smithay_client::protocol::wl_shm::Format::Argb8888,
            )
            .context("Failed to create buffer")?;
        let surface = unsafe {
            cairo::ImageSurface::create_for_data_unsafe(
                canvas.as_mut_ptr(),
                cairo::Format::ARgb32,
                width as i32,
                height as i32,
                stride,
            )
            .unwrap()
        };
        let cr = cairo::Context::new(&surface).unwrap();
        let mut context = drawing::Context::new(
            cr,
            self.font_cache.clone(),
            #[cfg(feature = "image")]
            self.image_loader.clone(),
            drawing::Mode::Full,
        )
        .context("Failed to create drawing context")?;

        let state = self.state.clone();
        let state = state.read().unwrap();
        let pointer_position = state.pointer_position.get(&self.name).copied();
        let mut error = state.build_error_msg();

        let updates = match self.bar.update(&mut context, &state.vars, pointer_position) {
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
        tracing::debug!("Updates: {:#?}", updates);

        self.bar.set_error(&mut context, error.clone());

        if !updates.block_updates.popup.is_empty() {
            tracing::debug!("Showing popups: {:#?}", updates.block_updates.popup);
        }
        for popup in updates.block_updates.popup.values() {
            for block in popup {
                PopupManager::trigger_popup(
                    &self.popup_manager_mutex,
                    loop_handle,
                    self.update_tx.clone(),
                    block.clone(),
                );
            }
        }

        let layout_changed = self.bar.layout_groups(self.width as f64, &None);
        tracing::debug!("Layout changed: {}", layout_changed);

        self.bar
            .render(&context, &bar::RedrawScope::All)
            .context("Failed to render bar")?;

        buffer
            .attach_to(self.layer_surface.wl_surface())
            .context("Failed to attach buffer")?;
        self.layer_surface
            .wl_surface()
            .damage(0, 0, width as i32, height as i32);

        // Set input region to only accept clicks on blocks
        let input_rects = self.bar.get_input_rects();
        if let Ok(region) = sct::compositor::Region::new(compositor_state) {
            for rect in &input_rects {
                region.add(rect.x, rect.y, rect.width, rect.height);
            }
            self.layer_surface
                .wl_surface()
                .set_input_region(Some(region.wl_region()));
        }

        self.layer_surface.wl_surface().commit();
        Ok(())
    }
    pub fn wl_surface(&self) -> &wayland_client::protocol::wl_surface::WlSurface {
        self.layer_surface.wl_surface()
    }

    pub fn handle_motion(&self, x: f64, y: f64) -> anyhow::Result<()> {
        // Need to replicate x11 behavior: update state with motion
        self.update_tx()
            .send(state::Update::MotionUpdate(state::MotionUpdate {
                window_name: self.name.clone(),
                position: Some((x as i16, y as i16)),
            }))?;
        Ok(())
    }

    fn update_tx(&self) -> crossbeam_channel::Sender<state::Update> {
        self.update_tx.clone()
    }

    pub fn handle_motion_leave(&self) -> anyhow::Result<()> {
        self.update_tx()
            .send(state::Update::MotionUpdate(state::MotionUpdate {
                window_name: self.name.clone(),
                position: None,
            }))?;
        Ok(())
    }

    pub fn handle_button_press(
        &mut self,
        x: f64,
        y: f64,
        button: bar::Button,
    ) -> anyhow::Result<()> {
        self.bar.handle_button_press(x as i16, y as i16, button)
    }
}

pub struct WaylandEngine {
    state: Arc<RwLock<state::State>>,
    conn: smithay_client::Connection,
    registry_state: sct::registry::RegistryState,
    output_state: sct::output::OutputState,
    compositor_state: sct::compositor::CompositorState,
    shm: sct::shm::Shm,
    seat_state: sct::seat::SeatState,
    layer_shell: sct::shell::wlr_layer::LayerShell,
    event_queue: Option<smithay_client::EventQueue<WaylandEngine>>,
    pub update_tx: crossbeam_channel::Sender<state::Update>,
    update_rx: Option<crossbeam_channel::Receiver<state::Update>>,
    windows: Vec<WaylandWindow>,
    qh: smithay_client::QueueHandle<WaylandEngine>,
    pointer_surface: Option<wayland_client::protocol::wl_surface::WlSurface>,
    last_pointer_pos: (f64, f64),
    popup_manager: std::sync::Arc<std::sync::Mutex<PopupManager>>,
    // Set during run().
    loop_handle: Option<calloop::LoopHandle<'static, WaylandEngine>>,
}

impl sct::seat::pointer::PointerHandler for WaylandEngine {
    fn pointer_frame(
        &mut self,
        _conn: &smithay_client::Connection,
        _qh: &smithay_client::QueueHandle<Self>,
        _pointer: &wl_pointer::WlPointer,
        events: &[PointerEvent],
    ) {
        for event in events {
            match event.kind {
                PointerEventKind::Enter { .. } => {
                    self.pointer_surface = Some(event.surface.clone());
                    self.last_pointer_pos = event.position;
                    for window in &mut self.windows {
                        if window.wl_surface() == &event.surface {
                            if let Err(e) = window.handle_motion(event.position.0, event.position.1)
                            {
                                tracing::error!("handle_motion error: {}", e);
                            }
                            break;
                        }
                    }
                }
                PointerEventKind::Leave { .. } => {
                    if self.pointer_surface.as_ref() == Some(&event.surface) {
                        self.pointer_surface = None;
                        for window in &mut self.windows {
                            if window.wl_surface() == &event.surface {
                                if let Err(e) = window.handle_motion_leave() {
                                    tracing::error!("handle_motion_leave error: {}", e);
                                }
                                break;
                            }
                        }
                    }
                }
                PointerEventKind::Motion { .. } => {
                    self.last_pointer_pos = event.position;
                    if let Some(surface) = &self.pointer_surface {
                        for window in &mut self.windows {
                            if window.wl_surface() == surface {
                                if let Err(e) =
                                    window.handle_motion(event.position.0, event.position.1)
                                {
                                    tracing::error!("handle_motion error: {}", e);
                                }
                                break;
                            }
                        }
                    }
                }
                PointerEventKind::Press { button, .. } => {
                    let button = match button {
                        BTN_LEFT => bar::Button::Left,
                        BTN_RIGHT => bar::Button::Right,
                        BTN_MIDDLE => bar::Button::Middle,
                        _ => return,
                    };
                    if let Some(surface) = &self.pointer_surface {
                        for window in &mut self.windows {
                            if window.wl_surface() == surface {
                                if let Err(e) = window.handle_button_press(
                                    self.last_pointer_pos.0,
                                    self.last_pointer_pos.1,
                                    button,
                                ) {
                                    tracing::error!("handle_button_press error: {}", e);
                                }
                                break;
                            }
                        }
                    }
                }
                PointerEventKind::Axis {
                    vertical,
                    horizontal: _,
                    ..
                } => {
                    let value = if vertical.absolute > 0.0 {
                        vertical.absolute
                    } else {
                        0.0
                    };
                    if value != 0.0 {
                        let button = if value > 0.0 {
                            bar::Button::ScrollDown
                        } else {
                            bar::Button::ScrollUp
                        };
                        if let Some(surface) = &self.pointer_surface {
                            for window in &mut self.windows {
                                if window.wl_surface() == surface {
                                    if let Err(e) = window.handle_button_press(
                                        self.last_pointer_pos.0,
                                        self.last_pointer_pos.1,
                                        button,
                                    ) {
                                        tracing::error!(
                                            "handle_button_press (scroll) error: {}",
                                            e
                                        );
                                    }
                                    break;
                                }
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }
}

impl WaylandEngine {
    pub fn new(
        config: config::Config<parse::Placeholder>,
        initial_state: state::State,
        notifier: notify::Notifier,
    ) -> anyhow::Result<Self> {
        let state = Arc::new(RwLock::new(initial_state));
        let (update_tx, update_rx) = crossbeam_channel::unbounded();

        let conn =
            smithay_client::Connection::connect_to_env().context("Unable to connect to Wayland")?;

        let (globals, event_queue) =
            smithay_client::globals::registry_queue_init::<WaylandEngine>(&conn)
                .context("Unable to connect to Wayland")
                .unwrap();

        let qh = event_queue.handle();

        let registry_state = sct::registry::RegistryState::new(&globals);
        let output_state = sct::output::OutputState::new(&globals, &qh);
        let compositor_state = sct::compositor::CompositorState::bind(&globals, &qh)
            .context("Unable to create compositor state")?;
        let shm = sct::shm::Shm::bind(&globals, &qh).context("Unable to create shm state")?;
        let seat_state = sct::seat::SeatState::new(&globals, &qh);
        let layer_shell = sct::shell::wlr_layer::LayerShell::bind(&globals, &qh)
            .context("Unable to create layer shell state")?;
        let popup_manager = Arc::new(Mutex::new(PopupManager::new()));

        let mut windows = Vec::with_capacity(config.bar.len());

        for (index, bar) in config.bar.iter().enumerate() {
            let output = bar.monitor.as_ref().and_then(|name| {
                output_state.outputs().find(|output| {
                    if let Some(info) = output_state.info(output) {
                        if let Some(output_name) = info.name {
                            if output_name == *name {
                                return true;
                            }
                        }
                    }
                    false
                })
            });

            let output = output.or_else(|| output_state.outputs().next());

            if let Some(name) = &bar.monitor {
                tracing::info!(
                    "Creating wayland window for bar {} on monitor {:?}: output {:?}",
                    index,
                    bar.monitor,
                    output
                );

                if output.is_none() {
                    return Err(anyhow::anyhow!(
                        "Monitor {:?} not found, but specified for the bar",
                        name
                    ));
                }
            }

            let wayland_window = WaylandWindow::create_and_show(
                format!("oatbar-bar-{}", index),
                &config,
                bar.clone(),
                state.clone(),
                update_tx.clone(),
                notifier.clone(),
                &qh,
                &compositor_state,
                &layer_shell,
                output.as_ref(),
                popup_manager.clone(),
            )
            .context("Unable to create wayland window")?;
            windows.push(wayland_window);
        }

        Ok(Self {
            state,
            conn,
            update_tx,
            update_rx: Some(update_rx),
            registry_state,
            shm,
            seat_state,
            layer_shell,
            output_state,
            compositor_state,
            event_queue: Some(event_queue),
            windows,
            qh,
            pointer_surface: None,
            last_pointer_pos: (0.0, 0.0),
            popup_manager,
            loop_handle: None,
        })
    }
}

impl Engine for WaylandEngine {
    fn run(&mut self) -> anyhow::Result<()> {
        let mut event_loop: calloop::EventLoop<Self> =
            calloop::EventLoop::try_new().expect("Failed to create event loop");
        let loop_handle = event_loop.handle();
        self.loop_handle = Some(loop_handle.clone());

        calloop_wayland_source::WaylandSource::new(
            self.conn.clone(),
            self.event_queue.take().unwrap(),
        )
        .insert(loop_handle.clone())
        .unwrap();

        let (tx, channel) = calloop::channel::channel();

        // Convert channel type by resending, not optimal, but simple.
        if let Some(update_rx) = self.update_rx.take() {
            thread::spawn("eng-state", move || {
                while let Ok(state_update) = update_rx.recv() {
                    if tx.send(state_update).is_err() {
                        break;
                    }
                }
                tracing::debug!("eng-state thread exiting");
                Ok(())
            })
            .context("unable to spawn eng-state")?;
        }
        loop_handle
            .insert_source(channel, move |state_update, _metadata, engine| {
                if let calloop::channel::Event::Msg(state_update) = state_update {
                    tracing::trace!("state_update: {:?}", state_update);
                    {
                        let mut state = engine.state.write().unwrap();
                        state.handle_state_update(state_update);
                    }
                    for window in engine.windows.iter_mut() {
                        if let Err(err) = window.draw(
                            &engine.qh,
                            &engine.shm,
                            &engine.compositor_state,
                            &mut engine.loop_handle,
                        ) {
                            tracing::error!("unable to draw window: {}", err);
                        }
                    }
                }
            })
            .expect("Failed to insert source");

        loop {
            event_loop
                .dispatch(None, self)
                .context("Failed to dispatch event loop")?;
        }
    }

    fn update_tx(&self) -> crossbeam_channel::Sender<state::Update> {
        self.update_tx.clone()
    }
}

impl sct::shm::ShmHandler for WaylandEngine {
    fn shm_state(&mut self) -> &mut sct::shm::Shm {
        &mut self.shm
    }
}

impl sct::registry::ProvidesRegistryState for WaylandEngine {
    fn registry(&mut self) -> &mut sct::registry::RegistryState {
        &mut self.registry_state
    }
    registry_handlers!(sct::output::OutputState, sct::seat::SeatState);
}

impl sct::seat::SeatHandler for WaylandEngine {
    fn seat_state(&mut self) -> &mut sct::seat::SeatState {
        &mut self.seat_state
    }

    fn new_seat(
        &mut self,
        _: &smithay_client::Connection,
        _: &smithay_client::QueueHandle<Self>,
        _: wl_seat::WlSeat,
    ) {
    }

    fn new_capability(
        &mut self,
        _conn: &smithay_client::Connection,
        qh: &smithay_client::QueueHandle<Self>,
        seat: wl_seat::WlSeat,
        capability: sct::seat::Capability,
    ) {
        if capability == sct::seat::Capability::Pointer {
            self.seat_state.get_pointer(qh, &seat).unwrap();
        }
    }

    fn remove_capability(
        &mut self,
        _conn: &smithay_client::Connection,
        _qh: &smithay_client::QueueHandle<Self>,
        _seat: wl_seat::WlSeat,
        _capability: sct::seat::Capability,
    ) {
    }

    fn remove_seat(
        &mut self,
        _: &smithay_client::Connection,
        _: &smithay_client::QueueHandle<Self>,
        _: wl_seat::WlSeat,
    ) {
    }
}

impl sct::output::OutputHandler for WaylandEngine {
    fn output_state(&mut self) -> &mut sct::output::OutputState {
        &mut self.output_state
    }

    fn new_output(
        &mut self,
        _conn: &smithay_client::Connection,
        _qh: &smithay_client::QueueHandle<Self>,
        _output: smithay_client::protocol::wl_output::WlOutput,
    ) {
    }

    fn update_output(
        &mut self,
        _conn: &smithay_client::Connection,
        _qh: &smithay_client::QueueHandle<Self>,
        _output: smithay_client::protocol::wl_output::WlOutput,
    ) {
    }

    fn output_destroyed(
        &mut self,
        _conn: &smithay_client::Connection,
        _qh: &smithay_client::QueueHandle<Self>,
        _output: smithay_client::protocol::wl_output::WlOutput,
    ) {
    }
}

impl sct::compositor::CompositorHandler for WaylandEngine {
    fn scale_factor_changed(
        &mut self,
        _conn: &smithay_client::Connection,
        _qh: &smithay_client::QueueHandle<Self>,
        _surface: &wayland_client::protocol::wl_surface::WlSurface,
        _new_factor: i32,
    ) {
    }

    fn frame(
        &mut self,
        _conn: &smithay_client::Connection,
        _qh: &smithay_client::QueueHandle<Self>,
        _surface: &wayland_client::protocol::wl_surface::WlSurface,
        _time: u32,
    ) {
    }

    fn surface_enter(
        &mut self,
        _conn: &smithay_client::Connection,
        _qh: &smithay_client::QueueHandle<Self>,
        _surface: &wayland_client::protocol::wl_surface::WlSurface,
        _output: &wayland_client::protocol::wl_output::WlOutput,
    ) {
    }

    fn surface_leave(
        &mut self,
        _conn: &smithay_client::Connection,
        _qh: &smithay_client::QueueHandle<Self>,
        _surface: &wayland_client::protocol::wl_surface::WlSurface,
        _output: &wayland_client::protocol::wl_output::WlOutput,
    ) {
    }

    fn transform_changed(
        &mut self,
        _conn: &smithay_client::Connection,
        _qh: &smithay_client::QueueHandle<Self>,
        _surface: &wayland_client::protocol::wl_surface::WlSurface,
        _new_transform: wayland_client::protocol::wl_output::Transform,
    ) {
    }
}

impl sct::shell::wlr_layer::LayerShellHandler for WaylandEngine {
    fn closed(
        &mut self,
        _conn: &smithay_client::Connection,
        _qh: &smithay_client::QueueHandle<Self>,
        _layer: &sct::shell::wlr_layer::LayerSurface,
    ) {
    }

    fn configure(
        &mut self,
        _conn: &smithay_client::Connection,
        qh: &smithay_client::QueueHandle<Self>,
        layer: &sct::shell::wlr_layer::LayerSurface,
        configure: sct::shell::wlr_layer::LayerSurfaceConfigure,
        _serial: u32,
    ) {
        for window in &mut self.windows {
            if window.layer_surface == *layer {
                if configure.new_size.0 > 0 {
                    window.width = configure.new_size.0;
                }
                if configure.new_size.1 > 0 {
                    window.height = configure.new_size.1;
                }

                // Initial draw or redraw on resize
                if let Err(e) =
                    window.draw(qh, &self.shm, &self.compositor_state, &mut self.loop_handle)
                {
                    tracing::error!("Failed to draw: {}", e);
                }
            }
        }
    }
}

delegate_output!(WaylandEngine);
delegate_registry!(WaylandEngine);
delegate_compositor!(WaylandEngine);
delegate_shm!(WaylandEngine);
delegate_seat!(WaylandEngine);
delegate_pointer!(WaylandEngine);
delegate_layer!(WaylandEngine);
