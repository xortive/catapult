# CAT Monitor: Full Project Specification

## Overview

A cross-platform desktop application and companion ESP32-S3 firmware that creates an inline monitor/passthrough for amateur radio CAT (Computer Aided Transceiver) protocol traffic between a radio and an amplifier.

## System Architecture

```
┌─────────┐     ┌──────────────────────────────────┐     ┌───────────┐
│  Radio  │────▶│         ESP32-S3 Device          │────▶│ Amplifier │
│(CAT out)│     │                                  │     │ (CAT in)  │
└─────────┘     │  ┌────────┐    ┌─────────────┐  │     └───────────┘
                │  │ UART   │───▶│ USB Device  │  │
                │  │(radio) │    │ (to desktop)│  │
                │  └────────┘    └─────────────┘  │
                │       │              │          │
                │       ▼              ▼          │
                │  ┌─────────────────────────────┐│
                │  │   Passthrough + Logging     ││
                │  └─────────────────────────────┘│
                │              │                  │
                │              ▼                  │
                │  ┌─────────────────┐           │
                │  │ USB Host (amp)  │           │
                │  │ via Pico-PIO-USB│           │
                │  └─────────────────┘           │
                └──────────────────────────────────┘
                               │
                               │ USB (dual CDC-ACM)
                               ▼
                ┌──────────────────────────────────┐
                │      Desktop Application         │
                │                                  │
                │  • Monitor CAT traffic           │
                │  • Decode protocol state         │
                │  • Flash firmware                │
                │  • Configure device              │
                └──────────────────────────────────┘
```

## Hardware Configuration

**Target device:** ESP32-S3 DevKitC or similar with:
- Native USB port → connects to desktop (dual CDC-ACM)
- UART pins → connects to radio's CAT output via USB-serial adapter
- USB host via GPIO bit-banging (esp-idf USB host or custom) → connects to amplifier

**Alternative simpler topology** (if amplifier accepts serial directly):
- Native USB → desktop
- UART0 → radio
- UART1 → amplifier

For this spec, we'll assume the simpler UART-based approach for the amplifier connection, with USB-serial adapters on both radio and amp sides external to the device.

---

## Part 1: ESP32-S3 Firmware

### Language & Framework

- **Rust** using `esp-idf-hal` and `esp-idf-svc`
- USB via `esp-idf-sys` bindings to TinyUSB (ESP-IDF includes it)
- Alternative: `embassy` async framework with `embassy-usb` (more idiomatic Rust, but less mature on ESP32)

### Firmware Features

1. **Dual CDC-ACM USB Device**
   - Interface 0: Radio traffic stream
   - Interface 1: Amplifier traffic stream
   - Both interfaces echo all passthrough traffic to desktop

2. **UART Handling**
   - UART0: Radio CAT input (configurable baud, typically 9600 or 19200)
   - UART1: Amplifier CAT output

3. **Passthrough Logic**
   - Radio RX → Amplifier TX (forwarded)
   - Radio RX → USB CDC 0 (mirrored to desktop)
   - Amplifier RX → USB CDC 1 (mirrored to desktop, if bidirectional)

4. **Configuration**
   - Stored in NVS (non-volatile storage)
   - Baud rates, parity, stop bits for each UART
   - Optional filtering/injection modes

5. **Bootloader Mode**
   - Device can be put into download mode via USB command
   - Enables flashing without physical button press

### Firmware Project Structure

```
cat-monitor-firmware/
├── Cargo.toml
├── sdkconfig.defaults
├── build.rs
├── src/
│   ├── main.rs
│   ├── usb.rs          # Dual CDC setup
│   ├── uart.rs         # UART configuration and handling
│   ├── passthrough.rs  # Core forwarding logic
│   ├── config.rs       # NVS configuration
│   └── protocol.rs     # Optional CAT frame parsing
└── README.md
```

### Key Firmware Dependencies

```toml
[dependencies]
esp-idf-hal = "0.44"
esp-idf-svc = "0.49"
esp-idf-sys = { version = "0.35", features = ["binstart"] }
embedded-hal = "1.0"
heapless = "0.8"          # Static buffers
log = "0.4"
```

### Firmware Pseudocode

