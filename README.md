# Catapult - CAT Protocol Multiplexer

A desktop-primary CAT (Computer Aided Transceiver) Protocol Multiplexer for amateur radio.

Connect unlimited transceivers to a single amplifier via CAT protocol translation.

## Authorship

This project was entirely authored by Claude (Anthropic's AI assistant) through Claude Code. It has
not been tested with real hardware yet.

## Features

- **Multi-Protocol Support**: Yaesu CAT (binary), Yaesu ASCII, Icom CI-V, Kenwood, Elecraft, and FlexRadio protocols
- **Auto-Detection**: Automatically detects CAT-capable radios on serial ports
- **Protocol Translation**: Translates between any supported protocol
- **Intelligent Switching**: Manual, PTT-triggered, or frequency-change triggered
- **Desktop Application**: Cross-platform GUI built with egui

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
     ┌─────────┐  ┌─────────┐  ┌─────────┐  ┌───────────┐
     │ Radio A │  │ Radio B │  │ Radio C │  │ Amplifier │
     │ (Yaesu) │  │ (Icom)  │  │(Kenwood)│  │           │
     └─────────┘  └─────────┘  └─────────┘  └───────────┘
```

## Project Structure

```
catapult/
├── crates/
│   ├── cat-protocol/     # CAT protocol parsing/encoding library
│   ├── cat-detect/       # Auto-detection of radios
│   ├── cat-mux/          # Multiplexer engine
│   └── cat-sim/          # Simulation framework
└── cat-desktop/          # Desktop application (egui)
```

## Building

```bash
# Build the desktop app
cargo build --release -p cat-desktop

# Run the desktop app
cargo run -p cat-desktop
```

## Supported Protocols

### Yaesu CAT (Binary)
- 5-byte binary command format
- BCD frequency encoding
- For older radios: FT-817, FT-857, FT-897, FT-450, etc.

### Yaesu ASCII
- ASCII semicolon-terminated commands (similar to Kenwood)
- 9-digit frequency format with hex mode codes (A-E for extended modes)
- For newer radios: FT-991, FT-991A, FTDX-101D, FTDX-10, FT-710

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

### FlexRadio SmartSDR
- Kenwood-compatible base with ZZ extensions
- ZZFA/ZZMD commands for frequency/mode
- For FLEX-6000 series radios

## Configuration

### Switching Modes

- **Manual**: User explicitly selects the active radio
- **PTT-Triggered**: Automatically switch to the radio that keys PTT
- **Frequency-Triggered**: Switch when a radio changes frequency
- **Automatic**: Combination of PTT and frequency triggers

### Auto-Information Mode

When a radio is connected, Catapult automatically enables **Auto-Information** (also called **Transceive** mode) on the radio. This tells the radio to send unsolicited status updates whenever its state changes - frequency, mode, PTT, etc.

This is essential for the multiplexer to track radio state in real-time without polling:
- **Kenwood/Elecraft/FlexRadio**: `AI1;` command enables auto-info
- **Icom CI-V**: Transceive mode (command 0x1A) enables unsolicited updates
- **Yaesu ASCII**: `AI1;` command (same as Kenwood)
- **Yaesu Binary**: No auto-info support; state is polled on PTT changes

With auto-info enabled, the multiplexer receives immediate notification when you tune the VFO, change modes, or key the transmitter - enabling responsive automatic switching.

### Lockout

A configurable lockout time (default 500ms) prevents rapid switching between radios.

## Hardware Requirements

- USB serial ports or adapters connected to radios
- Common USB-serial adapters: FTDI, CP210x, CH340
- For RS232 amplifiers: USB-to-RS232 adapter with null modem cable

## License

MIT License - see LICENSE file for details.

## Contributing

Contributions are welcome! Please read the contributing guidelines before submitting PRs.
