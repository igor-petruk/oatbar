use pangocairo::pango;
use xcb::x;

#[derive(PartialEq, Eq)]
pub enum Mode {
    Full,
    Shape,
}

pub struct Context {
    pub buffer: x::Pixmap,
    pub buffer_surface: cairo::XCBSurface,
    pub context: cairo::Context,
    pub pango_context: pango::Context,
    pub width: f64,
    pub height: f64,
    pub mode: Mode,
}

impl Context {
    pub fn new(
        buffer: x::Pixmap,
        buffer_surface: cairo::XCBSurface,
        width: f64,
        height: f64,
        mode: Mode,
    ) -> anyhow::Result<Self> {
        let context = cairo::Context::new(buffer_surface.clone())?;
        let pango_context = pangocairo::create_context(&context);
        Ok(Self {
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