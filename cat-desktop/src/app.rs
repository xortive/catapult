//! Main application state and UI

use std::collections::{HashMap, HashSet};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};

use cat_detect::{probe_port, PortScanner, ProbeResult, SerialPortInfo};
use cat_mux::{
    run_mux_actor, AmplifierChannel, AmplifierChannelMeta, MuxActorCommand, MuxEvent,
    RadioChannelMeta, RadioHandle, RadioStateSummary, SwitchingMode, VirtualAmplifierIo,
};
use cat_protocol::{OperatingMode, Protocol};
use cat_sim::VirtualRadio;
use eframe::CreationContext;
use egui::{Color32, RichText, Ui};
use tokio::sync::{mpsc as tokio_mpsc, oneshot};
use tokio_serial::SerialPortBuilderExt;
use tracing::Level;

use crate::amp_task::run_amp_task;
use crate::async_serial::{AsyncRadioConnection, RadioTaskCommand};
use crate::diagnostics_layer::{DiagnosticEvent, DiagnosticLevelState};
use crate::virtual_radio_task::{run_virtual_radio_task, VirtualRadioCommand};
use crate::radio_panel::RadioPanel;
use crate::settings::{AmplifierSettings, ConfiguredRadio, Settings};
use crate::simulation_panel::{SimulationAction, SimulationPanel};
use crate::traffic_monitor::{DiagnosticSeverity, ExportAction, TrafficMonitor};

/// Connection type for amplifier
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AmplifierConnectionType {
    /// Physical amplifier connected via COM/serial port
    ComPort,
    /// Simulated amplifier (commands go to traffic monitor)
    Simulated,
}

/// Get a display name for an operating mode
fn mode_name(mode: OperatingMode) -> &'static str {
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
    /// I/O error from radio or amplifier
    IoError { source: String, message: String },
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
    /// Radio disconnected (async task ended)
    RadioDisconnected { handle: RadioHandle },
    /// Radio state sync response from mux actor
    RadioStateSync {
        handle: RadioHandle,
        state: RadioStateSummary,
    },
}

/// Configuration for connecting a COM port radio
struct ComRadioConfig {
    port: String,
    protocol: Protocol,
    baud_rate: u32,
    civ_address: Option<u8>,
    model_name: String,
    /// Whether to query initial state (skip for scanned radios that already have state)
    query_initial_state: bool,
}

/// Main application state
pub struct CatapultApp {
    /// Settings
    settings: Settings,
    /// Port scanner
    scanner: PortScanner,
    /// Available serial ports
    available_ports: Vec<SerialPortInfo>,
    /// Radio panels for UI (unified list of COM and Virtual radios)
    radio_panels: Vec<RadioPanel>,
    /// Async radio task command senders (keyed by radio_id) -> (port_name, cmd_sender)
    radio_task_senders: HashMap<u32, (String, tokio_mpsc::Sender<RadioTaskCommand>)>,
    /// Traffic monitor
    traffic_monitor: TrafficMonitor,
    /// Status message
    status_message: Option<(String, Instant)>,
    /// Show settings panel
    show_settings: bool,
    /// Show traffic monitor/log console
    show_traffic_monitor: bool,
    /// Simulation panel for virtual radio state management
    simulation_panel: SimulationPanel,
    /// Background message receiver
    bg_rx: Receiver<BackgroundMessage>,
    /// Background message sender (for cloning to tasks)
    bg_tx: Sender<BackgroundMessage>,
    /// Tokio runtime handle for spawning async tasks
    rt_handle: tokio::runtime::Handle,
    /// Selected amplifier port
    amp_port: String,
    /// Selected amplifier protocol
    amp_protocol: Protocol,
    /// Selected amplifier baud rate
    amp_baud: u32,
    /// CI-V address for Icom amplifiers (0x00-0xFF)
    amp_civ_address: u8,
    /// Amplifier connection type
    amp_connection_type: AmplifierConnectionType,
    /// Amplifier data sender (for async amplifier task)
    amp_data_tx: Option<tokio_mpsc::Sender<Vec<u8>>>,
    /// Amplifier shutdown sender
    amp_shutdown_tx: Option<oneshot::Sender<()>>,
    /// Maps simulation radio IDs to RadioHandle
    sim_radio_ids: HashMap<String, RadioHandle>,
    /// Selected port for adding a new COM radio
    add_radio_port: String,
    /// Selected protocol for adding a new COM radio
    add_radio_protocol: Protocol,
    /// Selected baud rate for adding a new COM radio
    add_radio_baud: u32,
    /// CI-V address for new Icom COM radio
    add_radio_civ_address: u8,
    /// Model name for new radio (from probe or manual entry)
    add_radio_model: String,
    /// Is probing in progress
    probing: bool,
    /// Diagnostic event receiver (from tracing layer)
    diag_rx: Receiver<DiagnosticEvent>,
    /// Next correlation_id to assign for pending registrations
    next_correlation_id: u64,
    /// Mux command sender (for sending commands to mux actor)
    mux_cmd_tx: tokio_mpsc::Sender<MuxActorCommand>,
    /// Mux event receiver (for receiving events from mux actor)
    mux_event_rx: tokio_mpsc::Receiver<MuxEvent>,
    /// Pending registrations: correlation_id -> panel index
    pending_registrations: HashMap<u64, usize>,
    /// Currently active radio handle (tracked locally from events)
    active_radio: Option<RadioHandle>,
    /// Current switching mode (tracked locally from events)
    switching_mode: SwitchingMode,
    /// Pending radio configs awaiting handle from mux actor
    pending_radio_configs: HashMap<u64, ComRadioConfig>,
    /// Virtual radio task shutdown senders (keyed by sim_id)
    virtual_radio_task_senders: HashMap<String, tokio_mpsc::Sender<RadioTaskCommand>>,
    /// Next simulation ID counter for virtual radios
    next_sim_id: u32,
    /// Last time we synced radio states with mux actor
    last_state_sync: Instant,
    /// Tokio runtime (must be kept alive for async tasks)
    _runtime: Option<tokio::runtime::Runtime>,
    /// Shared state for dynamic diagnostics level filtering
    diagnostic_level_state: Arc<DiagnosticLevelState>,
    /// Previous diagnostic level (for detecting changes)
    prev_diagnostic_level: Option<Level>,
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
        let (bg_tx, bg_rx) = mpsc::channel();
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

