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
#[allow(unused)]
mod config;
mod drawing;
mod engine;
mod protocol;
mod source;
mod state;
mod thread;
mod timer;
mod window;
mod wmready;
mod xutils;

use anyhow::Context;

use crate::state::Source;

fn main() -> anyhow::Result<()> {
    #[cfg(feature = "profile")]
    let guard = pprof::ProfilerGuardBuilder::default()
        .frequency(100)
        .blocklist(&["libc", "libgcc", "pthread", "vdso"])
        .build()
        .unwrap();

    let sub = tracing_subscriber::fmt().compact().with_thread_names(true);

    #[cfg(debug_assertions)]
    let sub = sub.with_max_level(tracing::Level::TRACE);

    sub.init();

    let config = config::load()?;
    let commands = config.commands.clone();

    wmready::wait().context("Unable to connect to WM")?;

    let state: state::State = state::State::new(config.clone());
    let (state_update_tx, state_update_rx) = std::sync::mpsc::channel();

    let mut engine = engine::Engine::new(config, state)?;

    for (index, config) in commands.into_iter().enumerate() {
        let command = source::Command { index, config };
        command.spawn(state_update_tx.clone())?;
    }

    #[cfg(feature = "profile")]
    std::thread::spawn(move || loop {
        if let Ok(report) = guard.report().build() {
            let file = std::fs::File::create("flamegraph.svg").unwrap();
            report.flamegraph(file).unwrap();
        };
        std::thread::sleep(std::time::Duration::from_secs(5));
    });

    engine.run(state_update_rx)?;
    Ok(())
}
