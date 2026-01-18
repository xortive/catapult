//! CAT Auto-Detection Library
//!
//! This crate provides automatic detection and identification of CAT-capable
//! amateur radio transceivers connected via serial ports.
//!
//! # Features
//!
//! - Enumerate all serial ports on the host system
//! - Filter by USB VID/PID for known serial adapter chips
//! - Probe ports with protocol-specific commands
//! - Identify radio model from responses
//! - Track port appearance/disappearance (hot-plug)
//!
//! # Example
//!
//! ```rust,no_run
//! use cat_detect::{PortScanner, DetectedRadio};
//!
//! #[tokio::main]
//! async fn main() {
//!     let mut scanner = PortScanner::new();
//!     let radios = scanner.scan().await;
//!
//!     for radio in radios {
//!         println!("Found {} on {}", radio.model_name(), radio.port);
//!     }
//! }
//! ```

pub mod error;
pub mod probe;
pub mod scanner;
pub mod usb_ids;

pub use error::DetectError;
pub use probe::{ProbeResult, RadioProber};
pub use scanner::{DetectedRadio, PortScanner, SerialPortInfo};
