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

use tracing::*;

/// Display server type (duplicated from engine module for standalone binary)
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
// Common types and functions
// ============================================================================

#[derive(Debug, Clone)]
struct Workspaces {
    current: usize,
    names: Vec<String>,
}

#[derive(Debug, Clone)]
struct DesktopState {
    workspaces: Workspaces,
    active_window_title: String,
}

fn print_update(state: &DesktopState) -> anyhow::Result<()> {
    let workspace_value = state
        .workspaces
        .names
        .get(state.workspaces.current)
        .unwrap_or(&"?".to_string())
        .to_string();
    let mut other = BTreeMap::new();
    other.insert("active".into(), state.workspaces.current.into());
    other.insert("variants".into(), state.workspaces.names.join(",").into());
    other.insert("value".into(), workspace_value.clone().into());
    let mut title_other = BTreeMap::new();
    title_other.insert("value".into(), state.active_window_title.clone().into());
    let blocks = vec![
        i3bar::Block {
            full_text: format!("workspace: {}", workspace_value),
            name: Some("workspace".into()),
            instance: None,
            other,
        },
        i3bar::Block {
            name: Some("window_title".into()),
            full_text: format!("window: {}", state.active_window_title),
            other: title_other,
            ..Default::default()
        },
    ];
    println!("{},", serde_json::to_string(&blocks)?);
    Ok(())
}

// ============================================================================
// X11 Implementation
// ============================================================================

mod x11_impl {
    use super::*;
    use xcb::x::{self, Atom, Window};
    use xcb::Xid;
    use xutils::get_atom;

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

    pub fn run(set_workspace: Option<u32>) -> anyhow::Result<()> {
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

        if let Some(workspace) = set_workspace {
            set_current_workspace(screen.root(), &conn, &current_desktop, workspace)?;
            return Ok(());
        }

        println!("{}", serde_json::to_string(&i3bar::Header::default())?);
        println!("[");

        let workspaces = get_workspaces(screen.root(), &conn, &current_desktop, &desktop_names)?;
        let title = get_active_window_title(&conn, screen.root(), &active_window, &window_name)?;
        let mut state = DesktopState {
            workspaces,
            active_window_title: title,
        };
        print_update(&state)?;

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
                        state.workspaces =
                            get_workspaces(screen.root(), &conn, &current_desktop, &desktop_names)?;
                        print_update(&state)?;
                    }
                    if ev.atom() == active_window || ev.atom() == window_name {
                        state.active_window_title = get_active_window_title(
                            &conn,
                            screen.root(),
                            &active_window,
                            &window_name,
                        )?;
                        print_update(&state)?;
                    }
                }
                _ => {
                    debug!("Unhandled XCB event: {:?}", event);
                }
            }
            conn.flush()?;
        }
    }
}

// ============================================================================
// Generic wlroots Implementation (using wlr-foreign-toplevel-management)
// ============================================================================
//
// This implementation uses the wlr-foreign-toplevel-management-unstable-v1
// protocol, which is supported by wlroots-based compositors:
// - Sway, Hyprland, River, wayfire, etc.
//
// The protocol provides:
// - List of all toplevel (window) surfaces
// - Window title, app_id, and state (including Activated/focused)
// - Events when windows are created, updated, or closed
//
// Limitations:
// - Does NOT support workspace management (no way to switch workspaces)
// - Only tracks windows, not workspace information
//
// For full workspace support on Sway, use sway_impl instead.
// ============================================================================

mod wayland_impl {
    use super::*;
    use std::collections::HashMap;
    use wayland_client::{
        globals::{registry_queue_init, GlobalListContents},
        protocol::wl_registry,
        Connection, Dispatch, EventQueue, Proxy, QueueHandle,
    };
    use wayland_protocols_wlr::foreign_toplevel::v1::client::{
        zwlr_foreign_toplevel_handle_v1::{
            self, State as ToplevelState, ZwlrForeignToplevelHandleV1,
        },
        zwlr_foreign_toplevel_manager_v1::{self, ZwlrForeignToplevelManagerV1},
    };

    /// Tracks information about a single toplevel (window)
    #[derive(Default)]
    struct ToplevelInfo {
        title: String,
        app_id: String,
        is_active: bool, // True when window has Activated state
    }

    /// Main state for the Wayland event loop
    struct WaylandState {
        /// All known toplevels, keyed by our internal ID
        toplevels: HashMap<u32, ToplevelInfo>,
        /// Counter for generating unique internal IDs
        next_id: u32,
        /// Maps Wayland protocol object IDs to our internal IDs
        object_to_id: HashMap<u32, u32>,
        /// Flag to batch updates until "done" event
        needs_print: bool,
    }

