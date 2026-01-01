use crossbeam_channel::Sender;

use crate::{config, notify, parse, state};

/// Enum representing the detected display server.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisplayServer {
    /// Wayland display server
    Wayland,
    /// X11/XOrg display server
    X11,
}

/// Detect the current display server based on environment variables.
///
/// Detection priority:
/// 1. `WAYLAND_DISPLAY` set → Wayland
/// 2. `DISPLAY` set → X11
/// 3. Returns `None` if neither is set
pub fn detect() -> Option<DisplayServer> {
    if std::env::var("WAYLAND_DISPLAY").is_ok() {
        Some(DisplayServer::Wayland)
    } else if std::env::var("DISPLAY").is_ok() {
        Some(DisplayServer::X11)
    } else {
        None
    }
}

/// Common trait for display server engines (X11 and Wayland).
pub trait Engine {
    /// Run the engine's main event loop.
    fn run(&mut self) -> anyhow::Result<()>;
    /// Get a clone of the update sender channel.
    fn update_tx(&self) -> Sender<state::Update>;
}

/// Load the appropriate engine based on feature flags and environment.
///
/// Detection priority:
/// 1. If `WAYLAND_DISPLAY` is set and `wayland` feature is enabled, use Wayland.
/// 2. If `DISPLAY` is set and `x11` feature is enabled, use X11.
/// 3. Fall back to Wayland if available, then X11.
pub fn load(
    config: config::Config<parse::Placeholder>,
    state: state::State,
    notifier: notify::Notifier,
) -> anyhow::Result<Box<dyn Engine>> {
    let detected = detect();

    // Try Wayland first if WAYLAND_DISPLAY is set
    #[cfg(feature = "wayland")]
    if detected == Some(DisplayServer::Wayland) {
        tracing::info!("WAYLAND_DISPLAY is set, using Wayland engine");
        return Ok(Box::new(crate::wayland::WaylandEngine::new(
            config, state, notifier,
        )?));
    }

    // Try X11 if DISPLAY is set
    #[cfg(feature = "x11")]
    if detected == Some(DisplayServer::X11) {
        tracing::info!("DISPLAY is set, using X11 engine");
        return Ok(Box::new(crate::x11::XOrgEngine::new(
            config, state, notifier,
        )?));
    }

    // Fallback to Wayland if no env var is set but feature is enabled
    #[allow(unreachable_code)]
    #[cfg(feature = "wayland")]
    {
        tracing::info!("No display env var set, trying Wayland engine as fallback");
        return Ok(Box::new(crate::wayland::WaylandEngine::new(
            config, state, notifier,
        )?));
    }

    // Fallback to X11 if no env var is set but feature is enabled
    #[allow(unreachable_code)]
    #[cfg(feature = "x11")]
    {
        tracing::info!("No display env var set, trying X11 engine as fallback");
        return Ok(Box::new(crate::x11::XOrgEngine::new(
            config, state, notifier,
        )?));
    }

    #[allow(unreachable_code)]
    Err(anyhow::anyhow!(
        "No suitable engine found. Ensure WAYLAND_DISPLAY or DISPLAY is set, \
         and the corresponding feature (x11 or wayland) is enabled."
    ))
}
