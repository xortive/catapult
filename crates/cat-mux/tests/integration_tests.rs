//! Integration tests for the CAT Multiplexer
//!
//! These tests verify end-to-end behavior of the multiplexer including:
//! - Switching modes (manual, PTT-triggered, frequency-triggered, automatic)
//! - Protocol translation between all protocol pairs
//! - State tracking for multiple radios
//! - Event emission for UI updates
//! - Lockout behavior and edge cases

use cat_mux::{Multiplexer, MultiplexerConfig, MultiplexerEvent, RadioHandle, SwitchingMode};
use cat_protocol::{OperatingMode, Protocol, RadioCommand};

// ============================================================================
// Helper Functions
// ============================================================================

mod helpers {
    use super::*;

    /// Create a multiplexer with lockout disabled for deterministic testing
    pub fn mux_no_lockout() -> Multiplexer {
        let config = MultiplexerConfig {
            lockout_ms: 0,
            ..Default::default()
        };
        Multiplexer::with_config(config)
    }

    /// Create a multiplexer with a specific amplifier protocol
    pub fn mux_with_amp_protocol(protocol: Protocol) -> Multiplexer {
        let mut config = MultiplexerConfig {
            lockout_ms: 0,
            ..Default::default()
        };
        config.amplifier.protocol = protocol;
        Multiplexer::with_config(config)
    }

    /// Extract AmplifierCommand events as raw bytes
    pub fn get_amp_commands(events: &[MultiplexerEvent]) -> Vec<Vec<u8>> {
        events
            .iter()
            .filter_map(|e| match e {
                MultiplexerEvent::AmplifierCommand(data) => Some(data.clone()),
                _ => None,
            })
            .collect()
    }

    /// Check if events contain an ActiveRadioChanged to a specific handle
    pub fn has_switch_to(events: &[MultiplexerEvent], handle: RadioHandle) -> bool {
        events.iter().any(|e| {
            matches!(
                e,
                MultiplexerEvent::ActiveRadioChanged { to, .. } if *to == handle
            )
        })
    }

    /// Check if events contain a SwitchingBlocked event
    pub fn has_switching_blocked(events: &[MultiplexerEvent]) -> bool {
        events
            .iter()
            .any(|e| matches!(e, MultiplexerEvent::SwitchingBlocked { .. }))
    }

    /// Check if events contain a RadioAdded event for a handle
    pub fn has_radio_added(events: &[MultiplexerEvent], handle: RadioHandle) -> bool {
        events
            .iter()
            .any(|e| matches!(e, MultiplexerEvent::RadioAdded(h) if *h == handle))
    }

    /// Check if events contain a RadioRemoved event for a handle
    pub fn has_radio_removed(events: &[MultiplexerEvent], handle: RadioHandle) -> bool {
        events
            .iter()
            .any(|e| matches!(e, MultiplexerEvent::RadioRemoved(h) if *h == handle))
    }
}

// ============================================================================
// Switching Mode Tests
// ============================================================================

mod switching_tests {
    use super::*;

    #[test]
    fn first_radio_becomes_active() {
        let mut mux = helpers::mux_no_lockout();

        let h1 = mux.add_radio("Radio 1".into(), "/dev/tty0".into(), Protocol::Kenwood);

        assert_eq!(mux.active_radio(), Some(h1));
    }

    #[test]
    fn second_radio_does_not_change_active() {
        let mut mux = helpers::mux_no_lockout();

        let h1 = mux.add_radio("Radio 1".into(), "/dev/tty0".into(), Protocol::Kenwood);
        let _h2 = mux.add_radio("Radio 2".into(), "/dev/tty1".into(), Protocol::IcomCIV);

        assert_eq!(mux.active_radio(), Some(h1));
    }

    #[test]
    fn manual_switch_changes_active() {
        let mut mux = helpers::mux_no_lockout();
        mux.set_switching_mode(SwitchingMode::Manual);

        let h1 = mux.add_radio("Radio 1".into(), "/dev/tty0".into(), Protocol::Kenwood);
        let h2 = mux.add_radio("Radio 2".into(), "/dev/tty1".into(), Protocol::IcomCIV);
        mux.drain_events(); // Clear add events

        assert_eq!(mux.active_radio(), Some(h1));

        mux.select_radio(h2).unwrap();

        assert_eq!(mux.active_radio(), Some(h2));

        let events = mux.drain_events();
        assert!(helpers::has_switch_to(&events, h2));
    }

