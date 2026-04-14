use crate::{bar, drawing};
use anyhow::Context;
use std::sync::{Arc, Mutex};

pub fn dump(
    path: &str,
    width: f64,
    height: f64,
    font_cache: Arc<Mutex<drawing::FontCache>>,
    #[cfg(feature = "image")] image_loader: drawing::ImageLoader,
    bar: &mut bar::Bar,
) -> anyhow::Result<()> {
    let surface = cairo::SvgSurface::new(width, height, Some(path))
        .map_err(|e| anyhow::anyhow!("Failed to create SvgSurface at {}: {:?}", path, e))?;

    let cr = cairo::Context::new(&surface)
        .map_err(|e| anyhow::anyhow!("Failed to create Cairo context for SVG dump: {:?}", e))?;

    let context = drawing::Context::new(
        cr,
        font_cache,
        #[cfg(feature = "image")]
        image_loader,
        drawing::Mode::Full,
    )
    .context("Failed to create drawing context for SVG dump")?;

    bar.render(&context, &bar::RedrawScope::All)
        .context("Failed to render bar to SVG")?;

    surface.finish();
    tracing::info!("Successfully rendered SVG to {}", path);
    Ok(())
}