```rust
// src/main.rs

use esp_idf_hal::prelude::*;
use esp_idf_hal::uart::{self, UartDriver};
use esp_idf_svc::eventloop::EspSystemEventLoop;

mod usb;
mod passthrough;
mod config;

fn main() -> anyhow::Result<()> {
    esp_idf_svc::sys::link_patches();
    esp_idf_svc::log::EspLogger::initialize_default();

    let peripherals = Peripherals::take()?;
    
    // Initialize UARTs
    let uart_radio = UartDriver::new(
        peripherals.uart1,
        peripherals.pins.gpio17,  // TX
        peripherals.pins.gpio18,  // RX
        Option::<gpio::Gpio0>::None,
        Option::<gpio::Gpio0>::None,
        &uart::config::Config::default().baudrate(Hertz(9600)),
    )?;
    
    let uart_amp = UartDriver::new(
        peripherals.uart2,
        peripherals.pins.gpio19,  // TX
        peripherals.pins.gpio20,  // RX
        Option::<gpio::Gpio0>::None,
        Option::<gpio::Gpio0>::None,
        &uart::config::Config::default().baudrate(Hertz(9600)),
    )?;

    // Initialize dual CDC USB
    let (cdc_radio, cdc_amp) = usb::init_dual_cdc()?;

    // Main passthrough loop
    passthrough::run(uart_radio, uart_amp, cdc_radio, cdc_amp)?;

    Ok(())
}
```

```rust
// src/passthrough.rs

use heapless::Vec;

const BUF_SIZE: usize = 256;

pub fn run(
    mut uart_radio: UartDriver,
    mut uart_amp: UartDriver,
    mut cdc_radio: CdcInterface,
    mut cdc_amp: CdcInterface,
) -> anyhow::Result<()> {
    let mut radio_buf: Vec<u8, BUF_SIZE> = Vec::new();
    let mut amp_buf: Vec<u8, BUF_SIZE> = Vec::new();

    loop {
        // Radio → Amp passthrough + mirror to desktop
        if let Ok(bytes) = uart_radio.read(&mut radio_buf, 0) {
            if bytes > 0 {
                // Forward to amplifier
                uart_amp.write(&radio_buf[..bytes])?;
                
                // Mirror to desktop via CDC interface 0
                cdc_radio.write(&radio_buf[..bytes])?;
            }
        }

        // Amp → Radio passthrough (if bidirectional) + mirror
        if let Ok(bytes) = uart_amp.read(&mut amp_buf, 0) {
            if bytes > 0 {
                uart_radio.write(&amp_buf[..bytes])?;
                cdc_amp.write(&amp_buf[..bytes])?;
            }
        }

        // Check for commands from desktop (optional)
        // Could implement injection or configuration here

        // Small delay to prevent tight spinning
        std::thread::sleep(std::time::Duration::from_micros(100));
    }
}
```

---

## Part 2: Desktop Application

### Language & Framework

- **Rust**
- **GUI:** `egui` with `eframe` (immediate mode, very cross-platform)
- **Serial:** `serialport` crate
- **Flashing:** `espflash` crate (library form)
- **Async:** `tokio` for background serial handling

### Desktop App Features

1. **Device Management**
   - Auto-detect connected CAT Monitor devices (by VID/PID)
   - Display connection status
   - Flash firmware to device
   - Reset device into bootloader mode

2. **Traffic Monitor**
   - Real-time display of CAT traffic (hex + decoded)
   - Separate panels for radio→amp and amp→radio
   - Scrolling log with timestamps
   - Pause/resume
   - Clear
   - Export to file

3. **Protocol Decoding**
   - Parse common CAT protocols (Yaesu, Icom, Kenwood, Elecraft)
   - Display decoded state: frequency, mode, power, PTT status
   - State history/timeline

4. **Configuration**
   - Set UART baud rates
   - Select CAT protocol variant
   - Dark/light theme

5. **Firmware Management**
   - Bundled firmware binary (embedded in app)
   - Download latest from GitHub releases
   - One-click flash
   - Version detection

### Desktop Project Structure

