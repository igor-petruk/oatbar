use anyhow::Context;
use clap::{Parser, Subcommand};
use futures_util::stream::StreamExt;
use std::collections::BTreeMap;
use tracing_subscriber::prelude::*;
use tracing_subscriber::EnvFilter;
use zbus::proxy;

use tracing::*;

#[allow(non_snake_case)]
mod mpris {
    pub mod media_player2;
    pub mod player;
    pub mod playlists;
    pub mod track_list;
}
mod protocol;

use protocol::i3bar;

const MPRIS_PREFIX: &str = "org.mpris.MediaPlayer2.";
const MPRIS_PATH: &str = "/org/mpris/MediaPlayer2";

// ---------------------------------------------------------------------------
// CLI
// ---------------------------------------------------------------------------

#[derive(Parser)]
#[command(
    author, version,
    about = "MPRIS media player util for oatbar",
    long_about = None)]
#[command(propagate_version = true)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Start/resume playback.
    Play,
    /// Pause playback.
    Pause,
    /// Toggle play/pause.
    PlayPause,
    /// Skip to next track.
    Next,
    /// Go to previous track.
    Previous,
    /// Stop playback.
    Stop,
    /// Seek to a percentage of the track (0-100).
    Seek {
        /// Target position as a percentage (0-100).
        percent: f64,
    },
}

// ---------------------------------------------------------------------------
// Active player detection
// ---------------------------------------------------------------------------

/// Find MPRIS bus names and return the "best" one (prefer Playing, then Paused).
async fn find_active_player(
    session: &zbus::Connection,
) -> anyhow::Result<Option<String>> {
    let dbus = zbus::fdo::DBusProxy::new(session).await?;
    let names = dbus.list_names().await?;

    let mut candidates: Vec<String> = names
        .iter()
        .filter(|n| n.as_str().starts_with(MPRIS_PREFIX))
        .map(|n| n.to_string())
        .collect();

    if candidates.is_empty() {
        return Ok(None);
    }

    // Sort: prefer "Playing", then "Paused", then anything else.
    let mut scored: Vec<(String, u8)> = Vec::with_capacity(candidates.len());
    for name in candidates.drain(..) {
        let score = match get_playback_status(session, &name).await {
            Ok(ref s) if s == "Playing" => 0,
            Ok(ref s) if s == "Paused" => 1,
            _ => 2,
        };
        scored.push((name, score));
    }
    scored.sort_by_key(|(_, s)| *s);

    Ok(scored.into_iter().next().map(|(n, _)| n))
}

async fn get_playback_status(
    session: &zbus::Connection,
    bus_name: &str,
) -> anyhow::Result<String> {
    let player = build_player_proxy(session, bus_name).await?;
    Ok(player.playback_status().await?)
}

async fn build_player_proxy<'a>(
    session: &'a zbus::Connection,
    bus_name: &str,
) -> anyhow::Result<mpris::player::PlayerProxy<'a>> {
    mpris::player::PlayerProxy::builder(session)
        .destination(bus_name.to_string())
        .context("Error setting destination")?
        .path(MPRIS_PATH)
        .context("Error setting path")?
        .cache_properties(proxy::CacheProperties::No)
        .build()
        .await
        .context(format!("Error building PlayerProxy for {}", bus_name))
}

async fn build_mp2_proxy<'a>(
    session: &'a zbus::Connection,
    bus_name: &str,
) -> anyhow::Result<mpris::media_player2::MediaPlayer2Proxy<'a>> {
    mpris::media_player2::MediaPlayer2Proxy::builder(session)
        .destination(bus_name.to_string())
        .context("Error setting destination")?
        .path(MPRIS_PATH)
        .context("Error setting path")?
        .cache_properties(proxy::CacheProperties::No)
        .build()
        .await
        .context(format!("Error building MediaPlayer2Proxy for {}", bus_name))
}

/// Short identity for a bus name (e.g. "org.mpris.MediaPlayer2.spotify" -> "spotify").
fn short_name(bus_name: &str) -> String {
    bus_name
        .strip_prefix(MPRIS_PREFIX)
        .unwrap_or(bus_name)
        .to_string()
}

// ---------------------------------------------------------------------------
// Metadata extraction
// ---------------------------------------------------------------------------

fn extract_string(
    metadata: &std::collections::HashMap<String, zbus::zvariant::OwnedValue>,
    key: &str,
) -> String {
    metadata
        .get(key)
        .and_then(|v| {
            // Try as plain string first.
            if let Ok(s) = String::try_from(v.clone()) {
                return Some(s);
            }
            // Some players return artist as Vec<String>.
            if let Ok(arr) = <Vec<String>>::try_from(v.clone()) {
                if !arr.is_empty() {
                    return Some(arr.join(", "));
                }
            }
            None
        })
        .unwrap_or_default()
}

fn extract_i64(
    metadata: &std::collections::HashMap<String, zbus::zvariant::OwnedValue>,
    key: &str,
) -> Option<i64> {
    metadata
        .get(key)
        .and_then(|v| i64::try_from(v.clone()).ok())
}

