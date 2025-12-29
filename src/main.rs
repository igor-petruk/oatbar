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
// #[allow(unused)]
mod config;
mod drawing;
mod engine;
#[allow(unused)]
mod ipc;
mod ipcserver;
mod logging;
mod notify;
#[allow(unused_macros)]
mod parse;
mod process;
mod protocol;
mod source;
mod state;
mod thread;
mod timer;
#[cfg(feature = "wayland")]
mod wayland;
#[cfg(feature = "x11")]
mod wmready;
#[cfg(feature = "x11")]
mod x11;
#[cfg(feature = "x11")]
mod xrandr;
#[cfg(feature = "x11")]
mod xutils;

use clap::Parser;

#[derive(Parser)]
#[command(
    author, version,
    about = "Oatbar window manager bar",
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

    let _logging_guard = logging::init(&cli.instance_name)?;

    let config = config::load()?;
    let commands = config.commands.clone();

    let (ipc_server_tx, ipc_server_rx) = crossbeam_channel::unbounded();

    let mut state: state::State = state::State::new(config.clone(), vec![ipc_server_tx]);
    state.initialize_vars();

    let mut engine = engine::load(config, state, notify::Notifier::new())?;

    let mut poker = source::Poker::new();
    for (index, config) in commands.into_iter().enumerate() {
        let command = source::Command { index, config };
        let command_name = command.name();
        command.spawn(engine.update_tx().clone(), poker.add(command_name))?;
    }

    ipcserver::Server::spawn(
        &cli.instance_name,
        poker,
        engine.update_tx().clone(),
        ipc_server_rx,
    )?;

    #[cfg(feature = "profile")]
    std::thread::spawn(move || loop {
        if let Ok(report) = guard.report().build() {
            let file = std::fs::File::create("flamegraph.svg").unwrap();
            report.flamegraph(file).unwrap();
        };
        std::thread::sleep(std::time::Duration::from_secs(5));
    });

    engine.run()?;
    Ok(())
}