    #[test]
    fn manual_mode_ignores_ptt_from_inactive() {
        let mut mux = helpers::mux_no_lockout();
        mux.set_switching_mode(SwitchingMode::Manual);

        let h1 = mux.add_radio("Radio 1".into(), "/dev/tty0".into(), Protocol::Kenwood);
        let h2 = mux.add_radio("Radio 2".into(), "/dev/tty1".into(), Protocol::Kenwood);
        mux.drain_events();

        // PTT from inactive radio should not switch
        mux.process_radio_command(h2, RadioCommand::SetPtt { active: true });

        assert_eq!(mux.active_radio(), Some(h1));
    }

    #[test]
    fn automatic_mode_switches_on_ptt() {
        let mut mux = helpers::mux_no_lockout();
        mux.set_switching_mode(SwitchingMode::Automatic);

        let h1 = mux.add_radio("Radio 1".into(), "/dev/tty0".into(), Protocol::Kenwood);
        let h2 = mux.add_radio("Radio 2".into(), "/dev/tty1".into(), Protocol::Kenwood);
        mux.drain_events();

        assert_eq!(mux.active_radio(), Some(h1));

        // PTT from h2 should trigger switch in automatic mode
        mux.process_radio_command(h2, RadioCommand::SetPtt { active: true });

        assert_eq!(mux.active_radio(), Some(h2));

        let events = mux.drain_events();
        assert!(helpers::has_switch_to(&events, h2));
    }

    #[test]
    fn automatic_mode_switches_on_frequency() {
        let mut mux = helpers::mux_no_lockout();
        mux.set_switching_mode(SwitchingMode::Automatic);

        let _h1 = mux.add_radio("Radio 1".into(), "/dev/tty0".into(), Protocol::Kenwood);
        let h2 = mux.add_radio("Radio 2".into(), "/dev/tty1".into(), Protocol::Kenwood);
        mux.drain_events();

        // Frequency change from h2 should switch in automatic mode
        mux.process_radio_command(h2, RadioCommand::SetFrequency { hz: 14_250_000 });

        assert_eq!(
            mux.active_radio(),
            Some(h2),
            "Should switch on frequency in automatic mode"
        );
    }

    #[test]
    fn frequency_triggered_switches_on_frequency() {
        let mut mux = helpers::mux_no_lockout();
        mux.set_switching_mode(SwitchingMode::FrequencyTriggered);

        let h1 = mux.add_radio("Radio 1".into(), "/dev/tty0".into(), Protocol::Kenwood);
        let h2 = mux.add_radio("Radio 2".into(), "/dev/tty1".into(), Protocol::Kenwood);
        mux.drain_events();

        assert_eq!(mux.active_radio(), Some(h1), "Should start on first radio");

        // Frequency change from h2 should trigger switch
        mux.process_radio_command(h2, RadioCommand::SetFrequency { hz: 7_150_000 });

        assert_eq!(mux.active_radio(), Some(h2));
    }

    #[test]
    fn frequency_triggered_ignores_ptt() {
        let mut mux = helpers::mux_no_lockout();
        mux.set_switching_mode(SwitchingMode::FrequencyTriggered);

        let h1 = mux.add_radio("Radio 1".into(), "/dev/tty0".into(), Protocol::Kenwood);
        let h2 = mux.add_radio("Radio 2".into(), "/dev/tty1".into(), Protocol::Kenwood);
        mux.drain_events();

        // PTT from h2 should NOT switch
        mux.process_radio_command(h2, RadioCommand::SetPtt { active: true });

        assert_eq!(mux.active_radio(), Some(h1), "Should remain on first radio");
    }

    #[test]
    fn inactive_radio_commands_return_none() {
        let mut mux = helpers::mux_no_lockout();
        mux.set_switching_mode(SwitchingMode::Manual);

        let _h1 = mux.add_radio("Radio 1".into(), "/dev/tty0".into(), Protocol::Kenwood);
        let h2 = mux.add_radio("Radio 2".into(), "/dev/tty1".into(), Protocol::Kenwood);
        mux.drain_events();

        // Command from inactive radio should return None (not forwarded to amp)
        let result = mux.process_radio_command(h2, RadioCommand::SetFrequency { hz: 14_250_000 });

        assert!(result.is_none());
    }