fn extract_track_id(
    metadata: &std::collections::HashMap<String, zbus::zvariant::OwnedValue>,
) -> Option<zbus::zvariant::OwnedObjectPath> {
    metadata
        .get("mpris:trackid")
        .and_then(|v| zbus::zvariant::OwnedObjectPath::try_from(v.clone()).ok())
}

// ---------------------------------------------------------------------------
// Block building
// ---------------------------------------------------------------------------

fn build_block(
    bus_name: &str,
    identity: &str,
    playback_status: &str,
    metadata: &std::collections::HashMap<String, zbus::zvariant::OwnedValue>,
    volume: f64,
    position_us: i64,
    rate: f64,
) -> i3bar::Block {
    let title = extract_string(metadata, "xesam:title");
    let artist = extract_string(metadata, "xesam:artist");
    let album = extract_string(metadata, "xesam:album");
    let length_us = extract_i64(metadata, "mpris:length");

    let track_text = match (artist.is_empty(), title.is_empty()) {
        (false, false) => format!("{} - {}", artist, title),
        (true, false) => title.clone(),
        (false, true) => artist.clone(),
        (true, true) => String::new(),
    };

    let full_text = if track_text.is_empty() {
        String::new()
    } else {
        format!("music: {}", track_text)
    };

    let mut other = BTreeMap::new();
    other.insert("playback_status".into(), playback_status.into());
    other.insert("player_name".into(), identity.into());
    other.insert("volume".into(), serde_json::Value::from((volume * 100.0) as i64));
    other.insert("track".into(), track_text.into());

    // Position in seconds.
    // Consumer can interpolate: current_pos = position + (now - position_ts) * rate
    let position_sec = position_us / 1_000_000;
    other.insert("position".into(), serde_json::Value::from(position_sec));
    other.insert("position_str".into(), format_duration(position_sec).into());
    other.insert("rate".into(), serde_json::json!(rate));

    if !title.is_empty() {
        other.insert("title".into(), title.into());
    }
    if !artist.is_empty() {
        other.insert("artist".into(), artist.into());
    }
    if !album.is_empty() {
        other.insert("album".into(), album.into());
    }
    if let Some(len) = length_us {
        let len_sec = len / 1_000_000;
        // Export as seconds.
        other.insert("length".into(), serde_json::Value::from(len_sec));
        other.insert("length_str".into(), format_duration(len_sec).into());
    }

    other.insert("player".into(), short_name(bus_name).into());

    i3bar::Block {
        name: Some("mpris".into()),
        instance: None,
        full_text,
        other,
    }
}

fn empty_block() -> i3bar::Block {
    let mut other = BTreeMap::new();
    other.insert("playback_status".into(), "".into());
    i3bar::Block {
        name: Some("mpris".into()),
        instance: None,
        full_text: String::new(),
        other,
    }
}

fn emit_blocks(blocks: &[i3bar::Block]) {
    if let Ok(output) = serde_json::to_string(blocks) {
        println!("{},", output);
    }
}

/// Stamp blocks with current unix timestamp and emit.
fn stamp_and_emit(blocks: &mut [i3bar::Block]) {
    let now_ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    for block in blocks.iter_mut() {
        block.other.insert("position_ts".into(), serde_json::Value::from(now_ts));
    }
    emit_blocks(blocks);
}

fn format_duration(seconds: i64) -> String {
    let seconds = seconds.max(0);
    let hours = seconds / 3600;
    let minutes = (seconds % 3600) / 60;
    let seconds = seconds % 60;
    if hours > 0 {
        format!("{}:{:02}:{:02}", hours, minutes, seconds)
    } else {
        format!("{:02}:{:02}", minutes, seconds)
    }
}

// ---------------------------------------------------------------------------
// Streaming mode
// ---------------------------------------------------------------------------

async fn fetch_block(
    player: &mpris::player::PlayerProxy<'_>,
    bus_name: &str,
    identity: &str,
) -> i3bar::Block {
    let status = player.playback_status().await.unwrap_or_default();
    let metadata = player.metadata().await.unwrap_or_default();
    let volume = player.volume().await.unwrap_or(0.0);
    let position_us = player.position().await.unwrap_or(0);
    let rate = player.rate().await.unwrap_or(1.0);
    build_block(bus_name, identity, &status, &metadata, volume, position_us, rate)
}

