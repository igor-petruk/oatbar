use tracing::*;
use xcb::{randr, x};

use crate::xutils;

#[derive(Debug, Clone)]
pub struct Monitor {
    pub name: String,
    pub primary: bool,
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub height: u16,
}

pub fn get_monitor(
    conn: &xcb::Connection,
    root: x::Window,
    name: &Option<String>,
) -> anyhow::Result<Option<Monitor>> {
    if let Some(name) = &name {
        tracing::info!("Trying to find monitor {:?}", name);
    } else {
        tracing::info!("Trying to find the primary monitor.");
    }

    let monitors_reply = xutils::query(
        conn,
        &randr::GetMonitors {
            window: root,
            get_active: true,
        },
    )?;

    let mut monitors = Vec::<Monitor>::with_capacity(monitors_reply.monitors().count());

    for info in monitors_reply.monitors() {
        let name_reply = xutils::query(conn, &x::GetAtomName { atom: info.name() })?;

        let monitor = Monitor {
            name: name_reply.name().to_utf8().into(),
            primary: info.primary(),
            x: info.x() as u16,
            y: info.y() as u16,
            width: info.width(),
            height: info.height(),
        };

        info!("Detected {:?}", monitor);
        monitors.push(monitor);
    }

    if monitors.is_empty() {
        warn!("No monitors returned by XRandr");
        return Ok(None);
    }

    let monitor_found = if let Some(name) = &name {
        monitors.iter().find(|m| m.name == *name)
    } else {
        monitors.iter().find(|m| m.primary)
    };

    let monitor = if let Some(monitor) = monitor_found {
        info!("Monitor found: {}", monitor.name);
        monitor_found
    } else if let Some(name) = name {
        return Err(anyhow::anyhow!(
            "Monitor {:?} not found, but specified for the bar",
            name
        ));
    } else {
        info!("Primary monitor not found, picking the first one");
        monitors.first()
    };
    Ok(monitor.cloned())
}