```
cat-monitor-desktop/
├── Cargo.toml
├── build.rs                    # Embed firmware binary
├── src/
│   ├── main.rs
│   ├── app.rs                  # Main egui application
│   ├── ui/
│   │   ├── mod.rs
│   │   ├── traffic_view.rs     # Traffic monitor panel
│   │   ├── state_view.rs       # Decoded state panel
│   │   ├── device_panel.rs     # Connection/flash controls
│   │   └── settings.rs         # Configuration UI
│   ├── serial/
│   │   ├── mod.rs
│   │   ├── connection.rs       # Serial port handling
│   │   └── detector.rs         # Device auto-detection
│   ├── protocol/
│   │   ├── mod.rs
│   │   ├── cat.rs              # Generic CAT traits
│   │   ├── yaesu.rs            # Yaesu CAT decoding
│   │   ├── icom.rs             # Icom CI-V decoding
│   │   └── kenwood.rs          # Kenwood decoding
│   ├── flash/
│   │   ├── mod.rs
│   │   └── espflash.rs         # Firmware flashing
│   └── state.rs                # Application state
├── assets/
│   └── firmware.bin            # Embedded firmware (built separately)
└── README.md
```

### Key Desktop Dependencies

```toml
[dependencies]
eframe = "0.29"
egui = "0.29"
egui_extras = "0.29"

tokio = { version = "1", features = ["full"] }
serialport = "4.5"
espflash = "3"               # For flashing ESP32

serde = { version = "1", features = ["derive"] }
serde_json = "1"
chrono = "0.4"
log = "0.4"
env_logger = "0.11"
anyhow = "1"
thiserror = "1"

# For bundling firmware
include_bytes_plus = "1"

[target.'cfg(windows)'.dependencies]
winapi = { version = "0.3", features = ["winuser"] }
```

### Core Desktop Code

```rust
// src/main.rs

mod app;
mod ui;
mod serial;
mod protocol;
mod flash;
mod state;

fn main() -> eframe::Result<()> {
    env_logger::init();

    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1200.0, 800.0])
            .with_min_inner_size([800.0, 600.0]),
        ..Default::default()
    };

    eframe::run_native(
        "CAT Monitor",
        native_options,
        Box::new(|cc| Ok(Box::new(app::CatMonitorApp::new(cc)))),
    )
}
```

```rust
// src/app.rs

use eframe::egui;
use std::sync::{Arc, Mutex};
use tokio::runtime::Runtime;

use crate::serial::SerialManager;
use crate::state::AppState;
use crate::ui::{TrafficView, StateView, DevicePanel, SettingsPanel};

pub struct CatMonitorApp {
    state: Arc<Mutex<AppState>>,
    serial_manager: SerialManager,
    runtime: Runtime,
    
    // UI components
    traffic_view: TrafficView,
    state_view: StateView,
    device_panel: DevicePanel,
    settings_open: bool,
}

impl CatMonitorApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let runtime = Runtime::new().expect("Failed to create Tokio runtime");
        let state = Arc::new(Mutex::new(AppState::default()));
        let serial_manager = SerialManager::new(state.clone());

        Self {
            state,
            serial_manager,
            runtime,
            traffic_view: TrafficView::new(),
            state_view: StateView::new(),
            device_panel: DevicePanel::new(),
            settings_open: false,
        }
    }
}

impl eframe::App for CatMonitorApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Request continuous repaints for real-time updates
        ctx.request_repaint();

        // Top panel: device connection and controls
        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            ui.horizontal(|ui| {
                self.device_panel.show(ui, &mut self.serial_manager, &self.runtime);
                
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("⚙ Settings").clicked() {
                        self.settings_open = true;
                    }
                });
            });
        });

        // Left panel: decoded state
        egui::SidePanel::left("state_panel")
            .default_width(300.0)
            .show(ctx, |ui| {
                let state = self.state.lock().unwrap();
                self.state_view.show(ui, &state);
            });

        // Central panel: traffic monitor
        egui::CentralPanel::default().show(ctx, |ui| {
            let state = self.state.lock().unwrap();
            self.traffic_view.show(ui, &state);
        });

        // Settings window
        if self.settings_open {
            egui::Window::new("Settings")
                .open(&mut self.settings_open)
                .show(ctx, |ui| {
                    SettingsPanel::show(ui, &self.state);
                });
        }
    }
}
```

