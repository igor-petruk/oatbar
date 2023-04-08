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

use crate::config::{Config, Placeholder};
use crate::{state, thread, window, xutils};

pub struct Engine {
    pub state_update_tx: crossbeam_channel::Sender<state::Update>,
    state_update_rx: crossbeam_channel::Receiver<state::Update>,
    windows: HashMap<x::Window, window::Window>,
    window_ids: Vec<x::Window>,
    state: Arc<RwLock<state::State>>,
    conn: Arc<xcb::Connection>,
}

impl Engine {
    pub fn new(config: Config<Placeholder>, initial_state: state::State) -> anyhow::Result<Self> {
        let state = Arc::new(RwLock::new(initial_state));

        let (state_update_tx, state_update_rx) = crossbeam_channel::unbounded();

        let (conn, _) = xcb::Connection::connect_with_xlib_display_and_extensions(
            &[xcb::Extension::Input],
            &[],
        )
        .unwrap();
        let conn = Arc::new(conn);
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

        let window = window::Window::create_and_show(
            config.bar.get(0).unwrap().clone(),
            conn.clone(),
            state.clone(),
        )?;
        windows.insert(window.id, window);

        let window_ids = windows.keys().cloned().collect();
        //  let window_control = window.window_control();

        Ok(Self {
            state_update_tx,
            state_update_rx,
            windows,
            window_ids,
            state,
            conn,
        })
    }

    /*
        fn popup_bar(&self, state: &mut state::State) -> anyhow::Result<()> {
            if state.autohide_bar_visible || !self.config.bar.get(0).unwrap().autohide {
                return Ok(());
            }
            let reset_timer_at = SystemTime::now()
                .checked_add(Duration::from_secs(1))
                .unwrap();
            match &state.show_panel_timer {
                Some(timer) => {
                    timer.set_at(reset_timer_at);
                }
                None => {
                    /*
                    let timer = {
                        let window_control = self.window_control.clone();
                        window_control.set_visible(true)?;
                        let state = self.state.clone();
                        timer::Timer::new("autohide-timer", reset_timer_at, move || {
                            let mut state = state.write().expect("RwLock");
                            state.show_panel_timer = None;
                            window_control.set_visible(false).expect("autohide-hide");
                        })?
                    };
                    state.show_panel_timer = Some(timer);
                    */
                }
            }

            Ok(())
        }
    fn handle_mouse_motion(&self, s: &window::ScreenMouseMoved) -> anyhow::Result<()> {
        if !self.config.bar.get(0).unwrap().autohide {
            return Ok(());
        }
        let mut state = self.state.write().expect("RwLock");
        if !state.autohide_bar_visible && s.over_edge {
            state.autohide_bar_visible = true;
        //       self.window_control.set_visible(true)?;
        } else if state.autohide_bar_visible && !s.over_window {
            state.autohide_bar_visible = false;
            //     self.window_control.set_visible(false)?;
        }

        Ok(())
    }
    */

    pub fn spawn_state_update_thread(&self) -> anyhow::Result<()> {
        let state_update_rx = self.state_update_rx.clone();
        let window_ids = self.window_ids.clone();
        let conn = self.conn.clone();
        let state = self.state.clone();

        thread::spawn("eng-state", move || loop {
            while let Ok(state_update) = state_update_rx.recv() {
                {
                    let mut state = state.write().unwrap();
                    state.handle_state_update(state_update)?;
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

    pub fn run(&mut self) -> anyhow::Result<()> {
        self.spawn_state_update_thread()
            .context("engine state update")?;

        loop {
            let event = xutils::get_event(&self.conn)?;
            match event {
                Some(xcb::Event::X(x::Event::Expose(ev))) => {
                    if let Some(window) = self.windows.get(&ev.window()) {
                        window.render()?;
                    }
                }
                Some(xcb::Event::Input(xinput::Event::RawMotion(_))) => {}
                None => {
                    return Ok(());
                }
                _ => {
                    tracing::debug!("Unhandled XCB event: {:?}", event);
                }
            }
        }
    }
}
