//! CAT Serial Port Detection Library
//!
//! This crate provides serial port enumeration and manual probing for
//! CAT-capable amateur radio transceivers.
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
pub mod probe;
pub mod scanner;

pub use error::DetectError;
pub use probe::{probe_port, probe_port_with_protocol, ProbeResult, RadioProber};
pub use scanner::{PortScanner, SerialPortInfo};
