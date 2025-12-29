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

mod protocol;
#[cfg(feature = "x11")]
#[allow(unused)]
mod xutils;

use anyhow::anyhow;
use clap::{Parser, Subcommand};
use protocol::i3bar;
use std::collections::BTreeMap;
use tracing::*;

#[derive(Debug, Clone)]
struct KeyboardState {
    current: usize,
    variants: Vec<String>,
    indicators: BTreeMap<String, bool>,
}

fn to_indicator_name(s: &str) -> String {
    let mut result = String::with_capacity(s.len() * 2);
    for (i, ch) in s.char_indices() {
        if ch.is_whitespace() {
            continue;
        }
        if ch.is_uppercase() {
            if i > 0 {
                result.push('_');
            }
            result.push(ch.to_ascii_lowercase());
        } else {
            result.push(ch);
        }
    }
    result
}

fn state_to_blocks(state: KeyboardState) -> Vec<i3bar::Block> {
    let mut result = Vec::with_capacity(state.indicators.len() + 1);

    let value = state
        .variants
        .get(state.current)
        .unwrap_or(&"?".to_string())
        .to_string();
    let mut other = BTreeMap::new();
    other.insert("active".into(), state.current.into());
    other.insert("variants".into(), state.variants.join(",").into());
    other.insert("value".into(), value.clone().into());
    result.push(i3bar::Block {
        name: Some("layout".into()),
        full_text: format!("layout: {}", value),
        instance: None,
        other,
    });

    let indicator_blocks: Vec<_> = state
        .indicators
        .into_iter()
        .map(|(k, v)| {
            let value = if v { "on" } else { "off" };
            let mut other = BTreeMap::new();
            other.insert("value".into(), value.into());
            i3bar::Block {
                name: Some("indicator".into()),
                full_text: format!("{}:{:>3}", k, value),
                instance: Some(k),
                other,
            }
        })
        .collect();
    result.extend_from_slice(&indicator_blocks);

    result
}

#[derive(Parser)]
#[command(
    author, version,
    about = "Keyboard util for oatbar",
    long_about = None)]
#[command(propagate_version = true)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum LayoutSubcommand {
    /// Set a keyboard layout.
    Set {
        /// Layout index as returned by oatbar-keyboard stream.
        layout: usize,
    },
}

#[derive(Subcommand)]
enum Commands {
    /// Work with keyboard layouts.
    Layout {
        #[clap(subcommand)]
        layout_cmd: LayoutSubcommand,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DisplayServer {
    Wayland,
    X11,
}

fn detect_display_server() -> Option<DisplayServer> {
    if std::env::var("WAYLAND_DISPLAY").is_ok() {
        Some(DisplayServer::Wayland)
    } else if std::env::var("DISPLAY").is_ok() {
        Some(DisplayServer::X11)
    } else {
        None
    }
}

// ============================================================================
// X11 Implementation
// ============================================================================

#[cfg(feature = "x11")]
mod x11_impl {
    use super::*;
    use xcb::{
        x,
        xkb::{self, StatePart},
    };

    fn get_current_state(
        conn: &xcb::Connection,
        group: xkb::Group,
    ) -> anyhow::Result<KeyboardState> {
        let reply = xutils::query(
            conn,
            &xkb::GetNames {
                device_spec: xkb::Id::UseCoreKbd as xkb::DeviceSpec,
                which: xkb::NameDetail::SYMBOLS,
            },
        )?;
        let one_value = reply
            .value_list()
            .first()
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("More than one value"))?;
        let atom_name = if let xkb::GetNamesReplyValueList::Symbols(atom) = one_value {
            let reply = xutils::query(conn, &x::GetAtomName { atom })?;
            Ok(reply.name().to_utf8().to_string())
        } else {
            Err(anyhow::anyhow!("Unexpected reply type"))
        }?;

        let variants: Vec<String> = atom_name
            .split('+')
            .filter(|s| !s.contains('('))
            .map(|s| s.split(':').next().unwrap())
            .filter(|s| s != &"pc")
            .map(String::from)
            .collect();

        let layout_index = match group {
            xkb::Group::N1 => 0,
            xkb::Group::N2 => 1,
            xkb::Group::N3 => 2,
            xkb::Group::N4 => 3,
        };

        let indicator_atoms = get_indicator_atoms(conn)?;
        let mut indicators = BTreeMap::new();
        for atom in indicator_atoms {
            let reply = xutils::query(conn, &x::GetAtomName { atom })?;
            let name = to_indicator_name(&reply.name().to_utf8());
            let reply = xutils::query(
                conn,
                &xkb::GetNamedIndicator {
                    device_spec: xkb::Id::UseCoreKbd as xkb::DeviceSpec,
                    indicator: atom,
                    led_class: xkb::LedClass::KbdFeedbackClass,
                    led_id: 0,
                },
            )?;
            indicators.insert(name, reply.on());
        }

        debug!(
            "atom={},layout_index={},indicators={:?}",
            atom_name, layout_index, indicators
        );

        Ok(KeyboardState {
            current: layout_index,
            variants,
            indicators,
        })
    }

    fn get_indicator_atoms(conn: &xcb::Connection) -> anyhow::Result<Vec<x::Atom>> {
        let reply = xutils::query(
            conn,
            &xkb::GetNames {
                device_spec: xkb::Id::UseCoreKbd as xkb::DeviceSpec,
                which: xkb::NameDetail::INDICATOR_NAMES,
            },
        )?;
        let one_value = reply
            .value_list()
            .first()
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("More than one value"))?;
        if let xkb::GetNamesReplyValueList::IndicatorNames(atoms) = one_value {
            Ok(atoms)
        } else {
            Err(anyhow::anyhow!("Unexpected reply type"))
        }
    }