```rust
// src/state.rs

use chrono::{DateTime, Local};
use std::collections::VecDeque;

#[derive(Default)]
pub struct AppState {
    pub connected: bool,
    pub device_port: Option<String>,
    pub firmware_version: Option<String>,
    
    pub radio_traffic: VecDeque<TrafficEntry>,
    pub amp_traffic: VecDeque<TrafficEntry>,
    
    pub decoded_state: RadioState,
    
    pub config: AppConfig,
}

pub struct TrafficEntry {
    pub timestamp: DateTime<Local>,
    pub data: Vec<u8>,
    pub decoded: Option<String>,
}

#[derive(Default)]
pub struct RadioState {
    pub frequency_hz: Option<u64>,
    pub mode: Option<String>,
    pub ptt: bool,
    pub power_watts: Option<u16>,
    pub swr: Option<f32>,
}

#[derive(Default)]
pub struct AppConfig {
    pub radio_baud: u32,
    pub amp_baud: u32,
    pub protocol: CatProtocol,
    pub max_log_entries: usize,
}

#[derive(Default, Clone, Copy, PartialEq)]
pub enum CatProtocol {
    #[default]
    Yaesu,
    Icom,
    Kenwood,
    Elecraft,
}
```

```rust
// src/serial/connection.rs

use serialport::{SerialPort, SerialPortInfo};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::mpsc;

use crate::state::AppState;

const VENDOR_ID: u16 = 0x303A;  // Espressif VID
const PRODUCT_ID: u16 = 0x1001; // Custom PID for CAT Monitor

pub struct SerialManager {
    state: Arc<Mutex<AppState>>,
    radio_port: Option<Box<dyn SerialPort>>,
    amp_port: Option<Box<dyn SerialPort>>,
    shutdown_tx: Option<mpsc::Sender<()>>,
}

impl SerialManager {
    pub fn new(state: Arc<Mutex<AppState>>) -> Self {
        Self {
            state,
            radio_port: None,
            amp_port: None,
            shutdown_tx: None,
        }
    }

    pub fn detect_devices() -> Vec<SerialPortInfo> {
        serialport::available_ports()
            .unwrap_or_default()
            .into_iter()
            .filter(|port| {
                if let serialport::SerialPortType::UsbPort(usb) = &port.port_type {
                    usb.vid == VENDOR_ID && usb.pid == PRODUCT_ID
                } else {
                    false
                }
            })
            .collect()
    }

    pub fn connect(&mut self, port_name: &str) -> anyhow::Result<()> {
        // ESP32-S3 with dual CDC will enumerate as two sequential ports
        // e.g., /dev/ttyACM0 and /dev/ttyACM1
        let radio_port_name = port_name.to_string();
        let amp_port_name = increment_port_name(port_name)?;

        self.radio_port = Some(
            serialport::new(&radio_port_name, 115200)
                .timeout(Duration::from_millis(10))
                .open()?
        );

        self.amp_port = Some(
            serialport::new(&amp_port_name, 115200)
                .timeout(Duration::from_millis(10))
                .open()?
        );

        let mut state = self.state.lock().unwrap();
        state.connected = true;
        state.device_port = Some(port_name.to_string());

        Ok(())
    }

    pub fn disconnect(&mut self) {
        self.radio_port = None;
        self.amp_port = None;
        
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.blocking_send(());
        }

        let mut state = self.state.lock().unwrap();
        state.connected = false;
        state.device_port = None;
    }

    pub fn poll(&mut self) {
        // Read from radio CDC
        if let Some(ref mut port) = self.radio_port {
            let mut buf = [0u8; 256];
            if let Ok(bytes_read) = port.read(&mut buf) {
                if bytes_read > 0 {
                    let data = buf[..bytes_read].to_vec();
                    let mut state = self.state.lock().unwrap();
                    state.radio_traffic.push_back(TrafficEntry {
                        timestamp: chrono::Local::now(),
                        data: data.clone(),
                        decoded: None, // Protocol decoder fills this in
                    });
                    
                    // Trim log if needed
                    while state.radio_traffic.len() > state.config.max_log_entries {
                        state.radio_traffic.pop_front();
                    }
                }
            }
        }

        // Same for amp CDC
        if let Some(ref mut port) = self.amp_port {
            let mut buf = [0u8; 256];
            if let Ok(bytes_read) = port.read(&mut buf) {
                if bytes_read > 0 {
                    let data = buf[..bytes_read].to_vec();
                    let mut state = self.state.lock().unwrap();
                    state.amp_traffic.push_back(TrafficEntry {
                        timestamp: chrono::Local::now(),
                        data,
                        decoded: None,
                    });
                    
                    while state.amp_traffic.len() > state.config.max_log_entries {
                        state.amp_traffic.pop_front();
                    }
                }
            }
        }
    }
}

fn increment_port_name(port: &str) -> anyhow::Result<String> {
    // /dev/ttyACM0 -> /dev/ttyACM1
    // COM3 -> COM4
    // This is a simplification; real implementation needs platform handling
    if let Some(prefix_end) = port.rfind(|c: char| !c.is_ascii_digit()) {
        let prefix = &port[..=prefix_end];
        let num: u32 = port[prefix_end + 1..].parse()?;
        Ok(format!("{}{}", prefix, num + 1))
    } else {
        anyhow::bail!("Cannot parse port name: {}", port)
    }
}
```

