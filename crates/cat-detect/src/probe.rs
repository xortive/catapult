//! Protocol probing for radio detection
//!
//! This module sends protocol-specific commands to serial ports
//! and analyzes responses to determine if a radio is present
//! and which protocol it uses.

use std::time::Duration;

use cat_protocol::{
    elecraft, flex, icom, kenwood, models::RadioDatabase, yaesu, yaesu_ascii, Protocol, RadioModel,
};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::time::timeout;
use tracing::{debug, info, trace, warn};

/// Result of probing a serial port
#[derive(Debug, Clone)]
pub struct ProbeResult {
    /// Detected protocol
    pub protocol: Protocol,
    /// Identified radio model (if known)
    pub model: Option<RadioModel>,
    /// Raw identification data
    pub id_data: Vec<u8>,
    /// Protocol-specific address (for CI-V)
    pub address: Option<u8>,
}

/// Configuration for probing
#[derive(Debug, Clone)]
pub struct ProbeConfig {
    /// Timeout for each probe attempt
    pub timeout: Duration,
    /// Delay between protocol attempts
    pub inter_probe_delay: Duration,
}

impl Default for ProbeConfig {
    fn default() -> Self {
        Self {
            timeout: Duration::from_millis(500),
            inter_probe_delay: Duration::from_millis(100),
        }
    }
}

/// Radio protocol prober
pub struct RadioProber {
    config: ProbeConfig,
}

impl RadioProber {
    /// Create a new prober with default configuration
    pub fn new() -> Self {
        Self {
            config: ProbeConfig::default(),
        }
    }

    /// Create a prober with custom configuration
    pub fn with_config(config: ProbeConfig) -> Self {
        Self { config }
    }

    /// Probe a stream to detect any connected radio
    pub async fn probe<S>(&self, stream: &mut S) -> Option<ProbeResult>
    where
        S: AsyncRead + AsyncWrite + Unpin,
    {
        // Try protocols in order of popularity/reliability
        // FlexRadio, Kenwood, Elecraft first (ASCII, easy to detect)
        // Then Icom CI-V (framed binary)
        // Finally Yaesu (raw binary, harder to detect)

        if let Some(result) = self.probe_flex_kenwood_elecraft(stream).await {
            return Some(result);
        }

        tokio::time::sleep(self.config.inter_probe_delay).await;

        if let Some(result) = self.probe_icom(stream).await {
            return Some(result);
        }

        tokio::time::sleep(self.config.inter_probe_delay).await;

        if let Some(result) = self.probe_yaesu(stream).await {
            return Some(result);
        }

        debug!("No radio detected (tried all protocols)");
        None
    }

    /// Probe for FlexRadio, Kenwood, Elecraft, or Yaesu ASCII radios
    async fn probe_flex_kenwood_elecraft<S>(&self, stream: &mut S) -> Option<ProbeResult>
    where
        S: AsyncRead + AsyncWrite + Unpin,
    {
        debug!("Probing for FlexRadio/Kenwood/Elecraft/YaesuAscii...");

        // Try Elecraft K3 first
        if let Some(result) = self.try_elecraft_k3(stream).await {
            return Some(result);
        }

        // Try standard ID command (works for FlexRadio and Kenwood)
        // FlexRadio responds with ID904-913, Kenwood with ID019-023 etc
        if let Some(result) = self.try_kenwood_flex_id(stream).await {
            return Some(result);
        }

        None
    }

    /// Try Elecraft K3 identification
    async fn try_elecraft_k3<S>(&self, stream: &mut S) -> Option<ProbeResult>
    where
        S: AsyncRead + AsyncWrite + Unpin,
    {
        let probe = b"K3;";
        trace!("Sending K3 probe");

        if let Err(e) = stream.write_all(probe).await {
            warn!("Failed to write K3 probe: {}", e);
            return None;
        }

        let mut buf = [0u8; 64];
        match timeout(self.config.timeout, stream.read(&mut buf)).await {
            Ok(Ok(n)) if n > 0 => {
                let response = &buf[..n];
                trace!("K3 response: {:?}", String::from_utf8_lossy(response));

                if let Some(model_name) = elecraft::is_elecraft_response(response) {
                    let model = RadioDatabase::by_elecraft_id(model_name);
                    info!(
                        "Identified {} via Elecraft protocol",
                        model
                            .as_ref()
                            .map(|m| m.model.as_str())
                            .unwrap_or(model_name)
                    );
                    return Some(ProbeResult {
                        protocol: Protocol::Elecraft,
                        model,
                        id_data: response.to_vec(),
                        address: None,
                    });
                }
            }
            Ok(Ok(_)) => trace!("No response to K3 probe"),
            Ok(Err(e)) => trace!("K3 read error: {}", e),
            Err(_) => trace!("K3 probe timeout"),
        }

        None
    }

