#![allow(dead_code, unused_variables)]
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
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use xcb::{x, xinput};

use crate::{bar, config, notify, parse, state, thread, window, wmready, xutils};

use sct::reexports::client as smithay_client;
use sct::shell::WaylandSurface;
use sct::{
    delegate_compositor, delegate_layer, delegate_output, delegate_registry, delegate_shm,
    registry_handlers,
};
use smithay_client_toolkit::{self as sct};

pub struct WaylandWindow {
    name: String,
    state: Arc<RwLock<state::State>>,
    _surface: wayland_client::protocol::wl_surface::WlSurface, // Keep surface alive
    layer_surface: sct::shell::wlr_layer::LayerSurface,
    pool: Option<sct::shm::slot::SlotPool>,
    width: u32,
    height: u32,
}

impl WaylandWindow {
    #[allow(clippy::too_many_arguments)]
    pub fn create_and_show(
        name: String,
        // bar_index: usize,
        config: &config::Config<parse::Placeholder>,
        bar_config: config::Bar<parse::Placeholder>,
        state: Arc<RwLock<state::State>>,
        state_update_tx: crossbeam_channel::Sender<state::Update>,
        notifier: notify::Notifier,
        qh: &smithay_client::QueueHandle<WaylandEngine>,
        compositor_state: &sct::compositor::CompositorState,
        layer_shell: &sct::shell::wlr_layer::LayerShell,
    ) -> anyhow::Result<Self> {
        let surface = compositor_state.create_surface(qh);

        let layer_surface = layer_shell.create_layer_surface(
            qh,
            surface.clone(),
            sct::shell::wlr_layer::Layer::Top,
            Some(&name),
            None,
        );

        layer_surface.set_anchor(
            sct::shell::wlr_layer::Anchor::LEFT
                | sct::shell::wlr_layer::Anchor::RIGHT
                | sct::shell::wlr_layer::Anchor::BOTTOM,
        );
        layer_surface.set_size(0, 32);
        layer_surface.set_exclusive_zone(32);
        layer_surface.set_keyboard_interactivity(
            smithay_client_toolkit::shell::wlr_layer::KeyboardInteractivity::None,
        );
        layer_surface.commit();

        Ok(Self {
            name,
            state,
            _surface: surface,
            layer_surface,
            pool: None,
            width: 0,
            height: 0,
        })
    }

    pub fn draw(
        &mut self,
        qh: &smithay_client::QueueHandle<WaylandEngine>,
        shm: &sct::shm::Shm,
    ) -> anyhow::Result<()> {
        tracing::info!("Drawing window {}", self.name);
        let width = self.width;
        let height = self.height;
        let stride = width as i32 * 4;
        let size = (width * height * 4) as usize;

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
        cr.set_operator(cairo::Operator::Source);
        cr.set_source_rgba(0.5, 0.5, 0.5, 0.5);
        cr.paint().unwrap();

        buffer
            .attach_to(self.layer_surface.wl_surface())
            .context("Failed to attach buffer")?;
        self.layer_surface
            .wl_surface()
            .damage(0, 0, width as i32, height as i32);

        self.layer_surface
            .wl_surface()
            .frame(qh, self.layer_surface.wl_surface().clone());

        self.layer_surface.wl_surface().commit();
        Ok(())
    }
}
//         let surface = conn.create_surface().context("Unable to create surface")?;

//         let layer_surface = sct::shell::wlr_layer::LayerSurface::cre(
//             conn,
//             qh,
//             &surface,
//             output,
//             sct::shell::wlr_layer::LayerSurfaceRole::Overlay,
//         )
//         .context("Unable to create layer surface")?;

//         Ok(Self {
//             _surface: surface,
//             layer_surface,
//         })
//     }
// }

pub struct WaylandEngine {
    state: Arc<RwLock<state::State>>,
    conn: smithay_client::Connection,
    registry_state: sct::registry::RegistryState,
    output_state: sct::output::OutputState,
    compositor_state: sct::compositor::CompositorState,
    shm: sct::shm::Shm,
    layer_shell: sct::shell::wlr_layer::LayerShell,
    event_queue: Option<smithay_client::EventQueue<WaylandEngine>>,
    pub update_tx: crossbeam_channel::Sender<state::Update>,
    update_rx: Option<crossbeam_channel::Receiver<state::Update>>,
    windows: Vec<WaylandWindow>,
    qh: smithay_client::QueueHandle<WaylandEngine>,
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
        let layer_shell = sct::shell::wlr_layer::LayerShell::bind(&globals, &qh)
            .context("Unable to create layer shell state")?;

