//! CAT Multiplexer Desktop Application
//!
//! A desktop application for managing multiple amateur radio transceivers
//! connected to a single amplifier via CAT protocol translation.

mod amp_task;
mod app;
mod async_serial;
mod diagnostics_layer;
mod radio_panel;
mod settings;
mod simulation_panel;
mod traffic_monitor;

use std::sync::mpsc;

use app::CatapultApp;
use diagnostics_layer::{build_diagnostics_filter, DiagnosticEvent, DiagnosticsLayer};
use eframe::NativeOptions;
use settings::Settings;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::reload;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::Layer;

fn main() -> eframe::Result<()> {
    // Load settings to get initial diagnostic level
    let settings = Settings::load();

    // Create channel for diagnostic events (before tracing init so we can capture all logs)
    let (diag_tx, diag_rx) = mpsc::channel::<DiagnosticEvent>();

    // Build initial filter based on saved diagnostic level
    let initial_filter = build_diagnostics_filter(settings.diagnostic_level);

    // Create reload layer for dynamic filter updates
    // This filter controls what events reach the DiagnosticsLayer
    let (diagnostics_filter, diagnostics_filter_handle) =
        reload::Layer::<tracing_subscriber::filter::EnvFilter, _>::new(initial_filter);

    // Initialize logging with two separate filter chains:
    // 1. Diagnostics layer - filtered by dynamic reload layer controlled by UI (attached first)
    // 2. Console output (fmt layer) - always shows debug level for our crates
    //
    // Order matters: diagnostics layer is attached to Registry first so the reload handle
    // type matches DiagnosticsFilterHandle (Handle<EnvFilter, Registry>)
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

    // Pass runtime and filter handle to app (app stores them to keep alive and manage updates)
    eframe::run_native(
        "Catapult",
        options,
        Box::new(move |cc| {
            Ok(Box::new(CatapultApp::new(
                cc,
                diag_rx,
                rt,
                diagnostics_filter_handle,
            )))
        }),
    )
}