```rust
// src/flash/espflash.rs

use espflash::cli::flash;
use espflash::flasher::{FlashFrequency, FlashMode, FlashSize};
use std::path::Path;

const EMBEDDED_FIRMWARE: &[u8] = include_bytes!("../../assets/firmware.bin");

pub struct Flasher;

impl Flasher {
    pub fn flash_embedded(port: &str) -> anyhow::Result<()> {
        // Write embedded firmware to temp file
        let temp_dir = std::env::temp_dir();
        let firmware_path = temp_dir.join("cat-monitor-firmware.bin");
        std::fs::write(&firmware_path, EMBEDDED_FIRMWARE)?;

        Self::flash_file(port, &firmware_path)
    }

    pub fn flash_file(port: &str, firmware_path: &Path) -> anyhow::Result<()> {
        // espflash library usage
        // This is simplified; real implementation needs more setup
        
        let port_info = serialport::new(port, 115200);
        
        // Connect to bootloader
        // Flash firmware
        // Reset device
        
        log::info!("Flashing firmware from {:?} to {}", firmware_path, port);
        
        // Actual espflash integration would go here
        // The espflash crate provides programmatic access to flashing
        
        Ok(())
    }

    pub fn reset_to_bootloader(port: &str) -> anyhow::Result<()> {
        // Send command to firmware to reset into bootloader
        // This uses the standard ESP32 RTS/DTR reset sequence
        // or a custom USB command if we implement one
        
        let mut port = serialport::new(port, 115200)
            .timeout(std::time::Duration::from_millis(100))
            .open()?;

        // Toggle DTR and RTS to enter bootloader
        port.write_data_terminal_ready(false)?;
        port.write_request_to_send(true)?;
        std::thread::sleep(std::time::Duration::from_millis(100));
        port.write_data_terminal_ready(true)?;
        port.write_request_to_send(false)?;
        std::thread::sleep(std::time::Duration::from_millis(100));
        port.write_data_terminal_ready(false)?;

        Ok(())
    }
}
```