    #[test]
    fn active_radio_commands_return_translated_bytes() {
        let mut mux = helpers::mux_with_amp_protocol(Protocol::Kenwood);
        mux.set_switching_mode(SwitchingMode::Manual);

        let h1 = mux.add_radio("Radio 1".into(), "/dev/tty0".into(), Protocol::Kenwood);
        mux.drain_events();

        let result = mux.process_radio_command(h1, RadioCommand::SetFrequency { hz: 14_250_000 });

        assert!(result.is_some());
        let bytes = result.unwrap();
        assert!(bytes.ends_with(b";"));
    }
}

// ============================================================================
// Lockout Tests
// ============================================================================

mod lockout_tests {
    use super::*;

    #[test]
    fn lockout_blocks_rapid_manual_switch() {
        let config = MultiplexerConfig {
            lockout_ms: 1000, // 1 second lockout
            ..Default::default()
        };
        let mut mux = Multiplexer::with_config(config);

        let h1 = mux.add_radio("Radio 1".into(), "/dev/tty0".into(), Protocol::Kenwood);
        let h2 = mux.add_radio("Radio 2".into(), "/dev/tty1".into(), Protocol::Kenwood);
        mux.drain_events();

        // First switch should succeed
        mux.select_radio(h2).unwrap();
        assert_eq!(mux.active_radio(), Some(h2));

        // Immediate second switch should fail due to lockout
        let result = mux.select_radio(h1);
        assert!(result.is_err());
        assert_eq!(mux.active_radio(), Some(h2));
    }

    #[test]
    fn lockout_emits_blocked_event() {
        let config = MultiplexerConfig {
            lockout_ms: 1000,
            ..Default::default()
        };
        let mut mux = Multiplexer::with_config(config);

        let h1 = mux.add_radio("Radio 1".into(), "/dev/tty0".into(), Protocol::Kenwood);
        let h2 = mux.add_radio("Radio 2".into(), "/dev/tty1".into(), Protocol::Kenwood);
        mux.drain_events();

        mux.select_radio(h2).unwrap();
        mux.drain_events();

        // Try to switch back during lockout
        let _ = mux.select_radio(h1);

        let events = mux.drain_events();
        assert!(helpers::has_switching_blocked(&events));
    }

    #[test]
    fn lockout_blocks_auto_switch() {
        let config = MultiplexerConfig {
            lockout_ms: 1000,
            switching_mode: SwitchingMode::Automatic,
            ..Default::default()
        };
        let mut mux = Multiplexer::with_config(config);

        let h1 = mux.add_radio("Radio 1".into(), "/dev/tty0".into(), Protocol::Kenwood);
        let h2 = mux.add_radio("Radio 2".into(), "/dev/tty1".into(), Protocol::Kenwood);
        mux.drain_events();

        // PTT from h2 triggers switch
        mux.process_radio_command(h2, RadioCommand::SetPtt { active: true });
        assert_eq!(mux.active_radio(), Some(h2));

        // PTT from h1 during lockout should not switch
        mux.process_radio_command(h1, RadioCommand::SetPtt { active: true });
        assert_eq!(mux.active_radio(), Some(h2));
    }

    #[test]
    fn is_locked_returns_correct_state() {
        let config = MultiplexerConfig {
            lockout_ms: 1000,
            ..Default::default()
        };
        let mut mux = Multiplexer::with_config(config);

        let _h1 = mux.add_radio("Radio 1".into(), "/dev/tty0".into(), Protocol::Kenwood);
        let h2 = mux.add_radio("Radio 2".into(), "/dev/tty1".into(), Protocol::Kenwood);

        assert!(!mux.is_locked());

        mux.select_radio(h2).unwrap();

        assert!(mux.is_locked());
    }
}

// ============================================================================
// State Tracking Tests
// ============================================================================

mod state_tracking_tests {
    use super::*;

    #[test]
    fn frequency_tracked_from_set_command() {
        let mut mux = helpers::mux_no_lockout();

        let h1 = mux.add_radio("Radio 1".into(), "/dev/tty0".into(), Protocol::Kenwood);

        mux.process_radio_command(h1, RadioCommand::SetFrequency { hz: 14_250_000 });

        let state = mux.get_radio(h1).unwrap();
        assert_eq!(state.frequency_hz, Some(14_250_000));
    }