    /// Try standard ID command (works for Kenwood, FlexRadio, and Yaesu ASCII)
    async fn try_kenwood_flex_id<S>(&self, stream: &mut S) -> Option<ProbeResult>
    where
        S: AsyncRead + AsyncWrite + Unpin,
    {
        let probe = kenwood::probe_command(); // ID; works for all ASCII protocols
        trace!("Sending Kenwood/FlexRadio/YaesuAscii ID probe");

        if let Err(e) = stream.write_all(&probe).await {
            warn!("Failed to write ID probe: {}", e);
            return None;
        }

        let mut buf = [0u8; 64];
        match timeout(self.config.timeout, stream.read(&mut buf)).await {
            Ok(Ok(n)) if n > 0 => {
                let response = &buf[..n];
                trace!("ID response: {:?}", String::from_utf8_lossy(response));

                // Check for FlexRadio first (ID904-913)
                if flex::is_valid_id_response(response) {
                    let id_str = String::from_utf8_lossy(&response[2..response.len() - 1]);
                    let model = RadioDatabase::by_flex_id(&id_str);
                    info!(
                        "Identified {} via FlexRadio protocol",
                        model
                            .as_ref()
                            .map(|m| m.model.as_str())
                            .unwrap_or("FlexRadio")
                    );
                    return Some(ProbeResult {
                        protocol: Protocol::FlexRadio,
                        model,
                        id_data: response.to_vec(),
                        address: None,
                    });
                }

                // Check for Yaesu ASCII (ID0570, ID0670, ID0681, etc. - 4-digit IDs)
                if yaesu_ascii::is_valid_id_response(response) {
                    let id_str = String::from_utf8_lossy(&response[2..response.len() - 1]);
                    let model = RadioDatabase::by_yaesu_ascii_id(&id_str);
                    info!(
                        "Identified {} via Yaesu ASCII protocol",
                        model.as_ref().map(|m| m.model.as_str()).unwrap_or("Yaesu")
                    );
                    return Some(ProbeResult {
                        protocol: Protocol::YaesuAscii,
                        model,
                        id_data: response.to_vec(),
                        address: None,
                    });
                }

                // Check for standard Kenwood (ID019, ID021, etc. - 3-digit IDs)
                if kenwood::is_valid_id_response(response) {
                    let id_str = String::from_utf8_lossy(&response[2..response.len() - 1]);
                    let model = RadioDatabase::by_kenwood_id(&id_str);
                    info!(
                        "Identified {} via Kenwood protocol",
                        model
                            .as_ref()
                            .map(|m| m.model.as_str())
                            .unwrap_or("Kenwood")
                    );
                    return Some(ProbeResult {
                        protocol: Protocol::Kenwood,
                        model,
                        id_data: response.to_vec(),
                        address: None,
                    });
                }
            }
            Ok(Ok(_)) => trace!("No response to ID probe"),
            Ok(Err(e)) => trace!("ID read error: {}", e),
            Err(_) => trace!("ID probe timeout"),
        }

        None
    }

