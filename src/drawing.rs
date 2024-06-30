use std::{
    collections::HashMap,
    path::PathBuf,
    str::FromStr,
    sync::{Arc, Mutex},
};

use anyhow::Context as AnyhowContext;
use pangocairo::pango;
use resvg::{tiny_skia, usvg};
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

pub type Image = cairo::ImageSurface;

#[derive(Eq, Hash, Clone, PartialEq, Debug)]
pub struct ImageKey {
    file_name: String,
    fit_to_height: u32,
}

#[derive(Clone)]
pub struct ImageLoader {
    cache: HashMap<ImageKey, Image>,
}

impl ImageLoader {
    fn image_from_rgba8(
        buf: &mut [u8],
        width: i32,
        height: i32,
    ) -> anyhow::Result<cairo::ImageSurface> {
        let format = cairo::Format::ARgb32;
        let mut image = cairo::ImageSurface::create(format, width, height)?;
        // rgba => bgra (reverse argb)
        for rgba in buf.chunks_mut(4) {
            rgba.swap(0, 2);
        }
        image.data()?.copy_from_slice(buf);
        Ok(image)
    }

    fn load_raster(file_name: &str, fit_to_height: f64) -> anyhow::Result<cairo::ImageSurface> {
        let img_buf = image::io::Reader::open(file_name)?
            .decode()
            .context("Unable to decode image")?
            .into_rgba8();
        let mut scale = fit_to_height as f32 / img_buf.height() as f32;
        if scale > 1.0 {
            // Do not scale up.
            scale = 1.0;
        }
        let img_buf = image::imageops::resize(
            &img_buf,
            (img_buf.width() as f32 * scale) as u32,
            (img_buf.height() as f32 * scale) as u32,
            image::imageops::FilterType::Triangle,
        );
        let (w, h) = (img_buf.width(), img_buf.height());
        Self::image_from_rgba8(&mut img_buf.into_raw(), w.try_into()?, h.try_into()?)
    }

    fn load_svg(file_name: &str, fit_to_height: f64) -> anyhow::Result<cairo::ImageSurface> {
        let tree = {
            let mut opt = usvg::Options {
                resources_dir: std::fs::canonicalize(file_name)
                    .ok()
                    .and_then(|p| p.parent().map(|p| p.to_path_buf())),
                ..Default::default()
            };
            opt.fontdb_mut().load_system_fonts();
            let svg_data = std::fs::read(file_name).unwrap();
            usvg::Tree::from_data(&svg_data, &opt).unwrap()
        };
        let size = tree.size().to_int_size(); // cannot be zero.
        let mut scale = fit_to_height as f32 / size.height() as f32;
        if scale > 1.0 {
            // Do not scale up.
            scale = 1.0;
        }
        let (w, h) = (size.width() as f32 * scale, size.height() as f32 * scale);
        let mut pixmap = tiny_skia::Pixmap::new(w as u32, h as u32).unwrap();
        resvg::render(
            &tree,
            tiny_skia::Transform::from_scale(scale, scale),
            &mut pixmap.as_mut(),
        );
        Self::image_from_rgba8(pixmap.data_mut(), w as i32, h as i32)
    }

    fn do_load_image(&self, file_name: &str, fit_to_height: f64) -> anyhow::Result<Image> {
        match PathBuf::from_str(file_name)?.extension() {
            Some(s) if s == "svg" => Self::load_svg(file_name, fit_to_height),
            _ => Self::load_raster(file_name, fit_to_height),
        }
    }

    pub fn load_image(
        &mut self,
        file_name: &str,
        fit_to_height: f64,
        cache_images: bool,
    ) -> anyhow::Result<Image> {
        let key = ImageKey {
            file_name: file_name.into(),
            fit_to_height: fit_to_height as u32,
        };
        if cache_images {
            if let Some(image) = self.cache.get(&key) {
                tracing::debug!("Got {:?} from cache", key);
                return Ok(image.clone());
            }
            tracing::debug!("{:?} not in cache, loading...", key);
            let image = self.do_load_image(file_name, fit_to_height)?;
            self.cache.insert(key, image.clone());
            Ok(image)
        } else {
            tracing::debug!("Cache disabled, loading {:?}...", key);
            self.do_load_image(file_name, fit_to_height)
        }
    }

    pub fn new() -> Self {
        Self {
            cache: HashMap::new(),
        }
    }
}

#[derive(PartialEq, Eq, Clone)]
pub enum Mode {
    Full,
    Shape,
}

#[derive(Clone)]
pub struct Context {
    pub buffer: x::Pixmap,
    pub buffer_surface: cairo::XCBSurface,
    pub context: cairo::Context,
    pub pango_context: Option<pango::Context>,
    pub mode: Mode,
    pub font_cache: Arc<Mutex<FontCache>>,
    pub image_loader: ImageLoader,
    pub pointer_position: Option<(i16, i16)>,
    pub hover: bool,
}

pub struct Color {
    pub r: f64,
    pub g: f64,
    pub b: f64,
    pub a: f64,
}

impl Color {
    pub fn parse(color: &str) -> anyhow::Result<Self> {
        let (pango_color, alpha) = pango::Color::parse_with_alpha(color)?;
        let scale = 65536.0;
        Ok(Self {
            r: pango_color.red() as f64 / scale,
            g: pango_color.green() as f64 / scale,
            b: pango_color.blue() as f64 / scale,
            a: alpha as f64 / scale,
        })
    }
}

impl Context {
    pub fn new(
        font_cache: Arc<Mutex<FontCache>>,
        image_loader: ImageLoader,
        buffer: x::Pixmap,
        buffer_surface: cairo::XCBSurface,
        mode: Mode,
    ) -> anyhow::Result<Self> {
        let context = cairo::Context::new(buffer_surface.clone())?;
        context.set_antialias(cairo::Antialias::Fast);
        context.set_line_join(cairo::LineJoin::Round);
        context.set_line_cap(cairo::LineCap::Square);
        let pango_context = match mode {
            Mode::Full => Some(pangocairo::functions::create_context(&context)),
            Mode::Shape => None,
        };
        Ok(Self {
            font_cache,
            image_loader,
            buffer,
            buffer_surface,
            context,
            pango_context,
            mode,
            pointer_position: None,
            hover: false,
        })
    }

    pub fn set_source_color(&self, color: Color) {
        self.context
            .set_source_rgba(color.r, color.g, color.b, color.a);
    }

    pub fn set_source_rgba(&self, color: &str) -> anyhow::Result<()> {
        if color.is_empty() {
            return Ok(());
        }
        match Color::parse(color) {
            Ok(color) => {
                self.set_source_color(color);
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
        match Color::parse(color) {
            Ok(color) if self.mode == Mode::Shape => {
                self.set_source_color(Color {
                    r: 0.0,
                    g: 0.0,
                    b: 0.0,
                    a: if color.a == 0.0 { 0.0 } else { 1.0 },
                });
                Ok(())
            }
            Ok(color) => {
                self.set_source_color(color);
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
