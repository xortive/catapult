//! CAT Multiplexer Engine
//!
//! This crate provides the core multiplexer logic for switching between
//! multiple radios and translating CAT protocol commands to the amplifier.
//!
//! # Architecture
//!
//! The multiplexer tracks the state of multiple connected radios and
//! determines which radio should be "active" (controlling the amplifier).
//! It supports multiple switching modes:
//!
//! - **Manual**: User explicitly selects the active radio
//! - **Frequency-triggered**: Switch when a radio changes frequency (default)
//! - **Automatic**: Switch on frequency change or PTT
//!
//! # Example
//!
//! ```rust,no_run
//! use cat_mux::{Multiplexer, SwitchingMode, RadioHandle};
//! use cat_protocol::Protocol;
//!
//! let mut mux = Multiplexer::new();
//! // Default is FrequencyTriggered, but can change:
//! mux.set_switching_mode(SwitchingMode::Automatic);
//!
//! // Add radios
//! let radio_a = mux.add_radio("Radio A".into(), "/dev/ttyUSB0".into(), Protocol::Kenwood);
//! let radio_b = mux.add_radio("Radio B".into(), "/dev/ttyUSB1".into(), Protocol::IcomCIV);
//!
//! // Process incoming commands
//! // mux.process_radio_command(radio_a, command);
//! ```

pub mod engine;
pub mod error;
pub mod state;
pub mod translation;

pub use engine::{Multiplexer, MultiplexerConfig, MultiplexerEvent};
pub use error::MuxError;
pub use state::{RadioHandle, RadioState, SwitchingMode};
pub use translation::{ProtocolTranslator, TranslationConfig};
