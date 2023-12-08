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
#[allow(unused)]
mod xutils;

use anyhow::anyhow;
use protocol::i3bar;
use std::collections::BTreeMap;
use tracing::*;
use xcb::{
    x,
    xkb::{self, StatePart},
};

#[derive(Debug)]
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

fn get_current_state(conn: &xcb::Connection, group: xkb::Group) -> anyhow::Result<KeyboardState> {
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

fn main() -> anyhow::Result<()> {
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

    let args: Vec<String> = std::env::args().collect();
    if let Some(layout) = args.get(1) {
        let layout: usize = layout.parse()?;
        let group = match layout {
            0 => xkb::Group::N1,
            1 => xkb::Group::N2,
            2 => xkb::Group::N3,
            _ => xkb::Group::N4,
        };
        xutils::send(
            &conn,
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
