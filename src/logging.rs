use anyhow::Context;
use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::prelude::*;

pub fn init(instance_name: &str) -> anyhow::Result<tracing_appender::non_blocking::WorkerGuard> {
    let log_dir = dirs::cache_dir()
        .expect("Could not find cache directory")
        .join("oatbar")
        .join("logs");
    std::fs::create_dir_all(&log_dir).context("Failed to create log directory")?;

    let log_filename = format!("{}.log", instance_name);
    let file_appender = tracing_appender::rolling::Builder::new()
        .rotation(tracing_appender::rolling::Rotation::DAILY)
        .filename_prefix(&log_filename)
        .max_log_files(5)
        .build(&log_dir)
        .context("Failed to create file appender")?;
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);

    let file_layer = tracing_subscriber::fmt::layer()
        .with_writer(non_blocking)
        .with_ansi(false)
        .with_thread_names(true)
        .compact();

    let stderr_layer = tracing_subscriber::fmt::layer()
        .with_writer(std::io::stderr)
        .with_thread_names(true)
        .compact();

    let stderr_level = if cfg!(debug_assertions) {
        LevelFilter::TRACE
    } else {
        LevelFilter::INFO
    };

    let registry = tracing_subscriber::registry()
        .with(file_layer.with_filter(stderr_level))
        .with(stderr_layer.with_filter(stderr_level));

    registry.init();

    Ok(guard)
}