async fn stream_player(
    session: &zbus::Connection,
    bus_name: &str,
) -> anyhow::Result<()> {
    let player = build_player_proxy(session, bus_name).await?;
    let mp2 = build_mp2_proxy(session, bus_name).await?;

    let identity = mp2.identity().await.unwrap_or_else(|_| short_name(bus_name));

    // Emit initial state.
    let mut last_block = fetch_block(&player, bus_name, &identity).await;
    stamp_and_emit(&mut [last_block.clone()]);

    // Listen for PropertiesChanged signals on the MPRIS path.
    let props_proxy = zbus::fdo::PropertiesProxy::new(session, bus_name, MPRIS_PATH)
        .await
        .context("Failed to create PropertiesProxy")?;
    let mut prop_changes = props_proxy.receive_properties_changed().await?;

    // Listen for the Seeked signal (discontinuous position changes).
    let mut seeked_stream = player.receive_seeked().await?;

    // Also watch for the owner disappearing.
    let dbus = zbus::fdo::DBusProxy::new(session).await?;
    let mut owner_changes = dbus.receive_name_owner_changed().await?;

    // Periodic position poll (1s) for smooth progress updates.
    let mut tick = tokio::time::interval(std::time::Duration::from_secs(1));

    loop {
        tokio::select! {
            Some(_change) = prop_changes.next() => {
                debug!("Properties changed for {}", bus_name);
                let block = fetch_block(&player, bus_name, &identity).await;
                last_block = block.clone();
                stamp_and_emit(&mut [block]);
            }
            Some(_seeked) = seeked_stream.next() => {
                debug!("Seeked signal for {}", bus_name);
                let block = fetch_block(&player, bus_name, &identity).await;
                last_block = block.clone();
                stamp_and_emit(&mut [block]);
            }
            _ = tick.tick() => {
                let block = fetch_block(&player, bus_name, &identity).await;
                // Only emit if something changed (skip when paused/idle).
                if block.other != last_block.other || block.full_text != last_block.full_text {
                    last_block = block.clone();
                    stamp_and_emit(&mut [block]);
                }
            }
            Some(owner_change) = owner_changes.next() => {
                if let Ok(args) = owner_change.args() {
                    if args.name().as_str() == bus_name {
                        let new_owner: &str = args.new_owner().as_deref().unwrap_or("");
                        if new_owner.is_empty() {
                            // Player disappeared.
                            debug!("Player {} disconnected", bus_name);
                            return Ok(());
                        }
                    }
                }
            }
        }
    }
}

async fn process_streaming() -> anyhow::Result<()> {
    let stderr_layer = tracing_subscriber::fmt::layer()
        .with_writer(std::io::stderr)
        .with_thread_names(true)
        .compact();

    let filter = if cfg!(debug_assertions) {
        EnvFilter::new("none,oatbar_mpris=debug")
    } else {
        EnvFilter::new("info")
    };

    let registry = tracing_subscriber::registry().with(stderr_layer.with_filter(filter));
    registry.init();

    println!("{}", serde_json::to_string(&i3bar::Header::default())?);
    println!("[");

    let session = zbus::Connection::session().await?;
    let dbus = zbus::fdo::DBusProxy::new(&session).await?;

    loop {
        match find_active_player(&session).await? {
            Some(bus_name) => {
                info!("Streaming from player: {}", bus_name);
                if let Err(e) = stream_player(&session, &bus_name).await {
                    warn!("Player stream ended: {}", e);
                }
                // Player disconnected — emit empty, then look for another.
                emit_blocks(&[empty_block()]);
            }
            None => {
                // No players — emit empty and wait for one to appear.
                emit_blocks(&[empty_block()]);
            }
        }

        // Wait for a new MPRIS player to appear on the bus.
        debug!("Waiting for a new MPRIS player...");
        let mut owner_changes = dbus.receive_name_owner_changed().await?;
        loop {
            if let Some(change) = owner_changes.next().await {
                if let Ok(args) = change.args() {
                    let name = args.name().as_str();
                    let new_owner: &str = args.new_owner().as_deref().unwrap_or("");
                    if name.starts_with(MPRIS_PREFIX) && !new_owner.is_empty() {
                        info!("New MPRIS player appeared: {}", name);
                        break;
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Control commands
// ---------------------------------------------------------------------------

async fn process_command(cmd: Commands) -> anyhow::Result<()> {
    let session = zbus::Connection::session().await?;
    let bus_name = find_active_player(&session)
        .await?
        .context("No MPRIS player found")?;

    let player = build_player_proxy(&session, &bus_name).await?;

    match cmd {
        Commands::Play => player.play().await?,
        Commands::Pause => player.pause().await?,
        Commands::PlayPause => player.play_pause().await?,
        Commands::Next => player.next().await?,
        Commands::Previous => player.previous().await?,
        Commands::Stop => player.stop().await?,
        Commands::Seek { percent } => {
            if !(0.0..=100.0).contains(&percent) {
                anyhow::bail!("Percentage must be between 0 and 100, got {}", percent);
            }
            let metadata = player.metadata().await?;
            let length_us = extract_i64(&metadata, "mpris:length")
                .context("Track has no length metadata")?;
            let track_id = extract_track_id(&metadata)
                .context("Track has no trackid metadata")?;

            let target_us = (length_us as f64 * percent / 100.0) as i64;
            player
                .set_position(&track_id.into(), target_us)
                .await?;
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Some(cmd) => process_command(cmd).await?,
        None => process_streaming().await?,
    }
    Ok(())
}
