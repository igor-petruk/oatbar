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

mod bar;
mod clock;
#[allow(unused)]
mod config;
mod engine;
mod ewmh;
mod keyboard;
mod source;
mod state;
mod thread;
mod window;
mod wm;
mod xutils;

use anyhow::Context;

use crate::state::Source;

fn main() -> anyhow::Result<()> {
    let sub = tracing_subscriber::fmt().compact().with_thread_names(true);

    #[cfg(debug_assertions)]
    let sub = sub.with_max_level(tracing::Level::TRACE);

    sub.init();

    let config = config::load()?;
    let i3bars = config.i3bars.clone();
    let commands = config.commands.clone();
    let clock_format = config.bar.clock_format.clone();

    wm::wait_ready().context("Unable to connect to WM")?;

    let state: state::State = Default::default();
    let engine = engine::Engine::new(config, state)?;
    let state_update_tx = engine.state_update_tx.clone();
    let layout = keyboard::Layout {};
    layout.spawn(state_update_tx.clone())?;

    for (index,config) in i3bars.into_iter().enumerate() {
        let i3bar = source::I3Bar { index, config };
        i3bar.spawn(state_update_tx.clone())?;
    }
    for (index,config) in commands.into_iter().enumerate() {
        let command = source::Command { index, config };
        command.spawn(state_update_tx.clone())?;
    }
    let ewmh = ewmh::EWMH {};
    ewmh.spawn(state_update_tx.clone())?;

    let clock = clock::Clock {
        format: clock_format,
    };
    clock.spawn(state_update_tx)?;
    engine.run()?;
    Ok(())
}
