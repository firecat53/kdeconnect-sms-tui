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

    // Register HEIC/HEIF decoder hooks so the image crate can decode them.
    libheif_rs::integration::image::register_all_decoding_hooks();


    // Install a panic hook that restores the terminal before printing
    // the panic message, so the user doesn't end up with an unusable
    // terminal.
    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        let _ = crossterm::terminal::disable_raw_mode();
        let _ = crossterm::execute!(
            std::io::stdout(),
            crossterm::terminal::LeaveAlternateScreen
        );
        original_hook(panic_info);
    }));

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