    fn handle_set_layout(conn: &xcb::Connection, layout: usize) -> anyhow::Result<()> {
        let group = match layout {
            0 => xkb::Group::N1,
            1 => xkb::Group::N2,
            2 => xkb::Group::N3,
            _ => xkb::Group::N4,
        };
        xutils::send(
            conn,
            &xkb::LatchLockState {
                device_spec: xkb::Id::UseCoreKbd as xkb::DeviceSpec,
                group_lock: group,
                lock_group: true,
                latch_group: false,
                group_latch: 0,
                affect_mod_locks: x::ModMask::empty(),
                affect_mod_latches: x::ModMask::empty(),
                mod_locks: x::ModMask::empty(),
            },
        )?;
        Ok(())
    }

    pub fn run(command: Option<Commands>) -> anyhow::Result<()> {
        let (conn, _) =
            xcb::Connection::connect_with_xlib_display_and_extensions(&[xcb::Extension::Xkb], &[])?;

        let xkb_ver = xutils::query(
            &conn,
            &xkb::UseExtension {
                wanted_major: 1,
                wanted_minor: 0,
            },
        )?;

        if !xkb_ver.supported() {
            return Err(anyhow!("xkb-1.0 is not supported"));
        }

        if let Some(command) = command {
            match command {
                Commands::Layout { layout_cmd } => match layout_cmd {
                    LayoutSubcommand::Set { layout } => handle_set_layout(&conn, layout)?,
                },
            }
            return Ok(());
        }

        let events = xkb::EventType::NEW_KEYBOARD_NOTIFY | xkb::EventType::STATE_NOTIFY;
        xutils::send(
            &conn,
            &xkb::SelectEvents {
                device_spec: xkb::Id::UseCoreKbd as xkb::DeviceSpec,
                affect_which: events,
                clear: xkb::EventType::empty(),
                select_all: events,
                affect_map: xkb::MapPart::empty(),
                map: xkb::MapPart::empty(),
                details: &[],
            },
        )?;

        let reply = xutils::query(
            &conn,
            &xkb::GetState {
                device_spec: xkb::Id::UseCoreKbd as xkb::DeviceSpec,
            },
        )?;

        println!("{}", serde_json::to_string(&i3bar::Header::default())?);
        println!("[");

        let state = get_current_state(&conn, reply.group())?;
        debug!("Initial: {:?}", state);
        println!("{},", serde_json::to_string(&state_to_blocks(state))?);

        loop {
            let event = xutils::get_event(&conn)?;
            match event {
                Some(xcb::Event::Xkb(xkb::Event::StateNotify(n))) => {
                    if n.changed().contains(StatePart::GROUP_STATE)
                        || n.changed().contains(StatePart::MODIFIER_LOCK)
                    {
                        let state = get_current_state(&conn, n.group())?;
                        debug!("State updated: {:?}", state);
                        println!("{},", serde_json::to_string(&state_to_blocks(state))?);
                    }
                }
                None => return Ok(()),
                _ => {
                    debug!("Unhandled XCB event: {:?}", event);
                }
            }
        }
    }
}

