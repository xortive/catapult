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
//! # Channel-Based Architecture
//!
//! The multiplexer uses a channel-based architecture where:
//! - Each radio has `RadioChannelMeta` describing its connection
//! - The amplifier has an `AmplifierChannel` for bidirectional communication
//! - All events (traffic, state changes) emit through a unified `MuxEvent` stream
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

pub mod actor;
pub mod amplifier;
pub mod channel;
pub mod engine;
pub mod error;
pub mod events;
pub mod state;
pub mod translation;

// Re-export actor types
pub use actor::{run_mux_actor, MuxActorCommand, RadioStateSummary};

// Re-export channel types
pub use amplifier::{
    create_virtual_amp_channel, AmplifierChannel, AmplifierChannelMeta, AmplifierType,
    VirtualAmplifier, VirtualAmplifierIo,
};
pub use channel::{
    is_virtual_port, sim_id_from_port, virtual_port_name, RadioChannelMeta, RadioType,
    VIRTUAL_PORT_PREFIX,
};

// Re-export event types
pub use events::MuxEvent;

// Re-export engine types
pub use engine::{Multiplexer, MultiplexerConfig};
pub use error::MuxError;
pub use state::{AmplifierConfig, AmplifierEmulatedState, RadioHandle, RadioState, SwitchingMode};
pub use translation::{ProtocolTranslator, TranslationConfig};
