//! Traffic data ingestion methods

use std::time::SystemTime;

use cat_mux::{MuxEvent, RadioChannelMeta, RadioHandle};
use cat_protocol::Protocol;

use super::models::{DiagnosticSeverity, TrafficDirection, TrafficEntry, TrafficSource};
use super::TrafficMonitor;

impl TrafficMonitor {
    /// Add an incoming traffic entry from a real radio
    #[allow(dead_code)] // Direct API, may be used for testing
    pub fn add_incoming(&mut self, radio: RadioHandle, data: &[u8], protocol: Option<Protocol>) {
        self.add_incoming_with_port(radio, String::new(), data, protocol);
    }

    /// Add an incoming traffic entry from a real radio with port info
    #[allow(dead_code)] // Direct API, may be used for testing
    pub fn add_incoming_with_port(
        &mut self,
        radio: RadioHandle,
        port: String,
        data: &[u8],
        protocol: Option<Protocol>,
    ) {
        if self.paused {
            return;
        }

        let decoded = self.get_cached_annotation(data, protocol);
        self.add_entry(TrafficEntry::Data {
            timestamp: SystemTime::now(),
            direction: TrafficDirection::Incoming,
            source: TrafficSource::RealRadio {
                handle: radio,
                port,
            },
            data: data.to_vec(),
            decoded,
        });
    }

    /// Add an outgoing traffic entry to real amplifier
    #[allow(dead_code)] // Direct API, may be used for testing
    pub fn add_outgoing(&mut self, data: &[u8], protocol: Option<Protocol>) {
        self.add_outgoing_with_port(String::new(), data, protocol);
    }

    /// Add an outgoing traffic entry to real amplifier with port info
    #[allow(dead_code)] // Direct API, may be used for testing
    pub fn add_outgoing_with_port(
        &mut self,
        port: String,
        data: &[u8],
        protocol: Option<Protocol>,
    ) {
        if self.paused {
            return;
        }

        let decoded = self.get_cached_annotation(data, protocol);
        self.add_entry(TrafficEntry::Data {
            timestamp: SystemTime::now(),
            direction: TrafficDirection::Outgoing,
            source: TrafficSource::RealAmplifier { port },
            data: data.to_vec(),
            decoded,
        });
    }

    /// Add an outgoing traffic entry to real radio (command sent to radio)
    #[allow(dead_code)] // Direct API, may be used for testing
    pub fn add_to_real_radio(
        &mut self,
        handle: RadioHandle,
        port: String,
        data: &[u8],
        protocol: Option<Protocol>,
    ) {
        if self.paused {
            return;
        }

        let decoded = self.get_cached_annotation(data, protocol);
        self.add_entry(TrafficEntry::Data {
            timestamp: SystemTime::now(),
            direction: TrafficDirection::Outgoing,
            source: TrafficSource::ToRealRadio { handle, port },
            data: data.to_vec(),
            decoded,
        });
    }

    /// Add an incoming traffic entry from real amplifier
    #[allow(dead_code)] // Direct API, may be used for testing
    pub fn add_from_amplifier(&mut self, port: String, data: &[u8], protocol: Option<Protocol>) {
        if self.paused {
            return;
        }

        let decoded = self.get_cached_annotation(data, protocol);
        self.add_entry(TrafficEntry::Data {
            timestamp: SystemTime::now(),
            direction: TrafficDirection::Incoming,
            source: TrafficSource::FromRealAmplifier { port },
            data: data.to_vec(),
            decoded,
        });
    }

    /// Add a diagnostic entry (error or warning)
    pub fn add_diagnostic(
        &mut self,
        source: String,
        severity: DiagnosticSeverity,
        message: String,
    ) {
        if self.paused {
            return;
        }

        self.add_entry(TrafficEntry::Diagnostic {
            timestamp: SystemTime::now(),
            source,
            severity,
            message,
        });
    }

    /// Add an entry
    pub(super) fn add_entry(&mut self, entry: TrafficEntry) {
        if self.entries.len() >= self.max_entries {
            self.entries.pop_front();
        }
        self.entries.push_back(entry);
    }

