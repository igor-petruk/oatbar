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
    windows: HashMap<x::Window, window::Window>,
    window_ids: Vec<x::Window>,
    state: Arc<RwLock<state::State>>,
    conn: Arc<xcb::Connection>,
    screen: x::ScreenBuf,
}

impl Engine {
    pub fn new(config: Config<Placeholder>, initial_state: state::State) -> anyhow::Result<Self> {
        let state = Arc::new(RwLock::new(initial_state));

        let (conn, _) = xcb::Connection::connect_with_xlib_display_and_extensions(
            &[xcb::Extension::Input, xcb::Extension::Shape],
            &[],
        )
        .unwrap();
        let conn = Arc::new(conn);

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

        for bar in config.bar.iter() {
            let window = window::Window::create_and_show(bar.clone(), conn.clone(), state.clone())?;
            windows.insert(window.id, window);
        }

        let window_ids = windows.keys().cloned().collect();

        Ok(Self {
            windows,
            window_ids,
            state,
            conn,
            screen,
        })
    }

    pub fn spawn_state_update_thread(
        &self,
        state_update_rx: std::sync::mpsc::Receiver<state::Update>,
    ) -> anyhow::Result<()> {
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

    pub fn run(
        &mut self,
        state_update_rx: std::sync::mpsc::Receiver<state::Update>,
    ) -> anyhow::Result<()> {
        self.spawn_state_update_thread(state_update_rx)
            .context("engine state update")?;

        loop {
            let event = xutils::get_event(&self.conn)?;
            match event {
                Some(xcb::Event::X(x::Event::Expose(ev))) => {
                    if let Some(window) = self.windows.get_mut(&ev.window()) {
                        window.render()?;
                    }
                }
                Some(xcb::Event::Input(xinput::Event::RawMotion(_event))) => {
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
