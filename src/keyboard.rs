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

use crate::{state, thread, xutils};
use xcb::{
    x,
    xkb::{self, StatePart},
};

use anyhow::anyhow;
use tracing::*;

#[derive(Debug)]
struct LayoutState {
    current: usize,
    variants: Vec<String>,
}

fn get_current_layout(conn: &xcb::Connection, group: xkb::Group) -> anyhow::Result<LayoutState> {
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
    debug!("atom={},layout_index={}", atom_name, layout_index);

    Ok(LayoutState {
        current: layout_index,
        variants,
    })
}

fn layout_to_state_update(layout: LayoutState) -> state::Update {
    state::Update {
        entries: vec![
            state::UpdateEntry {
                name: "layout".into(),
                var: "value".into(),
                value: layout
                    .variants
                    .get(layout.current)
                    .unwrap_or(&"?".to_string())
                    .to_string(),
                ..Default::default()
            },
            state::UpdateEntry {
                name: "layout".into(),
                var: "active".into(),
                value: layout.current.to_string(),
                ..Default::default()
            },
            state::UpdateEntry {
                name: "layout".into(),
                var: "variants".into(),
                value: layout.variants.join(","),
                ..Default::default()
            },
        ],
        ..Default::default()
    }
}
pub struct Layout {}

impl state::Source for Layout {
    fn spawn(self, tx: crossbeam_channel::Sender<state::Update>) -> anyhow::Result<()> {
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
        let layout = get_current_layout(&conn, reply.group())?;
        tx.send(layout_to_state_update(layout))?;

        thread::spawn_loop("layout", move || {
            let event = xutils::get_event(&conn)?;
            match event {
                Some(xcb::Event::Xkb(xkb::Event::StateNotify(n))) => {
                    if n.changed().contains(StatePart::GROUP_STATE) {
                        let layout = get_current_layout(&conn, n.group())?;
                        debug!("Layout updated: {:?}", layout);
                        tx.send(layout_to_state_update(layout))?;
                    }
                }
                None => return Ok(false),
                _ => {
                    debug!("Unhandled XCB event: {:?}", event);
                }
            }
            Ok(true)
        })?;

        Ok(())
    }
}