    #[test]
    fn frequency_tracked_from_report() {
        let mut mux = helpers::mux_no_lockout();

        let h1 = mux.add_radio("Radio 1".into(), "/dev/tty0".into(), Protocol::Kenwood);

        mux.process_radio_command(h1, RadioCommand::FrequencyReport { hz: 7_074_000 });

        let state = mux.get_radio(h1).unwrap();
        assert_eq!(state.frequency_hz, Some(7_074_000));
    }

    #[test]
    fn mode_tracked_from_set_command() {
        let mut mux = helpers::mux_no_lockout();

        let h1 = mux.add_radio("Radio 1".into(), "/dev/tty0".into(), Protocol::Kenwood);

        mux.process_radio_command(
            h1,
            RadioCommand::SetMode {
                mode: OperatingMode::Usb,
            },
        );

        let state = mux.get_radio(h1).unwrap();
        assert_eq!(state.mode, Some(OperatingMode::Usb));
    }

    #[test]
    fn mode_tracked_from_report() {
        let mut mux = helpers::mux_no_lockout();

        let h1 = mux.add_radio("Radio 1".into(), "/dev/tty0".into(), Protocol::Kenwood);

        mux.process_radio_command(
            h1,
            RadioCommand::ModeReport {
                mode: OperatingMode::Cw,
            },
        );

        let state = mux.get_radio(h1).unwrap();
        assert_eq!(state.mode, Some(OperatingMode::Cw));
    }

    #[test]
    fn ptt_tracked_from_set_command() {
        let mut mux = helpers::mux_no_lockout();

        let h1 = mux.add_radio("Radio 1".into(), "/dev/tty0".into(), Protocol::Kenwood);

        assert!(!mux.get_radio(h1).unwrap().ptt);

        mux.process_radio_command(h1, RadioCommand::SetPtt { active: true });

        assert!(mux.get_radio(h1).unwrap().ptt);

        mux.process_radio_command(h1, RadioCommand::SetPtt { active: false });

        assert!(!mux.get_radio(h1).unwrap().ptt);
    }

    #[test]
    fn status_report_extracts_all_fields() {
        let mut mux = helpers::mux_no_lockout();

        let h1 = mux.add_radio("Radio 1".into(), "/dev/tty0".into(), Protocol::Kenwood);

        mux.process_radio_command(
            h1,
            RadioCommand::StatusReport {
                frequency_hz: Some(28_500_000),
                mode: Some(OperatingMode::Fm),
                ptt: Some(true),
                vfo: None,
            },
        );

        let state = mux.get_radio(h1).unwrap();
        assert_eq!(state.frequency_hz, Some(28_500_000));
        assert_eq!(state.mode, Some(OperatingMode::Fm));
        assert!(state.ptt);
    }

    #[test]
    fn multiple_radios_track_independently() {
        let mut mux = helpers::mux_no_lockout();

        let h1 = mux.add_radio("Radio 1".into(), "/dev/tty0".into(), Protocol::Kenwood);
        let h2 = mux.add_radio("Radio 2".into(), "/dev/tty1".into(), Protocol::IcomCIV);

        mux.process_radio_command(h1, RadioCommand::SetFrequency { hz: 14_250_000 });
        mux.process_radio_command(h2, RadioCommand::SetFrequency { hz: 7_074_000 });

        mux.process_radio_command(
            h1,
            RadioCommand::SetMode {
                mode: OperatingMode::Usb,
            },
        );
        mux.process_radio_command(
            h2,
            RadioCommand::SetMode {
                mode: OperatingMode::Lsb,
            },
        );

        let state1 = mux.get_radio(h1).unwrap();
        let state2 = mux.get_radio(h2).unwrap();

        assert_eq!(state1.frequency_hz, Some(14_250_000));
        assert_eq!(state1.mode, Some(OperatingMode::Usb));

        assert_eq!(state2.frequency_hz, Some(7_074_000));
        assert_eq!(state2.mode, Some(OperatingMode::Lsb));
    }

    #[test]
    fn inactive_radio_state_still_tracked() {
        let mut mux = helpers::mux_no_lockout();
        mux.set_switching_mode(SwitchingMode::Manual);

        let _h1 = mux.add_radio("Radio 1".into(), "/dev/tty0".into(), Protocol::Kenwood);
        let h2 = mux.add_radio("Radio 2".into(), "/dev/tty1".into(), Protocol::Kenwood);

        // h2 is inactive, but state should still be tracked
        mux.process_radio_command(h2, RadioCommand::SetFrequency { hz: 21_300_000 });

        let state = mux.get_radio(h2).unwrap();
        assert_eq!(state.frequency_hz, Some(21_300_000));
    }
}

