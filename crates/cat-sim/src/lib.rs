//! CAT Protocol Simulation Library
//!
//! This crate provides a simulation layer for testing CAT multiplexer functionality
//! without physical radio hardware. It includes:
//!
//! - **VirtualRadio**: Simulates a radio with protocol-accurate encoding
//! - **SimulationContext**: Orchestrates multiple virtual radios
//!
//! # Example
//!
//! ```rust
//! use cat_sim::{SimulationContext, VirtualRadio};
//! use cat_protocol::{Protocol, OperatingMode};
//!
//! // Create a simulation context
//! let mut ctx = SimulationContext::new();
//!
//! // Add a virtual radio
//! let id = ctx.add_radio("IC-7300", Protocol::IcomCIV);
//!
//! // Simulate radio state changes
//! ctx.set_radio_frequency(&id, 14_250_000);
//! ctx.set_radio_mode(&id, OperatingMode::Usb);
//!
//! // Get pending protocol-encoded output
//! if let Some(radio) = ctx.get_radio_mut(&id) {
//!     while let Some(bytes) = radio.take_output() {
//!         println!("Radio output: {:02X?}", bytes);
//!     }
//! }
//! ```

pub mod context;
pub mod radio;

pub use context::{SimulationContext, SimulationEvent};
pub use radio::VirtualRadio;
