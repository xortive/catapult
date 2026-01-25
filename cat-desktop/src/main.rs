//! CAT Multiplexer Desktop Application
//!
//! A desktop application for managing multiple amateur radio transceivers
//! connected to a single amplifier via CAT protocol translation.

mod app;
mod diagnostics_layer;
mod radio_panel;
mod settings;
mod simulation_panel;
mod traffic_monitor;
mod virtual_radio_task;

use std::sync::mpsc;
use std::sync::Arc;

use app::CatapultApp;
use diagnostics_layer::{DiagnosticEvent, DiagnosticLevelState, DiagnosticsLayer, ProjectCrateFilter};
use eframe::NativeOptions;
use settings::Settings;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::Layer;

fn main() -> eframe::Result<()> {
    // Load settings to get initial diagnostic level
    let settings = Settings::load();

    // Create channel for diagnostic events (before tracing init so we can capture all logs)
    let (diag_tx, diag_rx) = mpsc::channel::<DiagnosticEvent>();

    // Create shared state for dynamic level filtering (atomic, no parsing overhead on changes)
    let diagnostic_level_state = Arc::new(DiagnosticLevelState::new(settings.diagnostic_level));
    let diagnostics_filter = ProjectCrateFilter::new(Arc::clone(&diagnostic_level_state));

    // Initialize logging with two separate filter chains:
    // 1. Diagnostics layer - filtered by ProjectCrateFilter with atomic level state
    // 2. Console output (fmt layer) - always shows debug level for our crates
    tracing_subscriber::registry()
        .with(DiagnosticsLayer::new(diag_tx).with_filter(diagnostics_filter))
        .with(tracing_subscriber::fmt::layer().with_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
                "catapult=debug,cat_protocol=debug,cat_detect=debug,cat_mux=debug,cat_sim=debug"
                    .into()
            }),
        ))
        .init();

    tracing::info!("Starting Catapult CAT Multiplexer");

    // Create global tokio runtime for async serial I/O
    let rt = tokio::runtime::Runtime::new().expect("Failed to create tokio runtime");

    let options = NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1024.0, 768.0])
            .with_min_inner_size([800.0, 600.0])
            .with_title("Catapult - CAT Multiplexer"),
        ..Default::default()
    };

    // Pass runtime and level state to app (app stores runtime to keep alive, state for level updates)
    eframe::run_native(
        "Catapult",
        options,
        Box::new(move |cc| {
            Ok(Box::new(CatapultApp::new(
                cc,
                diag_rx,
                rt,
                diagnostic_level_state,
            )))
        }),
    )
}