        let mut windows = Vec::new();
        for (index, bar) in config.bar.iter().enumerate() {
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
            layer_shell,
            output_state,
            compositor_state,
            event_queue: Some(event_queue),
            windows,
            qh,
        })
    }

    pub fn run(mut self) -> anyhow::Result<()> {
        let mut event_loop: calloop::EventLoop<Self> =
            calloop::EventLoop::try_new().expect("Failed to create event loop");
        let loop_handle = event_loop.handle();

        calloop_wayland_source::WaylandSource::new(
            self.conn.clone(),
            self.event_queue.take().unwrap(),
        )
        .insert(loop_handle.clone())
        .unwrap();

        let (tx, channel) = calloop::channel::channel();

        // Convert channel type by resending, not optimal, but simple.
        if let Some(update_rx) = self.update_rx.take() {
            thread::spawn("eng-state", move || loop {
                while let Ok(state_update) = update_rx.recv() {
                    tx.send(state_update).unwrap();
                }
            })
            .context("unable to spawn eng-state")?;
        }

        let state = self.state.clone();

        loop_handle
            .insert_source(channel, move |state_update, _metadata, engine| {
                if let calloop::channel::Event::Msg(state_update) = state_update {
                    tracing::trace!("state_update: {:?}", state_update);
                    {
                        let mut state = engine.state.write().unwrap();
                        state.handle_state_update(state_update);
                    }
                    for window in engine.windows.iter_mut() {
                        if let Err(err) = window.draw(&engine.qh, &engine.shm) {
                            tracing::error!("unable to draw window: {}", err);
                        }
                    }
                }
            })
            .expect("Failed to insert source");

        loop {
            event_loop
                .dispatch(std::time::Duration::from_millis(16), &mut self)
                .context("Failed to dispatch event loop")?;

            // if self.exit {
            //     break;
            // }
        }
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
    registry_handlers!(sct::output::OutputState);
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
        // for bar in &mut self.bars {
        //     if bar.layer_surface.wl_surface() == surface {
        //         bar.draw(qh, &self.shm);
        //         break;
        //     }
        // }
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
        // If one bar closes, we could exit or just remove it.
        // For simplicity, let's exit if any bar is closed.
        // self.exit = true;
        // Ideally we would verify which bar it is.
        // But finding it in the Vec to remove it is tricky due to ownership if we are iterating?
        // Actually, we are not iterating here.
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
                if let Err(e) = window.draw(qh, &self.shm) {
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
delegate_layer!(WaylandEngine);

pub struct XOrgEngine {
    windows: HashMap<x::Window, window::Window>,
    window_ids: Vec<x::Window>,
    state: Arc<RwLock<state::State>>,
    conn: Arc<xcb::Connection>,
    screen: x::ScreenBuf,
    pub update_tx: crossbeam_channel::Sender<state::Update>,
    update_rx: Option<crossbeam_channel::Receiver<state::Update>>,
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
            let window = window::Window::create_and_show(
                format!("bar{}", index),
                // index,
                &config,
                bar.clone(),
                conn.clone(),
                state.clone(),
                update_tx.clone(),
                &wm_info,
                notifier.clone(),
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
        })
    }

    pub fn spawn_state_update_thread(
        &self,
        state_update_rx: crossbeam_channel::Receiver<state::Update>,
    ) -> anyhow::Result<()> {
        let window_ids = self.window_ids.clone();
        let conn = self.conn.clone();
        let state = self.state.clone();

        thread::spawn("eng-state", move || loop {
            while let Ok(state_update) = state_update_rx.recv() {
                {
                    let mut state = state.write().unwrap();
                    state.handle_state_update(state_update);
                }
                for window in window_ids.iter() {
                    xutils::send(
                        &conn,
                        &x::SendEvent {
                            destination: x::SendEventDest::Window(*window),
                            event_mask: x::EventMask::EXPOSURE,
                            propagate: false,
                            event: &x::ExposeEvent::new(*window, 0, 0, 1, 1, 1),
                        },
                    )?;
                }
            }
        })
    }

    fn handle_event(&mut self, event: &xcb::Event) -> anyhow::Result<()> {
        match event {
            xcb::Event::X(x::Event::Expose(event)) => {
                if let Some(window) = self.windows.get_mut(&event.window()) {
                    // Hack for now to distinguish on-demand expose.
                    if let Err(e) = window.render(event.width() != 1) {
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
                for window in self.windows.values() {
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

    pub fn run(&mut self) -> anyhow::Result<()> {
        match self.update_rx.take() {
            Some(update_rx) => {
                self.spawn_state_update_thread(update_rx)
                    .context("engine state update")?;
            }
            None => {
                return Err(anyhow::anyhow!("run() can be run only once"));
            }
        }
        loop {
            let event = xutils::get_event(&self.conn).context("failed getting an X event")?;
            match event {
                Some(event) => {
                    if let Err(e) = self.handle_event(&event) {
                        tracing::error!("Failed handling event {:?}, error: {:?}", event, e);
                    }
                }
                None => {
                    return Ok(());
                }
            }
        }
    }
}
