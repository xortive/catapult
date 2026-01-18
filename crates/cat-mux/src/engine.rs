//! Multiplexer engine
//!
//! The core multiplexer logic that handles radio switching,
//! state tracking, and command routing.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use cat_protocol::{Protocol, RadioCommand};
use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};

use crate::error::MuxError;
use crate::state::{AmplifierConfig, RadioHandle, RadioState, SwitchingMode};
use crate::translation::{filter_for_amplifier, ProtocolTranslator, TranslationConfig};

/// Multiplexer configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MultiplexerConfig {
    /// Switching mode
    pub switching_mode: SwitchingMode,
    /// Lockout time after switching (ms)
    pub lockout_ms: u64,
    /// Amplifier configuration
    pub amplifier: AmplifierConfig,
    /// Translation configuration
    pub translation: TranslationConfig,
    /// Radio priority order (handles)
    pub priority_order: Vec<u32>,
}

impl Default for MultiplexerConfig {
    fn default() -> Self {
        Self {
            switching_mode: SwitchingMode::FrequencyTriggered,
            lockout_ms: 500,
            amplifier: AmplifierConfig::default(),
            translation: TranslationConfig::default(),
            priority_order: Vec::new(),
        }
    }
}

/// Events emitted by the multiplexer
#[derive(Debug, Clone)]
pub enum MultiplexerEvent {
    /// A radio was added
    RadioAdded(RadioHandle),
    /// A radio was removed
    RadioRemoved(RadioHandle),
    /// The active radio changed
    ActiveRadioChanged {
        from: Option<RadioHandle>,
        to: RadioHandle,
    },
    /// Radio state updated
    RadioStateUpdated(RadioHandle),
    /// Command translated and ready for amplifier
    AmplifierCommand(Vec<u8>),
    /// Switching was blocked due to lockout
    SwitchingBlocked {
        requested: RadioHandle,
        current: RadioHandle,
        remaining_ms: u64,
    },
    /// Error occurred
    Error(String),
}

/// The multiplexer engine
pub struct Multiplexer {
    config: MultiplexerConfig,
    radios: HashMap<RadioHandle, RadioState>,
    next_handle: u32,
    active_radio: Option<RadioHandle>,
    lockout_until: Option<Instant>,
    translator: ProtocolTranslator,
    event_buffer: Vec<MultiplexerEvent>,
}

impl Multiplexer {
    /// Create a new multiplexer with default configuration
    pub fn new() -> Self {
        Self::with_config(MultiplexerConfig::default())
    }

    /// Create with custom configuration
    pub fn with_config(config: MultiplexerConfig) -> Self {
        let translator = ProtocolTranslator::with_config(
            config.amplifier.protocol,
            config.translation.clone(),
        );

        Self {
            config,
            radios: HashMap::new(),
            next_handle: 1,
            active_radio: None,
            lockout_until: None,
            translator,
            event_buffer: Vec::new(),
        }
    }

    /// Get the current configuration
    pub fn config(&self) -> &MultiplexerConfig {
        &self.config
    }

    /// Update the configuration
    pub fn set_config(&mut self, config: MultiplexerConfig) {
        self.translator = ProtocolTranslator::with_config(
            config.amplifier.protocol,
            config.translation.clone(),
        );
        self.config = config;
    }

    /// Set the switching mode
    pub fn set_switching_mode(&mut self, mode: SwitchingMode) {
        self.config.switching_mode = mode;
    }

    /// Get the switching mode
    pub fn switching_mode(&self) -> SwitchingMode {
        self.config.switching_mode
    }

    /// Add a radio to the multiplexer
    pub fn add_radio(
        &mut self,
        name: String,
        port: String,
        protocol: Protocol,
    ) -> RadioHandle {
        let handle = RadioHandle(self.next_handle);
        self.next_handle += 1;

        let state = RadioState::new(handle, name.clone(), port, protocol);
        self.radios.insert(handle, state);

        // If this is the first radio, make it active
        if self.active_radio.is_none() {
            self.active_radio = Some(handle);
        }

        self.event_buffer.push(MultiplexerEvent::RadioAdded(handle));
        info!("Added radio: {} (handle {})", name, handle.0);

        handle
    }

    /// Remove a radio from the multiplexer
    pub fn remove_radio(&mut self, handle: RadioHandle) -> Option<RadioState> {
        let state = self.radios.remove(&handle)?;

        // If this was the active radio, select another
        if self.active_radio == Some(handle) {
            self.active_radio = self.radios.keys().next().copied();
        }

        self.event_buffer.push(MultiplexerEvent::RadioRemoved(handle));
        Some(state)
    }

