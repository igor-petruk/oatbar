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

use std::sync::{Arc, RwLock};
use std::time::{Duration, SystemTime};

use crate::config::{Config, Placeholder, PlaceholderExt};
use crate::{state, timer, window};

pub struct Engine {
    pub state_update_tx: crossbeam_channel::Sender<state::Update>,
    state_update_rx: crossbeam_channel::Receiver<state::Update>,
    window: window::Window,
    state: Arc<RwLock<state::State>>,
    config: Config<Placeholder>,
}

impl Engine {
    pub fn new(config: Config<Placeholder>, initial_state: state::State) -> anyhow::Result<Self> {
        let (state_update_tx, state_update_rx) = crossbeam_channel::unbounded();

        let window = window::Window::create_and_show(config.clone())?;

        let state = Arc::new(RwLock::new(initial_state));
        Ok(Self {
            state_update_tx,
            state_update_rx,
            window,
            state,
            config,
        })
    }

    fn handle_state_update(&self, state_update: state::Update) -> anyhow::Result<()> {
        let mut state = self.state.write().unwrap();
        if let Some(prefix) = state_update.reset_prefix {
            state.vars.retain(|k, _| !k.starts_with(&prefix));
        }
        for update in state_update.entries.into_iter() {
            let var = match update.instance {
                Some(instance) => format!("{}.{}.{}", update.name, instance, update.var),
                None => format!("{}.{}", update.name, update.var),
            };
            state.vars.insert(var, update.value);
        }

        for var in self.config.vars.values() {
            let var_value = var.input.resolve_placeholders(&state.vars)?;
            let processed = if let Some(enum_separator) = &var.enum_separator {
                let vec: Vec<_> = var_value
                    .split(enum_separator)
                    .map(|s| var.process(s))
                    .collect();
                vec.join(enum_separator)
            } else {
                var.process(&var_value)
            };
            state.vars.insert(var.name.clone(), processed);
        }
        Ok(())
    }

    fn handle_mouse_motion(&self, s: &window::ScreenMouseMoved) -> anyhow::Result<()> {
        if !s.edge_entered {
            return Ok(());
        }
        let reset_timer_at = SystemTime::now()
            .checked_add(Duration::from_secs(2))
            .unwrap();
        let mut state = self.state.write().expect("RwLock");
        match &state.show_panel_timer {
            Some(timer) => {
                timer.set_at(reset_timer_at);
            }
            None => {
                let timer = {
                    let state = self.state.clone();
                    timer::Timer::new("autohide-timer", reset_timer_at, move || {
                        let mut state = state.write().expect("RwLock");
                        state.show_panel_timer = None;
                        tracing::info!("DONE!");
                    })?
                };
                state.show_panel_timer = Some(timer.clone());
            }
        };

        Ok(())
    }

    pub fn run(&self) -> anyhow::Result<()> {
        loop {
            crossbeam_channel::select! {
                recv(self.window.rx_events) -> msg => match msg {
                    Ok(window::Event::Exposed) => {
                        let state = self.state.read().expect("RwLock");
                        self.window.render(&state)?;
                    },
                    Ok(window::Event::ScreenMouseMoved(s)) => {
                        self.handle_mouse_motion(&s)?;
                   }
                    Err(e) => return Err(anyhow::anyhow!("Unexpected exit of engine incoming channel: {:?}", e)),
                },
                recv(self.state_update_rx) -> msg => match msg {
                    Ok(state_update) => {
                        self.handle_state_update(state_update)?;
                        let state = self.state.read().expect("RwLock");
                        self.window.render(&state)?;
                    },
                    Err(e) => return Err(anyhow::anyhow!("Unexpected exit of window incoming channel: {:?}", e)),
                },
            }
        }
    }
}
