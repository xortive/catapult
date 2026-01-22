//! Main application state and UI

use std::collections::{HashMap, HashSet};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Instant;

use cat_detect::{suggest_protocol_for_port, DetectedRadio, PortScanner, SerialPortInfo};
use cat_mux::{Multiplexer, MultiplexerEvent, RadioHandle, SwitchingMode};
use cat_protocol::{OperatingMode, Protocol, RadioCommand};
use cat_sim::SimulationEvent;
use eframe::CreationContext;
use egui::{Color32, RichText, Ui};

use crate::radio_panel::{RadioConnectionType, RadioPanel};
use crate::serial_io::{AmplifierConnection, RadioConnection};
use crate::settings::{AmplifierSettings, ConfiguredRadio, Settings};
use crate::simulation_panel::SimulationPanel;
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
    /// Scan completed
    ScanComplete(Vec<DetectedRadio>),
    /// Error occurred
    Error(String),
    /// Traffic received from radio
    TrafficIn { radio: RadioHandle, data: Vec<u8> },
    /// Traffic sent to amplifier
    TrafficOut { data: Vec<u8> },
    /// Traffic received from amplifier
    AmpTrafficIn { data: Vec<u8> },
    /// I/O error from radio or amplifier
    IoError { source: String, message: String },
}

/// Main application state
pub struct CatapultApp {
    /// Settings
    settings: Settings,
    /// Multiplexer engine
    multiplexer: Multiplexer,
    /// Port scanner
    scanner: PortScanner,
    /// Available serial ports
    available_ports: Vec<SerialPortInfo>,
    /// Detected radios from last scan
    detected_radios: Vec<DetectedRadio>,
    /// Radio panels for UI (unified list of COM and Virtual radios)
    radio_panels: Vec<RadioPanel>,
    /// Radio serial connections (keyed by RadioHandle)
    radio_connections: HashMap<RadioHandle, RadioConnection>,
    /// Traffic monitor
    traffic_monitor: TrafficMonitor,
    /// Is scanning in progress
    scanning: bool,
    /// Last scan time
    last_scan: Option<Instant>,
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
    /// Amplifier connection (when connected, only for ComPort type)
    amp_connection: Option<AmplifierConnection>,
    /// Maps simulation radio IDs to multiplexer handles
    sim_radio_handles: HashMap<String, RadioHandle>,
    /// Selected port for adding a new COM radio
    add_radio_port: String,
    /// Selected protocol for adding a new COM radio
    add_radio_protocol: Protocol,
    /// Selected baud rate for adding a new COM radio
    add_radio_baud: u32,
    /// CI-V address for new Icom COM radio
    add_radio_civ_address: u8,
}

impl CatapultApp {
    /// Create a new application
    pub fn new(_cc: &CreationContext<'_>) -> Self {
        let (bg_tx, bg_rx) = mpsc::channel();
        let settings = Settings::load();

        // Restore amplifier settings
        let amp_connection_type = if settings.amplifier.connection_type == "com" {
            AmplifierConnectionType::ComPort
        } else {
            AmplifierConnectionType::Simulated
        };

        let mut app = Self {
            traffic_monitor: TrafficMonitor::new(
                settings.traffic_history_size,
                settings.show_diagnostics,
                settings.show_diagnostic_info,
                settings.show_diagnostic_warning,
                settings.show_diagnostic_error,
            ),
            multiplexer: Multiplexer::new(),
            scanner: PortScanner::new(),
            available_ports: Vec::new(),
            detected_radios: Vec::new(),
            radio_panels: Vec::new(),
            radio_connections: HashMap::new(),
            scanning: false,
            last_scan: None,
            status_message: None,
            show_settings: false,
            show_traffic_monitor: true,
            simulation_panel: SimulationPanel::new(),
            bg_rx,
            bg_tx,
            amp_port: settings.amplifier.port.clone(),
            amp_protocol: settings.amplifier.protocol,
            amp_baud: settings.amplifier.baud_rate,
            amp_civ_address: settings.amplifier.civ_address,
            amp_connection_type,
            amp_connection: None,
            sim_radio_handles: HashMap::new(),
            add_radio_port: String::new(),
            add_radio_protocol: Protocol::Kenwood,
            add_radio_baud: 9600,
            add_radio_civ_address: 0x00,
            settings,
        };

        // Initial port enumeration
        app.refresh_ports();

        // Restore virtual radios from settings
        for config in app.settings.virtual_radios.clone() {
            let _sim_id = app
                .simulation_panel
                .context_mut()
                .add_radio_from_config(config);
        }

        // Restore configured COM radios from settings
        app.restore_configured_radios();

        // Auto-scan on startup if enabled
        if app.settings.auto_scan {
            app.detect_new_radios();
        }

        app
    }

