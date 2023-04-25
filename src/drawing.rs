use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use pangocairo::pango;
use xcb::x;

pub struct FontCache {
    cache: HashMap<String, pango::FontDescription>,
}

impl FontCache {
    pub fn new() -> Self {
        Self {
            cache: HashMap::new(),
        }
    }

    pub fn get(&mut self, font_str: &str) -> &pango::FontDescription {
        self.cache
            .entry(font_str.into())
            .or_insert_with(|| pango::FontDescription::from_string(font_str))
    }
}

#[derive(PartialEq, Eq)]
pub enum Mode {
    Full,
    Shape,
}

pub struct Context {
    pub buffer: x::Pixmap,
    pub buffer_surface: cairo::XCBSurface,
    pub context: cairo::Context,
    pub pango_context: Option<pango::Context>,
    pub width: f64,
    pub height: f64,
    pub mode: Mode,
    pub font_cache: Arc<Mutex<FontCache>>,
}

impl Context {
    pub fn new(
        font_cache: Arc<Mutex<FontCache>>,
        buffer: x::Pixmap,
        buffer_surface: cairo::XCBSurface,
        width: f64,
        height: f64,
        mode: Mode,
    ) -> anyhow::Result<Self> {
        let context = cairo::Context::new(buffer_surface.clone())?;
        context.set_antialias(cairo::Antialias::Fast);
        context.set_line_join(cairo::LineJoin::Round);
        context.set_line_cap(cairo::LineCap::Square);
        let pango_context = match mode {
            Mode::Full => Some(pangocairo::create_context(&context)),
            Mode::Shape => None,
        };
        Ok(Self {
            font_cache,
            buffer,
            buffer_surface,
            context,
            pango_context,
            width,
            height,
            mode,
        })
    }

    pub fn set_source_hexcolor(&self, color: hex_color::HexColor) {
        self.context.set_source_rgba(
            color.r as f64 / 256.,
            color.g as f64 / 256.,
            color.b as f64 / 256.,
            color.a as f64 / 256.,
        );
    }

    pub fn set_source_rgba(&self, color: &str) -> anyhow::Result<()> {
        if color.is_empty() {
            return Ok(());
        }
        match hex_color::HexColor::parse(color) {
            Ok(color) => {
                self.set_source_hexcolor(color);
                Ok(())
            }
            Err(e) => Err(anyhow::anyhow!(
                "failed to parse color: {:?}, err={:?}",
                color,
                e
            )),
        }
    }

    pub fn set_source_rgba_background(&self, color: &str) -> anyhow::Result<()> {
        if color.is_empty() {
            return Ok(());
        }
        match hex_color::HexColor::parse(color) {
            Ok(color) if self.mode == Mode::Shape => {
                self.set_source_hexcolor(hex_color::HexColor {
                    r: 0,
                    g: 0,
                    b: 0,
                    a: if color.a == 0 { 0 } else { 255 },
                });
                Ok(())
            }
            Ok(color) => {
                self.set_source_hexcolor(color);
                Ok(())
            }
            Err(e) => Err(anyhow::anyhow!(
                "failed to parse color: {:?}, err={:?}",
                color,
                e
            )),
        }
    }
}
