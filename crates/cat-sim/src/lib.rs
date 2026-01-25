//! CAT Protocol Simulation Library
//!
//! This crate provides a simulation layer for testing CAT multiplexer functionality
//! without physical radio hardware. It includes:
//!
//! - **VirtualRadio**: Simulates a radio with protocol-accurate encoding
//! - **VirtualAmplifier**: Simulates an amplifier that tracks frequency/mode state
//!
//! # Example
//!
//! ```rust
//! use cat_sim::VirtualRadio;
//! use cat_protocol::{Protocol, OperatingMode};
//!
//! // Create a virtual radio directly
//! let mut radio = VirtualRadio::new("IC-7300", Protocol::IcomCIV);
//!
//! // Manipulate the radio
//! radio.set_auto_info(true);
//! radio.set_frequency(14_250_000);
//! radio.set_mode(OperatingMode::Usb);
//!
//! // Get pending protocol-encoded output
//! while let Some(bytes) = radio.take_output() {
//!     println!("Radio output: {:02X?}", bytes);
//! }
//! ```

pub mod amplifier;
pub mod radio;

pub use amplifier::{VirtualAmplifier, VirtualAmplifierIo};
pub use radio::{VirtualRadio, VirtualRadioConfig};