// ============================================================================
// Protocol Translation Tests
// ============================================================================

mod translation_tests {
    use super::*;

    // Kenwood as target
    #[test]
    fn translate_frequency_to_kenwood() {
        let mut mux = helpers::mux_with_amp_protocol(Protocol::Kenwood);

        let h1 = mux.add_radio("IC-7300".into(), "/dev/tty0".into(), Protocol::IcomCIV);

        let result = mux
            .process_radio_command(h1, RadioCommand::SetFrequency { hz: 14_250_000 })
            .unwrap();

        // Kenwood format: FA00014250000;
        let s = String::from_utf8_lossy(&result);
        assert!(s.starts_with("FA"), "Expected FA prefix, got: {}", s);
        assert!(s.ends_with(";"), "Expected ; suffix, got: {}", s);
        assert!(
            s.contains("14250000"),
            "Expected frequency in output: {}",
            s
        );
    }

    #[test]
    fn translate_ptt_on_to_kenwood() {
        let mut mux = helpers::mux_with_amp_protocol(Protocol::Kenwood);

        let h1 = mux.add_radio("Radio".into(), "/dev/tty0".into(), Protocol::IcomCIV);

        let result = mux
            .process_radio_command(h1, RadioCommand::SetPtt { active: true })
            .unwrap();

        assert_eq!(result, b"TX1;");
    }

    #[test]
    fn translate_ptt_off_to_kenwood() {
        let mut mux = helpers::mux_with_amp_protocol(Protocol::Kenwood);

        let h1 = mux.add_radio("Radio".into(), "/dev/tty0".into(), Protocol::IcomCIV);

        let result = mux
            .process_radio_command(h1, RadioCommand::SetPtt { active: false })
            .unwrap();

        // Kenwood uses RX; or TX0; for PTT off
        let s = String::from_utf8_lossy(&result);
        assert!(
            s == "RX;" || s == "TX0;",
            "Expected RX; or TX0;, got: {}",
            s
        );
    }

    // Icom CI-V as target
    #[test]
    fn translate_frequency_to_icom() {
        let mut mux = helpers::mux_with_amp_protocol(Protocol::IcomCIV);

        let h1 = mux.add_radio("TS-590".into(), "/dev/tty0".into(), Protocol::Kenwood);

        let result = mux
            .process_radio_command(h1, RadioCommand::SetFrequency { hz: 14_250_000 })
            .unwrap();

        // CI-V format: FE FE <to> <from> <cmd> <data...> FD
        assert_eq!(result[0], 0xFE, "Expected CI-V preamble");
        assert_eq!(result[1], 0xFE, "Expected CI-V preamble");
        assert_eq!(*result.last().unwrap(), 0xFD, "Expected CI-V terminator");
    }

    // Yaesu as target
    #[test]
    fn translate_frequency_to_yaesu() {
        let mut mux = helpers::mux_with_amp_protocol(Protocol::Yaesu);

        let h1 = mux.add_radio("TS-590".into(), "/dev/tty0".into(), Protocol::Kenwood);

        let result = mux
            .process_radio_command(h1, RadioCommand::SetFrequency { hz: 14_250_000 })
            .unwrap();

        // Yaesu format: 5 bytes, last byte is command
        assert_eq!(result.len(), 5, "Yaesu commands are 5 bytes");
        assert_eq!(result[4], 0x01, "Yaesu set frequency command is 0x01");
    }

    // Elecraft as target
    #[test]
    fn translate_frequency_to_elecraft() {
        let mut mux = helpers::mux_with_amp_protocol(Protocol::Elecraft);

        let h1 = mux.add_radio("IC-7300".into(), "/dev/tty0".into(), Protocol::IcomCIV);

        let result = mux
            .process_radio_command(h1, RadioCommand::SetFrequency { hz: 14_250_000 })
            .unwrap();

        // Elecraft uses Kenwood-compatible format
        let s = String::from_utf8_lossy(&result);
        assert!(s.starts_with("FA"), "Expected FA prefix, got: {}", s);
        assert!(s.ends_with(";"), "Expected ; suffix, got: {}", s);
    }

