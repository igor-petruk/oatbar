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

#[macro_use]
extern crate macro_rules_attribute;

mod bar;
#[allow(unused)]
mod config;
mod drawing;
mod engine;
#[allow(unused)]
mod ipc;
mod ipcserver;
#[allow(unused_macros)]
mod parse;
mod process;
mod protocol;
mod source;
mod state;
mod thread;
mod timer;
mod window;
mod wmready;
mod xrandr;
mod xutils;

use clap::Parser;

#[derive(Parser)]
#[command(
    author, version,
    about = "Oatbar window manager ber",
    long_about = None)]
#[command(propagate_version = true)]
struct Cli {
    /// Unique name of the oatbar server instance.
    #[arg(long, default_value = "oatbar")]
    instance_name: String,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

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

    let (ipc_server_tx, ipc_server_rx) = crossbeam_channel::unbounded();

    let state: state::State = state::State::new(config.clone(), vec![ipc_server_tx]);
    let (state_update_tx, state_update_rx) = crossbeam_channel::unbounded();

    let mut engine = engine::Engine::new(config, state)?;

    let mut poker = source::Poker::new();
    for (index, config) in commands.into_iter().enumerate() {
        let command = source::Command { index, config };
        command.spawn(state_update_tx.clone(), poker.add())?;
    }

    ipcserver::Server::spawn(&cli.instance_name, poker, state_update_tx, ipc_server_rx)?;

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