```rust
// src/ui/traffic_view.rs

use eframe::egui;
use crate::state::{AppState, TrafficEntry};

pub struct TrafficView {
    auto_scroll: bool,
    paused: bool,
    show_hex: bool,
    show_ascii: bool,
}

impl TrafficView {
    pub fn new() -> Self {
        Self {
            auto_scroll: true,
            paused: false,
            show_hex: true,
            show_ascii: true,
        }
    }

    pub fn show(&mut self, ui: &mut egui::Ui, state: &AppState) {
        ui.horizontal(|ui| {
            ui.heading("Traffic Monitor");
            ui.separator();
            ui.checkbox(&mut self.paused, "Pause");
            ui.checkbox(&mut self.auto_scroll, "Auto-scroll");
            ui.checkbox(&mut self.show_hex, "Hex");
            ui.checkbox(&mut self.show_ascii, "ASCII");
            
            if ui.button("Clear").clicked() {
                // Would need mutable state access here
            }
            
            if ui.button("Export").clicked() {
                // Export to file
            }
        });

        ui.separator();

        // Split view: radio traffic and amp traffic
        ui.columns(2, |columns| {
            columns[0].vertical(|ui| {
                ui.heading("Radio → Amp");
                self.show_traffic_log(ui, &state.radio_traffic);
            });

            columns[1].vertical(|ui| {
                ui.heading("Amp → Radio");
                self.show_traffic_log(ui, &state.amp_traffic);
            });
        });
    }

    fn show_traffic_log(&self, ui: &mut egui::Ui, entries: &std::collections::VecDeque<TrafficEntry>) {
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .stick_to_bottom(self.auto_scroll)
            .show(ui, |ui| {
                for entry in entries.iter() {
                    ui.horizontal(|ui| {
                        // Timestamp
                        ui.monospace(
                            entry.timestamp.format("%H:%M:%S%.3f").to_string()
                        );
                        
                        ui.separator();

                        // Hex display
                        if self.show_hex {
                            let hex: String = entry.data
                                .iter()
                                .map(|b| format!("{:02X}", b))
                                .collect::<Vec<_>>()
                                .join(" ");
                            ui.monospace(&hex);
                        }

                        // ASCII display
                        if self.show_ascii {
                            let ascii: String = entry.data
                                .iter()
                                .map(|&b| {
                                    if b.is_ascii_graphic() || b == b' ' {
                                        b as char
                                    } else {
                                        '.'
                                    }
                                })
                                .collect();
                            ui.monospace(format!("│{}│", ascii));
                        }

                        // Decoded value if available
                        if let Some(ref decoded) = entry.decoded {
                            ui.label(decoded);
                        }
                    });
                }
            });
    }
}
```

```rust
// src/protocol/yaesu.rs

use crate::state::RadioState;

/// Yaesu CAT protocol decoder
/// Yaesu uses a 5-byte command format for most radios
pub struct YaesuDecoder;

impl YaesuDecoder {
    pub fn decode(data: &[u8], state: &mut RadioState) -> Option<String> {
        if data.len() < 5 {
            return None;
        }

        let cmd = data[4];
        
        match cmd {
            0x00 => {
                // Lock on
                Some("Lock ON".to_string())
            }
            0x01 => {
                // Set frequency
                let freq = Self::bcd_to_freq(&data[0..4]);
                state.frequency_hz = Some(freq);
                Some(format!("Set Freq: {} Hz", freq))
            }
            0x07 => {
                // Set mode
                let mode = Self::decode_mode(data[0]);
                state.mode = Some(mode.clone());
                Some(format!("Set Mode: {}", mode))
            }
            0x08 => {
                // PTT ON
                state.ptt = true;
                Some("PTT ON".to_string())
            }
            0x88 => {
                // PTT OFF
                state.ptt = false;
                Some("PTT OFF".to_string())
            }
            0x03 => {
                // Read frequency and mode (query)
                Some("Query Freq/Mode".to_string())
            }
            _ => {
                Some(format!("Unknown cmd: {:02X}", cmd))
            }
        }
    }

    fn bcd_to_freq(data: &[u8]) -> u64 {
        // Yaesu BCD frequency encoding
        // Each byte contains two BCD digits
        let mut freq: u64 = 0;
        for &byte in data {
            freq = freq * 100 + (byte >> 4) as u64 * 10 + (byte & 0x0F) as u64;
        }
        freq * 10 // Convert to Hz
    }

    fn decode_mode(mode_byte: u8) -> String {
        match mode_byte {
            0x00 => "LSB",
            0x01 => "USB",
            0x02 => "CW",
            0x03 => "CW-R",
            0x04 => "AM",
            0x06 => "WFM",
            0x08 => "FM",
            0x0A => "DIG",
            0x0C => "PKT",
            _ => "Unknown",
        }.to_string()
    }
}
```

---

## Part 3: Build System

### Firmware Build

```toml
# cat-monitor-firmware/Cargo.toml

[package]
name = "cat-monitor-firmware"
version = "0.1.0"
edition = "2021"

[dependencies]
esp-idf-hal = "0.44"
esp-idf-svc = "0.49"
esp-idf-sys = { version = "0.35", features = ["binstart"] }
embedded-hal = "1.0"
heapless = "0.8"
log = "0.4"
anyhow = "1"

[build-dependencies]
embuild = "0.32"
```

```bash
# Build firmware
cd cat-monitor-firmware
cargo build --release

# The binary will be at target/xtensa-esp32s3-espidf/release/cat-monitor-firmware
```