    /// Process a MuxEvent and add appropriate traffic entries
    ///
    /// This is the unified event processing method that handles all traffic
    /// events from the multiplexer. It replaces the individual add_* methods
    /// for new integrations.
    pub fn process_event(
        &mut self,
        event: MuxEvent,
        radio_metas: &dyn Fn(RadioHandle) -> Option<RadioChannelMeta>,
    ) {
        if self.paused {
            return;
        }

        match event {
            MuxEvent::RadioDataIn {
                handle,
                data,
                protocol,
                timestamp,
            } => {
                let decoded = self.get_cached_annotation(&data, Some(protocol));
                let port = radio_metas(handle)
                    .and_then(|m| m.port_name)
                    .unwrap_or_default();

                self.add_entry(TrafficEntry::Data {
                    timestamp,
                    direction: TrafficDirection::Incoming,
                    source: TrafficSource::RealRadio { handle, port },
                    data,
                    decoded,
                });
            }

            MuxEvent::RadioDataOut {
                handle,
                data,
                protocol,
                timestamp,
            } => {
                let decoded = self.get_cached_annotation(&data, Some(protocol));
                let port = radio_metas(handle)
                    .and_then(|m| m.port_name)
                    .unwrap_or_default();

                self.add_entry(TrafficEntry::Data {
                    timestamp,
                    direction: TrafficDirection::Outgoing,
                    source: TrafficSource::ToRealRadio { handle, port },
                    data,
                    decoded,
                });
            }

            MuxEvent::AmpDataOut {
                data,
                protocol,
                timestamp,
            } => {
                let decoded = self.get_cached_annotation(&data, Some(protocol));
                self.add_entry(TrafficEntry::Data {
                    timestamp,
                    direction: TrafficDirection::Outgoing,
                    source: TrafficSource::RealAmplifier {
                        port: String::new(),
                    },
                    data,
                    decoded,
                });
            }

            MuxEvent::AmpDataIn {
                data,
                protocol,
                timestamp,
            } => {
                let decoded = self.get_cached_annotation(&data, Some(protocol));
                self.add_entry(TrafficEntry::Data {
                    timestamp,
                    direction: TrafficDirection::Incoming,
                    source: TrafficSource::FromRealAmplifier {
                        port: String::new(),
                    },
                    data,
                    decoded,
                });
            }

            MuxEvent::Error { source, message } => {
                self.add_entry(TrafficEntry::Diagnostic {
                    timestamp: SystemTime::now(),
                    source,
                    severity: DiagnosticSeverity::Error,
                    message,
                });
            }

            // Non-traffic events are ignored by the traffic monitor
            MuxEvent::RadioConnected { .. }
            | MuxEvent::RadioDisconnected { .. }
            | MuxEvent::RadioStateChanged { .. }
            | MuxEvent::ActiveRadioChanged { .. }
            | MuxEvent::AmpConnected { .. }
            | MuxEvent::AmpDisconnected
            | MuxEvent::SwitchingModeChanged { .. }
            | MuxEvent::SwitchingBlocked { .. } => {}
        }
    }

    /// Process a MuxEvent with amplifier port info
    ///
    /// Enhanced version of process_event that includes amplifier port information
    /// for better traffic source display.
    pub fn process_event_with_amp_port(
        &mut self,
        event: MuxEvent,
        radio_metas: &dyn Fn(RadioHandle) -> Option<RadioChannelMeta>,
        amp_port: &str,
    ) {
        if self.paused {
            return;
        }

        match event {
            MuxEvent::AmpDataOut {
                data,
                protocol,
                timestamp,
            } => {
                let decoded = self.get_cached_annotation(&data, Some(protocol));
                self.add_entry(TrafficEntry::Data {
                    timestamp,
                    direction: TrafficDirection::Outgoing,
                    source: TrafficSource::RealAmplifier {
                        port: amp_port.to_string(),
                    },
                    data,
                    decoded,
                });
            }

            MuxEvent::AmpDataIn {
                data,
                protocol,
                timestamp,
            } => {
                let decoded = self.get_cached_annotation(&data, Some(protocol));
                self.add_entry(TrafficEntry::Data {
                    timestamp,
                    direction: TrafficDirection::Incoming,
                    source: TrafficSource::FromRealAmplifier {
                        port: amp_port.to_string(),
                    },
                    data,
                    decoded,
                });
            }

            // Delegate other events to the base process_event
            other => self.process_event(other, radio_metas),
        }
    }
}
