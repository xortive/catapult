# Multiplexer Integration Tests Plan

## Context

The Catapult project is a CAT (Computer Aided Transceiver) multiplexer that allows multiple amateur radios to share a single amplifier. The architecture consists of:

- **cat-protocol**: Protocol parsing/encoding for Yaesu, Icom CI-V, Kenwood, and Elecraft
- **cat-mux**: Core multiplexer engine that routes commands and translates protocols
- **cat-detect**: Auto-detection of connected radios
- **cat-desktop**: egui-based desktop application
- **cat-bridge**: ESP32-S3 firmware for USB bridging to amplifier

## Key Files

### Multiplexer Core
- `/crates/cat-mux/src/engine.rs` - `Multiplexer` struct with state management
- `/crates/cat-mux/src/state.rs` - `RadioHandle`, `RadioState`, `SwitchingMode`
- `/crates/cat-mux/src/translation.rs` - `ProtocolTranslator` for cross-protocol conversion

### Protocol Types
- `/crates/cat-protocol/src/command.rs` - `RadioCommand` enum (normalized representation)
- `/crates/cat-protocol/src/lib.rs` - `Protocol` enum, codec traits

## Multiplexer API Summary

### State
```rust
pub struct Multiplexer {
    config: MultiplexerConfig,
    radios: HashMap<RadioHandle, RadioState>,
    active_radio: Option<RadioHandle>,
    lockout_until: Option<Instant>,
    translator: ProtocolTranslator,
    event_buffer: Vec<MultiplexerEvent>,
}
```

### Key Methods
- `add_radio(name, port, protocol) -> RadioHandle`
- `remove_radio(handle)`
- `select_radio(handle)` - manual switch
- `set_switching_mode(mode)` - Manual, PttTriggered, FrequencyTriggered, Automatic
- `process_radio_command(handle, cmd) -> Option<Vec<u8>>` - main entry point
- `drain_events() -> Vec<MultiplexerEvent>` - get pending events

### Events Emitted
```rust
pub enum MultiplexerEvent {
    RadioAdded(RadioHandle),
    RadioRemoved(RadioHandle),
    ActiveRadioChanged { from: Option<RadioHandle>, to: RadioHandle },
    RadioStateUpdated(RadioHandle),
    AmplifierCommand(Vec<u8>),  // Translated bytes for amplifier
    SwitchingBlocked { requested, current, remaining_ms },
    Error(String),
}
```

### RadioCommand Variants (from cat-protocol)
```rust
pub enum RadioCommand {
    SetFrequency { hz: u64 },
    GetFrequency,
    FrequencyReport { hz: u64 },
    SetMode { mode: OperatingMode },
    GetMode,
    ModeReport { mode: OperatingMode },
    SetPtt { active: bool },
    GetPtt,
    PttReport { active: bool },
    SetVfo { vfo: Vfo },
    GetVfo,
    VfoReport { vfo: Vfo },
    GetId,
    IdReport { id: String },
    GetStatus,
    StatusReport { frequency_hz, mode, ptt, vfo },
    SetPower { on: bool },
    Unknown { data: Vec<u8> },
}
```

### Protocols
- `Protocol::Kenwood` - ASCII, semicolon-terminated (e.g., `FA00014250000;`)
- `Protocol::Elecraft` - Kenwood-compatible with extensions
- `Protocol::IcomCIV` - Binary framed (0xFE 0xFE ... 0xFD)
- `Protocol::Yaesu` - 5-byte binary with BCD frequencies

## Test Plan

### File Location
`/crates/cat-mux/tests/integration_tests.rs`

### Test Categories

#### 1. Switching Mode Tests
- Manual switching between radios
- PTT-triggered auto-switch
- Frequency-triggered auto-switch
- Automatic mode (PTT + frequency)
- Verify non-active radio commands are ignored

#### 2. Lockout Tests
- Lockout prevents rapid switching
- `SwitchingBlocked` event emitted during lockout
- Lockout expiry allows switching

#### 3. State Tracking Tests
- Frequency updates tracked per radio
- Mode updates tracked per radio
- PTT state tracked per radio
- StatusReport extracts all fields

#### 4. Protocol Translation Tests
All 12 combinations:
| Source | Target | Test |
|--------|--------|------|
| Kenwood | IcomCIV | Frequency → CI-V frame |
| Kenwood | Yaesu | Frequency → 5-byte BCD |
| Kenwood | Elecraft | Pass-through |
| IcomCIV | Kenwood | Frequency → ASCII |
| IcomCIV | Yaesu | Frequency → BCD |
| IcomCIV | Elecraft | Frequency → ASCII |
| Yaesu | Kenwood | Frequency → ASCII |
| Yaesu | IcomCIV | Frequency → CI-V |
| Yaesu | Elecraft | Frequency → ASCII |
| Elecraft | Kenwood | Pass-through |
| Elecraft | IcomCIV | Frequency → CI-V |
| Elecraft | Yaesu | Frequency → BCD |

