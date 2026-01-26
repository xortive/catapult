# System Overview

Catapult is built as a modular Rust application with clear separation of concerns.

## Crate Structure

```
catapult/
├── cat-protocol/     # Protocol parsing and encoding
├── cat-mux/          # Multiplexer engine
├── cat-detect/       # Hardware detection
├── cat-sim/          # Simulation framework
└── cat-desktop/      # GUI application
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
- Organized into focused modules for maintainability

## Internal Module Structure

The `cat-desktop` crate uses a modular organization:

### `app/` Directory
Core application logic split into focused files:
- `mod.rs` - Main app struct and update loop
- `radio.rs` - Radio connection and state management
- `amplifier.rs` - Amplifier connection and command handling
- `events.rs` - Event processing from the multiplexer
- `ui_panels.rs` - UI panel rendering
- `status.rs` - Status bar and notifications
- `ports.rs` - Serial port enumeration and virtual port management

### `traffic_monitor/` Directory
Traffic monitoring functionality:
- `mod.rs` - Traffic monitor state and public API
- `ingest.rs` - Incoming data processing and parsing
- `ui.rs` - Traffic display rendering
- `export.rs` - Traffic log export functionality
- `models.rs` - Traffic entry data structures
- `cache.rs` - Protocol-specific caching

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
