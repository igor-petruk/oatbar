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

use anyhow::{anyhow, Context};
use protocol::i3bar;
use std::collections::BTreeMap;
use xcb::x::{self, Atom, Window};
use xcb::Xid;
use xutils::get_atom;

use tracing::*;

#[derive(Debug)]
struct Workspaces {
    current: usize,
    names: Vec<String>,
}

fn print_update(workspaces: &Workspaces, title: &str) -> anyhow::Result<()> {
    let workspace_value = workspaces
        .names
        .get(workspaces.current)
        .unwrap_or(&"?".to_string())
        .to_string();
    let mut other = BTreeMap::new();
    other.insert("active".into(), workspaces.current.into());
    other.insert("variants".into(), workspaces.names.join(",").into());
    other.insert("value".into(), workspace_value.clone().into());
    let mut title_other = BTreeMap::new();
    title_other.insert("value".into(), title.into());
    let blocks = vec![
        i3bar::Block {
            full_text: format!("workspace: {}", workspace_value),
            name: Some("workspace".into()),
            instance: None,
            other,
        },
        i3bar::Block {
            name: Some("window_title".into()),
            full_text: format!("window: {}", title),
            other: title_other,
            ..Default::default()
        },
    ];
    println!("{},", serde_json::to_string(&blocks)?);
    Ok(())
}

fn get_workspaces(
    root: Window,
    conn: &xcb::Connection,
    current: &Atom,
    names: &Atom,
) -> anyhow::Result<Workspaces> {
    let reply = xutils::get_property(conn, root, *current, x::ATOM_CARDINAL, 1)?;
    let current: u32 = *reply
        .value()
        .first()
        .ok_or_else(|| anyhow!("Empty reply"))?;
    let reply = xutils::get_property(conn, root, *names, x::ATOM_ANY, 1024)?;
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

fn set_current_workspace(
    root: Window,
    conn: &xcb::Connection,
    current: &Atom,
    current_value: u32,
) -> anyhow::Result<()> {
    xutils::send(
        conn,
        &x::SendEvent {
            propagate: false,
            destination: x::SendEventDest::Window(root),
            event_mask: x::EventMask::all(),
            event: &x::ClientMessageEvent::new(
                root,
                *current,
                x::ClientMessageData::Data32([current_value, 0, 0, 0, 0]),
            ),
        },
    )?;
    Ok(())
}

fn get_active_window_title(
    conn: &xcb::Connection,
    root: Window,
    active_window: &Atom,
    window_name: &Atom,
) -> anyhow::Result<String> {
    let reply = xutils::get_property(conn, root, *active_window, x::ATOM_WINDOW, 1)
        .context("Getting active window")?;
    let window: Option<&Window> = reply.value().first();
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
        conn,
        &x::ChangeWindowAttributes {
            window,
            value_list: &[x::Cw::EventMask(x::EventMask::PROPERTY_CHANGE)],
        },
    )
    .context("Unable to monitor active window")?;
    let reply = xutils::get_property(conn, window, *window_name, x::ATOM_ANY, 1024)
        .context("Getting window title")?;
    let buf: &[u8] = reply.value();
    let title = String::from_utf8_lossy(buf).into_owned();
    Ok(title)
}

fn main() -> anyhow::Result<()> {
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

    let args: Vec<String> = std::env::args().collect();
    if let Some(workspace) = args.get(1) {
        let workspace = workspace.parse()?;
        set_current_workspace(screen.root(), &conn, &current_desktop, workspace)?;
        return Ok(());
    }

    println!("{}", serde_json::to_string(&i3bar::Header::default())?);
    println!("[");

    let mut workspaces = get_workspaces(screen.root(), &conn, &current_desktop, &desktop_names)?;
    let mut title = get_active_window_title(&conn, screen.root(), &active_window, &window_name)?;
    print_update(&workspaces, &title)?;

    loop {
        let event = match conn.wait_for_event() {
            Err(xcb::Error::Connection(xcb::ConnError::Connection)) => {
                debug!(
                    "Exiting event thread gracefully: {}",
                    std::thread::current().name().unwrap_or("<unnamed>")
                );
                return Ok(());
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
                    workspaces =
                        get_workspaces(screen.root(), &conn, &current_desktop, &desktop_names)?;
                    print_update(&workspaces, &title)?;
                }
                if ev.atom() == active_window || ev.atom() == window_name {
                    title = get_active_window_title(
                        &conn,
                        screen.root(),
                        &active_window,
                        &window_name,
                    )?;
                    print_update(&workspaces, &title)?;
                }
            }
            _ => {
                debug!("Unhandled XCB event: {:?}", event);
            }
        }
        conn.flush()?;
    }
}
