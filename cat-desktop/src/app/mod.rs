//! Main application state and UI
//!
//! This module contains the core `CatapultApp` struct and is organized into submodules:
//! - `ports`: Port enumeration and validation
//! - `status`: Status messaging and settings save helpers
//! - `radio`: Radio management (COM and virtual)
//! - `events`: Event processing from mux actor and background tasks
//! - `amplifier`: Amplifier connection and management
//! - `ui_panels`: UI panel drawing methods

mod amplifier;
mod events;
mod ports;
mod radio;
mod status;
mod ui_panels;

use std::collections::HashMap;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::Arc;
use std::time::Instant;

use cat_detect::{PortScanner, ProbeResult, SerialPortInfo};
use cat_mux::{
    run_mux_actor, MuxActorCommand, MuxEvent, RadioHandle, RadioStateSummary, RadioTaskCommand,
    SwitchingMode,
};
use cat_protocol::{OperatingMode, Protocol};
use eframe::CreationContext;
use tokio::sync::{mpsc as tokio_mpsc, oneshot};
use tracing::Level;

use cat_sim::VirtualRadioCommand;

use crate::diagnostics_layer::{DiagnosticEvent, DiagnosticLevelState};
use crate::radio_panel::RadioPanel;
use crate::settings::Settings;
use crate::simulation_panel::{SimulationAction, SimulationPanel};
use crate::traffic_monitor::TrafficMonitor;

/// Connection type for amplifier
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AmplifierConnectionType {
    /// Physical amplifier connected via COM/serial port
    ComPort,
    /// Simulated amplifier (commands go to traffic monitor)
    Simulated,
}

/// Get a display name for an operating mode
pub(crate) fn mode_name(mode: OperatingMode) -> &'static str {
    match mode {
        OperatingMode::Lsb => "LSB",
        OperatingMode::Usb => "USB",
        OperatingMode::Cw => "CW",
        OperatingMode::CwR => "CW-R",
        OperatingMode::Am => "AM",
        OperatingMode::Fm => "FM",
        OperatingMode::FmN => "FM-N",
        OperatingMode::Dig => "DIG",
        OperatingMode::DigU => "DIG-U",
        OperatingMode::DigL => "DIG-L",
        OperatingMode::Pkt => "PKT",
        OperatingMode::Data => "DATA",
        OperatingMode::DataU => "DATA-U",
        OperatingMode::DataL => "DATA-L",
        OperatingMode::Rtty => "RTTY",
        OperatingMode::RttyR => "RTTY-R",
    }
}

/// Messages from background tasks
pub enum BackgroundMessage {
    /// Probe completed (from manual probe button)
    ProbeComplete {
        port: String,
        baud_rate: u32,
        result: Option<ProbeResult>,
    },
    /// Radio registered with mux actor (handle assigned)
    RadioRegistered {
        correlation_id: u64,
        handle: RadioHandle,
    },
    /// Radio successfully connected (async task started)
    RadioConnected {
        handle: RadioHandle,
        model: String,
        port: String,
    },
    /// Radio state sync response from mux actor
    RadioStateSync {
        handle: RadioHandle,
        state: RadioStateSummary,
    },
}

/// Configuration for connecting a COM port radio
pub(super) struct ComRadioConfig {
    pub port: String,
    pub protocol: Protocol,
    pub baud_rate: u32,
    pub civ_address: Option<u8>,
    pub model_name: String,
    /// Whether to query initial state (skip for scanned radios that already have state)
    pub query_initial_state: bool,
}

