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

use anyhow::Context;
use std::sync::Arc;
use xcb::{x, Xid};

use crate::{bar, config, state, thread, xutils};
use tracing::*;

#[derive(Debug)]
pub enum Event {
    Exposed,
}

pub struct Window {
    pub conn: Arc<xcb::Connection>,
    pub id: x::Window,
    pub width: u16,
    pub height: u16,
    back_buffer: x::Pixmap,
    back_buffer_surface: cairo::XCBSurface,
    swap_gc: x::Gcontext,
    bar: bar::Bar,
    pub rx_events: crossbeam_channel::Receiver<Event>,
}

impl Window {
    pub fn create_and_show(config: config::Config<config::Placeholder>) -> anyhow::Result<Self> {
        let (conn, screen_num) = xcb::Connection::connect_with_xlib_display().unwrap();
        let conn = Arc::new(conn);
        let screen = {
            let setup = conn.get_setup();
            setup.roots().nth(screen_num as usize).unwrap()
        }
        .to_owned();

        let mut vis32 = find_32bit_visual(&screen).unwrap();

        let margin = &config.bar.margin;

        let height = config.bar.height;
        let window_width = screen.width_in_pixels();
        let window_height = height + margin.top + margin.bottom;

        let cid = conn.generate_id();
        xutils::send(
            &conn,
            &x::CreateColormap {
                mid: cid,
                window: screen.root(),
                visual: vis32.visual_id(),
                alloc: x::ColormapAlloc::None,
            },
        )?;

        let id: x::Window = conn.generate_id();
        let top = config.bar.position == config::BarPosition::Top;
        let y = if top {
            0
        } else {
            screen.height_in_pixels() as i16 - window_height as i16
        };
        conn.send_request(&x::CreateWindow {
            depth: 32,
            wid: id,
            parent: screen.root(),
            x: 0,
            y,
            width: window_width,
            height: window_height,
            border_width: 0,
            class: x::WindowClass::InputOutput,
            visual: vis32.visual_id(),
            value_list: &[
                x::Cw::BorderPixel(screen.white_pixel()),
                x::Cw::EventMask(
                    x::EventMask::EXPOSURE | x::EventMask::KEY_PRESS | x::EventMask::BUTTON_PRESS,
                ),
                x::Cw::Colormap(cid),
            ],
        });

        let app_name = "oatbar".as_bytes();
        xutils::replace_property_atom(&conn, id, x::ATOM_WM_NAME, x::ATOM_STRING, app_name)?;
        xutils::replace_property_atom(&conn, id, x::ATOM_WM_CLASS, x::ATOM_STRING, app_name)?;
        if let Err(e) = xutils::replace_atom_property(
            &conn,
            id,
            "_NET_WM_WINDOW_TYPE",
            &["_NET_WM_WINDOW_TYPE_DOCK"],
        ) {
            warn!("Unable to set window property: {:?}", e);
        }
        xutils::replace_atom_property(
            &conn,
            id,
            "_NET_WM_STATE",
            &["_NET_WM_STATE_STICKY", "_NET_WM_STATE_ABOVE"],
        )?;
        let sp_result = xutils::replace_property(
            &conn,
            id,
            "_NET_WM_STRUT_PARTIAL",
            x::ATOM_CARDINAL,
            &[
                0,
                0,
                if top { window_height } else { 0 },
                if top { 0 } else { window_height },
                0,
                0,
                0,
                0,
                0,
                if top { window_width } else { 0 },
                0,
                if top { 0 } else { window_width },
            ],
        )
        .context("_NET_WM_STRUT_PARTIAL");
        if let Err(e) = sp_result {
            debug!("Unable to set _NET_WM_STRUT_PARTIAL: {:?}", e);
        }
        let s_result = xutils::replace_property(
            &conn,
            id,
            "_NET_WM_STRUT",
            x::ATOM_CARDINAL,
            &[
                0_u32,
                0,
                if top { window_height.into() } else { 0 },
                if top { 0 } else { window_height.into() },
            ],
        )
        .context("_NET_WM_STRUT");
        if let Err(e) = s_result {
            debug!("Unable to set _NET_WM_STRUT: {:?}", e);
        }
        let back_buffer: x::Pixmap = conn.generate_id();
        xutils::send(
            &conn,
            &x::CreatePixmap {
                depth: 32,
                pid: back_buffer,
                drawable: xcb::x::Drawable::Window(id),
                width: window_width,
                height: window_height,
            },
        )?;

        let back_buffer_surface =
            make_pixmap_surface(&conn, &back_buffer, &mut vis32, window_width, window_height)?;

        let swap_gc: x::Gcontext = conn.generate_id();
        xutils::send(
            &conn,
            &x::CreateGc {
                cid: swap_gc,
                drawable: x::Drawable::Window(id),
                value_list: &[x::Gc::GraphicsExposures(false)],
            },
        )?;
        conn.flush()?;

        xutils::send(&conn, &x::MapWindow { window: id })?;

        xutils::send(
            &conn,
            &x::ConfigureWindow {
                window: id,
                value_list: &[x::ConfigWindow::X(0), x::ConfigWindow::Y(y.into())],
            },
        )?;
        conn.flush()?;

        let bar = bar::Bar::new(&config)?;

        let (tx, rx) = crossbeam_channel::unbounded();

        {
            let conn = conn.clone();
            thread::spawn_loop("window", move || {
                let event = xutils::get_event(&conn)?;
                match event {
                    Some(xcb::Event::X(x::Event::Expose(_ev))) => {
                        tx.send(Event::Exposed)?;
                    }
                    None => return Ok(false),
                    _ => {
                        debug!("Unhandled XCB event: {:?}", event);
                    }
                }
                Ok(true)
            })?;
        }

        Ok(Self {
            conn,
            id,
            width: window_width,
            height: window_height,
            back_buffer,
            back_buffer_surface,
            swap_gc,
            bar,
            rx_events: rx,
        })
    }

