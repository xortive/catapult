//! CAT Multiplexer Desktop Application
//!
//! A desktop application for managing multiple amateur radio transceivers
//! connected to a single amplifier via CAT protocol translation.

mod app;
mod diagnostics_layer;
mod radio_panel;
mod serial_io;
mod settings;
mod simulation_panel;
mod traffic_monitor;

use std::sync::mpsc;

use app::CatapultApp;
use diagnostics_layer::{DiagnosticEvent, DiagnosticsLayer};
use eframe::NativeOptions;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

fn main() -> eframe::Result<()> {
    // Create channel for diagnostic events (before tracing init so we can capture all logs)
    let (diag_tx, diag_rx) = mpsc::channel::<DiagnosticEvent>();

    // Initialize logging with our custom diagnostics layer
    // Include all our crates in the default filter
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
                "catapult=info,cat_protocol=info,cat_detect=info,cat_mux=info,cat_sim=info".into()
            }),
        )
        .with(tracing_subscriber::fmt::layer())
        .with(DiagnosticsLayer::new(diag_tx))
        .init();

    tracing::info!("Starting Catapult CAT Multiplexer");

    let options = NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1024.0, 768.0])
            .with_min_inner_size([800.0, 600.0])
            .with_title("Catapult - CAT Multiplexer"),
        ..Default::default()
    };

    eframe::run_native(
        "Catapult",
        options,
        Box::new(move |cc| Ok(Box::new(CatapultApp::new(cc, diag_rx)))),
    )
}