    /// Refresh available ports
    fn refresh_ports(&mut self) {
        match self.scanner.enumerate_ports() {
            Ok(mut ports) => {
                PortScanner::sort_by_classification(&mut ports);
                self.available_ports = ports;

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
                        if self.amp_connection.is_some() {
                            self.amp_connection = None;
                            self.set_status(
                                "Amplifier disconnected: port no longer available".into(),
                            );
                        }
                        self.save_amplifier_settings();
                    }
                }
            }
            Err(e) => {
                self.report_warning("System", format!("Failed to enumerate ports: {}", e));
            }
        }
    }

    /// Save current virtual radios to settings
    fn save_virtual_radios(&mut self) {
        use cat_sim::VirtualRadioConfig;

        let configs: Vec<VirtualRadioConfig> = self
            .simulation_panel
            .context()
            .radios()
            .map(|(_, radio)| VirtualRadioConfig {
                id: radio.id().to_string(),
                protocol: radio.protocol(),
                model_name: radio.model().map(|m| m.model.clone()),
                initial_frequency_hz: radio.frequency_hz(),
                initial_mode: radio.mode(),
                civ_address: radio.civ_address(),
            })
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

            // Add to multiplexer
            let handle = self.multiplexer.add_radio(
                config.model_name.clone(),
                config.port.clone(),
                config.protocol,
            );

            // Create RadioPanel
            let mut panel = RadioPanel::new_from_config(handle, &config);
            if !port_available {
                panel.unavailable = true;
            }
            self.radio_panels.push(panel);

            // Open the serial connection if port is available
            if port_available {
                match RadioConnection::new(
                    handle,
                    &config.port,
                    config.baud_rate,
                    config.protocol,
                    self.bg_tx.clone(),
                ) {
                    Ok(mut conn) => {
                        // Set CI-V address for Icom radios
                        if let Some(civ_addr) = config.civ_address {
                            conn.set_civ_address(civ_addr);
                        }

                        // Try to enable auto-info mode
                        if let Err(e) = conn.enable_auto_info() {
                            let msg = format!("Failed to enable auto-info on {}: {}", config.port, e);
                            tracing::warn!("{}", msg);
                            // Add to traffic monitor so users can see it
                            let _ = self.bg_tx.send(BackgroundMessage::IoError {
                                source: format!("Radio {}", config.port),
                                message: "Auto-info not enabled - radio won't send automatic updates".to_string(),
                            });
                        }

                        self.radio_connections.insert(handle, conn);
                        self.report_info("Radio", format!("Connected on {}", config.port));
                    }
                    Err(e) => {
                        self.report_err(
                            "Radio",
                            format!("Failed to connect to {}: {}", config.port, e),
                        );
                    }
                }
            } else {
                self.report_warning("Radio", format!("{} not available", config.port));
            }
        }
    }

    /// Save current configured COM radios to settings
    fn save_configured_radios(&mut self) {
        let configs: Vec<ConfiguredRadio> = self
            .radio_panels
            .iter()
            .filter(|p| p.connection_type == RadioConnectionType::ComPort)
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

    /// Set a status message (also adds Info diagnostic to traffic monitor)
    fn set_status(&mut self, msg: String) {
        self.status_message = Some((msg.clone(), Instant::now()));
        self.traffic_monitor
            .add_diagnostic("Status".to_string(), DiagnosticSeverity::Info, msg.clone());
        tracing::info!("[Status] {}", msg);
    }

    /// Report a message to all logging destinations (traffic monitor, tracing, and optionally status bar)
    fn report_error(
        &mut self,
        source: &str,
        severity: DiagnosticSeverity,
        message: impl Into<String>,
    ) {
        let message = message.into();
        // Warning/Error messages update status bar (set directly to avoid double-logging)
        if severity != DiagnosticSeverity::Info {
            self.status_message = Some((format!("{}: {}", source, message), Instant::now()));
        }
        self.traffic_monitor
            .add_diagnostic(source.to_string(), severity, message.clone());
        match severity {
            DiagnosticSeverity::Info => tracing::info!("[{}] {}", source, message),
            DiagnosticSeverity::Warning => tracing::warn!("[{}] {}", source, message),
            DiagnosticSeverity::Error => tracing::error!("[{}] {}", source, message),
        }
    }

    /// Report an info message to traffic monitor and tracing (not status bar)
    fn report_info(&mut self, source: &str, message: impl Into<String>) {
        self.report_error(source, DiagnosticSeverity::Info, message);
    }

    /// Report a warning to all logging destinations
    fn report_warning(&mut self, source: &str, message: impl Into<String>) {
        self.report_error(source, DiagnosticSeverity::Warning, message);
    }

    /// Report an error to all logging destinations
    fn report_err(&mut self, source: &str, message: impl Into<String>) {
        self.report_error(source, DiagnosticSeverity::Error, message);
    }

    /// Handle a settings save error
    fn handle_save_error(&mut self, error: String) {
        self.report_err("Settings", error);
    }

    /// Get the protocol for a radio by its handle
    fn get_protocol_for_radio(&self, handle: RadioHandle) -> Option<Protocol> {
        self.radio_panels
            .iter()
            .find(|p| p.handle == handle)
            .map(|p| p.protocol)
    }

    /// Get the protocol for a virtual radio by its simulation ID
    fn get_protocol_for_sim_radio(&self, radio_id: &str) -> Option<Protocol> {
        self.simulation_panel
            .context()
            .get_radio(radio_id)
            .map(|r| r.protocol())
    }

    /// Get set of ports currently used by radios
    fn radio_ports_in_use(&self) -> HashSet<String> {
        self.radio_panels
            .iter()
            .filter(|p| p.connection_type == RadioConnectionType::ComPort)
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

    /// Format a port label with classification hint
    fn format_port_label(port: &SerialPortInfo) -> String {
        match &port.classification_hint {
            Some(hint) => format!("{} ({})", port.port, hint),
            None => port.port.clone(),
        }
    }

    /// Process background messages
    fn process_messages(&mut self) {
        while let Ok(msg) = self.bg_rx.try_recv() {
            match msg {
                BackgroundMessage::ScanComplete(radios) => {
                    self.scanning = false;
                    self.last_scan = Some(Instant::now());
                    self.report_info(
                        "Scanner",
                        format!("Scan complete - detected {} radio(s)", radios.len()),
                    );

                    // Get existing ports to filter out duplicates
                    let existing_ports: std::collections::HashSet<_> = self
                        .radio_panels
                        .iter()
                        .filter(|p| p.connection_type == RadioConnectionType::ComPort)
                        .map(|p| p.port.clone())
                        .collect();

                    // Only add radios on ports that aren't already configured
                    let mut new_count = 0;
                    for radio in &radios {
                        if existing_ports.contains(&radio.port) {
                            tracing::debug!(
                                "Skipping {} on {} - already configured",
                                radio.model_name(),
                                radio.port
                            );
                            continue;
                        }

                        self.report_info(
                            "Scanner",
                            format!("New radio detected: {} on {}", radio.model_name(), radio.port),
                        );

                        let handle = self.multiplexer.add_radio(
                            radio.model_name(),
                            radio.port.clone(),
                            radio.protocol,
                        );

                        // Update radio state from detection
                        if let Some(state) = self.multiplexer.get_radio_mut(handle) {
                            state.update_from_detection(radio);
                        }

                        self.radio_panels.push(RadioPanel::new(handle, radio));
                        new_count += 1;
                    }

                    self.detected_radios = radios;

                    // Save newly detected radios to config
                    if new_count > 0 {
                        self.save_configured_radios();
                        self.set_status(format!("Found {} new radio(s)", new_count));
                    } else {
                        self.set_status("No new radios found".into());
                    }
                }
                BackgroundMessage::Error(e) => {
                    self.scanning = false;
                    self.report_err("System", e);
                }
                BackgroundMessage::TrafficIn { radio, data } => {
                    let protocol = self.get_protocol_for_radio(radio);
                    self.traffic_monitor.add_incoming(radio, &data, protocol);
                }
                BackgroundMessage::TrafficOut { data } => {
                    // Outgoing to amplifier - use amplifier protocol
                    self.traffic_monitor
                        .add_outgoing(&data, Some(self.amp_protocol));
                }
                BackgroundMessage::AmpTrafficIn { data } => {
                    // Incoming from amplifier - use amplifier protocol
                    self.traffic_monitor.add_from_amplifier(
                        self.amp_port.clone(),
                        &data,
                        Some(self.amp_protocol),
                    );
                }
                BackgroundMessage::IoError { source, message } => {
                    self.report_err(&source, message);
                }
            }
        }
    }

    /// Process multiplexer events
    fn process_mux_events(&mut self) {
        for event in self.multiplexer.drain_events() {
            match event {
                MultiplexerEvent::ActiveRadioChanged { from: _, to } => {
                    let name = self
                        .multiplexer
                        .get_radio(to)
                        .map(|r| r.name.clone())
                        .unwrap_or_default();
                    self.set_status(format!("Switched to {}", name));
                }
                MultiplexerEvent::AmplifierCommand(data) => {
                    // Send to amplifier if connected, otherwise log as simulated
                    let amp_protocol = Some(self.amp_protocol);
                    if let Some(ref mut conn) = self.amp_connection {
                        self.traffic_monitor.add_outgoing(&data, amp_protocol);
                        if let Err(e) = conn.write(&data) {
                            self.report_err("Amplifier", format!("Write error: {}", e));
                        }
                    } else {
                        self.traffic_monitor
                            .add_simulated_outgoing(&data, amp_protocol);
                    }
                }
                MultiplexerEvent::Error(e) => {
                    self.report_err("Multiplexer", e);
                }
                _ => {}
            }
        }
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

            if self.scanning {
                ui.separator();
                ui.spinner();
                ui.label("Scanning...");
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                // Active radio indicator
                if self.multiplexer.active_radio().is_some() {
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
                        if self.amp_connection.is_some() {
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

    /// Add a new virtual radio
    fn add_virtual_radio(&mut self, protocol: Protocol) {
        let name = format!(
            "Virtual {}",
            self.simulation_panel.context().radio_count() + 1
        );
        // The returned ID is not used here since the SimulationEvent::RadioAdded will be
        // processed in process_simulation_events, which creates the RadioPanel
        let _sim_id = self
            .simulation_panel
            .context_mut()
            .add_radio(&name, protocol);
        self.set_status(format!("Adding virtual radio: {}", name));
    }

    /// Add a new COM radio with the current add_radio_* settings
    fn add_com_radio(&mut self) {
        if self.add_radio_port.is_empty() {
            return;
        }

        // Generate a model name based on port/protocol
        let model_name = format!("{} Radio", self.add_radio_protocol.name());

        // Add to multiplexer
        let handle = self.multiplexer.add_radio(
            model_name.clone(),
            self.add_radio_port.clone(),
            self.add_radio_protocol,
        );

        // Open the serial connection
        match RadioConnection::new(
            handle,
            &self.add_radio_port,
            self.add_radio_baud,
            self.add_radio_protocol,
            self.bg_tx.clone(),
        ) {
            Ok(mut conn) => {
                // Set CI-V address for Icom radios
                if self.add_radio_protocol == Protocol::IcomCIV {
                    conn.set_civ_address(self.add_radio_civ_address);
                }

                // Try to enable auto-info mode
                if let Err(e) = conn.enable_auto_info() {
                    let msg = format!("Failed to enable auto-info on {}: {}", self.add_radio_port, e);
                    tracing::warn!("{}", msg);
                    // Add to traffic monitor so users can see it
                    let _ = self.bg_tx.send(BackgroundMessage::IoError {
                        source: format!("Radio {}", self.add_radio_port),
                        message: "Auto-info not enabled - radio won't send automatic updates".to_string(),
                    });
                }

                self.radio_connections.insert(handle, conn);
                self.set_status(format!("Connected to radio on {}", self.add_radio_port));
            }
            Err(e) => {
                // Remove from multiplexer since we couldn't connect
                self.multiplexer.remove_radio(handle);
                self.report_err(
                    "Radio",
                    format!("Failed to connect to {}: {}", self.add_radio_port, e),
                );
                return; // Don't create panel for failed connection
            }
        }

        // Create RadioPanel only after successful connection
        let panel = RadioPanel::new_com(
            handle,
            model_name,
            self.add_radio_port.clone(),
            self.add_radio_protocol,
            self.add_radio_baud,
            if self.add_radio_protocol == Protocol::IcomCIV {
                Some(self.add_radio_civ_address)
            } else {
                None
            },
        );
        self.radio_panels.push(panel);

        // If this port was selected as amp port, clear it
        if self.amp_port == self.add_radio_port {
            self.amp_port.clear();
            // Disconnect amp if it was using this port
            if self.amp_connection.is_some() {
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

                        // Track if we should suggest a protocol after the dropdown
                        let mut suggest_for_port: Option<(Option<u16>, String)> = None;

                        egui::ComboBox::from_id_salt("add_radio_port")
                            .selected_text(&selected_label)
                            .width(160.0)
                            .show_ui(ui, |ui| {
                                for (port_name, label, vid) in &available_ports {
                                    if ui
                                        .selectable_value(
                                            &mut self.add_radio_port,
                                            port_name.clone(),
                                            label,
                                        )
                                        .changed()
                                    {
                                        suggest_for_port = Some((*vid, port_name.clone()));
                                    }
                                }
                            });

                        // Auto-suggest protocol after dropdown closes (outside closure)
                        if let Some((vid, port_name)) = suggest_for_port {
                            if let Some(protocol) = suggest_protocol_for_port(vid, &port_name) {
                                self.add_radio_protocol = protocol;
                            }
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

                        // Add Radio button
                        let can_add = !self.add_radio_port.is_empty();
                        if ui
                            .add_enabled(can_add, egui::Button::new("+"))
                            .on_hover_text("Add radio")
                            .clicked()
                        {
                            self.add_com_radio();
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
                            self.add_virtual_radio(proto);
                            ui.close_menu();
                        }
                    }
                });

                // Scan for radios button
                if self.scanning {
                    ui.spinner();
                } else if ui.button("↻").on_hover_text("Scan for radios").clicked() {
                    self.detect_new_radios();
                }
            });
        });

        if self.radio_panels.is_empty() {
            ui.label("No radios. Click '+' to add or '↻' to scan for radios.");
            return;
        }

        let active = self.multiplexer.active_radio();

        // Collect radio info to avoid borrow conflicts
        // For virtual radios, get state from simulation context
        // For COM radios, get state from multiplexer
        let radio_info: Vec<_> = self
            .radio_panels
            .iter()
            .enumerate()
            .map(|(idx, panel)| {
                let (freq_display, mode_display, ptt, freq_hz, mode) =
                    if panel.connection_type == RadioConnectionType::Virtual {
                        // Get state from simulation context
                        if let Some(sim_id) = &panel.sim_radio_id {
                            if let Some(radio) = self.simulation_panel.context().get_radio(sim_id) {
                                let freq = radio.frequency_hz();
                                (
                                    format!("{:.3} MHz", freq as f64 / 1_000_000.0),
                                    mode_name(radio.mode()).to_string(),
                                    radio.ptt(),
                                    freq,
                                    radio.mode(),
                                )
                            } else {
                                (
                                    "---.--- MHz".to_string(),
                                    "---".to_string(),
                                    false,
                                    0,
                                    OperatingMode::Usb,
                                )
                            }
                        } else {
                            (
                                "---.--- MHz".to_string(),
                                "---".to_string(),
                                false,
                                0,
                                OperatingMode::Usb,
                            )
                        }
                    } else {
                        // Get state from multiplexer
                        let state = self.multiplexer.get_radio(panel.handle);
                        (
                            state
                                .map(|s| s.frequency_display())
                                .unwrap_or_else(|| "---".to_string()),
                            state
                                .map(|s| s.mode_display())
                                .unwrap_or_else(|| "---".to_string()),
                            state.map(|s| s.ptt).unwrap_or(false),
                            state.and_then(|s| s.frequency_hz).unwrap_or(0),
                            state.and_then(|s| s.mode).unwrap_or(OperatingMode::Usb),
                        )
                    };
                (
                    idx,
                    panel.handle,
                    panel.name.clone(),
                    panel.port.clone(),
                    panel.connection_type,
                    panel.sim_radio_id.clone(),
                    panel.expanded,
                    panel.protocol,
                    freq_display,
                    mode_display,
                    ptt,
                    freq_hz,
                    mode,
                )
            })
            .collect();

        let mut selected_handle = None;
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
            conn_type,
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
            let is_active = active == Some(*handle);
            let is_virtual = *conn_type == RadioConnectionType::Virtual;

            // Determine background color based on state
            let bg_color = if *ptt {
                if is_virtual {
                    Color32::from_rgb(80, 40, 20) // Red-orange tint for virtual
                } else {
                    Color32::from_rgb(80, 30, 30) // Red tint for COM
                }
            } else if is_active {
                if is_virtual {
                    Color32::from_rgb(60, 50, 30)
                } else {
                    Color32::from_rgb(40, 60, 40)
                }
            } else if is_virtual {
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
                    // Top row: Badge, TX indicator, and Select/Expand button
                    ui.horizontal(|ui| {
                        // Connection type badge
                        let badge_color = if is_virtual {
                            Color32::from_rgb(255, 165, 0) // Orange for virtual
                        } else {
                            Color32::from_rgb(100, 180, 100) // Green for COM
                        };
                        ui.label(
                            RichText::new(conn_type.badge())
                                .color(badge_color)
                                .strong()
                                .size(10.0),
                        );

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
                                selected_handle = Some(*handle);
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
                        let detail = if is_virtual {
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
                    if is_virtual && *expanded {
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
            if let Err(e) = self.multiplexer.select_radio(handle) {
                self.report_warning("Multiplexer", format!("Radio selection failed: {}", e));
            }
        }
        if let Some(idx) = toggle_expanded_idx {
            self.radio_panels[idx].expanded = !self.radio_panels[idx].expanded;
        }
        if let Some((sim_id, freq)) = freq_change {
            self.simulation_panel
                .context_mut()
                .set_radio_frequency(&sim_id, freq);
        }
        if let Some((sim_id, m)) = mode_change {
            self.simulation_panel
                .context_mut()
                .set_radio_mode(&sim_id, m);
        }
        if let Some((sim_id, active)) = ptt_change {
            self.simulation_panel
                .context_mut()
                .set_radio_ptt(&sim_id, active);
        }
        if let Some(idx) = remove_radio_idx {
            let Some(panel) = self.radio_panels.get(idx) else {
                return; // Index no longer valid
            };
            // Extract data before mutating
            let is_virtual = panel.connection_type == RadioConnectionType::Virtual;
            let sim_id = panel.sim_radio_id.clone();
            let handle = panel.handle;

            if is_virtual {
                // Virtual radio - remove via simulation panel (event will clean up)
                if let Some(sim_id) = sim_id {
                    self.simulation_panel.context_mut().remove_radio(&sim_id);
                }
            } else {
                // COM radio - remove connection and panel, save config
                self.radio_connections.remove(&handle);
                self.multiplexer.remove_radio(handle);
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
                            self.amp_connection = None;
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
                    let is_connected = self.amp_connection.is_some();
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

        let mut mode = self.multiplexer.switching_mode();

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
                                self.multiplexer.set_switching_mode(mode);
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

        if self.multiplexer.is_locked() {
            ui.horizontal(|ui| {
                ui.label("Lockout:");
                ui.label(format!("{}ms", self.multiplexer.lockout_remaining_ms()));
            });
        }
    }

    /// Draw the traffic monitor panel
    fn draw_traffic_panel(&mut self, ui: &mut Ui) {
        ui.heading("Traffic Monitor");

        // Draw and handle export actions
        if let Some(action) = self.traffic_monitor
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

        // Sync diagnostic filter settings back to settings and save if changed
        let (show_diag, show_info, show_warn, show_err) =
            self.traffic_monitor.diagnostic_settings_mut();
        if self.settings.show_diagnostics != *show_diag
            || self.settings.show_diagnostic_info != *show_info
            || self.settings.show_diagnostic_warning != *show_warn
            || self.settings.show_diagnostic_error != *show_err
        {
            self.settings.show_diagnostics = *show_diag;
            self.settings.show_diagnostic_info = *show_info;
            self.settings.show_diagnostic_warning = *show_warn;
            self.settings.show_diagnostic_error = *show_err;
            if let Err(e) = self.settings.save() {
                self.handle_save_error(e);
            }
        }
    }

    /// Detect new radios (without clearing existing configured radios)
    fn detect_new_radios(&mut self) {
        self.report_info("Scanner", "Starting radio detection scan...");
        self.scanning = true;
        self.set_status("Detecting new radios...".into());

        // Spawn background thread for async scanning
        let tx = self.bg_tx.clone();
        std::thread::spawn(move || {
            // Create a tokio runtime for the async scan
            let rt = match tokio::runtime::Runtime::new() {
                Ok(rt) => rt,
                Err(e) => {
                    let _ = tx.send(BackgroundMessage::Error(format!(
                        "Failed to create runtime: {}",
                        e
                    )));
                    return;
                }
            };

            // Run the scan
            let result = rt.block_on(async {
                let mut scanner = PortScanner::new();
                scanner.scan().await
            });

            // Send results back
            let _ = tx.send(BackgroundMessage::ScanComplete(result));
        });
    }

    /// Connect to the amplifier
    fn connect_amplifier(&mut self) {
        if self.amp_port.is_empty() {
            self.set_status("No amplifier port selected".into());
            return;
        }

        // Update multiplexer config with amplifier settings
        let amp_config = cat_mux::state::AmplifierConfig {
            port: self.amp_port.clone(),
            protocol: self.amp_protocol,
            baud_rate: self.amp_baud,
            civ_address: if self.amp_protocol == Protocol::IcomCIV {
                Some(self.amp_civ_address)
            } else {
                None
            },
        };
        self.multiplexer.set_amplifier_config(amp_config);

        match AmplifierConnection::new(&self.amp_port, self.amp_baud, self.bg_tx.clone()) {
            Ok(conn) => {
                self.amp_connection = Some(conn);
                self.set_status(format!(
                    "Connected to amplifier on {} @ {} baud",
                    self.amp_port, self.amp_baud
                ));
            }
            Err(e) => {
                self.report_err("Amplifier", format!("Failed to connect: {}", e));
            }
        }
    }

    /// Disconnect from the amplifier
    fn disconnect_amplifier(&mut self) {
        self.amp_connection = None;
        self.set_status("Amplifier disconnected".into());
    }

    /// Process simulation events and update traffic monitor
    fn process_simulation_events(&mut self) {
        // Ensure translator uses current UI-selected amplifier protocol
        // This is needed because in simulation mode, connect_amplifier() is never called
        if self.multiplexer.amplifier_config().protocol != self.amp_protocol {
            let amp_config = cat_mux::state::AmplifierConfig {
                port: self.amp_port.clone(),
                protocol: self.amp_protocol,
                baud_rate: self.amp_baud,
                civ_address: if self.amp_protocol == Protocol::IcomCIV {
                    Some(self.amp_civ_address)
                } else {
                    None
                },
            };
            self.multiplexer.set_amplifier_config(amp_config);
        }

        for event in self.simulation_panel.drain_events() {
            match event {
                SimulationEvent::RadioOutput { radio_id, data } => {
                    // Add to traffic monitor as simulated incoming (response from radio)
                    let protocol = self.get_protocol_for_sim_radio(&radio_id);
                    self.traffic_monitor
                        .add_simulated_incoming(radio_id, &data, protocol);
                }
                SimulationEvent::RadioCommandSent { radio_id, data } => {
                    // Add to traffic monitor as outgoing to simulated radio
                    let protocol = self.get_protocol_for_sim_radio(&radio_id);
                    self.traffic_monitor
                        .add_to_simulated_radio(radio_id, &data, protocol);
                }
                SimulationEvent::RadioAdded { radio_id } => {
                    // Register the simulated radio with the multiplexer
                    if let Some(radio) = self.simulation_panel.context().get_radio(&radio_id) {
                        let name = radio.id().to_string();
                        let protocol = radio.protocol();
                        let handle =
                            self.multiplexer
                                .add_radio(name.clone(), "VRT".to_string(), protocol);
                        self.sim_radio_handles.insert(radio_id.clone(), handle);

                        // Create a RadioPanel for the unified list
                        self.radio_panels.push(RadioPanel::new_virtual(
                            handle,
                            name.clone(),
                            protocol,
                            radio_id.clone(),
                        ));

                        // Enable auto-info mode on the radio so it sends unsolicited updates
                        // This is done via send_command which generates traffic monitor events
                        self.simulation_panel.context_mut().send_command(
                            &radio_id,
                            &RadioCommand::EnableAutoInfo { enabled: true },
                        );
                    }
                    self.set_status(format!("Virtual radio added: {}", radio_id));
                    // Save virtual radios to settings
                    self.save_virtual_radios();
                }
                SimulationEvent::RadioRemoved { radio_id } => {
                    // Remove the simulated radio from the multiplexer
                    if let Some(handle) = self.sim_radio_handles.remove(&radio_id) {
                        self.multiplexer.remove_radio(handle);
                    }
                    // Remove from radio_panels
                    self.radio_panels
                        .retain(|p| p.sim_radio_id.as_ref() != Some(&radio_id));
                    self.set_status(format!("Virtual radio removed: {}", radio_id));
                    // Save virtual radios to settings
                    self.save_virtual_radios();
                }
                SimulationEvent::RadioStateChanged {
                    radio_id,
                    frequency_hz,
                    mode,
                    ptt,
                } => {
                    // Feed state changes to the multiplexer to trigger auto-switching
                    if let Some(&handle) = self.sim_radio_handles.get(&radio_id) {
                        if let Some(hz) = frequency_hz {
                            self.multiplexer
                                .process_radio_command(handle, RadioCommand::SetFrequency { hz });
                        }
                        if let Some(m) = mode {
                            self.multiplexer
                                .process_radio_command(handle, RadioCommand::SetMode { mode: m });
                        }
                        if let Some(active) = ptt {
                            self.multiplexer
                                .process_radio_command(handle, RadioCommand::SetPtt { active });
                        }
                    }
                    // Save frequency/mode changes (but not PTT which is transient)
                    if frequency_hz.is_some() || mode.is_some() {
                        self.save_virtual_radios();
                    }
                }
            }
        }
    }
}

impl eframe::App for CatapultApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Process background messages
        self.process_messages();
        self.process_simulation_events();
        self.process_mux_events();

        // Poll all radio connections and collect commands
        let radio_commands: Vec<_> = self
            .radio_connections
            .iter_mut()
            .filter_map(|(&handle, conn)| conn.poll().map(|cmd| (handle, cmd)))
            .collect();

        // Process radio commands through multiplexer
        for (handle, cmd) in radio_commands {
            if let Some(amp_cmd) = self.multiplexer.process_radio_command(handle, cmd) {
                // Send to amplifier
                if let Some(ref mut amp_conn) = self.amp_connection {
                    if let Err(e) = amp_conn.write(&amp_cmd) {
                        self.report_err("Amplifier", format!("Write error: {}", e));
                    }
                }
            }
        }

        // Poll amplifier for incoming data
        if let Some(ref mut conn) = self.amp_connection {
            conn.poll();
        }

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
            });
        });

        // Request repaint only when animations are active or virtual radios exist
        let has_virtual_radios = self
            .radio_panels
            .iter()
            .any(|p| p.connection_type == RadioConnectionType::Virtual);
        if self.scanning || has_virtual_radios {
            ctx.request_repaint();
        }
    }
}