// ============================================================================
// Sway Implementation
// ============================================================================

#[cfg(feature = "wayland")]
mod sway_impl {
    use super::*;
    use swayipc::{Connection as SwayConnection, EventType};

    pub fn run(command: Option<Commands>) -> anyhow::Result<()> {
        // Create a connection for commands/queries
        let mut conn = SwayConnection::new()?;

        if let Some(command) = command {
            match command {
                Commands::Layout { layout_cmd } => match layout_cmd {
                    LayoutSubcommand::Set { layout } => {
                        // Find keyboards and set layout
                        let inputs = conn.get_inputs()?;
                        for input in inputs {
                            if input.input_type == "keyboard" {
                                conn.run_command(format!(
                                    "input {} xkb_switch_layout {}",
                                    input.identifier, layout
                                ))?;
                            }
                        }
                    }
                },
            }
            return Ok(());
        }

        println!("{}", serde_json::to_string(&i3bar::Header::default())?);
        println!("[");

        // Helper to get current state from Sway inputs
        let get_state = |conn: &mut SwayConnection| -> anyhow::Result<Option<KeyboardState>> {
            let inputs = conn.get_inputs()?;
            // Use the first keyboard that has layouts configured
            for input in inputs {
                if input.input_type == "keyboard" && !input.xkb_layout_names.is_empty() {
                    let current = input.xkb_active_layout_index.unwrap_or(0) as usize;
                    // Sway doesn't give us indicator state easily via IPC without polling or extra complexity
                    // For now, we omit indicators or implementing them would require creating an input device monitor
                    let indicators = BTreeMap::new();

                    return Ok(Some(KeyboardState {
                        current,
                        variants: input.xkb_layout_names,
                        indicators,
                    }));
                }
            }
            Ok(None)
        };

        if let Some(state) = get_state(&mut conn)? {
            println!("{},", serde_json::to_string(&state_to_blocks(state))?);
        }

        let subs = [EventType::Input];
        let event_conn = SwayConnection::new()?.subscribe(subs)?;

        for event in event_conn {
            match event {
                Ok(swayipc::Event::Input(input_event)) => {
                    if matches!(
                        input_event.change,
                        swayipc::InputChange::XkbKeymap | swayipc::InputChange::XkbLayout
                    ) {
                        if let Some(state) = get_state(&mut conn)? {
                            println!("{},", serde_json::to_string(&state_to_blocks(state))?);
                        }
                    }
                }
                Err(e) => {
                    tracing::error!("Sway IPC error: {}", e);
                    break;
                }
                _ => {}
            }
        }

        Ok(())
    }
}

// ============================================================================
// Hyprland Implementation
// ============================================================================

#[cfg(feature = "wayland")]
mod hyprland_impl {
    use super::*;
    use anyhow::Context;
    use hyprland::{ctl::switch_xkb_layout, event_listener::EventListener};
    use serde::Deserialize;
    use std::process::Command;

    #[derive(Debug, Deserialize)]
    struct HyprlandKeyboard {
        name: String,
        main: bool,
        layout: String,
        active_layout_index: usize,
    }