    /// Get a radio's state
    pub fn get_radio(&self, handle: RadioHandle) -> Option<&RadioState> {
        self.radios.get(&handle)
    }

    /// Get a mutable reference to a radio's state
    pub fn get_radio_mut(&mut self, handle: RadioHandle) -> Option<&mut RadioState> {
        self.radios.get_mut(&handle)
    }

    /// Iterate over all radios
    pub fn radios(&self) -> impl Iterator<Item = &RadioState> {
        self.radios.values()
    }

    /// Get the currently active radio
    pub fn active_radio(&self) -> Option<RadioHandle> {
        self.active_radio
    }

    /// Get the active radio's state
    pub fn active_radio_state(&self) -> Option<&RadioState> {
        self.active_radio.and_then(|h| self.radios.get(&h))
    }

    /// Manually select the active radio
    pub fn select_radio(&mut self, handle: RadioHandle) -> Result<(), MuxError> {
        if !self.radios.contains_key(&handle) {
            return Err(MuxError::RadioNotFound(format!("handle {}", handle.0)));
        }

        // Check lockout
        if let Some(until) = self.lockout_until {
            if Instant::now() < until {
                let remaining = until.duration_since(Instant::now()).as_millis() as u64;
                if let Some(current) = self.active_radio {
                    self.event_buffer.push(MultiplexerEvent::SwitchingBlocked {
                        requested: handle,
                        current,
                        remaining_ms: remaining,
                    });
                }
                return Err(MuxError::SwitchingLocked(remaining));
            }
        }

        self.switch_to(handle);
        Ok(())
    }

    /// Internal switch implementation
    fn switch_to(&mut self, handle: RadioHandle) {
        let old = self.active_radio;
        if old == Some(handle) {
            return;
        }

        self.active_radio = Some(handle);
        self.lockout_until = Some(Instant::now() + Duration::from_millis(self.config.lockout_ms));

        self.event_buffer.push(MultiplexerEvent::ActiveRadioChanged {
            from: old,
            to: handle,
        });

        if let Some(radio) = self.radios.get(&handle) {
            info!("Switched to radio: {} ({})", radio.name, radio.port);
        }
    }

    /// Process a command from a radio
    ///
    /// Returns the translated command to send to the amplifier (if any)
    pub fn process_radio_command(
        &mut self,
        handle: RadioHandle,
        cmd: RadioCommand,
    ) -> Option<Vec<u8>> {
        // Update radio state
        if let Some(radio) = self.radios.get_mut(&handle) {
            match &cmd {
                RadioCommand::SetFrequency { hz } | RadioCommand::FrequencyReport { hz } => {
                    radio.set_frequency(*hz);
                }
                RadioCommand::SetMode { mode } | RadioCommand::ModeReport { mode } => {
                    radio.set_mode(*mode);
                }
                RadioCommand::SetPtt { active } | RadioCommand::PttReport { active } => {
                    radio.set_ptt(*active);
                }
                RadioCommand::StatusReport {
                    frequency_hz,
                    mode,
                    ptt,
                    ..
                } => {
                    if let Some(hz) = frequency_hz {
                        radio.set_frequency(*hz);
                    }
                    if let Some(m) = mode {
                        radio.set_mode(*m);
                    }
                    if let Some(p) = ptt {
                        radio.set_ptt(*p);
                    }
                }
                _ => {
                    radio.touch();
                }
            }

            self.event_buffer
                .push(MultiplexerEvent::RadioStateUpdated(handle));
        }

        // Check if we should switch radios
        self.check_auto_switch(handle, &cmd);

        // Only forward if this is the active radio
        if self.active_radio != Some(handle) {
            debug!("Ignoring command from non-active radio {}", handle.0);
            return None;
        }

        // Filter and translate for amplifier
        let filtered = filter_for_amplifier(&cmd)?;

        match self.translator.translate(&filtered) {
            Ok(bytes) => {
                self.event_buffer
                    .push(MultiplexerEvent::AmplifierCommand(bytes.clone()));
                Some(bytes)
            }
            Err(e) => {
                warn!("Translation failed: {}", e);
                self.event_buffer
                    .push(MultiplexerEvent::Error(e.to_string()));
                None
            }
        }
    }