/// Main application state
pub struct CatapultApp {
    /// Settings
    pub(super) settings: Settings,
    /// Port scanner
    pub(super) scanner: PortScanner,
    /// Available serial ports
    pub(super) available_ports: Vec<SerialPortInfo>,
    /// Radio panels for UI (unified list of COM and Virtual radios)
    pub(super) radio_panels: Vec<RadioPanel>,
    /// Async radio task command senders (keyed by radio_id) -> (port_name, cmd_sender)
    pub(super) radio_task_senders: HashMap<u32, (String, tokio_mpsc::Sender<RadioTaskCommand>)>,
    /// Traffic monitor
    pub(super) traffic_monitor: TrafficMonitor,
    /// Status message
    pub(super) status_message: Option<(String, Instant)>,
    /// Show settings panel
    pub(super) show_settings: bool,
    /// Show traffic monitor/log console
    pub(super) show_traffic_monitor: bool,
    /// Simulation panel for virtual radio state management
    pub(super) simulation_panel: SimulationPanel,
    /// Background message receiver
    pub(super) bg_rx: Receiver<BackgroundMessage>,
    /// Background message sender (for cloning to tasks)
    pub(super) bg_tx: Sender<BackgroundMessage>,
    /// Tokio runtime handle for spawning async tasks
    pub(super) rt_handle: tokio::runtime::Handle,
    /// Selected amplifier port
    pub(super) amp_port: String,
    /// Selected amplifier protocol
    pub(super) amp_protocol: Protocol,
    /// Selected amplifier baud rate
    pub(super) amp_baud: u32,
    /// CI-V address for Icom amplifiers (0x00-0xFF)
    pub(super) amp_civ_address: u8,
    /// Amplifier connection type
    pub(super) amp_connection_type: AmplifierConnectionType,
    /// Amplifier data sender (for async amplifier task)
    pub(super) amp_data_tx: Option<tokio_mpsc::Sender<Vec<u8>>>,
    /// Amplifier shutdown sender
    pub(super) amp_shutdown_tx: Option<oneshot::Sender<()>>,
    /// Maps simulation radio IDs to RadioHandle
    pub(super) sim_radio_ids: HashMap<String, RadioHandle>,
    /// Selected port for adding a new COM radio
    pub(super) add_radio_port: String,
    /// Selected protocol for adding a new COM radio
    pub(super) add_radio_protocol: Protocol,
    /// Selected baud rate for adding a new COM radio
    pub(super) add_radio_baud: u32,
    /// CI-V address for new Icom COM radio
    pub(super) add_radio_civ_address: u8,
    /// Model name for new radio (from probe or manual entry)
    pub(super) add_radio_model: String,
    /// Is probing in progress
    pub(super) probing: bool,
    /// Diagnostic event receiver (from tracing layer)
    pub(super) diag_rx: Receiver<DiagnosticEvent>,
    /// Next correlation_id to assign for pending registrations
    pub(super) next_correlation_id: u64,
    /// Mux command sender (for sending commands to mux actor)
    pub(super) mux_cmd_tx: tokio_mpsc::Sender<MuxActorCommand>,
    /// Mux event sender (for async connection tasks to send events)
    pub(super) mux_event_tx: tokio_mpsc::Sender<MuxEvent>,
    /// Mux event receiver (for receiving events from mux actor)
    pub(super) mux_event_rx: tokio_mpsc::Receiver<MuxEvent>,
    /// Pending registrations: correlation_id -> panel index
    pub(super) pending_registrations: HashMap<u64, usize>,
    /// Currently active radio handle (tracked locally from events)
    pub(super) active_radio: Option<RadioHandle>,
    /// Current switching mode (tracked locally from events)
    pub(super) switching_mode: SwitchingMode,
    /// Pending radio configs awaiting handle from mux actor
    pub(super) pending_radio_configs: HashMap<u64, ComRadioConfig>,
    /// Virtual radio task shutdown senders (keyed by sim_id)
    pub(super) virtual_radio_task_senders: HashMap<String, tokio_mpsc::Sender<RadioTaskCommand>>,
    /// Next simulation ID counter for virtual radios
    pub(super) next_sim_id: u32,
    /// Last time we synced radio states with mux actor
    pub(super) last_state_sync: Instant,
    /// Tokio runtime (must be kept alive for async tasks)
    _runtime: Option<tokio::runtime::Runtime>,
    /// Shared state for dynamic diagnostics level filtering
    pub(super) diagnostic_level_state: Arc<DiagnosticLevelState>,
    /// Previous diagnostic level (for detecting changes)
    pub(super) prev_diagnostic_level: Option<Level>,
}

impl CatapultApp {
    /// Create a new application
    pub fn new(
        _cc: &CreationContext<'_>,
        diag_rx: Receiver<DiagnosticEvent>,
        runtime: tokio::runtime::Runtime,
        diagnostic_level_state: Arc<DiagnosticLevelState>,
    ) -> Self {
        let rt_handle = runtime.handle().clone();
        let (bg_tx, bg_rx) = std::sync::mpsc::channel();
        let settings = Settings::load();

        // Restore amplifier settings
        let amp_connection_type = if settings.amplifier.connection_type == "com" {
            AmplifierConnectionType::ComPort
        } else {
            AmplifierConnectionType::Simulated
        };

        // Create channels for mux actor
        let (mux_cmd_tx, mux_cmd_rx) = tokio_mpsc::channel::<MuxActorCommand>(256);
        let (mux_event_tx, mux_event_rx) = tokio_mpsc::channel::<MuxEvent>(256);

        // Clone mux_event_tx for async connection tasks (they'll send events on the same channel)
        let mux_event_tx_for_actor = mux_event_tx.clone();

        // Spawn the mux actor (from cat-mux crate)
        rt_handle.spawn(async move {
            run_mux_actor(mux_cmd_rx, mux_event_tx_for_actor).await;
            tracing::error!("Mux actor exited unexpectedly");
        });

        // Track initial diagnostic level for change detection
        let initial_diagnostic_level = settings.diagnostic_level;

        let mut app = Self {
            traffic_monitor: TrafficMonitor::new(
                settings.traffic_history_size,
                settings.diagnostic_level,
            ),
            scanner: PortScanner::new(),
            available_ports: Vec::new(),
            radio_panels: Vec::new(),
            radio_task_senders: HashMap::new(),
            status_message: None,
            show_settings: false,
            show_traffic_monitor: true,
            simulation_panel: SimulationPanel::new(),
            bg_rx,
            bg_tx,
            rt_handle,
            amp_port: settings.amplifier.port.clone(),
            amp_protocol: settings.amplifier.protocol,
            amp_baud: settings.amplifier.baud_rate,
            amp_civ_address: settings.amplifier.civ_address,
            amp_connection_type,
            amp_data_tx: None,
            amp_shutdown_tx: None,
            sim_radio_ids: HashMap::new(),
            add_radio_port: String::new(),
            add_radio_protocol: Protocol::Kenwood,
            add_radio_baud: 9600,
            add_radio_civ_address: 0x00,
            add_radio_model: String::new(),
            probing: false,
            diag_rx,
            settings,
            next_correlation_id: 1,
            mux_cmd_tx,
            mux_event_tx,
            mux_event_rx,
            pending_registrations: HashMap::new(),
            active_radio: None,
            switching_mode: SwitchingMode::default(),
            pending_radio_configs: HashMap::new(),
            virtual_radio_task_senders: HashMap::new(),
            next_sim_id: 1,
            last_state_sync: Instant::now(),
            _runtime: Some(runtime),
            diagnostic_level_state,
            prev_diagnostic_level: initial_diagnostic_level,
        };

        // Initial port enumeration
        app.refresh_ports();

        // Restore virtual radios from settings
        for config in app.settings.virtual_radios.clone() {
            app.add_virtual_radio_from_config(config);
        }

        // Restore configured COM radios from settings
        app.restore_configured_radios();

        app
    }

