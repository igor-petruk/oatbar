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

use std::fmt::Debug;

use anyhow::Context;
use tracing::*;
use xcb::x;

pub fn get_atom(conn: &xcb::Connection, name: &str) -> anyhow::Result<x::Atom> {
    let reply = query(
        conn,
        &x::InternAtom {
            only_if_exists: true,
            name: name.as_bytes(),
        },
    )
    .context(format!("get_atom: {}", name))?;

    Ok(reply.atom())
}

pub fn get_property(
    conn: &xcb::Connection,
    window: x::Window,
    atom: x::Atom,
    atom_type: x::Atom,
    long_length: u32,
) -> anyhow::Result<x::GetPropertyReply> {
    let reply = query(
        &conn,
        &x::GetProperty {
            property: atom,
            window,
            r#type: atom_type,
            long_offset: 0,
            long_length,
            delete: false,
        },
    )?;
    Ok(reply)
}

pub fn replace_property_atom<P: x::PropEl + Debug>(
    conn: &xcb::Connection,
    window: x::Window,
    atom: x::Atom,
    atom_type: x::Atom,
    value: &[P],
) -> anyhow::Result<()> {
    send(
        &conn,
        &x::ChangeProperty {
            mode: x::PropMode::Replace,
            window,
            property: atom,
            r#type: atom_type,
            data: value,
        },
    )
    .context(format!("replace_property_atom: {:?}", atom))?;
    Ok(())
}

pub fn replace_property<P: x::PropEl + Debug>(
    conn: &xcb::Connection,
    window: x::Window,
    atom_name: &str,
    atom_type: x::Atom,
    value: &[P],
) -> anyhow::Result<x::Atom> {
    let atom = get_atom(conn, atom_name)?;
    send(
        &conn,
        &x::ChangeProperty {
            mode: x::PropMode::Replace,
            window,
            property: atom,
            r#type: atom_type,
            data: value,
        },
    )
    .context(format!("replace_property: {}", atom_name))?;
    Ok(atom)
}

pub fn replace_atom_property(
    conn: &xcb::Connection,
    window: x::Window,
    atom_name: &str,
    value_atom_name: &str,
) -> anyhow::Result<(x::Atom, x::Atom)> {
    let value_atom = get_atom(conn, value_atom_name)?;
    let atom = replace_property(&conn, window, atom_name, x::ATOM_ATOM, &[value_atom])?;
    Ok((atom, value_atom))
}

pub fn send<X: xcb::RequestWithoutReply + Debug>(
    conn: &xcb::Connection,
    req: &X,
) -> anyhow::Result<()> {
    let cookie = conn.send_request_checked(req);
    conn.check_request(cookie)
        .with_context(|| format!("xcb request failed: req={:?}", req))?;
    Ok(())
}

pub fn query<X: xcb::Request + Debug>(
    conn: &xcb::Connection,
    req: &X,
) -> anyhow::Result<<<X as xcb::Request>::Cookie as xcb::CookieWithReplyChecked>::Reply>
where
    <X as xcb::Request>::Cookie: xcb::CookieWithReplyChecked,
{
    let cookie = conn.send_request(req);
    Ok(conn
        .wait_for_reply(cookie)
        .with_context(|| format!("xcb request failed: req={:?}", req))?)
}

#[inline]
pub fn handler_event_errors(
    event: Result<xcb::Event, xcb::Error>,
) -> anyhow::Result<Option<xcb::Event>> {
    let event = match event {
        Err(xcb::Error::Connection(xcb::ConnError::Connection)) => {
            debug!(
                "XCB connection terminated for thread {}",
                std::thread::current().name().unwrap_or("<unnamed>")
            );
            return Ok(None);
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
    Ok(Some(event))
}

#[inline]
pub fn get_event(conn: &xcb::Connection) -> anyhow::Result<Option<xcb::Event>> {
    handler_event_errors(conn.wait_for_event())
}