    fn make_drawing_context(&self) -> anyhow::Result<bar::DrawingContext> {
        Ok(bar::DrawingContext {
            width: self.width.into(),
            height: self.height.into(),
            context: cairo::Context::new(&self.back_buffer_surface)?,
        })
    }

    pub fn render(&self, state: &state::State) -> anyhow::Result<()> {
        let dc = self.make_drawing_context()?;
        self.bar.render(&dc, state)?;
        self.swap_buffers()?;
        Ok(())
    }

    fn swap_buffers(&self) -> anyhow::Result<()> {
        self.back_buffer_surface.flush();
        xutils::send(
            &self.conn,
            &xcb::x::ClearArea {
                window: self.id,
                x: 0,
                y: 0,
                height: self.height,
                width: self.width,
                exposures: false,
            },
        )?;
        self.conn.flush()?;
        xutils::send(
            &self.conn,
            &xcb::x::CopyArea {
                src_drawable: xcb::x::Drawable::Pixmap(self.back_buffer),
                dst_drawable: xcb::x::Drawable::Window(self.id),
                src_x: 0,
                src_y: 0,
                dst_x: 0,
                dst_y: 0,
                width: self.width,
                height: self.height,
                gc: self.swap_gc,
            },
        )?;
        self.conn.flush()?;
        Ok(())
    }
}

fn find_32bit_visual(screen: &xcb::x::Screen) -> Option<xcb::x::Visualtype> {
    let d_iter: xcb::x::DepthIterator = screen.allowed_depths();
    for depth in d_iter {
        if depth.depth() != 32 {
            continue;
        }
        for vis in depth.visuals() {
            if vis.class() == xcb::x::VisualClass::TrueColor {
                return Some(*vis);
            }
        }
    }
    None
}

fn make_pixmap_surface(
    conn: &xcb::Connection,
    pixmap: &x::Pixmap,
    visual: &mut x::Visualtype,
    width: u16,
    height: u16,
) -> anyhow::Result<cairo::XCBSurface> {
    let cairo_xcb_connection =
        unsafe { cairo::XCBConnection::from_raw_none(std::mem::transmute(conn.get_raw_conn())) };
    let cairo_xcb_visual =
        unsafe { cairo::XCBVisualType::from_raw_none(std::mem::transmute(visual as *mut _)) };

    let pixmap_surface = cairo::XCBSurface::create(
        &cairo_xcb_connection,
        &cairo::XCBDrawable(pixmap.resource_id()),
        &cairo_xcb_visual,
        width.into(),
        height.into(),
    )?;

    conn.flush()?;

    Ok(pixmap_surface)
}