    /// Send a command to the mux actor, logging a warning if the channel is full
    pub(super) fn send_mux_command(&self, cmd: MuxActorCommand, context: &str) {
        match self.mux_cmd_tx.try_send(cmd) {
            Ok(()) => {}
            Err(tokio_mpsc::error::TrySendError::Full(_)) => {
                tracing::warn!(
                    source = "MuxChannel",
                    "Failed to send {} command: channel full",
                    context,
                );
            }
            Err(tokio_mpsc::error::TrySendError::Closed(_)) => {
                tracing::error!(
                    source = "MuxChannel",
                    "Failed to send {} command: channel CLOSED (mux actor not running!)",
                    context,
                );
            }
        }
    }

    /// Send a command to a radio task, logging a warning if the channel is full
    pub(super) fn send_radio_task_command(
        sender: &tokio_mpsc::Sender<RadioTaskCommand>,
        cmd: RadioTaskCommand,
        context: &str,
    ) {
        if let Err(e) = sender.try_send(cmd) {
            tracing::warn!(
                source = "RadioTask",
                "Failed to send {} command: {} (channel full or closed)",
                context,
                e
            );
        }
    }
}

impl eframe::App for CatapultApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Process background messages and events (non-blocking)
        // All I/O data now comes through channels from async tasks
        // Command processing happens in mux actor - UI receives events
        self.process_diagnostic_events();
        self.process_messages();
        self.process_mux_events();
        self.maybe_sync_radio_states();

        // Clear old status messages
        if let Some((_, when)) = &self.status_message {
            if when.elapsed().as_secs() > 5 {
                self.status_message = None;
            }
        }

        // Top panel - toolbar
        egui::TopBottomPanel::top("toolbar").show(ctx, |ui| {
            self.draw_toolbar(ui);
        });

        // Settings panel (side panel)
        if self.show_settings {
            egui::SidePanel::right("settings")
                .default_width(300.0)
                .show(ctx, |ui| {
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        ui.heading("Amplifier");
                        ui.separator();
                        self.draw_amplifier_panel(ui);

                        ui.add_space(16.0);
                        ui.heading("Switching");
                        ui.separator();
                        self.draw_switching_panel(ui);

                        ui.add_space(16.0);
                        ui.heading("Settings");
                        ui.separator();
                        if let Some(error) = self.settings.draw(ui) {
                            self.handle_save_error(error);
                        }

                        ui.add_space(16.0);
                        ui.separator();
                        if ui.button("Close").clicked() {
                            self.show_settings = false;
                        }
                    });
                });
        }

        // Console panel - pops out from right when shown
        if self.show_traffic_monitor {
            egui::SidePanel::right("console")
                .default_width(400.0)
                .min_width(300.0)
                .show(ctx, |ui| {
                    self.draw_traffic_panel(ui);
                });
        }

        // Central panel - radio list (takes full space when console is closed)
        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                self.draw_radio_panel(ui);

                // Simulation panel (debug mode) - show at bottom of central panel
                ui.add_space(16.0);
                ui.separator();
                if let Some(action) = self.simulation_panel.ui(ui) {
                    match action {
                        SimulationAction::AddRadio { name, protocol } => {
                            self.add_virtual_radio(&name, protocol);
                        }
                        SimulationAction::RemoveRadio { sim_id } => {
                            self.remove_virtual_radio(&sim_id);
                        }
                    }
                }
            });
        });

        // Request repaint when we have active connections (for receiving async messages)
        let has_virtual_radios = self.radio_panels.iter().any(|p| p.is_virtual());
        let has_com_radios = !self.radio_task_senders.is_empty();
        let has_amplifier = self.amp_data_tx.is_some();

        if has_virtual_radios || has_com_radios || has_amplifier {
            ctx.request_repaint();
        }
    }
}
