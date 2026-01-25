//! CAT Serial Port Detection Library
//!
//! This crate provides serial port enumeration for CAT-capable
//! amateur radio transceivers.
//!
//! # Example
//!
//! ```rust,no_run
//! use cat_detect::PortScanner;
//!
//! let scanner = PortScanner::new();
//! let ports = scanner.enumerate_ports().unwrap();
//!
//! for port in ports {
//!     println!("Found port: {}", port.port);
//! }
//! ```

pub mod error;
pub mod scanner;

pub use error::DetectError;
pub use scanner::{PortScanner, SerialPortInfo};
