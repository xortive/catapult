//! Firmware panel UI component
//!
//! Provides UI for detecting ESP32-S3 devices and flashing firmware.

use std::collections::VecDeque;
use std::sync::mpsc::{self, Receiver, Sender};

use egui::{Color32, RichText, Ui};

use crate::firmware::{
    detect_devices, flash_firmware, DetectedDevice, FlashMessage, FlashState, FIRMWARE_VERSION,
};

/// Maximum log entries to keep
const MAX_LOG_ENTRIES: usize = 100;

/// Firmware panel state
pub struct FirmwarePanel {
    /// Current flash state
    state: FlashState,
    /// Detected devices
    devices: Vec<DetectedDevice>,
    /// Selected device port (if any)
    selected_port: Option<String>,
    /// Message receiver from flash thread
    rx: Receiver<FlashMessage>,
    /// Message sender for flash thread
    tx: Sender<FlashMessage>,
    /// Log messages
    log: VecDeque<String>,
    /// Show log output
    show_log: bool,
}

impl FirmwarePanel {
    /// Create a new firmware panel
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel();

        let mut panel = Self {
            state: FlashState::Idle,
            devices: Vec::new(),
            selected_port: None,
            rx,
            tx,
            log: VecDeque::with_capacity(MAX_LOG_ENTRIES),
            show_log: false,
        };

        // Initial device scan
        panel.refresh_devices();

        panel
    }

    /// Refresh detected devices
    pub fn refresh_devices(&mut self) {
        self.devices = detect_devices();

        // Auto-select first device if none selected
        if self.selected_port.is_none() && !self.devices.is_empty() {
            self.selected_port = Some(self.devices[0].port.clone());
        }

        // Clear selection if device no longer available
        if let Some(ref port) = self.selected_port {
            if !self.devices.iter().any(|d| &d.port == port) {
                self.selected_port = None;
            }
        }
    }

    /// Process messages from flash thread
    pub fn process_messages(&mut self) {
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                FlashMessage::StateChanged(state) => {
                    self.state = state;
                }
                FlashMessage::Log(msg) => {
                    self.add_log(msg);
                }
            }
        }
    }

    /// Add a log message
    fn add_log(&mut self, msg: String) {
        if self.log.len() >= MAX_LOG_ENTRIES {
            self.log.pop_front();
        }
        self.log.push_back(msg);
    }

    /// Start flashing firmware
    fn start_flash(&mut self) {
        if let Some(port) = self.selected_port.clone() {
            self.log.clear();
            self.add_log(format!("Starting firmware update on {}", port));
            flash_firmware(&port, self.tx.clone());
        }
    }

    /// Check if currently flashing
    fn is_flashing(&self) -> bool {
        !matches!(self.state, FlashState::Idle | FlashState::Complete | FlashState::Error(_))
    }

    /// Draw the firmware panel UI
    pub fn draw(&mut self, ui: &mut Ui) {
        // Process any pending messages
        self.process_messages();

        ui.heading("Firmware Update");
        ui.add_space(8.0);

        // Firmware version info
        ui.horizontal(|ui| {
            ui.label("Bundled firmware version:");
            ui.label(RichText::new(FIRMWARE_VERSION).strong());
        });

        ui.add_space(16.0);
        ui.separator();
        ui.add_space(8.0);

        // Device detection section
        ui.horizontal(|ui| {
            ui.heading("Device");

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add_enabled(!self.is_flashing(), egui::Button::new("Refresh"))
                    .clicked()
                {
                    self.refresh_devices();
                }
            });
        });

        ui.add_space(4.0);

        if self.devices.is_empty() {
            ui.label(
                RichText::new("No ESP32-S3 devices detected")
                    .color(Color32::GRAY)
                    .italics(),
            );
            ui.label(
                RichText::new("Connect the cat-bridge via the USB programming port")
                    .color(Color32::GRAY)
                    .size(11.0),
            );
        } else {
            // Device selector
            egui::ComboBox::from_id_salt("device_select")
                .selected_text(
                    self.selected_port
                        .as_deref()
                        .unwrap_or("Select device..."),
                )
                .show_ui(ui, |ui| {
                    for device in &self.devices {
                        let label = if let Some(ref sn) = device.serial_number {
                            format!("{} ({})", device.port, sn)
                        } else {
                            device.port.clone()
                        };
                        ui.selectable_value(
                            &mut self.selected_port,
                            Some(device.port.clone()),
                            label,
                        );
                    }
                });

            if let Some(ref port) = self.selected_port {
                ui.label(
                    RichText::new(format!("Selected: {}", port))
                        .color(Color32::GREEN)
                        .size(11.0),
                );
            }
        }

        ui.add_space(16.0);
        ui.separator();
        ui.add_space(8.0);

        // Flash status section
        ui.heading("Status");
        ui.add_space(4.0);

        // Status display based on state
        match &self.state {
            FlashState::Idle => {
                ui.label("Ready to flash");
            }
            FlashState::Connecting => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label("Connecting to device...");
                });
            }
            FlashState::Erasing => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label("Erasing flash...");
                });
            }
            FlashState::Writing { progress } => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(format!("Writing firmware... {:.0}%", progress * 100.0));
                });
                ui.add(egui::ProgressBar::new(*progress).animate(true));
            }
            FlashState::Verifying => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label("Verifying...");
                });
            }
            FlashState::Complete => {
                ui.label(
                    RichText::new("Firmware update complete!")
                        .color(Color32::GREEN)
                        .strong(),
                );
                ui.label(
                    RichText::new("Device will restart automatically")
                        .color(Color32::GRAY)
                        .size(11.0),
                );
            }
            FlashState::Error(msg) => {
                ui.label(RichText::new("Error:").color(Color32::RED).strong());
                ui.label(RichText::new(msg).color(Color32::RED));
            }
        }

        ui.add_space(16.0);

        // Flash button
        let can_flash = self.selected_port.is_some() && !self.is_flashing();
        let button_text = if self.is_flashing() {
            "Flashing..."
        } else {
            "Update Firmware"
        };

        if ui
            .add_enabled(can_flash, egui::Button::new(button_text).min_size([120.0, 32.0].into()))
            .clicked()
        {
            self.start_flash();
        }

        // Reset button after complete/error
        if matches!(self.state, FlashState::Complete | FlashState::Error(_))
            && ui.button("Reset").clicked()
        {
            self.state = FlashState::Idle;
        }

        ui.add_space(16.0);
        ui.separator();
        ui.add_space(8.0);

        // Log section (collapsible)
        ui.horizontal(|ui| {
            ui.checkbox(&mut self.show_log, "Show Log");
            if !self.log.is_empty() {
                ui.label(
                    RichText::new(format!("({} entries)", self.log.len()))
                        .color(Color32::GRAY)
                        .size(11.0),
                );
            }
        });

        if self.show_log {
            ui.add_space(4.0);

            egui::Frame::none()
                .fill(Color32::from_rgb(20, 20, 20))
                .rounding(4.0)
                .inner_margin(8.0)
                .show(ui, |ui| {
                    egui::ScrollArea::vertical()
                        .max_height(150.0)
                        .stick_to_bottom(true)
                        .show(ui, |ui| {
                            for msg in &self.log {
                                ui.label(RichText::new(msg).monospace().size(11.0));
                            }
                        });
                });
        }
    }
}

impl Default for FirmwarePanel {
    fn default() -> Self {
        Self::new()
    }
}