    // Mode translation
    #[test]
    fn translate_mode_to_kenwood() {
        let mut mux = helpers::mux_with_amp_protocol(Protocol::Kenwood);

        let h1 = mux.add_radio("Radio".into(), "/dev/tty0".into(), Protocol::IcomCIV);

        let result = mux
            .process_radio_command(
                h1,
                RadioCommand::SetMode {
                    mode: OperatingMode::Usb,
                },
            )
            .unwrap();

        // Kenwood mode command: MD<mode>;
        let s = String::from_utf8_lossy(&result);
        assert!(s.starts_with("MD"), "Expected MD prefix, got: {}", s);
        assert!(s.ends_with(";"), "Expected ; suffix, got: {}", s);
    }

    // Query commands should not be forwarded
    #[test]
    fn query_commands_not_forwarded() {
        let mut mux = helpers::mux_with_amp_protocol(Protocol::Kenwood);

        let h1 = mux.add_radio("Radio".into(), "/dev/tty0".into(), Protocol::Kenwood);

        let result = mux.process_radio_command(h1, RadioCommand::GetFrequency);
        assert!(result.is_none(), "GetFrequency should not be forwarded");

        let result = mux.process_radio_command(h1, RadioCommand::GetMode);
        assert!(result.is_none(), "GetMode should not be forwarded");

        let result = mux.process_radio_command(h1, RadioCommand::GetId);
        assert!(result.is_none(), "GetId should not be forwarded");
    }
}

// ============================================================================
// Event Emission Tests
// ============================================================================

mod event_tests {
    use super::*;

    #[test]
    fn radio_added_event_emitted() {
        let mut mux = helpers::mux_no_lockout();

        let h1 = mux.add_radio("Radio 1".into(), "/dev/tty0".into(), Protocol::Kenwood);

        let events = mux.drain_events();
        assert!(helpers::has_radio_added(&events, h1));
    }

    #[test]
    fn radio_removed_event_emitted() {
        let mut mux = helpers::mux_no_lockout();

        let h1 = mux.add_radio("Radio 1".into(), "/dev/tty0".into(), Protocol::Kenwood);
        mux.drain_events();

        mux.remove_radio(h1);

        let events = mux.drain_events();
        assert!(helpers::has_radio_removed(&events, h1));
    }

    #[test]
    fn active_radio_changed_event_on_switch() {
        let mut mux = helpers::mux_no_lockout();

        let _h1 = mux.add_radio("Radio 1".into(), "/dev/tty0".into(), Protocol::Kenwood);
        let h2 = mux.add_radio("Radio 2".into(), "/dev/tty1".into(), Protocol::Kenwood);
        mux.drain_events();

        mux.select_radio(h2).unwrap();

        let events = mux.drain_events();
        assert!(helpers::has_switch_to(&events, h2));
    }

    #[test]
    fn radio_state_updated_event_on_command() {
        let mut mux = helpers::mux_no_lockout();

        let h1 = mux.add_radio("Radio 1".into(), "/dev/tty0".into(), Protocol::Kenwood);
        mux.drain_events();

        mux.process_radio_command(h1, RadioCommand::SetFrequency { hz: 14_250_000 });

        let events = mux.drain_events();
        assert!(events
            .iter()
            .any(|e| matches!(e, MultiplexerEvent::RadioStateUpdated(h) if *h == h1)));
    }

    #[test]
    fn amplifier_command_event_contains_translated_bytes() {
        let mut mux = helpers::mux_with_amp_protocol(Protocol::Kenwood);

        let h1 = mux.add_radio("Radio 1".into(), "/dev/tty0".into(), Protocol::Kenwood);
        mux.drain_events();

        mux.process_radio_command(h1, RadioCommand::SetFrequency { hz: 14_250_000 });

        let events = mux.drain_events();
        let amp_commands = helpers::get_amp_commands(&events);

        assert_eq!(amp_commands.len(), 1);
        assert!(amp_commands[0].ends_with(b";"));
    }
}

// ============================================================================
// Edge Case Tests
// ============================================================================

mod edge_case_tests {
    use super::*;

    #[test]
    fn remove_active_radio_selects_next() {
        let mut mux = helpers::mux_no_lockout();

        let h1 = mux.add_radio("Radio 1".into(), "/dev/tty0".into(), Protocol::Kenwood);
        let h2 = mux.add_radio("Radio 2".into(), "/dev/tty1".into(), Protocol::Kenwood);

        assert_eq!(mux.active_radio(), Some(h1));

        mux.remove_radio(h1);

        assert_eq!(mux.active_radio(), Some(h2));
    }

