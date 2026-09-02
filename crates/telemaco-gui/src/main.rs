//! telemaco-gui: a native window over the Telemaco engine.
//!
//! Embeds the CDP layer in-process (the same way telemaco-mcp does) and drives
//! it from the UI thread through two channels. See the crate README for usage
//! and packaging.

use clap::Parser;

use telemaco_gui::engine;

#[derive(Parser, Debug)]
#[command(
    name = "telemaco-gui",
    version,
    about = "Interactive native window over the Telemaco headless browser engine"
)]
struct Args {
    /// URL to open at startup (defaults to about:blank)
    url: Option<String>,
    /// HTTP/SOCKS proxy URL
    #[arg(long)]
    proxy: Option<String>,
    /// Stealth fingerprinting (wreq transport where built)
    #[arg(long)]
    stealth: bool,
    /// Override the user agent
    #[arg(long)]
    user_agent: Option<String>,
    /// Allow navigation to loopback/private addresses (SSRF gate)
    #[arg(long)]
    allow_private_network: bool,
}

fn main() {
    let args = Args::parse();
    if args.allow_private_network {
        std::env::set_var("TELEMACO_ALLOW_PRIVATE_NETWORK", "1");
    }

    let (command_tx, update_rx) = engine::spawn(engine::EngineOptions {
        proxy: args.proxy,
        stealth: args.stealth,
        user_agent: args.user_agent,
        start_url: args.url,
    });

    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 860.0])
            .with_min_inner_size([640.0, 420.0])
            .with_title("Telemaco"),
        ..Default::default()
    };
    // eframe::Error is not Send/Sync, so it cannot go through anyhow's `?`.
    if let Err(error) = eframe::run_native(
        "Telemaco",
        options,
        Box::new(move |cc| {
            Ok(Box::new(telemaco_gui::app::TelemacoApp::new(cc, command_tx, update_rx)))
        }),
    ) {
        eprintln!("telemaco-gui: window loop failed: {error}");
        std::process::exit(1);
    }
}