    impl WaylandState {
        fn new() -> Self {
            Self {
                toplevels: HashMap::new(),
                next_id: 0,
                object_to_id: HashMap::new(),
                needs_print: false,
            }
        }

        fn get_active_title(&self) -> String {
            self.toplevels
                .values()
                .find(|t| t.is_active)
                .map(|t| t.title.clone())
                .unwrap_or_default()
        }

        fn print_state(&self) -> anyhow::Result<()> {
            let desktop_state = DesktopState {
                workspaces: Workspaces {
                    current: 0,
                    names: vec!["default".to_string()],
                },
                active_window_title: self.get_active_title(),
            };
            print_update(&desktop_state)
        }
    }

    impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for WaylandState {
        fn event(
            _state: &mut Self,
            _proxy: &wl_registry::WlRegistry,
            _event: wl_registry::Event,
            _data: &GlobalListContents,
            _conn: &Connection,
            _qh: &QueueHandle<Self>,
        ) {
        }
    }

    impl Dispatch<ZwlrForeignToplevelManagerV1, ()> for WaylandState {
        fn event(
            state: &mut Self,
            _proxy: &ZwlrForeignToplevelManagerV1,
            event: zwlr_foreign_toplevel_manager_v1::Event,
            _data: &(),
            _conn: &Connection,
            _qh: &QueueHandle<Self>,
        ) {
            match event {
                zwlr_foreign_toplevel_manager_v1::Event::Toplevel { toplevel } => {
                    let id = state.next_id;
                    state.next_id += 1;
                    let object_id = toplevel.id().protocol_id();
                    state.object_to_id.insert(object_id, id);
                    state.toplevels.insert(id, ToplevelInfo::default());
                    tracing::debug!("New toplevel: id={}, object_id={}", id, object_id);
                }
                zwlr_foreign_toplevel_manager_v1::Event::Finished => {
                    tracing::info!("Foreign toplevel manager finished");
                }
                _ => {}
            }
        }

        wayland_client::event_created_child!(WaylandState, ZwlrForeignToplevelManagerV1, [
            zwlr_foreign_toplevel_manager_v1::EVT_TOPLEVEL_OPCODE => (ZwlrForeignToplevelHandleV1, ())
        ]);
    }

    impl Dispatch<ZwlrForeignToplevelHandleV1, ()> for WaylandState {
        fn event(
            state: &mut Self,
            proxy: &ZwlrForeignToplevelHandleV1,
            event: zwlr_foreign_toplevel_handle_v1::Event,
            _data: &(),
            _conn: &Connection,
            _qh: &QueueHandle<Self>,
        ) {
            let object_id = proxy.id().protocol_id();
            let id = match state.object_to_id.get(&object_id) {
                Some(id) => *id,
                None => {
                    tracing::warn!("Unknown toplevel object_id={}", object_id);
                    return;
                }
            };

            match event {
                zwlr_foreign_toplevel_handle_v1::Event::Title { title } => {
                    if let Some(info) = state.toplevels.get_mut(&id) {
                        info.title = title;
                    }
                }
                zwlr_foreign_toplevel_handle_v1::Event::AppId { app_id } => {
                    if let Some(info) = state.toplevels.get_mut(&id) {
                        info.app_id = app_id;
                    }
                }
                zwlr_foreign_toplevel_handle_v1::Event::State { state: wl_state } => {
                    let bytes: &[u8] = &wl_state;
                    let states: Vec<u32> = bytes
                        .chunks_exact(4)
                        .map(|chunk| u32::from_ne_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
                        .collect();

                    let is_active = states.contains(&(ToplevelState::Activated as u32));

                    if let Some(info) = state.toplevels.get_mut(&id) {
                        if info.is_active != is_active {
                            info.is_active = is_active;
                            state.needs_print = true;
                        }
                    }
                }
                zwlr_foreign_toplevel_handle_v1::Event::Done => {
                    if state.needs_print {
                        state.needs_print = false;
                        if let Err(e) = state.print_state() {
                            tracing::error!("Failed to print update: {}", e);
                        }
                    }
                }
                zwlr_foreign_toplevel_handle_v1::Event::Closed => {
                    state.toplevels.remove(&id);
                    state.object_to_id.remove(&object_id);
                    tracing::debug!("Toplevel {} closed", id);
                    if let Err(e) = state.print_state() {
                        tracing::error!("Failed to print update: {}", e);
                    }
                }
                _ => {}
            }
        }
    }