### Desktop Build

```bash
# Build for current platform
cd cat-monitor-desktop
cargo build --release

# Cross-compile for other platforms
# Windows (from Linux)
cargo build --release --target x86_64-pc-windows-gnu

# macOS (requires osxcross)
cargo build --release --target x86_64-apple-darwin
```

### CI/CD (GitHub Actions)

```yaml
# .github/workflows/release.yml

name: Release

on:
  push:
    tags:
      - 'v*'

jobs:
  build-firmware:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      
      - name: Setup Rust (ESP)
        uses: esp-rs/xtensa-toolchain@v1.5
        with:
          default: true
          ldproxy: true
      
      - name: Build firmware
        run: |
          cd cat-monitor-firmware
          cargo build --release
      
      - uses: actions/upload-artifact@v4
        with:
          name: firmware
          path: cat-monitor-firmware/target/xtensa-esp32s3-espidf/release/cat-monitor-firmware.bin

  build-desktop:
    needs: build-firmware
    strategy:
      matrix:
        include:
          - os: ubuntu-latest
            target: x86_64-unknown-linux-gnu
            artifact: cat-monitor-linux
          - os: windows-latest
            target: x86_64-pc-windows-msvc
            artifact: cat-monitor-windows.exe
          - os: macos-latest
            target: x86_64-apple-darwin
            artifact: cat-monitor-macos

    runs-on: ${{ matrix.os }}
    
    steps:
      - uses: actions/checkout@v4
      
      - uses: actions/download-artifact@v4
        with:
          name: firmware
          path: cat-monitor-desktop/assets/
      
      - name: Setup Rust
        uses: dtolnay/rust-toolchain@stable
      
      - name: Build
        run: |
          cd cat-monitor-desktop
          cargo build --release --target ${{ matrix.target }}
      
      - uses: actions/upload-artifact@v4
        with:
          name: ${{ matrix.artifact }}
          path: cat-monitor-desktop/target/${{ matrix.target }}/release/cat-monitor*
```

---

## Part 4: Protocol Reference

### Yaesu CAT Protocol

Yaesu radios use a 5-byte command format:

```
[P1] [P2] [P3] [P4] [CMD]
```

Common commands:
- `0x01` - Set frequency (P1-P4 = BCD frequency)
- `0x03` - Read frequency and mode
- `0x07` - Set operating mode
- `0x08` - PTT ON
- `0x88` - PTT OFF
- `0x00` - Lock ON
- `0x80` - Lock OFF

### Icom CI-V Protocol

Icom uses variable-length messages with framing:

```
[0xFE] [0xFE] [TO] [FROM] [CMD] [SUB] [DATA...] [0xFD]
```

- `0xFE 0xFE` - Preamble
- `TO` - Destination address (radio default: 0x00-0x7F depending on model)
- `FROM` - Source address (controller default: 0xE0)
- `0xFD` - End of message

### Kenwood Protocol

Kenwood uses ASCII-based commands terminated with semicolon:

```
FA00014250000;  // Set VFO A to 14.250 MHz
FA;             // Query VFO A frequency
MD2;            // Set mode to USB
IF;             // Read transceiver status
```

---

## Part 5: Testing Strategy

### Firmware Testing

1. **Unit tests** for protocol parsing (run on host)
2. **Integration tests** with USB loopback
3. **Hardware-in-loop** with actual radio/amp

### Desktop Testing

1. **Unit tests** for protocol decoders
2. **Mock serial port** for UI testing
3. **Integration tests** with firmware simulator

### Test Fixtures

Create recorded CAT traffic samples for each protocol variant to use in automated testing.

---

## Summary

| Component | Language | Key Libraries |
|-----------|----------|---------------|
| Firmware | Rust | esp-idf-hal, TinyUSB (via esp-idf-sys), heapless |
| Desktop | Rust | egui/eframe, serialport, espflash, tokio |
| Protocol | Rust | Custom decoders for Yaesu, Icom, Kenwood |

## Next Steps

1. Set up ESP32-S3 development environment with Rust
2. Implement basic dual CDC USB device
3. Add UART passthrough
4. Build minimal desktop app with device detection
5. Add flashing capability
6. Implement protocol decoders
7. Polish UI and add configuration persistence