    fn get_keyboard() -> anyhow::Result<HyprlandKeyboard> {
        let output = Command::new("hyprctl")
            .arg("devices")
            .arg("-j")
            .output()
            .context("Failed to execute hyprctl")?;

        if !output.status.success() {
            return Err(anyhow::anyhow!(
                "hyprctl failed with status: {}",
                output.status
            ));
        }

        let devices: serde_json::Value = serde_json::from_slice(&output.stdout)
            .context("Failed to parse hyprctl JSON output")?;

        let keyboards: Vec<HyprlandKeyboard> =
            serde_json::from_value(devices["keyboards"].clone())
                .context("Failed to parse keyboards from hyprctl output")?;

        keyboards
            .into_iter()
            .find(|kbd| kbd.main)
            .ok_or_else(|| anyhow!("No main keyboard found"))
    }

    fn get_state() -> anyhow::Result<KeyboardState> {
        let keyboard = get_keyboard()?;
        let variants: Vec<String> = keyboard.layout.split(',').map(String::from).collect();
        let current = keyboard.active_layout_index;
        let indicators = BTreeMap::new(); // Hyprland does not expose this yet.
        Ok(KeyboardState {
            current,
            variants,
            indicators,
        })
    }

    pub fn run(command: Option<Commands>) -> anyhow::Result<()> {
        if let Some(command) = command {
            match command {
                Commands::Layout { layout_cmd } => match layout_cmd {
                    LayoutSubcommand::Set { layout } => {
                        let keyboard = get_keyboard()?;
                        switch_xkb_layout::call(
                            &keyboard.name,
                            switch_xkb_layout::SwitchXKBLayoutCmdTypes::Id(layout as u8),
                        )?;
                    }
                },
            }
            return Ok(());
        }

        println!("{}", serde_json::to_string(&i3bar::Header::default())?);
        println!("[");

        let initial_state = get_state()?;
        println!(
            "{},",
            serde_json::to_string(&state_to_blocks(initial_state))?
        );

        let mut event_listener = EventListener::new();
        event_listener.add_layout_changed_handler(|_| {
            if let Ok(state) = get_state() {
                if let Ok(line) = serde_json::to_string(&state_to_blocks(state)) {
                    println!("{},", line);
                }
            }
        });

        event_listener
            .start_listener()
            .context("Failed to start Hyprland event listener")
    }
}

// ============================================================================
// Main
// ============================================================================

#[cfg(feature = "wayland")]
fn is_sway() -> bool {
    std::env::var("SWAYSOCK").is_ok()
}

#[cfg(feature = "wayland")]
fn is_hyprland() -> bool {
    std::env::var("HYPRLAND_INSTANCE_SIGNATURE").is_ok()
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match detect_display_server() {
        Some(DisplayServer::Wayland) => {
            #[cfg(feature = "wayland")]
            {
                if is_sway() {
                    tracing::info!("Detected Sway, using swayipc");
                    sway_impl::run(cli.command)
                } else if is_hyprland() {
                    tracing::info!("Detected Hyprland, using hyprland-rs");
                    hyprland_impl::run(cli.command)
                } else {
                    anyhow::bail!(
                        "Generic Wayland keyboard layout management not implemented. Use Sway or Hyprland."
                    );
                }
            }
            #[cfg(not(feature = "wayland"))]
            {
                anyhow::bail!("Detected Wayland but 'wayland' feature is disabled");
            }
        }
        Some(DisplayServer::X11) | None => {
            #[cfg(feature = "x11")]
            {
                tracing::info!("Using X11 backend");
                x11_impl::run(cli.command)
            }
            #[cfg(not(feature = "x11"))]
            {
                // If X11 is not enabled, try Wayland as fallback if enabled
                #[cfg(feature = "wayland")]
                {
                    tracing::info!("X11 disabled, trying Wayland backend");
                    if is_sway() {
                        sway_impl::run(cli.command)
                    } else if is_hyprland() {
                        hyprland_impl::run(cli.command)
                    } else {
                        anyhow::bail!(
                            "Generic Wayland keyboard layout management not implemented. Use Sway or Hyprland."
                        );
                    }
                }
                #[cfg(not(feature = "wayland"))]
                {
                    anyhow::bail!(
                        "No suitable backend enabled. Please enable 'x11' or 'wayland' feature."
                    );
                }
            }
        }
    }
}
