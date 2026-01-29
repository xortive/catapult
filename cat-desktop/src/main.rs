//! CAT Multiplexer Desktop Application
//!
//! A desktop application for managing multiple amateur radio transceivers
//! connected to a single amplifier via CAT protocol translation.

// Hide the console window on Windows release builds
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod diagnostics_layer;
mod port_info;
mod radio_panel;
mod settings;
mod simulation_panel;
mod traffic_monitor;

use std::sync::mpsc;
use std::sync::Arc;

use app::CatapultApp;

/// Install a custom panic handler that shows a message box on Windows
fn install_panic_handler() {
    std::panic::set_hook(Box::new(|panic_info| {
        // Capture backtrace immediately
        let backtrace = std::backtrace::Backtrace::force_capture();

        // Format the panic message
        let payload = if let Some(s) = panic_info.payload().downcast_ref::<&str>() {
            s.to_string()
        } else if let Some(s) = panic_info.payload().downcast_ref::<String>() {
            s.clone()
        } else {
            "Unknown panic payload".to_string()
        };

        let location = if let Some(loc) = panic_info.location() {
            format!("{}:{}:{}", loc.file(), loc.line(), loc.column())
        } else {
            "unknown location".to_string()
        };

        let message = format!(
            "Catapult has crashed!\n\n\
             Panic: {}\n\
             Location: {}\n\n\
             Backtrace:\n{}",
            payload, location, backtrace
        );

        // Print to stderr (useful if console is available in debug builds)
        eprintln!("{}", message);

        // On Windows, show a message box
        #[cfg(windows)]
        {
            show_windows_error_dialog(&message);
        }
    }));
}

/// Show an error dialog on Windows using the native MessageBox API
#[cfg(windows)]
fn show_windows_error_dialog(message: &str) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK};

    // Convert strings to wide (UTF-16) for Windows API
    fn to_wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    let title = to_wide("Catapult Crash");

    // Truncate message if too long for message box (keep first ~4000 chars)
    let truncated_msg = if message.len() > 4000 {
        format!("{}...\n\n[Message truncated]", &message[..4000])
    } else {
        message.to_string()
    };
    let msg = to_wide(&truncated_msg);

    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            msg.as_ptr(),
            title.as_ptr(),
            MB_OK | MB_ICONERROR,
        );
    }
}
use diagnostics_layer::{
    DiagnosticEvent, DiagnosticLevelState, DiagnosticsLayer, ProjectCrateFilter,
};
use eframe::NativeOptions;
use settings::Settings;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::Layer;

fn main() -> eframe::Result<()> {
    // Install panic handler first, before anything else can panic
    install_panic_handler();

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