        // Spawn the mux actor (from cat-mux crate)
        rt_handle.spawn(async move {
            run_mux_actor(mux_cmd_rx, mux_event_tx).await;
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

    /// Refresh available ports (sync version for initialization)
    fn refresh_ports(&mut self) {
        match self.scanner.enumerate_ports() {
            Ok(ports) => {
                self.available_ports = ports;
                self.validate_port_selections();
            }
            Err(e) => {
                self.report_warning("System", format!("Failed to enumerate ports: {}", e));
            }
        }
    }

    /// Validate port selections after port list changes
    fn validate_port_selections(&mut self) {
        // Clear add_radio_port if it's no longer available
        if !self.add_radio_port.is_empty() {
            let port_exists = self
                .available_ports
                .iter()
                .any(|p| p.port == self.add_radio_port);
            if !port_exists {
                self.add_radio_port.clear();
            }
        }

        // Clear amp_port if it's no longer available or is now used by a radio
        if !self.amp_port.is_empty() {
            let port_exists = self.available_ports.iter().any(|p| p.port == self.amp_port);
            let in_use_by_radio = self.radio_ports_in_use().contains(&self.amp_port);
            if !port_exists || in_use_by_radio {
                self.amp_port.clear();
                if self.amp_data_tx.is_some() {
                    self.disconnect_amplifier();
                    self.set_status("Amplifier disconnected: port no longer available".into());
                }
                self.save_amplifier_settings();
            }
        }
    }

    /// Save current virtual radios to settings
    ///
    /// Gets state from SimulationPanel's radio_states since the actual VirtualRadio
    /// instances are owned by the actor tasks.
    fn save_virtual_radios(&mut self) {
        use cat_sim::VirtualRadioConfig;

        // Get configs from SimulationPanel's display state
        let configs: Vec<VirtualRadioConfig> = self
            .simulation_panel
            .get_radio_configs()
            .collect();

        if self.settings.virtual_radios != configs {
            self.settings.virtual_radios = configs;
            if let Err(e) = self.settings.save() {
                self.handle_save_error(e);
            }
        }
    }

    /// Save current amplifier settings
    fn save_amplifier_settings(&mut self) {
        let amp_settings = AmplifierSettings {
            connection_type: match self.amp_connection_type {
                AmplifierConnectionType::ComPort => "com".to_string(),
                AmplifierConnectionType::Simulated => "simulated".to_string(),
            },
            protocol: self.amp_protocol,
            port: self.amp_port.clone(),
            baud_rate: self.amp_baud,
            civ_address: self.amp_civ_address,
        };

        if self.settings.amplifier != amp_settings {
            self.settings.amplifier = amp_settings;
            if let Err(e) = self.settings.save() {
                self.handle_save_error(e);
            }
        }
    }

    /// Restore configured COM radios from settings
    fn restore_configured_radios(&mut self) {
        let available_ports: std::collections::HashSet<_> = self
            .available_ports
            .iter()
            .map(|p| p.port.clone())
            .collect();

        for config in self.settings.configured_radios.clone() {
            let port_available = available_ports.contains(&config.port);

            // Create ComRadioConfig
            let com_config = ComRadioConfig {
                port: config.port.clone(),
                protocol: config.protocol,
                baud_rate: config.baud_rate,
                civ_address: config.civ_address,
                model_name: config.model_name.clone(),
                query_initial_state: false,
            };

            if port_available {
                // Create RadioPanel with no handle (will be updated when handle arrives)
                let panel = RadioPanel::new_from_config(None, &config);
                self.radio_panels.push(panel);
                let panel_index = self.radio_panels.len() - 1;

                // Register with mux actor (handle will arrive via RadioRegistered message)
                let _correlation_id = self.register_com_radio(com_config, panel_index);
            } else {
                // Port not available - create panel without registering
                let mut panel = RadioPanel::new_from_config(None, &config);
                panel.unavailable = true;
                self.radio_panels.push(panel);
                self.report_warning("Radio", format!("{} not available", config.port));
            }
        }
    }

    /// Spawn an async task for a radio connection
    #[allow(clippy::too_many_arguments)]
    fn spawn_radio_task(
        &mut self,
        handle: RadioHandle,
        port: String,
        baud_rate: u32,
        protocol: Protocol,
        civ_address: Option<u8>,
        model_name: String,
        query_initial_state: bool,
    ) {
        let tx = self.bg_tx.clone();
        let mux_tx = self.mux_cmd_tx.clone();
        let rt = self.rt_handle.clone();

        // Create channel for sending commands to the task
        let (cmd_tx, cmd_rx) = tokio_mpsc::channel::<RadioTaskCommand>(32);

        // Store the sender so we can send commands to this radio (keyed by handle)
        self.radio_task_senders
            .insert(handle.0, (port.clone(), cmd_tx));

        // Spawn the async connection task
        rt.spawn(async move {
            match AsyncRadioConnection::connect(
                handle,
                &port,
                baud_rate,
                protocol,
                tx.clone(),
                mux_tx,
            ) {
                Ok(mut conn) => {
                    // Set CI-V address for Icom radios
                    if let Some(civ_addr) = civ_address {
                        conn.set_civ_address(civ_addr);
                    }

                    // Small delay to let the radio settle after port open
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

                    // Query radio ID to get actual model name
                    let actual_model_name = conn.query_id().await.unwrap_or(model_name);

                    // Query initial state if requested
                    if query_initial_state {
                        if let Err(e) = conn.query_initial_state().await {
                            tracing::warn!("Failed to query initial state on {}: {}", port, e);
                        }
                    }

                    // Try to enable auto-info mode
                    if let Err(e) = conn.enable_auto_info().await {
                        tracing::warn!("Failed to enable auto-info on {}: {}", port, e);
                        let _ = tx.send(BackgroundMessage::IoError {
                            source: format!("Radio {}", port),
                            message: "Auto-info not enabled - radio won't send automatic updates"
                                .to_string(),
                        });
                    }

                    // Notify UI of successful connection
                    let _ = tx.send(BackgroundMessage::RadioConnected {
                        handle,
                        model: actual_model_name,
                        port: port.clone(),
                    });

                    // Start read loop (runs until error or shutdown)
                    conn.run_read_loop(cmd_rx).await;
                }
                Err(e) => {
                    let _ = tx.send(BackgroundMessage::IoError {
                        source: format!("Radio {}", port),
                        message: format!("Connection failed: {}", e),
                    });
                    let _ = tx.send(BackgroundMessage::RadioDisconnected { handle });
                }
            }
        });
    }

    /// Save current configured COM radios to settings
    fn save_configured_radios(&mut self) {
        let configs: Vec<ConfiguredRadio> = self
            .radio_panels
            .iter()
            .filter(|p| !p.is_virtual())
            .map(|p| ConfiguredRadio {
                port: p.port.clone(),
                protocol: p.protocol,
                model_name: p.name.clone(),
                baud_rate: p.baud_rate,
                civ_address: p.civ_address,
            })
            .collect();

        if self.settings.configured_radios != configs {
            self.settings.configured_radios = configs;
            if let Err(e) = self.settings.save() {
                self.handle_save_error(e);
            }
        }
    }

    /// Set a status message (also logs as Info via tracing, which goes to traffic monitor)
    fn set_status(&mut self, msg: String) {
        self.status_message = Some((msg.clone(), Instant::now()));
        tracing::info!(source = "Status", "{}", msg);
    }

    /// Report an info message via tracing (shows in console and traffic monitor)
    fn report_info(&mut self, source: &str, message: impl Into<String>) {
        let message = message.into();
        tracing::info!(source = source, "{}", message);
    }

    /// Report a warning via tracing (shows in console, traffic monitor, and status bar)
    fn report_warning(&mut self, source: &str, message: impl Into<String>) {
        let message = message.into();
        self.status_message = Some((format!("{}: {}", source, message), Instant::now()));
        tracing::warn!(source = source, "{}", message);
    }

    /// Report an error via tracing (shows in console, traffic monitor, and status bar)
    fn report_err(&mut self, source: &str, message: impl Into<String>) {
        let message = message.into();
        self.status_message = Some((format!("{}: {}", source, message), Instant::now()));
        tracing::error!(source = source, "{}", message);
    }

    /// Handle a settings save error
    fn handle_save_error(&mut self, error: String) {
        self.report_err("Settings", error);
    }

    /// Allocate a new correlation_id for pending registrations
    fn allocate_correlation_id(&mut self) -> u64 {
        let id = self.next_correlation_id;
        self.next_correlation_id += 1;
        id
    }

    /// Send a command to the mux actor, logging a warning if the channel is full
    fn send_mux_command(&self, cmd: MuxActorCommand, context: &str) {
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
    fn send_radio_task_command(
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

    /// Register a COM port radio with the mux actor
    /// Returns correlation_id - the RadioHandle will arrive via BackgroundMessage::RadioRegistered
    /// The async radio task is spawned when the handle is received
    /// Caller must store the panel index in pending_registrations with correlation_id as key
    fn register_com_radio(&mut self, config: ComRadioConfig, panel_index: usize) -> u64 {
        // Allocate a correlation_id
        let correlation_id = self.allocate_correlation_id();

        // Create metadata for the radio channel
        let meta = RadioChannelMeta::new_real(
            config.model_name.clone(),
            config.port.clone(),
            config.protocol,
            config.civ_address,
        );

        // Create oneshot for receiving the handle
        let (resp_tx, resp_rx) = oneshot::channel();

        // Send RegisterRadio to mux actor
        self.send_mux_command(
            MuxActorCommand::RegisterRadio {
                meta,
                response: resp_tx,
            },
            "RegisterRadio",
        );

        // Spawn a task to await the handle and send it back via BackgroundMessage
        let bg_tx = self.bg_tx.clone();
        self.rt_handle.spawn(async move {
            if let Ok(handle) = resp_rx.await {
                let _ = bg_tx.send(BackgroundMessage::RadioRegistered {
                    correlation_id,
                    handle,
                });
            }
        });

        // Store the config so we can spawn the task when the handle arrives
        self.pending_radio_configs.insert(correlation_id, config);

        // Store the panel index for when the handle arrives
        self.pending_registrations
            .insert(correlation_id, panel_index);

        correlation_id
    }

    /// Get set of ports currently used by radios
    fn radio_ports_in_use(&self) -> HashSet<String> {
        self.radio_panels
            .iter()
            .filter(|p| !p.is_virtual())
            .map(|p| p.port.clone())
            .collect()
    }

    /// Get available ports for adding a new radio (excludes ports already used by radios)
    fn available_radio_ports(&self) -> Vec<&SerialPortInfo> {
        let in_use = self.radio_ports_in_use();
        self.available_ports
            .iter()
            .filter(|p| !in_use.contains(&p.port))
            .collect()
    }

    /// Get available ports for amplifier (excludes ports used by radios)
    fn available_amp_ports(&self) -> Vec<&SerialPortInfo> {
        let in_use = self.radio_ports_in_use();
        self.available_ports
            .iter()
            .filter(|p| !in_use.contains(&p.port))
            .collect()
    }

    /// Format a port label with product description
    fn format_port_label(port: &SerialPortInfo) -> String {
        match &port.product {
            Some(product) => format!("{} ({})", port.port, product),
            None => port.port.clone(),
        }
    }

    /// Process diagnostic events from tracing layer
    fn process_diagnostic_events(&mut self) {
        while let Ok(event) = self.diag_rx.try_recv() {
            // Map tracing Level to DiagnosticSeverity
            let severity = match event.level {
                Level::ERROR => DiagnosticSeverity::Error,
                Level::WARN => DiagnosticSeverity::Warning,
                Level::INFO => DiagnosticSeverity::Info,
                // DEBUG and TRACE map to Debug
                _ => DiagnosticSeverity::Debug,
            };

            self.traffic_monitor
                .add_diagnostic(event.source, severity, event.message);
        }
    }

    /// Process background messages
    fn process_messages(&mut self) {
        while let Ok(msg) = self.bg_rx.try_recv() {
            match msg {
                // Note: Traffic is now handled via MuxEvent in process_mux_events()
                BackgroundMessage::IoError { source, message } => {
                    self.report_err(&source, message);
                }
                BackgroundMessage::ProbeComplete {
                    port,
                    baud_rate,
                    result,
                } => {
                    self.probing = false;
                    if port == self.add_radio_port {
                        if let Some(probe_result) = result {
                            // Update UI fields with probe result
                            self.add_radio_protocol = probe_result.protocol;
                            self.add_radio_baud = baud_rate;
                            if let Some(addr) = probe_result.address {
                                self.add_radio_civ_address = addr;
                            }
                            // Set model name from detected model
                            self.add_radio_model = probe_result
                                .model
                                .map(|m| format!("{} {}", m.manufacturer, m.model))
                                .unwrap_or_else(|| format!("{} radio", probe_result.protocol.name()));
                            self.set_status(format!(
                                "Detected {} on {}",
                                self.add_radio_model, port
                            ));
                        } else {
                            self.set_status(format!("No radio detected on {}", port));
                        }
                    }
                }
                BackgroundMessage::RadioRegistered {
                    correlation_id,
                    handle,
                } => {
                    // Look up panel index from pending_registrations
                    if let Some(panel_idx) = self.pending_registrations.remove(&correlation_id) {
                        if let Some(panel) = self.radio_panels.get_mut(panel_idx) {
                            panel.handle = Some(handle);
                            tracing::info!("Radio registered: handle={:?}", handle);

                            // For virtual radios, store handle in sim_radio_ids
                            if let Some(sim_id) = panel.sim_id() {
                                self.sim_radio_ids.insert(sim_id.to_string(), handle);
                            }

                            // For COM radios, spawn the connection task
                            if let Some(config) = self.pending_radio_configs.remove(&correlation_id)
                            {
                                self.spawn_radio_task(
                                    handle,
                                    config.port.clone(),
                                    config.baud_rate,
                                    config.protocol,
                                    config.civ_address,
                                    config.model_name.clone(),
                                    config.query_initial_state,
                                );
                            }
                        }
                    }
                }
                BackgroundMessage::RadioConnected {
                    handle,
                    model,
                    port,
                } => {
                    // Update radio panel with actual model name and send rename to mux actor
                    if let Some(panel) = self
                        .radio_panels
                        .iter_mut()
                        .find(|p| p.handle == Some(handle))
                    {
                        panel.name = model.clone();
                        self.send_mux_command(
                            MuxActorCommand::UpdateRadioMeta {
                                handle,
                                name: Some(model.clone()),
                            },
                            "UpdateRadioMeta",
                        );
                    }

                    self.report_info("Radio", format!("Connected {} on {}", model, port));
                }
                BackgroundMessage::RadioDisconnected { handle } => {
                    // Remove the task sender
                    self.radio_task_senders.remove(&handle.0);
                    tracing::debug!("Radio {:?} disconnected", handle);
                }
                BackgroundMessage::RadioStateSync { handle, state } => {
                    // Update RadioPanel from authoritative mux actor state
                    if let Some(panel) = self
                        .radio_panels
                        .iter_mut()
                        .find(|p| p.handle == Some(handle))
                    {
                        // Only update if different (avoid unnecessary changes)
                        if panel.frequency_hz != state.frequency_hz {
                            panel.frequency_hz = state.frequency_hz;
                        }
                        if panel.mode != state.mode {
                            panel.mode = state.mode;
                        }
                        if panel.ptt != state.ptt {
                            panel.ptt = state.ptt;
                        }
                    }
                }
            }
        }
    }

    /// Process events from the mux actor and update local state
    fn process_mux_events(&mut self) {
        while let Ok(event) = self.mux_event_rx.try_recv() {
            match event {
                MuxEvent::RadioStateChanged {
                    handle,
                    freq,
                    mode,
                    ptt,
                } => {
                    // Update the RadioPanel's local state
                    if let Some(panel) = self
                        .radio_panels
                        .iter_mut()
                        .find(|p| p.handle == Some(handle))
                    {
                        if let Some(f) = freq {
                            panel.frequency_hz = Some(f);
                        }
                        if let Some(m) = mode {
                            panel.mode = Some(m);
                        }
                        if let Some(p) = ptt {
                            panel.ptt = p;
                        }

                        // Also update SimulationPanel for virtual radios
                        if let Some(sim_id) = panel.sim_id() {
                            self.simulation_panel
                                .update_radio_state(sim_id, freq, mode, ptt);
                        }
                    }
                }
                MuxEvent::ActiveRadioChanged { from: _, to } => {
                    self.active_radio = Some(to);
                }
                MuxEvent::SwitchingModeChanged { mode } => {
                    self.switching_mode = mode;
                }
                MuxEvent::RadioConnected { handle, meta } => {
                    tracing::debug!(
                        "MuxEvent::RadioConnected: handle={}, name={}",
                        handle.0,
                        meta.display_name
                    );
                }
                MuxEvent::RadioDisconnected { handle } => {
                    tracing::debug!("MuxEvent::RadioDisconnected: handle={}", handle.0);
                }
                MuxEvent::Error { source, message } => {
                    self.report_err(&source, message);
                }
                MuxEvent::AmpConnected { meta: _ } => {
                    tracing::debug!("MuxEvent::AmpConnected");
                }
                MuxEvent::AmpDisconnected => {
                    tracing::debug!("MuxEvent::AmpDisconnected");
                }
                MuxEvent::SwitchingBlocked {
                    requested,
                    current,
                    remaining_ms,
                } => {
                    tracing::debug!(
                        "Switching blocked: requested={}, current={}, remaining={}ms",
                        requested.0,
                        current.0,
                        remaining_ms
                    );
                }
                // Traffic events - forward to traffic monitor
                MuxEvent::RadioDataIn { .. }
                | MuxEvent::RadioDataOut { .. }
                | MuxEvent::AmpDataOut { .. }
                | MuxEvent::AmpDataIn { .. } => {
                    self.forward_traffic_event(event);
                }
            }
        }
    }

    /// Periodically sync radio states from the mux actor (every 5 seconds)
    ///
    /// This ensures that the UI's RadioPanel state stays in sync with the
    /// authoritative state in the mux actor, even if events are dropped.
    fn maybe_sync_radio_states(&mut self) {
        const SYNC_INTERVAL_SECS: u64 = 5;

        if self.last_state_sync.elapsed().as_secs() < SYNC_INTERVAL_SECS {
            return;
        }

        self.last_state_sync = Instant::now();

        // Query state for each radio panel that has a valid handle
        for panel in &self.radio_panels {
            let Some(handle) = panel.handle else {
                // No handle yet, not registered
                continue;
            };

            let (resp_tx, resp_rx) = oneshot::channel();

            self.send_mux_command(
                MuxActorCommand::QueryRadioState {
                    handle,
                    response: resp_tx,
                },
                "QueryRadioState",
            );

            // Spawn task to handle the response
            let bg_tx = self.bg_tx.clone();
            self.rt_handle.spawn(async move {
                if let Ok(Some(summary)) = resp_rx.await {
                    let _ = bg_tx.send(BackgroundMessage::RadioStateSync {
                        handle,
                        state: summary,
                    });
                }
            });
        }
    }

    /// Forward a traffic event to the traffic monitor
    fn forward_traffic_event(&mut self, event: MuxEvent) {
        // Build radio metadata lookup from radio panels
        let radio_metas = |handle: RadioHandle| -> Option<RadioChannelMeta> {
            self.radio_panels
                .iter()
                .find(|p| p.handle == Some(handle))
                .map(|p| {
                    if p.is_virtual() {
                        RadioChannelMeta::new_virtual(
                            p.name.clone(),
                            p.sim_id().unwrap_or_default().to_string(),
                            p.protocol,
                        )
                    } else {
                        RadioChannelMeta::new_real(
                            p.name.clone(),
                            p.port.clone(),
                            p.protocol,
                            p.civ_address,
                        )
                    }
                })
        };
        let amp_port = self.amp_port.clone();
        let amp_is_virtual = self.amp_data_tx.is_none();
        self.traffic_monitor.process_event_with_amp_port(
            event,
            &radio_metas,
            &amp_port,
            amp_is_virtual,
        );
    }

    /// Draw the toolbar
    fn draw_toolbar(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            // Console toggle button
            if self.show_traffic_monitor {
                if ui.button("Hide Console").clicked() {
                    self.show_traffic_monitor = false;
                }
            } else if ui.button("Show Console").clicked() {
                self.show_traffic_monitor = true;
            }

            ui.separator();

            if ui.button("Settings").clicked() {
                self.show_settings = !self.show_settings;
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                // Active radio indicator
                let has_active = self.active_radio.is_some();
                if has_active {
                    ui.label(RichText::new("●").color(Color32::GREEN).size(16.0));
                    ui.label("Active");
                } else {
                    ui.label(RichText::new("○").color(Color32::GRAY).size(16.0));
                    ui.label("No radio");
                }

                ui.separator();

                // Amplifier status
                match self.amp_connection_type {
                    AmplifierConnectionType::ComPort => {
                        if self.amp_data_tx.is_some() {
                            ui.label(RichText::new("Amp: Connected").color(Color32::GREEN));
                        } else {
                            ui.label(RichText::new("Amp: Disconnected").color(Color32::GRAY));
                        }
                    }
                    AmplifierConnectionType::Simulated => {
                        ui.label(
                            RichText::new("Amp: Simulated").color(Color32::from_rgb(100, 180, 255)),
                        );
                    }
                }

                ui.separator();

                // Status message
                if let Some((msg, _)) = &self.status_message {
                    ui.label(msg);
                }
            });
        });
    }

    /// Add a new virtual radio - creates duplex stream, spawns actor, registers with mux
    ///
    /// Returns the sim_id for the new radio.
    fn add_virtual_radio(&mut self, name: &str, protocol: Protocol) -> String {
        let radio = VirtualRadio::new(name, protocol);
        self.add_virtual_radio_internal(radio)
    }

    /// Add a virtual radio from configuration (used when restoring from settings)
    fn add_virtual_radio_from_config(&mut self, config: cat_sim::VirtualRadioConfig) -> String {
        let radio = VirtualRadio::from_config(config);
        self.add_virtual_radio_internal(radio)
    }

    /// Internal implementation for adding a virtual radio
    ///
    /// Takes ownership of the VirtualRadio, creates duplex stream, spawns actor,
    /// and registers with mux actor.
    fn add_virtual_radio_internal(&mut self, radio: VirtualRadio) -> String {
        let sim_id = format!("sim-{}", self.next_sim_id);
        self.next_sim_id += 1;

        let name = radio.id().to_string();
        let protocol = radio.protocol();
        let model_name = radio.model_name().to_string();
        let civ_address = radio.civ_address();

        // Allocate a correlation_id for tracking the registration
        let correlation_id = self.allocate_correlation_id();

        // Create metadata for the virtual radio channel
        let meta = RadioChannelMeta::new_virtual(name.clone(), sim_id.clone(), protocol);

        // Create oneshot for receiving the handle
        let (resp_tx, resp_rx) = oneshot::channel();

        // Send RegisterRadio to mux actor
        self.send_mux_command(
            MuxActorCommand::RegisterRadio {
                meta,
                response: resp_tx,
            },
            "RegisterRadio (virtual)",
        );

        // Create duplex stream pair for communication
        // connection_stream -> AsyncRadioConnection
        // radio_stream -> virtual radio actor task
        let (connection_stream, radio_stream) = tokio::io::duplex(1024);

        // Create UI command channel for SimulationPanel -> actor
        let (ui_cmd_tx, ui_cmd_rx) = tokio_mpsc::channel::<VirtualRadioCommand>(32);

        // Create channel for task control commands (shutdown)
        let (task_cmd_tx, task_cmd_rx) = tokio_mpsc::channel::<RadioTaskCommand>(32);

        // Store the task shutdown sender
        self.virtual_radio_task_senders
            .insert(sim_id.clone(), task_cmd_tx);

        // Register with SimulationPanel for UI display and commands
        self.simulation_panel.register_radio(
            sim_id.clone(),
            name.clone(),
            protocol,
            ui_cmd_tx,
        );

        // Create a RadioPanel with no handle (will be updated when handle arrives)
        self.radio_panels.push(RadioPanel::new_virtual(
            None,
            name.clone(),
            protocol,
            sim_id.clone(),
        ));
        let panel_idx = self.radio_panels.len() - 1;

        // Store the pending registration
        self.pending_registrations.insert(correlation_id, panel_idx);

        // Spawn the virtual radio actor task
        self.rt_handle.spawn(async move {
            if let Err(e) = run_virtual_radio_task(radio_stream, radio, ui_cmd_rx).await {
                tracing::warn!("Virtual radio actor task error: {}", e);
            }
        });

        // Spawn a task to await the handle and run AsyncRadioConnection
        let bg_tx = self.bg_tx.clone();
        let mux_tx = self.mux_cmd_tx.clone();
        let sim_id_clone = sim_id.clone();
        self.rt_handle.spawn(async move {
            if let Ok(handle) = resp_rx.await {
                // Notify UI of registration
                let _ = bg_tx.send(BackgroundMessage::RadioRegistered {
                    correlation_id,
                    handle,
                });

                // Create the AsyncRadioConnection with the connection stream
                let mut conn = AsyncRadioConnection::new(
                    handle,
                    sim_id_clone.clone(),
                    connection_stream,
                    protocol,
                    bg_tx.clone(),
                    mux_tx,
                );

                // Set CI-V address for Icom radios
                if let Some(civ_addr) = civ_address {
                    conn.set_civ_address(civ_addr);
                }

                // Small delay to let the virtual radio actor settle
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;

                // Query radio ID to get actual model name
                let actual_model_name = conn.query_id().await.unwrap_or(model_name);

                // Query initial state
                if let Err(e) = conn.query_initial_state().await {
                    tracing::warn!(
                        "Failed to query initial state on {}: {}",
                        sim_id_clone,
                        e
                    );
                }

                // Enable auto-info mode
                if let Err(e) = conn.enable_auto_info().await {
                    tracing::warn!(
                        "Failed to enable auto-info on {}: {}",
                        sim_id_clone,
                        e
                    );
                }

                // Notify UI of successful connection
                let _ = bg_tx.send(BackgroundMessage::RadioConnected {
                    handle,
                    model: actual_model_name,
                    port: format!("Virtual ({})", sim_id_clone),
                });

                // Start read loop (runs until error or shutdown)
                conn.run_read_loop(task_cmd_rx).await;
            }
        });

        self.set_status(format!("Virtual radio added: {}", sim_id));
        self.save_virtual_radios();

        sim_id
    }

    /// Remove a virtual radio - sends shutdown, unregisters from mux
    fn remove_virtual_radio(&mut self, sim_id: &str) {
        // Shutdown the virtual radio task
        if let Some(task_tx) = self.virtual_radio_task_senders.remove(sim_id) {
            let _ = task_tx.try_send(RadioTaskCommand::Shutdown);
        }

        // Unregister from SimulationPanel
        self.simulation_panel.unregister_radio(sim_id);

        // Get the handle from the panel and unregister from mux actor
        if let Some(panel) = self
            .radio_panels
            .iter()
            .find(|p| p.sim_id() == Some(sim_id))
        {
            if let Some(handle) = panel.handle {
                self.send_mux_command(
                    MuxActorCommand::UnregisterRadio { handle },
                    "UnregisterRadio (virtual)",
                );
            }
        }

        // Remove sim_id mapping
        self.sim_radio_ids.remove(sim_id);

        // Remove from radio_panels
        self.radio_panels
            .retain(|p| p.sim_id() != Some(sim_id));
        self.set_status(format!("Virtual radio removed: {}", sim_id));
        self.save_virtual_radios();
    }

    /// Probe the selected port for radio detection
    fn probe_selected_port(&mut self) {
        if self.add_radio_port.is_empty() || self.probing {
            return;
        }

        self.probing = true;
        self.set_status(format!("Probing {}...", self.add_radio_port));

        let port = self.add_radio_port.clone();
        let baud_rate = self.add_radio_baud;
        let tx = self.bg_tx.clone();
        let rt_handle = self.rt_handle.clone();

        std::thread::spawn(move || {
            let result = rt_handle.block_on(async { probe_port(&port, baud_rate).await });
            let _ = tx.send(BackgroundMessage::ProbeComplete {
                port,
                baud_rate,
                result,
            });
        });
    }

    /// Add a new COM radio with the current add_radio_* settings
    fn add_com_radio(&mut self) {
        if self.add_radio_port.is_empty() {
            return;
        }

        let civ_address = if self.add_radio_protocol == Protocol::IcomCIV {
            Some(self.add_radio_civ_address)
        } else {
            None
        };
        // Use detected model name if available, otherwise generate from protocol
        let model_name = if self.add_radio_model.is_empty() {
            format!("{} Radio", self.add_radio_protocol.name())
        } else {
            self.add_radio_model.clone()
        };

        let config = ComRadioConfig {
            port: self.add_radio_port.clone(),
            protocol: self.add_radio_protocol,
            baud_rate: self.add_radio_baud,
            civ_address,
            model_name: model_name.clone(),
            query_initial_state: true,
        };

        // Create RadioPanel with no handle (will be updated when handle arrives)
        let panel = RadioPanel::new_com(
            None,
            model_name,
            self.add_radio_port.clone(),
            self.add_radio_protocol,
            self.add_radio_baud,
            civ_address,
        );
        self.radio_panels.push(panel);
        let panel_index = self.radio_panels.len() - 1;

        // Register with mux actor (handle will arrive via RadioRegistered)
        let _correlation_id = self.register_com_radio(config, panel_index);

        // If this port was selected as amp port, clear it
        if self.amp_port == self.add_radio_port {
            self.amp_port.clear();
            if self.amp_data_tx.is_some() {
                self.disconnect_amplifier();
            }
            self.save_amplifier_settings();
        }

        // Save to config
        self.save_configured_radios();

        // Clear the add_radio_port for next addition
        self.add_radio_port.clear();
    }

    /// Draw the radio list panel (unified COM and Virtual radios)
    fn draw_radio_panel(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.heading("Radios");

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                // Add Radio dropdown menu
                ui.menu_button("+", |ui| {
                    // Add COM Radio section
                    ui.label(RichText::new("Add COM Radio:").small());

                    // Collect available ports into owned data to avoid borrow conflicts
                    // Include vid for protocol suggestion and port name for the dropdown
                    let available_ports: Vec<(String, String, Option<u16>)> = self
                        .available_radio_ports()
                        .into_iter()
                        .map(|p| (p.port.clone(), Self::format_port_label(p), p.vid))
                        .collect();

                    if available_ports.is_empty() {
                        ui.label(
                            RichText::new("No ports available")
                                .color(Color32::GRAY)
                                .small(),
                        );
                    } else {
                        // Port dropdown
                        let selected_label = if self.add_radio_port.is_empty() {
                            "Select port...".to_string()
                        } else {
                            // Find the label for the selected port
                            available_ports
                                .iter()
                                .find(|(port, _, _)| *port == self.add_radio_port)
                                .map(|(_, label, _)| label.clone())
                                .unwrap_or_else(|| self.add_radio_port.clone())
                        };

                        let prev_port = self.add_radio_port.clone();
                        egui::ComboBox::from_id_salt("add_radio_port")
                            .selected_text(&selected_label)
                            .width(160.0)
                            .show_ui(ui, |ui| {
                                for (port_name, label, _vid) in &available_ports {
                                    ui.selectable_value(
                                        &mut self.add_radio_port,
                                        port_name.clone(),
                                        label,
                                    );
                                }
                            });
                        // Clear model when port changes
                        if self.add_radio_port != prev_port {
                            self.add_radio_model.clear();
                        }

                        // Protocol dropdown
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("Protocol:").small());
                            egui::ComboBox::from_id_salt("add_radio_protocol")
                                .selected_text(self.add_radio_protocol.name())
                                .width(100.0)
                                .show_ui(ui, |ui| {
                                    for proto in [
                                        Protocol::Kenwood,
                                        Protocol::IcomCIV,
                                        Protocol::Yaesu,
                                        Protocol::YaesuAscii,
                                        Protocol::Elecraft,
                                        Protocol::FlexRadio,
                                    ] {
                                        ui.selectable_value(
                                            &mut self.add_radio_protocol,
                                            proto,
                                            proto.name(),
                                        );
                                    }
                                });
                        });

                        // Baud rate dropdown
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("Baud:").small());
                            egui::ComboBox::from_id_salt("add_radio_baud")
                                .selected_text(format!("{}", self.add_radio_baud))
                                .width(80.0)
                                .show_ui(ui, |ui| {
                                    for &baud in &[4800u32, 9600, 19200, 38400, 57600, 115200] {
                                        ui.selectable_value(
                                            &mut self.add_radio_baud,
                                            baud,
                                            format!("{}", baud),
                                        );
                                    }
                                });
                        });

                        // CI-V address for Icom protocol
                        if self.add_radio_protocol == Protocol::IcomCIV {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new("CI-V:").small());
                                let mut addr_str = format!("{:02X}", self.add_radio_civ_address);
                                let response = ui.add(
                                    egui::TextEdit::singleline(&mut addr_str).desired_width(40.0),
                                );
                                if response.changed() {
                                    if let Ok(addr) =
                                        u8::from_str_radix(addr_str.trim_start_matches("0x"), 16)
                                    {
                                        self.add_radio_civ_address = addr;
                                    }
                                }
                            });
                        }

                        // Probe button and detected model display
                        ui.horizontal(|ui| {
                            let can_probe = !self.add_radio_port.is_empty() && !self.probing;
                            if self.probing {
                                ui.spinner();
                            } else if ui
                                .add_enabled(can_probe, egui::Button::new("Probe"))
                                .on_hover_text("Detect radio protocol and model")
                                .clicked()
                            {
                                self.probe_selected_port();
                            }
                            if !self.add_radio_model.is_empty() {
                                ui.label(
                                    RichText::new(&self.add_radio_model)
                                        .small()
                                        .color(Color32::GREEN),
                                );
                            }
                        });

                        // Add Radio button
                        let can_add = !self.add_radio_port.is_empty() && !self.probing;
                        if ui
                            .add_enabled(can_add, egui::Button::new("Add"))
                            .on_hover_text("Add radio")
                            .clicked()
                        {
                            self.add_com_radio();
                            // Reset the model field after adding
                            self.add_radio_model.clear();
                            ui.close_menu();
                        }
                    }

                    ui.separator();
                    ui.label(RichText::new("Add Virtual Radio:").small());

                    for proto in [
                        Protocol::Kenwood,
                        Protocol::IcomCIV,
                        Protocol::Yaesu,
                        Protocol::YaesuAscii,
                        Protocol::Elecraft,
                        Protocol::FlexRadio,
                    ] {
                        if ui.button(proto.name()).clicked() {
                            let name = format!("Virtual {}", self.simulation_panel.radio_count() + 1);
                            self.add_virtual_radio(&name, proto);
                            ui.close_menu();
                        }
                    }
                });
            });
        });

        if self.radio_panels.is_empty() {
            ui.label("No radios. Click '+' to add a radio.");
            return;
        }

        // Get active radio handle for comparison
        let active_handle = self.active_radio;

        // Collect radio info from local RadioPanel state
        let radio_info: Vec<_> = self
            .radio_panels
            .iter()
            .enumerate()
            .map(|(idx, panel)| {
                // Read state from local RadioPanel fields
                let freq = panel.frequency_hz.unwrap_or(0);
                let mode = panel.mode.unwrap_or(OperatingMode::Usb);
                let freq_display = if freq > 0 {
                    format!("{:.3} MHz", freq as f64 / 1_000_000.0)
                } else {
                    "---.--- MHz".to_string()
                };
                let mode_display = panel.mode.map(mode_name).unwrap_or("---").to_string();

                (
                    idx,
                    panel.handle,
                    panel.name.clone(),
                    panel.port.clone(),
                    panel.is_virtual(),
                    panel.sim_id().map(String::from),
                    panel.expanded,
                    panel.protocol,
                    freq_display,
                    mode_display,
                    panel.ptt,
                    freq,
                    mode,
                )
            })
            .collect::<Vec<_>>();

        let mut selected_handle: Option<RadioHandle> = None;
        let mut toggle_expanded_idx = None;
        let mut remove_radio_idx = None;
        let mut freq_change: Option<(String, u64)> = None;
        let mut mode_change: Option<(String, OperatingMode)> = None;
        let mut ptt_change: Option<(String, bool)> = None;

        for (
            idx,
            handle,
            name,
            port,
            is_virtual,
            sim_id,
            expanded,
            protocol,
            freq_display,
            mode_display,
            ptt,
            freq_hz,
            mode,
        ) in &radio_info
        {
            let is_active = handle.is_some() && active_handle == *handle;

            // Determine background color based on state
            let bg_color = if *ptt {
                if *is_virtual {
                    Color32::from_rgb(80, 40, 20) // Red-orange tint for virtual
                } else {
                    Color32::from_rgb(80, 30, 30) // Red tint for COM
                }
            } else if is_active {
                if *is_virtual {
                    Color32::from_rgb(60, 50, 30)
                } else {
                    Color32::from_rgb(40, 60, 40)
                }
            } else if *is_virtual {
                Color32::from_rgb(40, 35, 25)
            } else {
                Color32::from_rgb(30, 30, 30)
            };

            egui::Frame::none()
                .fill(bg_color)
                .rounding(4.0)
                .inner_margin(8.0)
                .outer_margin(4.0)
                .show(ui, |ui| {
                    // Top row: SIM badge (for virtual radios), TX indicator, and Select/Expand button
                    ui.horizontal(|ui| {
                        // SIM badge only for virtual radios
                        if *is_virtual {
                            ui.label(
                                RichText::new("[SIM]")
                                    .color(Color32::from_rgb(255, 165, 0)) // Orange for virtual
                                    .strong()
                                    .size(10.0),
                            );
                        }

                        if *ptt {
                            ui.label(
                                RichText::new("● TX")
                                    .color(Color32::from_rgb(255, 80, 80))
                                    .strong()
                                    .size(14.0),
                            );
                        }

                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if !is_active && ui.button("Select").clicked() {
                                selected_handle = *handle;
                            }
                            // Expand/collapse toggle
                            if ui.button(if *expanded { "Less" } else { "More" }).clicked() {
                                toggle_expanded_idx = Some(*idx);
                            }
                        });
                    });

                    // Frequency - large and prominent
                    ui.label(
                        RichText::new(freq_display)
                            .size(22.0)
                            .strong()
                            .color(Color32::WHITE),
                    );

                    // Mode - prominent
                    ui.label(
                        RichText::new(mode_display)
                            .size(16.0)
                            .color(Color32::from_rgb(180, 180, 255)),
                    );

                    ui.add_space(4.0);

                    // Radio name and port/protocol - small, secondary
                    ui.horizontal(|ui| {
                        if is_active {
                            ui.label(RichText::new("●").color(Color32::GREEN).size(10.0));
                        }
                        let detail = if *is_virtual {
                            protocol.name()
                        } else {
                            port.as_str()
                        };
                        ui.label(
                            RichText::new(format!("{} · {}", name, detail))
                                .color(Color32::GRAY)
                                .size(11.0),
                        );
                    });

                    // Expanded controls for virtual radios
                    if *is_virtual && *expanded {
                        if let Some(sim_id) = sim_id {
                            ui.add_space(8.0);
                            ui.separator();
                            ui.add_space(4.0);

                            // Band presets
                            ui.horizontal_wrapped(|ui| {
                                ui.label(RichText::new("Band:").small());
                                for (band_name, band_freq) in &[
                                    ("160m", 1_900_000u64),
                                    ("80m", 3_750_000),
                                    ("40m", 7_150_000),
                                    ("20m", 14_250_000),
                                    ("15m", 21_250_000),
                                    ("10m", 28_500_000),
                                    ("6m", 50_125_000),
                                    ("2m", 146_520_000),
                                ] {
                                    if ui.small_button(*band_name).clicked() {
                                        freq_change = Some((sim_id.clone(), *band_freq));
                                    }
                                }
                            });

                            // Tune buttons
                            ui.horizontal(|ui| {
                                ui.label(RichText::new("Tune:").small());
                                for (label, delta) in [
                                    ("-10k", -10_000i64),
                                    ("-1k", -1_000),
                                    ("+1k", 1_000),
                                    ("+10k", 10_000),
                                ] {
                                    if ui.small_button(label).clicked() {
                                        let new_freq = (*freq_hz as i64 + delta).max(0) as u64;
                                        freq_change = Some((sim_id.clone(), new_freq));
                                    }
                                }
                            });

                            // Mode buttons
                            ui.horizontal_wrapped(|ui| {
                                ui.label(RichText::new("Mode:").small());
                                for m in [
                                    OperatingMode::Lsb,
                                    OperatingMode::Usb,
                                    OperatingMode::Cw,
                                    OperatingMode::Am,
                                    OperatingMode::Fm,
                                    OperatingMode::Dig,
                                ] {
                                    let is_current = *mode == m;
                                    let button = egui::Button::new(mode_name(m)).small().fill(
                                        if is_current {
                                            Color32::from_rgb(60, 80, 60)
                                        } else {
                                            Color32::from_rgb(40, 40, 40)
                                        },
                                    );
                                    if ui.add(button).clicked() {
                                        mode_change = Some((sim_id.clone(), m));
                                    }
                                }
                            });

                            // PTT and Remove buttons
                            ui.horizontal(|ui| {
                                let ptt_text = if *ptt { "TX ON" } else { "TX OFF" };
                                let ptt_button = egui::Button::new(ptt_text).fill(if *ptt {
                                    Color32::from_rgb(150, 50, 50)
                                } else {
                                    Color32::from_rgb(50, 50, 50)
                                });
                                if ui.add(ptt_button).clicked() {
                                    ptt_change = Some((sim_id.clone(), !*ptt));
                                }

                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        if ui
                                            .button(
                                                RichText::new("Remove")
                                                    .color(Color32::from_rgb(255, 100, 100)),
                                            )
                                            .clicked()
                                        {
                                            remove_radio_idx = Some(*idx);
                                        }
                                    },
                                );
                            });
                        }
                    }

                    // Expanded controls for COM radios
                    if !is_virtual && *expanded {
                        ui.add_space(8.0);
                        ui.separator();
                        ui.add_space(4.0);

                        ui.horizontal(|ui| {
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if ui
                                        .button(
                                            RichText::new("Remove")
                                                .color(Color32::from_rgb(255, 100, 100)),
                                        )
                                        .clicked()
                                    {
                                        remove_radio_idx = Some(*idx);
                                    }
                                },
                            );
                        });
                    }
                });
        }

        // Handle deferred actions
        if let Some(handle) = selected_handle {
            // Send SetActiveRadio to mux actor
            self.send_mux_command(MuxActorCommand::SetActiveRadio { handle }, "SetActiveRadio");
        }
        if let Some(idx) = toggle_expanded_idx {
            self.radio_panels[idx].expanded = !self.radio_panels[idx].expanded;
        }
        if let Some((sim_id, freq)) = freq_change {
            self.simulation_panel
                .send_command(&sim_id, VirtualRadioCommand::SetFrequency(freq));
        }
        if let Some((sim_id, m)) = mode_change {
            self.simulation_panel
                .send_command(&sim_id, VirtualRadioCommand::SetMode(m));
        }
        if let Some((sim_id, active)) = ptt_change {
            self.simulation_panel
                .send_command(&sim_id, VirtualRadioCommand::SetPtt(active));
        }
        if let Some(idx) = remove_radio_idx {
            let Some(panel) = self.radio_panels.get(idx) else {
                return; // Index no longer valid
            };
            // Extract data before mutating
            let is_virtual = panel.is_virtual();
            let sim_id = panel.sim_id().map(String::from);
            let handle = panel.handle;

            if is_virtual {
                // Virtual radio - remove directly
                if let Some(sim_id) = sim_id {
                    self.remove_virtual_radio(&sim_id);
                }
            } else {
                // COM radio - shutdown async task and remove panel, save config
                if let Some(handle) = handle {
                    // Remove task sender (keyed by handle.0)
                    if let Some((_, sender)) = self.radio_task_senders.remove(&handle.0) {
                        // Send shutdown command to the async task
                        Self::send_radio_task_command(
                            &sender,
                            RadioTaskCommand::Shutdown,
                            "Shutdown",
                        );
                    }
                    // Send UnregisterRadio to mux actor
                    self.send_mux_command(
                        MuxActorCommand::UnregisterRadio { handle },
                        "UnregisterRadio",
                    );
                }
                // Remove from panels and save
                self.radio_panels.remove(idx);
                self.save_configured_radios();
                self.set_status("Radio removed".into());
            }
        }
    }

    /// Draw the amplifier configuration panel
    fn draw_amplifier_panel(&mut self, ui: &mut Ui) {
        // Capture previous state for change detection
        let prev_connection_type = self.amp_connection_type;
        let prev_protocol = self.amp_protocol;
        let prev_port = self.amp_port.clone();
        let prev_baud = self.amp_baud;
        let prev_civ = self.amp_civ_address;

        egui::Grid::new("amp_config")
            .num_columns(2)
            .spacing([10.0, 4.0])
            .show(ui, |ui| {
                // Connection type selector
                ui.label("Connection:");
                egui::ComboBox::from_id_salt("amp_connection_type")
                    .selected_text(match self.amp_connection_type {
                        AmplifierConnectionType::ComPort => "COM Port",
                        AmplifierConnectionType::Simulated => "Simulated",
                    })
                    .show_ui(ui, |ui| {
                        if ui
                            .selectable_value(
                                &mut self.amp_connection_type,
                                AmplifierConnectionType::ComPort,
                                "COM Port",
                            )
                            .changed()
                        {
                            // Disconnect when switching to COM port mode
                            if self.amp_data_tx.is_some() {
                                self.disconnect_amplifier();
                            }
                        }
                        ui.selectable_value(
                            &mut self.amp_connection_type,
                            AmplifierConnectionType::Simulated,
                            "Simulated",
                        );
                    });
                ui.end_row();

                ui.label("Protocol:");
                egui::ComboBox::from_id_salt("amp_protocol")
                    .selected_text(self.amp_protocol.name())
                    .show_ui(ui, |ui| {
                        ui.selectable_value(
                            &mut self.amp_protocol,
                            Protocol::Kenwood,
                            Protocol::Kenwood.name(),
                        );
                        ui.selectable_value(
                            &mut self.amp_protocol,
                            Protocol::IcomCIV,
                            Protocol::IcomCIV.name(),
                        );
                        ui.selectable_value(
                            &mut self.amp_protocol,
                            Protocol::Yaesu,
                            Protocol::Yaesu.name(),
                        );
                        ui.selectable_value(
                            &mut self.amp_protocol,
                            Protocol::YaesuAscii,
                            Protocol::YaesuAscii.name(),
                        );
                        ui.selectable_value(
                            &mut self.amp_protocol,
                            Protocol::Elecraft,
                            Protocol::Elecraft.name(),
                        );
                    });
                ui.end_row();

                // Only show port/baud for COM port mode
                if self.amp_connection_type == AmplifierConnectionType::ComPort {
                    ui.label("Port:");
                    // Get available ports (excludes ports used by radios)
                    // Collect into owned data to avoid borrow conflicts
                    let available_amp_ports: Vec<(String, String)> = self
                        .available_amp_ports()
                        .into_iter()
                        .map(|p| (p.port.clone(), Self::format_port_label(p)))
                        .collect();

                    // Find the selected port's hint for display
                    let selected_label = if self.amp_port.is_empty() {
                        "Select port...".to_string()
                    } else {
                        available_amp_ports
                            .iter()
                            .find(|(port, _)| *port == self.amp_port)
                            .map(|(_, label)| label.clone())
                            .unwrap_or_else(|| self.amp_port.clone())
                    };
                    egui::ComboBox::from_id_salt("amp_port")
                        .selected_text(selected_label)
                        .show_ui(ui, |ui| {
                            for (port, label) in &available_amp_ports {
                                ui.selectable_value(&mut self.amp_port, port.clone(), label);
                            }
                        });
                    ui.end_row();

                    ui.label("Baud Rate:");
                    egui::ComboBox::from_id_salt("amp_baud")
                        .selected_text(format!("{}", self.amp_baud))
                        .show_ui(ui, |ui| {
                            // Common amplifier baud rates
                            for &baud in &[4800u32, 9600, 19200, 38400, 57600, 115200, 230400] {
                                ui.selectable_value(&mut self.amp_baud, baud, format!("{}", baud));
                            }
                        });
                    ui.end_row();

                    // Show CI-V address for Icom protocol
                    if self.amp_protocol == Protocol::IcomCIV {
                        ui.label("CI-V Address:");
                        let mut addr_str = format!("{:02X}", self.amp_civ_address);
                        if ui.text_edit_singleline(&mut addr_str).changed() {
                            if let Ok(addr) =
                                u8::from_str_radix(addr_str.trim_start_matches("0x"), 16)
                            {
                                self.amp_civ_address = addr;
                            }
                        }
                        ui.end_row();
                    }
                }
            });

        // Status and controls based on connection type
        match self.amp_connection_type {
            AmplifierConnectionType::ComPort => {
                ui.horizontal(|ui| {
                    let is_connected = self.amp_data_tx.is_some();
                    let can_connect = !self.amp_port.is_empty() && !is_connected;

                    if ui
                        .add_enabled(can_connect, egui::Button::new("Connect"))
                        .clicked()
                    {
                        self.connect_amplifier();
                    }

                    if ui
                        .add_enabled(is_connected, egui::Button::new("Disconnect"))
                        .clicked()
                    {
                        self.disconnect_amplifier();
                    }

                    if is_connected {
                        ui.label(RichText::new("● Connected").color(Color32::GREEN));
                    } else if !self.amp_port.is_empty() {
                        ui.label(RichText::new("● Disconnected").color(Color32::GRAY));
                    }
                });
            }
            AmplifierConnectionType::Simulated => {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("● Simulated").color(Color32::from_rgb(100, 180, 255)));
                });
                ui.label(
                    RichText::new("Commands appear in Traffic Monitor")
                        .color(Color32::GRAY)
                        .small(),
                );
            }
        }

        // Save if any amplifier settings changed
        if self.amp_connection_type != prev_connection_type
            || self.amp_protocol != prev_protocol
            || self.amp_port != prev_port
            || self.amp_baud != prev_baud
            || self.amp_civ_address != prev_civ
        {
            self.save_amplifier_settings();
        }
    }

    /// Draw the switching mode panel
    fn draw_switching_panel(&mut self, ui: &mut Ui) {
        // Read from local state
        let mut mode = self.switching_mode;

        egui::Grid::new("switch_config")
            .num_columns(2)
            .spacing([10.0, 4.0])
            .show(ui, |ui| {
                ui.label("Mode:");
                egui::ComboBox::from_id_salt("switch_mode")
                    .selected_text(mode.name())
                    .show_ui(ui, |ui| {
                        for m in [
                            SwitchingMode::FrequencyTriggered,
                            SwitchingMode::Automatic,
                            SwitchingMode::Manual,
                        ] {
                            if ui.selectable_value(&mut mode, m, m.name()).changed() {
                                // Send SetSwitchingMode to mux actor
                                self.switching_mode = mode;
                                self.send_mux_command(
                                    MuxActorCommand::SetSwitchingMode { mode },
                                    "SetSwitchingMode",
                                );
                            }
                        }
                    });
                ui.end_row();
            });

        ui.label(
            RichText::new(mode.description())
                .color(Color32::GRAY)
                .size(11.0),
        );
    }

    /// Draw the traffic monitor panel
    fn draw_traffic_panel(&mut self, ui: &mut Ui) {
        ui.heading("Traffic Monitor");

        // Draw and handle export actions
        if let Some(action) =
            self.traffic_monitor
                .draw(ui, self.settings.show_hex, self.settings.show_decoded)
        {
            match action {
                ExportAction::CopyToClipboard(content) => {
                    ui.output_mut(|o| o.copied_text = content);
                    self.set_status("Log copied to clipboard".to_string());
                }
                ExportAction::SavedToFile(path) => {
                    self.set_status(format!("Log saved to {}", path.display()));
                }
                ExportAction::Cancelled => {
                    // User cancelled, do nothing
                }
                ExportAction::Error(e) => {
                    self.report_err("Export", e);
                }
            }
        }

        // Sync diagnostic level to settings and update tracing filter if changed
        let current_level = self.traffic_monitor.diagnostic_level();
        if self.prev_diagnostic_level != current_level {
            // Update settings
            self.settings.diagnostic_level = current_level;
            if let Err(e) = self.settings.save() {
                self.handle_save_error(e);
            }

            // Update the tracing filter dynamically (atomic store, no parsing)
            self.diagnostic_level_state.set_level(current_level);

            // Track the change
            self.prev_diagnostic_level = current_level;
        }
    }

    /// Connect to the amplifier (dispatches to COM or virtual based on connection type)
    fn connect_amplifier(&mut self) {
        match self.amp_connection_type {
            AmplifierConnectionType::ComPort => self.connect_amplifier_com(),
            AmplifierConnectionType::Simulated => self.connect_amplifier_virtual(),
        }
    }

    /// Connect to a physical amplifier via COM port
    fn connect_amplifier_com(&mut self) {
        if self.amp_port.is_empty() {
            self.set_status("No amplifier port selected".into());
            return;
        }

        let civ_address = if self.amp_protocol == Protocol::IcomCIV {
            Some(self.amp_civ_address)
        } else {
            None
        };

        // Open the serial port
        let stream = match tokio_serial::new(&self.amp_port, self.amp_baud)
            .timeout(Duration::from_millis(100))
            .open_native_async()
        {
            Ok(s) => s,
            Err(e) => {
                self.set_status(format!("Failed to open {}: {}", self.amp_port, e));
                return;
            }
        };

        // Send config to mux actor
        self.send_mux_command(
            MuxActorCommand::SetAmplifierConfig {
                port: self.amp_port.clone(),
                protocol: self.amp_protocol,
                baud_rate: self.amp_baud,
                civ_address,
            },
            "SetAmplifierConfig",
        );

        // Create channel for sending data to amp task
        let (amp_data_tx, amp_data_rx) = tokio_mpsc::channel::<Vec<u8>>(64);
        let (shutdown_tx, shutdown_rx) = oneshot::channel();

        // Create a dummy response channel (not used in current architecture)
        let (_response_tx, response_rx) = tokio_mpsc::channel::<Vec<u8>>(64);

        // Create amplifier channel metadata
        let amp_meta = AmplifierChannelMeta::new_real(
            self.amp_port.clone(),
            self.amp_protocol,
            self.amp_baud,
            civ_address,
        );

        // Create AmplifierChannel and tell mux actor
        let amp_channel = AmplifierChannel::new(amp_meta, amp_data_tx.clone(), response_rx);
        self.send_mux_command(
            MuxActorCommand::ConnectAmplifier {
                channel: amp_channel,
            },
            "ConnectAmplifier",
        );

        // Store senders
        self.amp_data_tx = Some(amp_data_tx);
        self.amp_shutdown_tx = Some(shutdown_tx);

        // Spawn the async amp task
        let mux_tx = self.mux_cmd_tx.clone();
        self.rt_handle.spawn(async move {
            run_amp_task(shutdown_rx, amp_data_rx, stream, mux_tx).await;
        });

        self.set_status(format!(
            "Connected to amplifier on {} @ {} baud",
            self.amp_port, self.amp_baud
        ));
    }

    /// Connect to a virtual/simulated amplifier
    fn connect_amplifier_virtual(&mut self) {
        let civ_address = if self.amp_protocol == Protocol::IcomCIV {
            Some(self.amp_civ_address)
        } else {
            None
        };

        // Create the virtual amplifier I/O
        let virtual_io = VirtualAmplifierIo::new(self.amp_protocol, civ_address);

        // Send config to mux actor (use "[VIRTUAL]" as port name)
        self.send_mux_command(
            MuxActorCommand::SetAmplifierConfig {
                port: "[VIRTUAL]".to_string(),
                protocol: self.amp_protocol,
                baud_rate: 0,
                civ_address,
            },
            "SetAmplifierConfig (virtual)",
        );

        // Create channel for sending data to virtual amp task
        let (amp_data_tx, amp_data_rx) = tokio_mpsc::channel::<Vec<u8>>(64);
        let (shutdown_tx, shutdown_rx) = oneshot::channel();

        // Create a dummy response channel (not used in current architecture)
        let (_response_tx, response_rx) = tokio_mpsc::channel::<Vec<u8>>(64);

        // Create amplifier channel metadata for virtual amp
        let amp_meta = AmplifierChannelMeta::new_virtual(self.amp_protocol, civ_address);

        // Create AmplifierChannel and tell mux actor
        let amp_channel = AmplifierChannel::new(amp_meta, amp_data_tx.clone(), response_rx);
        self.send_mux_command(
            MuxActorCommand::ConnectAmplifier {
                channel: amp_channel,
            },
            "ConnectAmplifier (virtual)",
        );

        // Store senders
        self.amp_data_tx = Some(amp_data_tx);
        self.amp_shutdown_tx = Some(shutdown_tx);

        // Spawn the amp task with virtual I/O
        let mux_tx = self.mux_cmd_tx.clone();
        self.rt_handle.spawn(async move {
            run_amp_task(shutdown_rx, amp_data_rx, virtual_io, mux_tx).await;
        });

        self.set_status(format!(
            "Connected to virtual amplifier (protocol: {})",
            self.amp_protocol.name()
        ));
    }

    /// Disconnect from the amplifier
    fn disconnect_amplifier(&mut self) {
        // Tell mux actor to stop sending to amp
        self.send_mux_command(MuxActorCommand::DisconnectAmplifier, "DisconnectAmplifier");

        // Send shutdown to amp task
        if let Some(tx) = self.amp_shutdown_tx.take() {
            let _ = tx.send(());
        }

        self.amp_data_tx = None;
        self.set_status("Amplifier disconnected".into());
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
