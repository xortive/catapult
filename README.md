# Catapult - CAT Protocol Multiplexer

A desktop-primary CAT (Computer Aided Transceiver) Protocol Multiplexer for amateur radio.

Connect unlimited transceivers to a single amplifier via CAT protocol translation.

## Features

- **Multi-Protocol Support**: Yaesu CAT, Icom CI-V, Kenwood, and Elecraft protocols
- **Auto-Detection**: Automatically detects CAT-capable radios on serial ports
- **Protocol Translation**: Translates between any supported protocol
- **Intelligent Switching**: Manual, PTT-triggered, or frequency-change triggered
- **Desktop Application**: Cross-platform GUI built with egui
- **ESP32 Bridge**: Dual USB serial bridge allowing host to appear as USB device to amplifier

## Architecture

```
┌──────────────────────────────────────────────────────────────────────┐
│                          HOST COMPUTER                               │
│                                                                      │
│  ┌────────────────────────────────────────────────────────────────┐  │
│  │                    Desktop Application                         │  │
│  │                                                                │  │
│  │   ┌─────────────┐    ┌──────────────┐    ┌───────────────┐   │  │
│  │   │  Protocol   │───▶│  Multiplexer │───▶│   Protocol    │   │  │
│  │   │  Parsers    │    │   & Switch   │    │  Translation  │   │  │
│  │   └─────────────┘    └──────────────┘    └───────────────┘   │  │
│  └────────────────────────────────────────────────────────────────┘  │
│                                                                      │
│    USB Serial Ports                                                  │
│    ┌──────────┐ ┌──────────┐ ┌──────────┐ ┌──────────┐              │
│    │/dev/ttyX │ │/dev/ttyY │ │/dev/ttyZ │ │/dev/ttyW │  ...         │
└────┴──────────┴─┴──────────┴─┴──────────┴─┴──────────┴───────────────┘
          │            │            │              │
          ▼            ▼            ▼              ▼
     ┌─────────┐  ┌─────────┐  ┌─────────┐  ┌─────────────────┐
     │ Radio A │  │ Radio B │  │ Radio C │  │  ESP32-S3       │
     │ (Yaesu) │  │ (Icom)  │  │(Kenwood)│  │  USB Bridge     │
     └─────────┘  └─────────┘  └─────────┘  │  (JTAG port)    │
                                            └────────┬────────┘
                                                     │ USB OTG
                                                     │ (CDC device)
                                                     ▼
                                               ┌───────────┐
                                               │ Amplifier │
                                               │ (USB host)│
                                               └───────────┘
```

### Why the ESP32 Bridge?

Many amplifiers have a USB host port expecting a USB serial device (like a USB-to-serial
adapter). However, desktop operating systems (Windows, macOS, Linux) can only act as USB
**hosts**, not USB **devices**. The ESP32-S3 solves this by:

1. Connecting to the host computer via its **USB-Serial-JTAG** port (appears as a COM port)
2. Acting as a USB **CDC device** on its **USB OTG** port (plugs into the amplifier)
3. Bridging data bidirectionally between the two USB interfaces

This allows the host computer to effectively "be" a USB serial device to the amplifier.

## Project Structure

```
catapult/
├── crates/
│   ├── cat-protocol/     # CAT protocol parsing/encoding library
│   ├── cat-detect/       # Auto-detection of radios
│   └── cat-mux/          # Multiplexer engine
├── cat-desktop/          # Desktop application (egui)
└── cat-bridge/           # ESP32-S3 firmware
```

## Building

### Desktop Application

```bash
# Build the desktop app
cargo build --release -p cat-desktop

# Run the desktop app
cargo run -p cat-desktop
```

### ESP32 Firmware

Requires the esp-rs toolchain:

```bash
# Install esp-rs toolchain
cargo install espup
espup install

# Build firmware
cd cat-bridge
cargo build --release

# Flash to ESP32-S3
cargo run --release
```

## Supported Protocols

### Yaesu CAT
- 5-byte binary command format
- BCD frequency encoding
- Tested with: FT-817, FT-857, FT-897, FT-991, FTDX10

### Icom CI-V
- Framed variable-length messages (0xFE preamble, 0xFD terminator)
- Address-based routing
- Tested with: IC-7300, IC-705, IC-7610

### Kenwood
- ASCII semicolon-terminated commands
- Human-readable command format
- Tested with: TS-990S, TS-590SG, TS-2000

### Elecraft
- Kenwood-compatible base with extensions
- Extended commands for K3, KX3, KX2

## Configuration

### Switching Modes

- **Manual**: User explicitly selects the active radio
- **PTT-Triggered**: Automatically switch to the radio that keys PTT
- **Frequency-Triggered**: Switch when a radio changes frequency
- **Automatic**: Combination of PTT and frequency triggers

### Lockout

A configurable lockout time (default 500ms) prevents rapid switching between radios.

## Hardware Requirements

### Desktop
- USB serial ports connected to radios
- Common USB-serial adapters: FTDI, CP210x, CH340

### ESP32 Bridge
- ESP32-S3 development board with **two USB ports** (e.g., ESP32-S3-DevKitC)
  - **USB-UART/JTAG port**: Connect to host computer
  - **USB OTG port**: Connect to amplifier's USB host port
- The amplifier must have a USB host port that accepts USB serial devices

## License

MIT License - see LICENSE file for details.

## Contributing

Contributions are welcome! Please read the contributing guidelines before submitting PRs.
