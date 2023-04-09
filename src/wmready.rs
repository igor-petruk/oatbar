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

use anyhow::anyhow;
use xcb::x;

use crate::xutils;
use tracing::*;

fn validate_wm(
    conn: &xcb::Connection,
    screen: &x::Screen,
    wm_support_atom: x::Atom,
    wm_name: x::Atom,
    wm_supported: x::Atom,
) -> anyhow::Result<String> {
    let reply = xutils::get_property(conn, screen.root(), wm_support_atom, x::ATOM_WINDOW, 2)?;

    let wm_window = reply
        .value::<x::Window>()
        .get(0)
        .ok_or_else(|| anyhow!("Failed to find wm window"))?;

    let reply = xutils::get_property(conn, *wm_window, wm_name, x::ATOM_ANY, 256)?;
    let wm_name = String::from_utf8_lossy(reply.value::<u8>());

    let reply = xutils::get_property(conn, screen.root(), wm_supported, x::ATOM_ATOM, 4096)?;

    info!("Supported EWMH: {:?}", reply);

    Ok(wm_name.into_owned())
}

fn refetch_atoms(conn: &xcb::Connection) -> anyhow::Result<(x::Atom, x::Atom, x::Atom)> {
    let wm_support_atom = xutils::get_atom(conn, "_NET_SUPPORTING_WM_CHECK")?;
    let wm_name = xutils::get_atom(conn, "_NET_WM_NAME")?;
    let wm_supported = xutils::get_atom(conn, "_NET_SUPPORTED")?;
    info!(
        "Debug: wm_support={:?}, wm_name={:?}, wm_net_supported={:?}",
        wm_support_atom, wm_name, wm_supported
    );
    Ok((wm_support_atom, wm_name, wm_supported))
}

pub fn wait() -> anyhow::Result<()> {
    let (conn, screen_num) = xcb::Connection::connect(None)?;
    let screen = {
        let setup = conn.get_setup();
        setup.roots().nth(screen_num as usize).unwrap()
    };
    xutils::send(
        &conn,
        &x::ChangeWindowAttributes {
            window: screen.root(),
            value_list: &[x::Cw::EventMask(x::EventMask::PROPERTY_CHANGE)],
        },
    )?;
    conn.flush()?;

    let (wm_support_atom, wm_name, wm_supported) = refetch_atoms(&conn)?;
    if let Ok(wm) = validate_wm(&conn, screen, wm_support_atom, wm_name, wm_supported) {
        info!("Detected WM: {:?}", wm);
        return Ok(());
    }

    info!("WM not detected on startup, waiting for it to initialize...");

    // TODO: fix infinite waiting here.

    while let Ok(event) = xutils::get_event(&conn) {
        let (wm_support_atom, wm_name, wm_supported) = refetch_atoms(&conn)?;
        match event {
            Some(xcb::Event::X(x::Event::PropertyNotify(pn))) if pn.atom() == wm_support_atom => {
                if let Ok(wm) = validate_wm(&conn, screen, wm_support_atom, wm_name, wm_supported) {
                    info!("Eventually detected WM: {:?}", wm);

                    return Ok(());
                }
            }
            other => {
                debug!("Unhandled event: {:?}", other);
            }
        }
    }

    Ok(())
}