    pub fn run(set_workspace: Option<u32>) -> anyhow::Result<()> {
        if set_workspace.is_some() {
            anyhow::bail!(
                "Setting workspace is not supported with wlr-foreign-toplevel-management protocol."
            );
        }

        let conn = Connection::connect_to_env().context("Failed to connect to Wayland")?;

        let (globals, mut event_queue): (_, EventQueue<WaylandState>) =
            registry_queue_init(&conn).context("Failed to init registry")?;
        let qh = event_queue.handle();

        let _manager: ZwlrForeignToplevelManagerV1 = globals
            .bind(&qh, 1..=3, ())
            .context("Compositor doesn't support zwlr_foreign_toplevel_manager_v1")?;

        println!("{}", serde_json::to_string(&i3bar::Header::default())?);
        println!("[");

        let mut state = WaylandState::new();
        state.print_state()?;

        loop {
            event_queue
                .blocking_dispatch(&mut state)
                .context("Wayland dispatch failed")?;
        }
    }
}

// ============================================================================
// Sway Implementation (using swayipc for Sway-specific features)
// ============================================================================

mod sway_impl {
    use super::*;
    use swayipc::{Connection as SwayConnection, Event, EventType, Node};

    /// Recursively find the focused node in the Sway tree
    fn find_focused_node(node: &Node) -> Option<String> {
        if node.focused {
            return node.name.clone();
        }
        for child in node.nodes.iter().chain(node.floating_nodes.iter()) {
            if let Some(name) = find_focused_node(child) {
                return Some(name);
            }
        }
        None
    }

    /// Refresh the entire state from Sway
    fn refresh_state(conn: &mut SwayConnection, state: &mut DesktopState) -> anyhow::Result<()> {
        let workspaces = conn.get_workspaces().context("Failed to get workspaces")?;
        let tree = conn.get_tree().context("Failed to get tree")?;

        state.workspaces.names = workspaces.iter().map(|w| w.name.clone()).collect();

        if let Some(focused) = workspaces.iter().find(|w| w.focused) {
            state.workspaces.current = workspaces
                .iter()
                .position(|w| w.id == focused.id)
                .unwrap_or(0);
        }

        state.active_window_title = find_focused_node(&tree).unwrap_or_default();
        Ok(())
    }

    pub fn run(set_workspace: Option<u32>) -> anyhow::Result<()> {
        // We need a dedicated connection for sending commands/queries
        let mut command_conn = SwayConnection::new().context("Failed to connect to Sway IPC")?;

        // Handle workspace switch command (input is 0-indexed for consistency with X11)
        if let Some(workspace) = set_workspace {
            command_conn
                .run_command(format!("workspace number {}", workspace + 1))
                .context("Failed to switch workspace")?;
            return Ok(());
        }

        // Initialize state
        let mut state = DesktopState {
            workspaces: Workspaces {
                current: 0,
                names: vec![],
            },
            active_window_title: String::new(),
        };

        // Initial refresh
        refresh_state(&mut command_conn, &mut state)?;

        println!("{}", serde_json::to_string(&i3bar::Header::default())?);
        println!("[");
        print_update(&state)?;

        // Subscribe to events for real-time updates
        let subs = [EventType::Window, EventType::Workspace];
        let event_conn = SwayConnection::new()
            .context("Failed to create event connection")?
            .subscribe(subs)
            .context("Failed to subscribe to events")?;

        for event_result in event_conn {
            let event = match event_result {
                Ok(e) => e,
                Err(e) => {
                    tracing::error!("Sway IPC error: {}", e);
                    break;
                }
            };

            match event {
                Event::Window(window_event) => {
                    if window_event.change == swayipc::WindowChange::Focus {
                        if let Some(container) = window_event.container.name {
                            state.active_window_title = container;
                            print_update(&state)?;
                        }
                    }
                }
                Event::Workspace(workspace_event) => {
                    if workspace_event.change == swayipc::WorkspaceChange::Focus {
                        // Reuse the command connection for refreshing state
                        refresh_state(&mut command_conn, &mut state)?;
                        print_update(&state)?;
                    }
                }
                _ => {}
            }
        }

        Ok(())
    }
}

// ============================================================================
// Main entry point with display server detection
// ============================================================================

fn is_sway() -> bool {
    std::env::var("SWAYSOCK").is_ok()
}

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let set_workspace: Option<u32> = args.get(1).and_then(|s| s.parse().ok());

    match detect_display_server() {
        Some(DisplayServer::Wayland) => {
            if is_sway() {
                tracing::info!("Detected Sway, using swayipc");
                sway_impl::run(set_workspace)
            } else {
                tracing::info!("Detected Wayland, using wlr-foreign-toplevel-management");
                wayland_impl::run(set_workspace)
            }
        }
        Some(DisplayServer::X11) | None => {
            tracing::info!("Using X11 backend");
            x11_impl::run(set_workspace)
        }
    }
}