    /// Check if we should automatically switch radios
    fn check_auto_switch(&mut self, handle: RadioHandle, cmd: &RadioCommand) {
        // Don't switch to a radio that doesn't exist
        if !self.radios.contains_key(&handle) {
            return;
        }

        if self.active_radio == Some(handle) {
            return;
        }

        // Check lockout
        if let Some(until) = self.lockout_until {
            if Instant::now() < until {
                return;
            }
        }

        let should_switch = match self.config.switching_mode {
            SwitchingMode::Manual => false,
            SwitchingMode::FrequencyTriggered => {
                matches!(
                    cmd,
                    RadioCommand::SetFrequency { .. } | RadioCommand::FrequencyReport { .. }
                )
            }
            SwitchingMode::Automatic => {
                matches!(
                    cmd,
                    RadioCommand::SetPtt { active: true }
                        | RadioCommand::PttReport { active: true }
                        | RadioCommand::SetFrequency { .. }
                        | RadioCommand::FrequencyReport { .. }
                )
            }
        };

        if should_switch {
            debug!(
                "Auto-switching to radio {} due to {:?}",
                handle.0,
                std::mem::discriminant(cmd)
            );
            self.switch_to(handle);
        }
    }

    /// Drain pending events
    pub fn drain_events(&mut self) -> Vec<MultiplexerEvent> {
        std::mem::take(&mut self.event_buffer)
    }

    /// Check if lockout is active
    pub fn is_locked(&self) -> bool {
        self.lockout_until
            .is_some_and(|until| Instant::now() < until)
    }

    /// Get remaining lockout time in ms
    pub fn lockout_remaining_ms(&self) -> u64 {
        self.lockout_until
            .map(|until| {
                until
                    .saturating_duration_since(Instant::now())
                    .as_millis() as u64
            })
            .unwrap_or(0)
    }

    /// Set amplifier configuration
    pub fn set_amplifier_config(&mut self, config: AmplifierConfig) {
        self.translator.set_target_protocol(config.protocol);
        self.config.amplifier = config;
    }

    /// Get amplifier configuration
    pub fn amplifier_config(&self) -> &AmplifierConfig {
        &self.config.amplifier
    }
}

impl Default for Multiplexer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_add_remove_radio() {
        let mut mux = Multiplexer::new();

        let h1 = mux.add_radio("Radio 1".into(), "/dev/ttyUSB0".into(), Protocol::Kenwood);
        let h2 = mux.add_radio("Radio 2".into(), "/dev/ttyUSB1".into(), Protocol::IcomCIV);

        assert_eq!(mux.radios().count(), 2);
        assert_eq!(mux.active_radio(), Some(h1));

        mux.remove_radio(h1);
        assert_eq!(mux.radios().count(), 1);
        assert_eq!(mux.active_radio(), Some(h2));
    }

    #[test]
    fn test_manual_switching() {
        let mut mux = Multiplexer::new();
        mux.set_switching_mode(SwitchingMode::Manual);

        let h1 = mux.add_radio("Radio 1".into(), "/dev/ttyUSB0".into(), Protocol::Kenwood);
        let h2 = mux.add_radio("Radio 2".into(), "/dev/ttyUSB1".into(), Protocol::Kenwood);

        assert_eq!(mux.active_radio(), Some(h1));

        mux.select_radio(h2).unwrap();
        assert_eq!(mux.active_radio(), Some(h2));
    }

    #[test]
    fn test_automatic_ptt_switching() {
        let mut mux = Multiplexer::new();
        mux.set_switching_mode(SwitchingMode::Automatic);
        // Disable lockout for testing
        mux.config.lockout_ms = 0;

        let h1 = mux.add_radio("Radio 1".into(), "/dev/ttyUSB0".into(), Protocol::Kenwood);
        let h2 = mux.add_radio("Radio 2".into(), "/dev/ttyUSB1".into(), Protocol::Kenwood);

        assert_eq!(mux.active_radio(), Some(h1));

        // PTT from radio 2 should switch (Automatic mode includes PTT)
        mux.process_radio_command(h2, RadioCommand::SetPtt { active: true });
        assert_eq!(mux.active_radio(), Some(h2));
    }

    #[test]
    fn test_frequency_update() {
        let mut mux = Multiplexer::new();
        let h1 = mux.add_radio("Radio 1".into(), "/dev/ttyUSB0".into(), Protocol::Kenwood);

        mux.process_radio_command(h1, RadioCommand::SetFrequency { hz: 14_250_000 });

        let state = mux.get_radio(h1).unwrap();
        assert_eq!(state.frequency_hz, Some(14_250_000));
    }

    #[test]
    fn test_command_translation() {
        let mut mux = Multiplexer::new();
        mux.config.amplifier.protocol = Protocol::Kenwood;

        let h1 = mux.add_radio("Radio 1".into(), "/dev/ttyUSB0".into(), Protocol::Kenwood);

        let result = mux.process_radio_command(h1, RadioCommand::SetFrequency { hz: 14_250_000 });

        assert!(result.is_some());
        let bytes = result.unwrap();
        assert!(bytes.ends_with(b";"));
    }
}
