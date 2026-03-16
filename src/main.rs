mod app;
mod config;
mod contacts;
mod dbus;
mod events;
mod models;
mod ui;

use clap::Parser;
use color_eyre::Result;
use tracing_subscriber::{fmt, EnvFilter};

#[derive(Parser, Debug)]
#[command(name = "kdeconnect-sms-tui", about = "TUI SMS client via KDE Connect")]
struct Args {
    /// Device ID to connect to (default: first available)
    #[arg(short, long)]
    device: Option<String>,

    /// Device name to connect to (alternative to --device)
    #[arg(short, long)]
    name: Option<String>,

    /// Log file path (logs are suppressed if not set)
    #[arg(long)]
    log_file: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;
    let args = Args::parse();

    // Only enable tracing if --log-file is specified; writing to stderr
    // corrupts the TUI display (especially inside tmux).
    if let Some(ref path) = args.log_file {
        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)?;
        fmt()
            .with_env_filter(EnvFilter::from_default_env())
            .with_writer(file)
            .with_ansi(false)
            .init();
    }

    let config = config::Config::load()?;

    let mut app = app::App::new(config, args.device, args.name).await?;
    app.run().await
}