#### 5. Event Emission Tests
- `RadioAdded` on add_radio
- `RadioRemoved` on remove_radio
- `ActiveRadioChanged` on switch
- `RadioStateUpdated` on command processing
- `AmplifierCommand` contains valid translated bytes

#### 6. Edge Cases
- Remove active radio → next radio becomes active
- Remove only radio → no active radio
- Command from unknown handle → ignored
- Query commands (GetFrequency) not forwarded to amp

#### 7. Property-Based Tests (proptest)
- Arbitrary frequency values translate correctly
- Roundtrip: RadioCommand → translate → parse → same RadioCommand
- Multiple radios with random commands maintain consistent state

## Implementation Skeleton

```rust
// tests/integration_tests.rs

use cat_mux::{Multiplexer, MultiplexerConfig, MultiplexerEvent, SwitchingMode};
use cat_protocol::{Protocol, RadioCommand, OperatingMode};

mod helpers {
    use super::*;

    pub fn collect_events<F>(events: Vec<MultiplexerEvent>, filter: F) -> Vec<MultiplexerEvent>
    where
        F: Fn(&MultiplexerEvent) -> bool,
    {
        events.into_iter().filter(filter).collect()
    }

    pub fn get_amp_commands(events: Vec<MultiplexerEvent>) -> Vec<Vec<u8>> {
        events.into_iter().filter_map(|e| match e {
            MultiplexerEvent::AmplifierCommand(data) => Some(data),
            _ => None,
        }).collect()
    }

    pub fn mux_with_lockout_disabled() -> Multiplexer {
        let mut config = MultiplexerConfig::default();
        config.lockout_ms = 0;
        Multiplexer::with_config(config)
    }
}

mod switching_tests {
    use super::*;

    #[test]
    fn test_manual_switch() { /* ... */ }

    #[test]
    fn test_ptt_triggered_switch() { /* ... */ }

    #[test]
    fn test_frequency_triggered_switch() { /* ... */ }

    #[test]
    fn test_automatic_mode_switch() { /* ... */ }

    #[test]
    fn test_inactive_radio_commands_ignored() { /* ... */ }
}

mod lockout_tests {
    use super::*;

    #[test]
    fn test_lockout_blocks_switch() { /* ... */ }

    #[test]
    fn test_lockout_emits_blocked_event() { /* ... */ }
}

mod state_tracking_tests {
    use super::*;

    #[test]
    fn test_frequency_tracking() { /* ... */ }

    #[test]
    fn test_mode_tracking() { /* ... */ }

    #[test]
    fn test_ptt_tracking() { /* ... */ }

    #[test]
    fn test_status_report_extracts_fields() { /* ... */ }
}

mod translation_tests {
    use super::*;

    // Test matrix for all protocol combinations
    #[test]
    fn test_kenwood_to_icom_frequency() { /* ... */ }

    #[test]
    fn test_kenwood_to_yaesu_frequency() { /* ... */ }

    // ... etc for all 12 combinations
}

mod event_tests {
    use super::*;

    #[test]
    fn test_radio_added_event() { /* ... */ }

    #[test]
    fn test_active_radio_changed_event() { /* ... */ }

    #[test]
    fn test_amplifier_command_event() { /* ... */ }
}

mod edge_case_tests {
    use super::*;

    #[test]
    fn test_remove_active_radio() { /* ... */ }

    #[test]
    fn test_remove_only_radio() { /* ... */ }

    #[test]
    fn test_query_commands_not_forwarded() { /* ... */ }
}

#[cfg(test)]
mod proptest_tests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn test_frequency_translation_roundtrip(hz in 1_800_000u64..54_000_000u64) {
            // Test that any valid amateur frequency translates correctly
        }

        #[test]
        fn test_state_consistency(commands in prop::collection::vec(any::<RadioCommand>(), 0..100)) {
            // Feed random commands and verify state remains consistent
        }
    }
}
```

## Existing Tests (for reference)

Unit tests already exist in:
- `cat-mux/src/engine.rs` - basic add/remove, manual switch, PTT switch, frequency update
- `cat-mux/src/translation.rs` - frequency translation, PTT translation, precision

The integration tests will be more comprehensive and test end-to-end flows.

## Next Steps

1. Create `/crates/cat-mux/tests/integration_tests.rs`
2. Implement helper functions
3. Implement switching mode tests
4. Implement lockout tests
5. Implement state tracking tests
6. Implement protocol translation matrix tests
7. Implement event emission tests
8. Implement edge case tests
9. Add proptest dependency and property-based tests
10. Run `cargo test -p cat-mux` to verify all pass

## Dependencies to Add

In `cat-mux/Cargo.toml`:
```toml
[dev-dependencies]
proptest = "1.5"
```