    /// Probe for Icom CI-V radios
    async fn probe_icom<S>(&self, stream: &mut S) -> Option<ProbeResult>
    where
        S: AsyncRead + AsyncWrite + Unpin,
    {
        debug!("Probing for Icom CI-V...");

        // Try common Icom addresses
        let addresses = [
            0x94, // IC-7300
            0xA4, // IC-705
            0x98, // IC-7610
            0x70, // IC-7000
            0x76, // IC-7200
            0x88, // IC-7100
            0x7C, // IC-7600
        ];

        for addr in addresses {
            if let Some(result) = self.try_icom_address(stream, addr).await {
                return Some(result);
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }

        None
    }

    /// Try a specific Icom CI-V address
    async fn try_icom_address<S>(&self, stream: &mut S, addr: u8) -> Option<ProbeResult>
    where
        S: AsyncRead + AsyncWrite + Unpin,
    {
        let probe = icom::probe_command(addr);
        trace!("Sending CI-V probe to 0x{:02X}", addr);

        if let Err(e) = stream.write_all(&probe).await {
            warn!("Failed to write CI-V probe: {}", e);
            return None;
        }

        let mut buf = [0u8; 64];
        match timeout(self.config.timeout, stream.read(&mut buf)).await {
            Ok(Ok(n)) if n > 0 => {
                let response = &buf[..n];
                trace!("CI-V response: {:02X?}", response);

                if icom::is_valid_frame(response) {
                    let source_addr = icom::extract_source_address(response);
                    let model =
                        source_addr.and_then(cat_protocol::models::RadioDatabase::by_civ_address);
                    info!(
                        "Identified {} via Icom CI-V protocol (address 0x{:02X})",
                        model.as_ref().map(|m| m.model.as_str()).unwrap_or("Icom"),
                        source_addr.unwrap_or(0)
                    );
                    return Some(ProbeResult {
                        protocol: Protocol::IcomCIV,
                        model,
                        id_data: response.to_vec(),
                        address: source_addr,
                    });
                }
            }
            Ok(Ok(_)) => {}
            Ok(Err(e)) => trace!("CI-V read error: {}", e),
            Err(_) => {}
        }

        None
    }

    /// Probe for Yaesu radios
    async fn probe_yaesu<S>(&self, stream: &mut S) -> Option<ProbeResult>
    where
        S: AsyncRead + AsyncWrite + Unpin,
    {
        debug!("Probing for Yaesu...");

        let probe = yaesu::probe_command();
        trace!("Sending Yaesu probe");

        if let Err(e) = stream.write_all(&probe).await {
            warn!("Failed to write Yaesu probe: {}", e);
            return None;
        }

        // Yaesu radios return 5 bytes: 4 frequency + 1 mode
        let mut buf = [0u8; 5];
        match timeout(self.config.timeout, stream.read_exact(&mut buf)).await {
            Ok(Ok(_)) => {
                trace!("Yaesu response: {:02X?}", buf);

                // Basic validation: mode byte should be in valid range
                let mode = buf[4];
                if mode <= 0x0C {
                    info!("Identified Yaesu radio via binary protocol");
                    return Some(ProbeResult {
                        protocol: Protocol::Yaesu,
                        model: None, // Yaesu identification is harder
                        id_data: buf.to_vec(),
                        address: None,
                    });
                }
            }
            Ok(Err(e)) => trace!("Yaesu read error: {}", e),
            Err(_) => trace!("Yaesu probe timeout"),
        }

        None
    }
}

impl Default for RadioProber {
    fn default() -> Self {
        Self::new()
    }
}

/// Probe a specific port at a given baud rate
///
/// This is a convenience function for manual probing from the UI.
/// Returns the probe result if a radio is detected.
pub async fn probe_port(port_name: &str, baud_rate: u32) -> Option<ProbeResult> {
    use std::time::Duration;
    use tokio_serial::SerialPortBuilderExt;

    debug!("Probing {} at {} baud", port_name, baud_rate);

    let mut stream = match tokio_serial::new(port_name, baud_rate)
        .timeout(Duration::from_millis(100))
        .open_native_async()
    {
        Ok(s) => s,
        Err(e) => {
            warn!("Failed to open {}: {}", port_name, e);
            return None;
        }
    };

    // Give the port a moment to settle
    tokio::time::sleep(Duration::from_millis(50)).await;

    let prober = RadioProber::new();
    prober.probe(&mut stream).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_probe_config_default() {
        let config = ProbeConfig::default();
        assert_eq!(config.timeout, Duration::from_millis(500));
    }
}
