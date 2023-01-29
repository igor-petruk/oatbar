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

use anyhow::{anyhow, Context};
use xcb::x::{self, Atom, Window};
use xcb::Xid;

use crate::xutils::get_atom;
use crate::{state, thread, xutils};

use tracing::*;

#[derive(Debug)]
struct Workspaces {
    current: usize,
    names: Vec<String>,
}

impl Workspaces {
    fn to_state_update(self) -> state::Update {
        state::Update {
            entries: vec![
                state::UpdateEntry {
                    name: "workspace".into(),
                    var: "active".into(),
                    value: self.current.to_string(),
                    ..Default::default()
                },
                state::UpdateEntry {
                    name: "workspace".into(),
                    var: "value".into(),
                    value: self
                        .names
                        .get(self.current)
                        .unwrap_or(&"?".to_string())
                        .to_string(),
                    ..Default::default()
                },
                state::UpdateEntry {
                    name: "workspace".into(),
                    var: "variants".into(),
                    value: self.names.join(",").into(),
                    ..Default::default()
                },
            ],
            ..Default::default()
        }
    }
}

fn get_workspaces(
    root: Window,
    conn: &xcb::Connection,
    current: &Atom,
    names: &Atom,
) -> anyhow::Result<Workspaces> {
    let reply = xutils::get_property(&conn, root, current.clone(), x::ATOM_CARDINAL, 1)?;
    let current: u32 = *reply.value().get(0).ok_or(anyhow!("Empty reply"))?;
    let reply = xutils::get_property(&conn, root, names.clone(), x::ATOM_ANY, 1024)?;
    let buf: &[u8] = reply.value();
    let bufs = buf.split(|f| *f == 0);
    let utf8 = bufs
        .map(|buf| String::from_utf8_lossy(buf).into_owned())
        .filter(|s| !s.is_empty());
    Ok(Workspaces {
        current: current as usize,
        names: utf8.collect(),
    })
}

fn get_active_window_title(
    conn: &xcb::Connection,
    root: Window,
    active_window: &Atom,
    window_name: &Atom,
) -> anyhow::Result<String> {
    let reply = xutils::get_property(&conn, root, active_window.clone(), x::ATOM_WINDOW, 1)
        .context("Getting active window")?;
    let window: Option<&Window> = reply.value().get(0);
    if window.is_none() {
        tracing::warn!(
            "Unable to get active window (maybe temporarily): {:?}",
            reply
        );
        return Ok("".into());
    }
    let window = *window.unwrap();
    if window.resource_id() == 0 || window.resource_id() == u32::MAX {
        return Ok("".into());
    }
    // TODO: fix a negligible memory leak monitoring all windows ever active.
    // There is a finite number of them possible.
    xutils::send(
        &conn,
        &x::ChangeWindowAttributes {
            window,
            value_list: &[x::Cw::EventMask(x::EventMask::PROPERTY_CHANGE)],
        },
    )
    .context("Unable to monitor active window")?;
    let reply = xutils::get_property(&conn, window, window_name.clone(), x::ATOM_ANY, 1024)
        .context("Getting window title")?;
    let buf: &[u8] = reply.value();
    let title = String::from_utf8_lossy(&buf).into_owned();
    Ok(title)
}

fn send_title(title: String, tx: &crossbeam_channel::Sender<state::Update>) -> anyhow::Result<()> {
    let update = state::Update {
        entries: vec![state::UpdateEntry {
            name: "active_window".into(),
            var: "title".into(),
            value: title,
            ..Default::default()
        }],
        ..Default::default()
    };
    tx.send(update)?;
    Ok(())
}

pub struct EWMH {}

impl state::Source for EWMH {
    fn spawn(self, tx: crossbeam_channel::Sender<state::Update>) -> anyhow::Result<()> {
        let (conn, screen_num) =
            xcb::Connection::connect_with_xlib_display_and_extensions(&[], &[]).unwrap();

        let screen = {
            let setup = conn.get_setup();
            setup.roots().nth(screen_num as usize).unwrap()
        }
        .to_owned();

        xutils::send(
            &conn,
            &x::ChangeWindowAttributes {
                window: screen.root(),
                value_list: &[x::Cw::EventMask(x::EventMask::PROPERTY_CHANGE)],
            },
        )
        .context("Unable to monitor root window")?;

        let current_desktop = get_atom(&conn, "_NET_CURRENT_DESKTOP")?;
        let desktop_names = get_atom(&conn, "_NET_DESKTOP_NAMES")?;
        let active_window = get_atom(&conn, "_NET_ACTIVE_WINDOW")?;
        let window_name = get_atom(&conn, "_NET_WM_NAME")?;

        let workspaces = get_workspaces(screen.root(), &conn, &current_desktop, &desktop_names)?;
        tx.send(workspaces.to_state_update())?;

        let title = get_active_window_title(&conn, screen.root(), &active_window, &window_name)?;
        send_title(title, &tx)?;

        thread::spawn_loop("ewmh", move || {
            let event = match conn.wait_for_event() {
                Err(xcb::Error::Connection(xcb::ConnError::Connection)) => {
                    debug!(
                        "Exiting event thread gracefully: {}",
                        std::thread::current().name().unwrap_or("<unnamed>")
                    );
                    return Ok(false);
                }
                Err(err) => {
                    return Err(anyhow::anyhow!(
                        "unexpected error: {:#?}, {}",
                        err,
                        err.to_string()
                    ));
                }
                Ok(event) => event,
            };
            match event {
                xcb::Event::X(x::Event::PropertyNotify(ev)) => {
                    if ev.atom() == current_desktop || ev.atom() == desktop_names {
                        let workspaces =
                            get_workspaces(screen.root(), &conn, &current_desktop, &desktop_names)?;
                        info!("{:?}", workspaces);
                        tx.send(workspaces.to_state_update())?;
                    }
                    if ev.atom() == active_window || ev.atom() == window_name {
                        let title = get_active_window_title(
                            &conn,
                            screen.root(),
                            &active_window,
                            &window_name,
                        )?;
                        send_title(title, &tx)?;
                    }
                }
                _ => {
                    debug!("Unhandled XCB event: {:?}", event);
                }
            }
            conn.flush()?;
            Ok(true)
        })?;
        Ok(())
    }
}
