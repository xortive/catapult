# Catapult Architecture Documentation

This document provides comprehensive architecture documentation for the Catapult CAT multiplexer application.

## Table of Contents

1. [Project Overview](#project-overview)
2. [Component Architecture](#component-architecture)
3. [Background Tasks](#background-tasks)
4. [Channel Architecture](#channel-architecture)
5. [Event Flow Diagrams](#event-flow-diagrams)
6. [State Management](#state-management)
7. [Design Patterns](#design-patterns)
8. [Switching Modes](#switching-modes)
9. [MuxEvent Types](#muxevent-types)

---

## Project Overview

Catapult is a CAT (Computer Aided Transceiver) multiplexer that allows multiple amateur radios to share a single amplifier. The application is built as a Rust workspace with the following crates:

| Crate | Purpose |
|-------|---------|
| **cat-desktop** | egui-based GUI application with async serial I/O |
| **cat-mux** | Core multiplexer engine (actor-based architecture) |
| **cat-protocol** | CAT protocol encoders/decoders (Kenwood, Icom CI-V, Yaesu, Elecraft, FlexRadio) |
| **cat-sim** | Virtual radio simulation framework |
| **cat-detect** | Serial port detection and radio identification |

---

## Component Architecture

### Major Components

| Component | Location | Key Structs/Functions | Purpose |
|-----------|----------|----------------------|---------|
| CatapultApp | `cat-desktop/src/app.rs` | `CatapultApp`, `BackgroundMessage` | Main app state, UI orchestration, task spawning |
| Mux Actor | `crates/cat-mux/src/actor.rs` | `run_mux_actor()`, `MuxActorCommand`, `MuxActorState` | Central command processor, routes all radio/amplifier commands |
| Multiplexer Engine | `crates/cat-mux/src/engine.rs` | `Multiplexer`, `MultiplexerConfig`, `MultiplexerEvent` | State tracking, switching logic, lockout timer |
| AsyncRadioConnection | `cat-desktop/src/async_serial.rs` | `AsyncRadioConnection`, `RadioTaskCommand` | Serial I/O for COM radios, protocol codec integration |
| Amplifier Task | `cat-desktop/src/amp_task.rs` | `run_amp_task()`, `run_virtual_amp_task()` | Bidirectional amplifier serial I/O |
| TrafficMonitor | `cat-desktop/src/traffic_monitor.rs` | `TrafficMonitor`, `TrafficEntry`, `TrafficSource` | Event logging, protocol decoding, export |
| SimulationPanel | `cat-desktop/src/simulation_panel.rs` | `SimulationPanel` | Virtual radio UI controls |
| RadioChannel | `crates/cat-mux/src/channel.rs` | `RadioChannel`, `RadioChannelMeta`, `RadioType` | Radio channel abstraction for mux |
| AmplifierChannel | `crates/cat-mux/src/amplifier.rs` | `AmplifierChannel`, `AmplifierChannelMeta`, `VirtualAmplifier` | Amplifier channel abstraction |
| RadioState | `crates/cat-mux/src/state.rs` | `RadioState`, `RadioHandle`, `SwitchingMode`, `AmplifierConfig` | Radio state tracking |
| MuxEvent | `crates/cat-mux/src/events.rs` | `MuxEvent` | Unified event stream enum |

### Component Relationships

```
┌─────────────────────────────────────────────────────────────────────────┐
│                           cat-desktop (GUI)                              │
│                                                                          │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐ │
│  │ CatapultApp  │  │ RadioPanel   │  │ TrafficMonitor│  │SimulationPanel│ │
│  └──────┬───────┘  └──────────────┘  └──────────────┘  └──────────────┘ │
│         │                                                                │
│         ├──────────────┬──────────────┬──────────────┐                  │
│         │              │              │              │                  │
│         ▼              ▼              ▼              ▼                  │
│  ┌─────────────┐ ┌─────────────┐ ┌─────────────┐ ┌─────────────┐        │
│  │AsyncRadio   │ │AsyncRadio   │ │ AmpTask     │ │VirtualRadio │        │
│  │Connection 1 │ │Connection 2 │ │             │ │(via sim)    │        │
│  └──────┬──────┘ └──────┬──────┘ └──────┬──────┘ └──────┬──────┘        │
│         │              │              │              │                  │
└─────────┼──────────────┼──────────────┼──────────────┼──────────────────┘
          │              │              │              │
          └──────────────┴──────┬───────┴──────────────┘
                                │
                                ▼
┌─────────────────────────────────────────────────────────────────────────┐
│                           cat-mux (Core Engine)                          │
│                                                                          │
│  ┌──────────────────────────────────────────────────────────────────┐   │
│  │                         Mux Actor Task                            │   │
│  │  ┌─────────────────┐  ┌─────────────────┐  ┌─────────────────┐   │   │
│  │  │ MuxActorState   │  │   Multiplexer   │  │ RadioChannels   │   │   │
│  │  │                 │  │    (engine)     │  │    HashMap      │   │   │
│  │  └─────────────────┘  └─────────────────┘  └─────────────────┘   │   │
│  └──────────────────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────────────────┘
```

---

## Background Tasks

All async tasks are spawned via `rt_handle.spawn()` on the Tokio runtime.

### 1. Mux Actor Task (Always Running)

**Spawned at:** `app.rs:195` in `CatapultApp::new()`

```rust
rt_handle.spawn(async move {
    run_mux_actor(mux_cmd_rx, mux_event_tx).await;
});
```

**Purpose:** Central command processor that:
- Receives all `MuxActorCommand` messages
- Owns the `Multiplexer` engine (single-threaded state)
- Emits `MuxEvent` for all state changes and traffic
- Routes translated commands to the amplifier

**Lifetime:** Runs for the entire application lifetime.

### 2. Radio Connection Tasks (One per COM Radio)

**Spawned in:** `spawn_radio_task()` at `app.rs:398`

```rust
rt.spawn(async move {
    match AsyncRadioConnection::connect(...) {
        Ok(mut conn) => {
            conn.query_id().await;
            conn.query_initial_state().await;
            conn.enable_auto_info().await;
            conn.run_read_loop(cmd_rx).await;
        }
        ...
    }
});
```

**Purpose:**
- Opens serial port with 100ms timeout
- Queries radio ID for model identification
- Enables auto-info mode for automatic updates
- Runs read loop: `serial → codec.push_bytes() → parse → RadioCommand → MuxActorCommand::RadioCommand`

**Shutdown:** Via `RadioTaskCommand::Shutdown` through the `cmd_rx` channel.

### 3. Amplifier Task (Zero or One)

**Spawned in:** `connect_amplifier()`

```rust
rt_handle.spawn(run_amp_task(cmd_rx, data_rx, port, baud, bg_tx, mux_tx));
```

**Purpose:** Bidirectional amplifier serial I/O:
- Receives translated commands from mux actor via `data_rx`
- Writes to amplifier serial port
- Reads responses and sends to mux actor as `AmpRawData`

**Shutdown:** Via oneshot channel (`shutdown_rx`).

### 4. Handle Response Waiters (Short-lived)

**Spawned in:** `register_com_radio()` at `app.rs:571`

```rust
self.rt_handle.spawn(async move {
    if let Ok(handle) = resp_rx.await {
        let _ = bg_tx.send(BackgroundMessage::RadioRegistered { radio_id, handle });
    }
});
```

**Purpose:** Waits for `RadioHandle` from mux actor via oneshot channel, then forwards to the app via `BackgroundMessage`.

---

## Channel Architecture

### Channel Diagram

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                              CatapultApp (UI Thread)                         │
│                                                                              │
│  ┌─────────────┐     ┌─────────────┐     ┌─────────────┐                    │
│  │ mux_cmd_tx  │     │ mux_event_rx│     │   bg_rx     │                    │
│  │  (tokio)    │     │  (tokio)    │     │  (std)      │                    │
│  └──────┬──────┘     └──────▲──────┘     └──────▲──────┘                    │
│         │                   │                   │                            │
│         │ 256 buffer        │ 256 buffer        │ unbounded                  │
│         │                   │                   │                            │
└─────────┼───────────────────┼───────────────────┼────────────────────────────┘
          │                   │                   │
          ▼                   │                   │
┌─────────────────────────────┴───────────────────┴────────────────────────────┐
│                              Mux Actor Task                                   │
│                                                                              │
│  ┌─────────────┐     ┌─────────────┐                                        │
│  │ mux_cmd_rx  │     │ mux_event_tx│                                        │
│  │  (tokio)    │     │  (tokio)    │                                        │
│  └─────────────┘     └─────────────┘                                        │
│                              │                                               │
│                              │ amp_data_tx (32 buffer)                       │
│                              ▼                                               │
└──────────────────────────────┬───────────────────────────────────────────────┘
                               │
                               ▼
┌──────────────────────────────────────────────────────────────────────────────┐
│                              Amp Task                                         │
│  ┌─────────────┐     ┌─────────────┐     ┌─────────────┐                    │
│  │ amp_cmd_rx  │     │ amp_data_rx │     │   mux_tx    │                    │
│  │  (tokio)    │     │  (tokio)    │     │  (tokio)    │                    │
│  │  32 buffer  │     │  32 buffer  │     │ → AmpRawData│                    │
│  └─────────────┘     └─────────────┘     └─────────────┘                    │
└──────────────────────────────────────────────────────────────────────────────┘

┌──────────────────────────────────────────────────────────────────────────────┐
│                         Radio Task (per COM radio)                            │
│  ┌─────────────┐                         ┌─────────────┐                    │
│  │ radio_cmd_rx│                         │   mux_tx    │                    │
│  │  (tokio)    │                         │  (tokio)    │                    │
│  │  32 buffer  │                         │→ RadioCommand│                    │
│  └─────────────┘                         │→ RadioRawData│                    │
│                                          └─────────────┘                    │
│                          ┌─────────────┐                                    │
│                          │   bg_tx     │                                    │
│                          │  (std)      │                                    │
│                          │→Disconnected│                                    │
│                          └─────────────┘                                    │
└──────────────────────────────────────────────────────────────────────────────┘
```

### Channel Types

| Channel | Type | Buffer | Direction | Purpose |
|---------|------|--------|-----------|---------|
| `mux_cmd_tx/rx` | tokio::mpsc | 256 | App → Mux Actor | Commands to mux |
| `mux_event_tx/rx` | tokio::mpsc | 256 | Mux Actor → App | Events from mux |
| `radio_cmd_tx/rx` | tokio::mpsc | 32 | App → Radio Task | Shutdown commands |
| `amp_cmd_tx/rx` | tokio::mpsc | 32 | App → Amp Task | Shutdown commands |
| `amp_data_tx/rx` | tokio::mpsc | 32 | Mux Actor → Amp Task | Translated commands |
| `bg_tx/rx` | std::mpsc | unbounded | Tasks → UI Thread | Background messages |
| oneshot | tokio::oneshot | 1 | Mux Actor → Waiter | RadioHandle responses |

---

## Event Flow Diagrams

### Flow 1: Radio Command Processing

```
Serial Port
    │
    ▼
AsyncRadioConnection::run_read_loop()
    │
    ├──► stream.read() with 100ms timeout
    │
    ├──► codec.push_bytes(data)
    │
    ├──► MuxActorCommand::RadioRawData { handle, data }
    │    (for traffic monitoring)
    │
    ▼
codec.next_command()
    │
    ▼
MuxActorCommand::RadioCommand { handle, command }
    │
    ▼
Mux Actor receives command
    │
    ├──► state.multiplexer.process_radio_command(handle, cmd)
    │    │
    │    ├──► Update RadioState (frequency, mode, PTT)
    │    │
    │    ├──► check_auto_switch() based on SwitchingMode
    │    │
    │    └──► filter_for_amplifier() → translator.translate()
    │
    ├──► If state changed: MuxEvent::RadioStateChanged
    │
    ├──► If active radio changed: MuxEvent::ActiveRadioChanged
    │
    └──► If amp_data: amp_tx.send() + MuxEvent::AmpDataOut
         │
         ▼
    Amp Task receives data
         │
         ▼
    stream.write_all(data)
```

### Flow 2: Radio Registration

```
User clicks "Add Radio"
    │
    ▼
register_com_radio(config)
    │
    ├──► Allocate radio_id
    │
    ├──► Create RadioChannelMeta
    │
    ├──► create_radio_channel(meta, 32)
    │
    ├──► Create oneshot channel for response
    │
    ├──► mux_cmd_tx.try_send(MuxActorCommand::RegisterRadio { ... })
    │
    └──► Spawn waiter task for oneshot response
         │
         ▼
    Mux Actor
         │
         ├──► multiplexer.add_radio(name, port, protocol)
         │
         ├──► RadioHandle assigned (sequential, 1-based)
         │
         ├──► response.send(handle)
         │
         └──► MuxEvent::RadioConnected { handle, meta }
              │
              ▼
    Waiter task receives handle
         │
         ▼
    bg_tx.send(BackgroundMessage::RadioRegistered { radio_id, handle })
         │
         ▼
    App receives BackgroundMessage
         │
         ├──► Store handle mappings
         │
         ├──► Update RadioPanel.handle
         │
         └──► spawn_radio_task(radio_id, handle, ...)
```

### Flow 3: Amplifier Command Translation

```
Active radio sends command
    │
    ▼
Multiplexer::process_radio_command(handle, cmd)
    │
    ├──► Update RadioState
    │
    ├──► Check: is this the active radio?
    │    │
    │    NO ──► Return None (command ignored)
    │    │
    │    YES
    │    │
    ▼
filter_for_amplifier(&cmd)
    │
    ├──► Only passes: SetFrequency, FrequencyReport, SetMode, ModeReport
    │
    └──► Returns None for non-amplifier commands
         │
         ▼ (if Some)
translator.translate(&filtered_cmd)
    │
    ├──► Protocol conversion (e.g., Icom CI-V → Kenwood)
    │
    └──► Returns encoded bytes for target amplifier protocol
         │
         ▼
MuxEvent::AmpDataOut { data, protocol }
    │
    ▼
amp_tx.send(data)
    │
    ▼
Amp Task
    │
    └──► stream.write_all(&data)
```

### Flow 4: Virtual Radio Integration

```
SimulationPanel UI
    │
    ├──► User changes frequency/mode/PTT
    │
    ▼
SimulationContext::set_radio_frequency/mode/ptt()
    │
    ▼
VirtualRadio state updated
    │
    ├──► pending_output generated (protocol-encoded bytes)
    │
    └──► SimulationEvent::StateChanged { id, output }
         │
         ▼
App::process_simulation_events()
    │
    ├──► Lookup handle for sim radio
    │
    ├──► Parse output into RadioCommands
    │
    └──► For each command:
         │
         ▼
    mux_cmd_tx.send(MuxActorCommand::RadioCommand { handle, command })
         │
         ▼
    Mux Actor (same path as physical radios)
```

---

## State Management

### State Ownership Hierarchy

```
┌─────────────────────────────────────────────────────────────────────┐
│                    Mux Actor Task (Authoritative)                    │
│                                                                      │
│  MuxActorState {                                                     │
│      multiplexer: Multiplexer {                                      │
│          config: MultiplexerConfig {                                 │
│              switching_mode: SwitchingMode,                          │
│              lockout_ms: u64,                                        │
│              amplifier: AmplifierConfig,                             │
│              translation: TranslationConfig,                         │
│          },                                                          │
│          radios: HashMap<RadioHandle, RadioState>,                   │
│          active_radio: Option<RadioHandle>,                          │
│          lockout_until: Option<Instant>,                             │
│          translator: ProtocolTranslator,                             │
│      },                                                              │
│      radio_channels: HashMap<RadioHandle, RadioChannelMeta>,         │
│      amp_tx: Option<Sender<Vec<u8>>>,                                │
│      amp_meta: Option<AmplifierChannelMeta>,                         │
│  }                                                                   │
└─────────────────────────────────────────────────────────────────────┘
                              │
                              │ MuxEvent
                              ▼
┌─────────────────────────────────────────────────────────────────────┐
│                    CatapultApp (UI Thread - Read-Only Copies)        │
│                                                                      │
│  radio_panels: Vec<RadioPanel> {                                     │
│      // Updated from MuxEvent::RadioStateChanged                     │
│      frequency_hz: Option<u64>,                                      │
│      mode: Option<OperatingMode>,                                    │
│      ptt: bool,                                                      │
│  },                                                                  │
│  active_radio: Option<RadioHandle>,  // from ActiveRadioChanged      │
│  switching_mode: SwitchingMode,      // from SwitchingModeChanged    │
│  radio_id_to_handle: HashMap<u32, RadioHandle>,                      │
│  handle_to_radio_id: HashMap<RadioHandle, u32>,                      │
└─────────────────────────────────────────────────────────────────────┘
```

### RadioState Structure

```rust
pub struct RadioState {
    pub handle: RadioHandle,        // Unique identifier (1-based)
    pub name: String,               // Display name (may be model name)
    pub port: String,               // Serial port or "[SIM]"
    pub protocol: Protocol,         // CAT protocol in use
    pub model: Option<RadioModel>,  // Identified radio model
    pub frequency_hz: Option<u64>,  // Current frequency
    pub mode: Option<OperatingMode>,// Current operating mode
    pub ptt: bool,                  // PTT active state
    pub civ_address: Option<u8>,    // CI-V address (Icom)
    pub last_activity: Instant,     // Last command timestamp
    pub last_freq_change: Option<Instant>,
    pub is_simulated: bool,
}
```

### State Synchronization Flow

1. **State Change Origin**: Physical radio sends command OR virtual radio UI action
2. **Mux Actor Updates**: `Multiplexer::process_radio_command()` updates `RadioState`
3. **Event Emission**: `MuxEvent::RadioStateChanged` sent to app
4. **UI Update**: App updates `RadioPanel` from event, egui renders

---

## Design Patterns

### 1. Actor Pattern (Mux Actor)

The mux actor follows the classic actor model:

```
                    ┌─────────────────────┐
     Commands ────► │    Mux Actor        │ ────► Events
                    │                     │
                    │  ┌───────────────┐  │
                    │  │ Owned State   │  │
                    │  │ (no locks)    │  │
                    │  └───────────────┘  │
                    └─────────────────────┘
```

**Benefits:**
- Single task owns all multiplexer state
- No `Arc<Mutex<>>` needed - all operations serialized
- Command queue naturally handles concurrency
- Clean separation between command and event streams

**Implementation:**
```rust
pub async fn run_mux_actor(
    mut cmd_rx: mpsc::Receiver<MuxActorCommand>,
    event_tx: mpsc::Sender<MuxEvent>,
) {
    let mut state = MuxActorState::new();

    while let Some(cmd) = cmd_rx.recv().await {
        match cmd {
            MuxActorCommand::RegisterRadio { .. } => { ... }
            MuxActorCommand::RadioCommand { .. } => { ... }
            // ... all state mutations happen here
        }
    }
}
```

### 2. Handle Indirection

Two-level ID system decouples UI from mux:

```
    UI Layer                    Mux Layer
    ────────                    ─────────
    radio_id: u32    ◄────►    RadioHandle(u32)
    (internal)                  (0-based sequential)
```

**Mappings maintained by app:**
- `radio_id_to_handle: HashMap<u32, RadioHandle>`
- `handle_to_radio_id: HashMap<RadioHandle, u32>`

**Benefits:**
- UI can track radios before handle assigned
- Handle assignment is mux actor's responsibility
- Clean separation of concerns

### 3. Async/Sync Boundary

```
┌─────────────────────────────────────────────────┐
│            Main Thread (Sync - egui)             │
│                                                  │
│  CatapultApp::update() called by eframe         │
│                                                  │
│  Uses std::mpsc for receiving:                   │
│  - bg_rx: BackgroundMessage                      │
│  - diag_rx: DiagnosticEvent                      │
│                                                  │
│  Uses tokio::mpsc for sending:                   │
│  - mux_cmd_tx: MuxActorCommand                   │
│                                                  │
│  Uses try_recv()/try_send() (non-blocking)       │
└─────────────────────────────────────────────────┘
                        │
        ────────────────┼────────────────
                        │
┌─────────────────────────────────────────────────┐
│         Tokio Runtime (Async Tasks)              │
│                                                  │
│  - Mux Actor (always running)                    │
│  - Radio Tasks (per COM radio)                   │
│  - Amp Task (optional)                           │
│  - Handle Waiters (short-lived)                  │
│                                                  │
│  All use tokio::mpsc for inter-task comms        │
└─────────────────────────────────────────────────┘
```

### 4. Protocol Codec Pattern

All protocol codecs follow a consistent streaming interface:

```rust
trait ProtocolCodec {
    /// Buffer incoming bytes (may be partial frames)
    fn push_bytes(&mut self, data: &[u8]);

    /// Parse and return next complete command (if any)
    fn next_command(&mut self) -> Option<ProtocolCommand>;

    /// Encode a command to bytes
    fn encode(cmd: &ProtocolCommand) -> Vec<u8>;
}
```

**Usage:**
```rust
// In radio read loop:
self.codec.push_bytes(data);
while let Some(cmd) = self.codec.next_command() {
    // Send to mux actor
}
```

**Benefits:**
- Handles fragmented serial data
- Buffers until complete frame received
- Same pattern for all protocols (Kenwood, Icom, Yaesu, etc.)

### 5. Unified Event Stream

All multiplexer activity flows through a single `MuxEvent` enum:

```rust
pub enum MuxEvent {
    // Radio lifecycle
    RadioConnected { handle, meta },
    RadioDisconnected { handle },
    RadioStateChanged { handle, freq, mode, ptt },
    ActiveRadioChanged { from, to },

    // Traffic (for monitoring)
    RadioDataIn { handle, data, protocol },
    RadioDataOut { handle, data, protocol },
    AmpDataOut { data, protocol },
    AmpDataIn { data, protocol },

    // Amplifier lifecycle
    AmpConnected { meta },
    AmpDisconnected,

    // Control
    SwitchingModeChanged { mode },
    SwitchingBlocked { requested, current, remaining_ms },
    Error { source, message },
}
```

**Benefits:**
- Single channel for all observations
- Guaranteed event ordering
- Traffic monitor receives complete picture
- Easy to add new observers

---

## Switching Modes

The multiplexer supports three switching modes defined in `SwitchingMode`:

| Mode | Trigger | Use Case |
|------|---------|----------|
| **Manual** | Only user click | Full control, contest operation |
| **FrequencyTriggered** (default) | Frequency change | Normal operation, follow VFO |
| **Automatic** | Frequency OR PTT | Legacy mode, catches PTT-first scenarios |

### Switching Logic

```rust
fn check_auto_switch(&mut self, handle: RadioHandle, cmd: &RadioCommand) {
    // Don't switch to non-existent radio
    if !self.radios.contains_key(&handle) { return; }

    // Already active? Nothing to do
    if self.active_radio == Some(handle) { return; }

    // Check lockout (500ms default)
    if let Some(until) = self.lockout_until {
        if Instant::now() < until { return; }
    }

    let should_switch = match self.config.switching_mode {
        SwitchingMode::Manual => false,
        SwitchingMode::FrequencyTriggered => matches!(
            cmd,
            RadioCommand::SetFrequency { .. } | RadioCommand::FrequencyReport { .. }
        ),
        SwitchingMode::Automatic => matches!(
            cmd,
            RadioCommand::SetPtt { active: true }
                | RadioCommand::PttReport { active: true }
                | RadioCommand::SetFrequency { .. }
                | RadioCommand::FrequencyReport { .. }
        ),
    };

    if should_switch {
        self.switch_to(handle);  // Sets lockout_until
    }
}
```

### Lockout Timer

After switching, a 500ms lockout prevents rapid switching:

```rust
fn switch_to(&mut self, handle: RadioHandle) {
    self.active_radio = Some(handle);
    self.lockout_until = Some(Instant::now() + Duration::from_millis(self.config.lockout_ms));
    // Emit ActiveRadioChanged event
}
```

---

## MuxEvent Types

### Radio Lifecycle Events

| Event | When Emitted | Data |
|-------|--------------|------|
| `RadioConnected` | Radio registered with mux | `handle`, `meta` (RadioChannelMeta) |
| `RadioDisconnected` | Radio unregistered | `handle` |
| `RadioStateChanged` | Frequency, mode, or PTT changed | `handle`, `freq?`, `mode?`, `ptt?` |
| `ActiveRadioChanged` | Active radio switched | `from: Option<Handle>`, `to: Handle` |

### Traffic Events

| Event | Direction | Data |
|-------|-----------|------|
| `RadioDataIn` | Radio → Mux | `handle`, `data`, `protocol` |
| `RadioDataOut` | Mux → Radio | `handle`, `data`, `protocol` |
| `AmpDataOut` | Mux → Amp | `data`, `protocol` |
| `AmpDataIn` | Amp → Mux | `data`, `protocol` |

### Amplifier Lifecycle Events

| Event | When Emitted | Data |
|-------|--------------|------|
| `AmpConnected` | Amplifier channel registered | `meta` (AmplifierChannelMeta) |
| `AmpDisconnected` | Amplifier channel removed | - |

### Control Events

| Event | When Emitted | Data |
|-------|--------------|------|
| `SwitchingModeChanged` | User changes mode | `mode` (SwitchingMode) |
| `SwitchingBlocked` | Switch attempted during lockout | `requested`, `current`, `remaining_ms` |
| `Error` | Various errors | `source`, `message` |

### Event Classification Methods

```rust
impl MuxEvent {
    pub fn is_traffic(&self) -> bool;      // RadioDataIn/Out, AmpDataIn/Out
    pub fn is_radio_lifecycle(&self) -> bool;  // Connected, Disconnected, ActiveChanged
    pub fn is_amp_lifecycle(&self) -> bool;    // AmpConnected, AmpDisconnected
    pub fn is_error(&self) -> bool;        // Error
    pub fn radio_handle(&self) -> Option<RadioHandle>;  // Extract handle if present
}
```

---

## TrafficMonitor Architecture

The TrafficMonitor (`cat-desktop/src/traffic_monitor.rs`) provides a real-time traffic logging UI with protocol-aware decoding.

### Data Flow

```
┌─────────────────────────────────────────────────────────────────────────┐
│                           Mux Actor                                      │
│                                                                          │
│  Emits traffic events:                                                   │
│  - RadioDataIn { handle, data, protocol }                                │
│  - RadioDataOut { handle, data, protocol }                               │
│  - AmpDataIn { data, protocol }                                          │
│  - AmpDataOut { data, protocol }                                         │
│  - Error { source, message }                                             │
└─────────────────────────────────┬───────────────────────────────────────┘
                                  │ MuxEvent
                                  ▼
┌─────────────────────────────────────────────────────────────────────────┐
│                         CatapultApp                                      │
│                                                                          │
│  process_mux_events() calls:                                             │
│  traffic_monitor.process_event_with_amp_port(event, radio_metas, ...)    │
└─────────────────────────────────┬───────────────────────────────────────┘
                                  │
                                  ▼
┌─────────────────────────────────────────────────────────────────────────┐
│                        TrafficMonitor                                    │
│                                                                          │
│  ┌─────────────────────────────────────────────────────────────────┐    │
│  │  VecDeque<TrafficEntry>  (max 5000 entries)                     │    │
│  │                                                                  │    │
│  │  TrafficEntry::Data {                                            │    │
│  │      timestamp, direction, source, data,                         │    │
│  │      decoded: Option<AnnotatedFrame>  ← Protocol decoding        │    │
│  │  }                                                               │    │
│  │                                                                  │    │
│  │  TrafficEntry::Diagnostic { timestamp, source, severity, msg }   │    │
│  └─────────────────────────────────────────────────────────────────┘    │
│                                                                          │
│  Filters: direction, simulated, diagnostic severity                      │
│  Features: auto-scroll, pause, export to clipboard/file                  │
└─────────────────────────────────────────────────────────────────────────┘
```

### Key Types

**TrafficSource** - Identifies where traffic originated:
```rust
pub enum TrafficSource {
    RealRadio { handle, port },      // Incoming from COM radio
    ToRealRadio { handle, port },    // Outgoing to COM radio
    SimulatedRadio { id },           // Incoming from virtual radio
    ToSimulatedRadio { id },         // Outgoing to virtual radio
    RealAmplifier { port },          // Outgoing to amplifier
    FromRealAmplifier { port },      // Incoming from amplifier
    SimulatedAmplifier,              // Outgoing to virtual amp
    FromSimulatedAmplifier,          // Incoming from virtual amp
}
```

**TrafficEntry** - Two variants:
```rust
pub enum TrafficEntry {
    Data {
        timestamp: SystemTime,
        direction: TrafficDirection,  // Incoming or Outgoing
        source: TrafficSource,
        data: Vec<u8>,                // Raw bytes
        decoded: Option<AnnotatedFrame>,  // Protocol-decoded with segments
    },
    Diagnostic {
        timestamp: SystemTime,
        source: String,
        severity: DiagnosticSeverity,  // Debug, Info, Warning, Error
        message: String,
    },
}
```

### Protocol Decoding

Traffic data is decoded using `decode_and_annotate_with_hint()` from `cat-protocol::display`:

```
Raw bytes: [0xFE, 0xFE, 0x94, 0xE0, 0x00, 0x00, 0x60, 0x25, 0x41, 0x01, 0xFD]

AnnotatedFrame {
    protocol: "CI-V",
    summary: [
        { text: "Freq: ", type: Command },
        { text: "14.256.000", type: Frequency, range: 5..9 },
        { text: " Hz", type: Data },
    ],
    segments: [
        { range: 0..2, type: Preamble, label: "Preamble" },
        { range: 2..3, type: Address, label: "To", value: "94" },
        { range: 3..4, type: Address, label: "From", value: "E0" },
        { range: 4..5, type: Command, label: "Cmd", value: "Set Freq" },
        { range: 5..9, type: Frequency, label: "Freq", value: "14.256.000" },
        { range: 10..11, type: Terminator, label: "End" },
    ],
}
```

### UI Features

1. **Color-coded display**: Each segment type has a distinct color (preamble=gray, address=blue, frequency=yellow, etc.)

2. **Hover highlighting**: Hovering over decoded summary highlights corresponding hex/ASCII bytes, and vice versa

3. **Filtering**:
   - Direction: All / In / Out
   - Simulated traffic toggle
   - Diagnostic levels: Debug, Info, Warning, Error

4. **Export**: Copy to clipboard or save to file (text format with timestamps)

5. **Virtual scrolling**: Only visible rows are rendered (handles 5000+ entries efficiently)

### Integration Point

The app calls `process_event_with_amp_port()` for each traffic-related `MuxEvent`:

```rust
// In CatapultApp::process_mux_events()
self.traffic_monitor.process_event_with_amp_port(
    event,
    &|handle| self.get_radio_meta(handle),  // Lookup radio metadata
    &self.amp_port,
    self.amp_connection_type == AmplifierConnectionType::Simulated,
);
```

---

## Key Insights

1. **Unified Event Stream**: Single `MuxEvent` enum for all activity enables easy observation and consistent event ordering.

2. **No Locks in Actor**: The mux actor owns all state in a single async task - no `Arc<Mutex<>>` needed anywhere in the core engine.

3. **Traffic Capture**: Both directions for radio and amplifier data are captured with protocol information for the traffic monitor.

4. **Virtual Radio Parity**: Simulated radios use the exact same `RadioCommand` pathway as physical radios - no special cases in the mux.

5. **100ms Timeout Pattern**: All serial reads use timeout to prevent blocking and allow clean shutdown.

6. **Protocol Translation**: The multiplexer can translate between protocols (e.g., Icom radio to Kenwood amplifier) via `ProtocolTranslator`.

7. **Graceful Degradation**: Ports can become unavailable without crashing - panels show "unavailable" state.
