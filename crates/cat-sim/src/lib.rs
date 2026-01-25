//! CAT Protocol Simulation Library
//!
//! This crate provides a simulation layer for testing CAT multiplexer functionality
//! without physical radio hardware. It includes:
//!
//! - **VirtualRadio**: Simulates a radio with protocol-accurate encoding
//! - **SimulationContext**: Manages lifecycle of virtual radios
//!
//! # Example
//!
//! ```rust
//! use cat_sim::{SimulationContext, VirtualRadio};
//! use cat_protocol::{Protocol, OperatingMode};
//!
//! // Create a simulation context for lifecycle management
//! let mut ctx = SimulationContext::new();
//!
//! // Add a virtual radio (returns ID, queues RadioAdded event)
//! let id = ctx.add_radio("IC-7300", Protocol::IcomCIV);
//!
//! // Take the radio to transfer ownership (e.g., to an actor task)
//! let mut radio = ctx.take_radio(&id).unwrap();
//!
//! // Now manipulate the radio directly
//! radio.set_auto_info(true);
//! radio.set_frequency(14_250_000);
//! radio.set_mode(OperatingMode::Usb);
//!
//! // Get pending protocol-encoded output
//! while let Some(bytes) = radio.take_output() {
//!     println!("Radio output: {:02X?}", bytes);
//! }
//! ```

pub mod context;
pub mod radio;

pub use context::{SimulationContext, SimulationEvent};
pub use radio::{VirtualRadio, VirtualRadioConfig};