    #[test]
    fn remove_only_radio_clears_active() {
        let mut mux = helpers::mux_no_lockout();

        let h1 = mux.add_radio("Radio 1".into(), "/dev/tty0".into(), Protocol::Kenwood);

        assert_eq!(mux.active_radio(), Some(h1));

        mux.remove_radio(h1);

        assert_eq!(mux.active_radio(), None);
    }

    #[test]
    fn select_nonexistent_radio_fails() {
        let mut mux = helpers::mux_no_lockout();

        let h1 = mux.add_radio("Radio 1".into(), "/dev/tty0".into(), Protocol::Kenwood);

        // Add and remove a radio to get a stale handle
        let h2 = mux.add_radio("Radio 2".into(), "/dev/tty1".into(), Protocol::Kenwood);
        mux.remove_radio(h2);

        let result = mux.select_radio(h2);
        assert!(result.is_err());
        assert_eq!(mux.active_radio(), Some(h1));
    }

    #[test]
    fn command_for_removed_radio_ignored() {
        let mut mux = helpers::mux_no_lockout();

        let h1 = mux.add_radio("Radio 1".into(), "/dev/tty0".into(), Protocol::Kenwood);
        mux.remove_radio(h1);

        // This should not panic or cause issues
        let result = mux.process_radio_command(h1, RadioCommand::SetFrequency { hz: 14_250_000 });

        assert!(result.is_none());
    }

    #[test]
    fn same_radio_switch_is_noop() {
        let mut mux = helpers::mux_no_lockout();

        let h1 = mux.add_radio("Radio 1".into(), "/dev/tty0".into(), Protocol::Kenwood);
        mux.drain_events();

        mux.select_radio(h1).unwrap();

        let events = mux.drain_events();
        // Should not emit ActiveRadioChanged when selecting already-active radio
        assert!(!helpers::has_switch_to(&events, h1));
    }

    #[test]
    fn unknown_command_not_forwarded() {
        let mut mux = helpers::mux_with_amp_protocol(Protocol::Kenwood);

        let h1 = mux.add_radio("Radio 1".into(), "/dev/tty0".into(), Protocol::Kenwood);

        let result = mux.process_radio_command(
            h1,
            RadioCommand::Unknown {
                data: vec![0x01, 0x02, 0x03],
            },
        );

        assert!(result.is_none());
    }

    #[test]
    fn frequency_display_formatting() {
        let mut mux = helpers::mux_no_lockout();

        let h1 = mux.add_radio("Radio 1".into(), "/dev/tty0".into(), Protocol::Kenwood);
        mux.process_radio_command(h1, RadioCommand::SetFrequency { hz: 14_250_000 });

        let state = mux.get_radio(h1).unwrap();
        let display = state.frequency_display();

        assert_eq!(display, "14.250 MHz");
    }

    #[test]
    fn no_frequency_displays_placeholder() {
        let mut mux = helpers::mux_no_lockout();

        let h1 = mux.add_radio("Radio 1".into(), "/dev/tty0".into(), Protocol::Kenwood);

        let state = mux.get_radio(h1).unwrap();
        let display = state.frequency_display();

        assert_eq!(display, "---");
    }
}

// ============================================================================
// Property-Based Tests
// ============================================================================

mod proptest_tests {
    use super::*;
    use proptest::prelude::*;

    // Strategy for generating valid amateur radio frequencies (in Hz)
    fn amateur_frequency() -> impl Strategy<Value = u64> {
        prop_oneof![
            // 160m band
            1_800_000u64..2_000_000u64,
            // 80m band
            3_500_000u64..4_000_000u64,
            // 40m band
            7_000_000u64..7_300_000u64,
            // 20m band
            14_000_000u64..14_350_000u64,
            // 15m band
            21_000_000u64..21_450_000u64,
            // 10m band
            28_000_000u64..29_700_000u64,
        ]
    }

    fn operating_mode() -> impl Strategy<Value = OperatingMode> {
        prop_oneof![
            Just(OperatingMode::Lsb),
            Just(OperatingMode::Usb),
            Just(OperatingMode::Cw),
            Just(OperatingMode::Am),
            Just(OperatingMode::Fm),
            Just(OperatingMode::Dig),
        ]
    }

