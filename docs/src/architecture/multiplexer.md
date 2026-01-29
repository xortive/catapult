# Multiplexer Engine

The multiplexer engine (`cat-mux`) is the core of Catapult, managing radio connections and switching logic.

## Overview

The multiplexer:
1. Tracks multiple connected radios
2. Maintains state for each radio (frequency, mode, PTT)
3. Decides which radio is "active"
4. Generates commands for the amplifier

## Key Types

### RadioHandle

A unique identifier for each connected radio:

```rust
#[derive(Copy, Clone, Eq, PartialEq, Hash)]
pub struct RadioHandle(u32);
```

Handles are opaque - use them to reference radios in API calls.

### RadioState

Tracked state for each radio:

```rust
pub struct RadioState {
    pub frequency_hz: Option<u64>,
    pub mode: Option<OperatingMode>,
    pub ptt: bool,
    pub control_band: Option<u8>,  // 0=Main/A, 1=Sub/B
    pub tx_band: Option<u8>,       // 0=Main/A, 1=Sub/B
}
```

The `control_band` and `tx_band` fields track VFO state for split operation. These are updated from:
- Direct CB/TB reports from radios that support them (e.g., TS-990S)
- Inferred from VFO selection and split mode commands for other radios

### SwitchingMode

How the active radio is selected:

```rust
pub enum SwitchingMode {
    Automatic,        // Switch on any activity
    FrequencyTriggered, // Switch only on frequency changes
    Manual,           // Only manual selection
}
```

## Core Operations

### Adding Radios

```rust
let handle = multiplexer.add_radio(name, port, protocol);
```

The first radio added becomes active.

### Processing Commands

When a radio sends a CAT command:

```rust
let bytes = multiplexer.process_radio_command(handle, command);
```

This:
1. Updates the radio's tracked state
2. Checks if switching is needed
3. Returns translated bytes for the amplifier (if active)

### Switching

```rust
// Manual switch
multiplexer.select_radio(handle);

// Query active
let active = multiplexer.active_radio();
```

## Switching Logic

### Automatic Mode

```
IF ptt_changed AND new_ptt == true:
    switch_to(radio)
ELSE IF frequency_changed:
    switch_to(radio)
ELSE IF mode_changed:
    switch_to(radio)
```

### Frequency Triggered Mode

```
IF frequency_changed:
    switch_to(radio)
```

### Manual Mode

```
// Only explicit select_radio() calls cause switches
```

## Events

The multiplexer emits events for state changes:

```rust
pub enum MultiplexerEvent {
    RadioAdded(RadioHandle),
    RadioRemoved(RadioHandle),
    ActiveRadioChanged { from: Option<RadioHandle>, to: Option<RadioHandle> },
    RadioStateUpdated { handle: RadioHandle, state: RadioState },
    AmplifierCommand(Vec<u8>),
    SwitchBlocked { reason: String },
    Error(String),
}
```

Poll events with:

```rust
while let Some(event) = multiplexer.poll_event() {
    // Handle event
}
```

## Lockout

To prevent relay damage from rapid switching:

```rust
multiplexer.set_lockout_duration(Duration::from_millis(500));
```

During lockout, automatic switches are blocked.

## Amplifier Communication

### Query Handling

When amplifiers query for state (frequency, mode, VFO), the multiplexer responds from cached state rather than forwarding to the radio. This ensures fast, consistent responses.

The multiplexer always identifies as a TS-990S (ID022) to amplifiers, providing compatibility with amplifiers expecting a high-end Kenwood transceiver.

### AI2 Heartbeat

The multiplexer sends `AI2;` (enable auto-information) to all connected Kenwood and Elecraft radios every second. This:
- Ensures radios continue pushing state updates
- Recovers auto-info mode if a radio restarts
- Maintains consistent behavior across reconnections

### VFO/Split Inference

For radios that don't report Control Band (CB) and Transmit Band (TB) directly, the multiplexer infers these from VFO and split commands:

```
VFO A selected → CB=0, TB=0
VFO B selected → CB=1, TB=1
Split enabled  → TB = opposite of CB
```

This allows amplifiers to track split operation even with radios that don't natively support CB/TB commands.
