# System Overview

Catapult is built as a modular Rust application with clear separation of concerns.

## Crate Structure

```
catapult/
├── cat-protocol/     # Protocol parsing and encoding
├── cat-mux/          # Multiplexer engine
├── cat-detect/       # Hardware detection
├── cat-sim/          # Simulation framework
├── cat-desktop/      # GUI application
└── cat-bridge/       # ESP32 firmware (optional)
```

## Data Flow

```
┌─────────────────────────────────────────────────────────────┐
│                      cat-desktop                             │
│  ┌─────────────┐    ┌─────────────┐    ┌─────────────┐     │
│  │   Serial    │    │ Multiplexer │    │   Serial    │     │
│  │   Input     │───▶│   Engine    │───▶│   Output    │     │
│  │  (radios)   │    │  (cat-mux)  │    │ (amplifier) │     │
│  └─────────────┘    └─────────────┘    └─────────────┘     │
│         │                  │                   │            │
│         ▼                  ▼                   ▼            │
│  ┌─────────────┐    ┌─────────────┐    ┌─────────────┐     │
│  │  Protocol   │    │   State     │    │  Protocol   │     │
│  │   Parser    │    │  Tracking   │    │  Encoder    │     │
│  │(cat-protocol│    │             │    │(cat-protocol│     │
│  └─────────────┘    └─────────────┘    └─────────────┘     │
└─────────────────────────────────────────────────────────────┘
```

## Core Components

### cat-protocol

Handles all protocol-specific parsing and encoding:
- Kenwood ASCII parser/encoder
- Icom CI-V binary parser/encoder
- Yaesu binary parser/encoder
- Elecraft extensions
- Common `RadioCommand` abstraction

### cat-mux

The multiplexer engine:
- Manages connected radios
- Tracks radio state (frequency, mode, PTT)
- Implements switching logic
- Generates amplifier commands
- Emits events for UI updates

### cat-detect

Hardware detection:
- Serial port enumeration
- USB device identification
- Protocol suggestion based on device IDs

### cat-sim

Simulation support:
- Virtual radio implementation
- Protocol-accurate output generation
- Event-based architecture

### cat-desktop

The GUI application:
- Built with egui for cross-platform support
- Manages serial connections
- Displays radio status
- Provides simulation controls
- Traffic monitoring

## Event System

Catapult uses an event-driven architecture:

```rust
enum MultiplexerEvent {
    RadioAdded(RadioHandle),
    RadioRemoved(RadioHandle),
    ActiveRadioChanged { from, to },
    AmplifierCommand(Vec<u8>),
    RadioStateUpdated { handle, state },
    // ...
}
```

The UI polls for events and updates accordingly.

## Threading Model

- **Main thread:** UI rendering (egui)
- **Serial I/O:** Handled synchronously in the main loop
- **No background threads:** Simple, predictable behavior

Future versions may add async I/O for better responsiveness.