    fn protocol() -> impl Strategy<Value = Protocol> {
        prop_oneof![
            Just(Protocol::Kenwood),
            Just(Protocol::Elecraft),
            Just(Protocol::IcomCIV),
            Just(Protocol::Yaesu),
        ]
    }

    proptest! {
        #[test]
        fn frequency_always_tracked(hz in amateur_frequency()) {
            let mut mux = helpers::mux_no_lockout();
            let h1 = mux.add_radio("Radio".into(), "/dev/tty0".into(), Protocol::Kenwood);

            mux.process_radio_command(h1, RadioCommand::SetFrequency { hz });

            let state = mux.get_radio(h1).unwrap();
            // Frequency may be rounded by translator, but should be close
            let tracked = state.frequency_hz.unwrap();
            prop_assert!((tracked as i64 - hz as i64).abs() < 100);
        }

        #[test]
        fn mode_always_tracked(mode in operating_mode()) {
            let mut mux = helpers::mux_no_lockout();
            let h1 = mux.add_radio("Radio".into(), "/dev/tty0".into(), Protocol::Kenwood);

            mux.process_radio_command(h1, RadioCommand::SetMode { mode });

            let state = mux.get_radio(h1).unwrap();
            prop_assert_eq!(state.mode, Some(mode));
        }

        #[test]
        fn ptt_always_tracked(active: bool) {
            let mut mux = helpers::mux_no_lockout();
            let h1 = mux.add_radio("Radio".into(), "/dev/tty0".into(), Protocol::Kenwood);

            mux.process_radio_command(h1, RadioCommand::SetPtt { active });

            let state = mux.get_radio(h1).unwrap();
            prop_assert_eq!(state.ptt, active);
        }

        #[test]
        fn translation_produces_valid_output(
            hz in amateur_frequency(),
            target in protocol()
        ) {
            let mut mux = helpers::mux_with_amp_protocol(target);
            let h1 = mux.add_radio("Radio".into(), "/dev/tty0".into(), Protocol::Kenwood);

            let result = mux.process_radio_command(h1, RadioCommand::SetFrequency { hz });

            // Should produce Some output for frequency commands
            prop_assert!(result.is_some());

            let bytes = result.unwrap();
            prop_assert!(!bytes.is_empty());

            // Verify protocol-specific markers
            match target {
                Protocol::Kenwood | Protocol::Elecraft | Protocol::FlexRadio | Protocol::YaesuAscii => {
                    prop_assert!(bytes.ends_with(b";"));
                }
                Protocol::IcomCIV => {
                    prop_assert_eq!(bytes[0], 0xFE);
                    prop_assert_eq!(*bytes.last().unwrap(), 0xFD);
                }
                Protocol::Yaesu => {
                    prop_assert_eq!(bytes.len(), 5);
                }
            }
        }

        #[test]
        fn multiple_frequency_updates_keep_latest(
            freqs in prop::collection::vec(amateur_frequency(), 1..10)
        ) {
            let mut mux = helpers::mux_no_lockout();
            let h1 = mux.add_radio("Radio".into(), "/dev/tty0".into(), Protocol::Kenwood);

            for &hz in &freqs {
                mux.process_radio_command(h1, RadioCommand::SetFrequency { hz });
            }

            let state = mux.get_radio(h1).unwrap();
            let expected = *freqs.last().unwrap();
            let tracked = state.frequency_hz.unwrap();

            // Should have the last frequency (with possible rounding)
            prop_assert!((tracked as i64 - expected as i64).abs() < 100);
        }

        #[test]
        fn radio_count_consistent_after_operations(
            adds in 1usize..5,
            removes in 0usize..3
        ) {
            let mut mux = helpers::mux_no_lockout();
            let mut handles = Vec::new();

            // Add radios
            for i in 0..adds {
                let h = mux.add_radio(
                    format!("Radio {}", i),
                    format!("/dev/tty{}", i),
                    Protocol::Kenwood
                );
                handles.push(h);
            }

            prop_assert_eq!(mux.radios().count(), adds);

            // Remove some radios
            let to_remove = removes.min(handles.len());
            for h in handles.iter().take(to_remove) {
                mux.remove_radio(*h);
            }

            prop_assert_eq!(mux.radios().count(), adds - to_remove);
        }
    }
}
