//! Radio management - COM and virtual radio handling

use std::time::Duration;

use cat_detect::{probe_port_with_protocol, ProbeResult, RadioProber};
use cat_mux::{
    AsyncRadioConnection, MuxActorCommand, MuxEvent, RadioChannelMeta, RadioHandle,
    RadioTaskCommand,
};
use cat_protocol::Protocol;
use cat_sim::{run_virtual_radio_task, VirtualRadio};
use tokio::sync::{mpsc as tokio_mpsc, oneshot};

use crate::radio_panel::RadioPanel;

use super::{BackgroundMessage, CatapultApp, ComRadioConfig, VirtualRadioCommand};

/// Configuration for connecting a radio (unified for COM and Virtual)
pub(crate) enum RadioConnectionConfig {
    /// Physical COM port radio
    Com(ComRadioConfig),
    /// Virtual/simulated radio
    Virtual {
        /// Simulation ID (e.g., "sim-1")
        sim_id: String,
        /// Virtual radio config (used to create VirtualRadio when spawning)
        config: cat_sim::VirtualRadioConfig,
    },
}

/// Probe a virtual port by creating a temporary virtual radio and probing it
///
/// This creates a temporary VirtualRadio with the given protocol, connects to it
/// via a duplex stream, probes it using the specified protocol, then tears everything down.
async fn probe_virtual_port(protocol: Protocol) -> Option<ProbeResult> {
    let radio = VirtualRadio::new("probe-temp", protocol);
    let (mut probe_stream, radio_stream) = tokio::io::duplex(1024);
    let (_cmd_tx, cmd_rx) = tokio_mpsc::channel(1);

    let task = tokio::spawn(run_virtual_radio_task(radio_stream, radio, cmd_rx));

    // Give the virtual radio task time to start
    tokio::time::sleep(Duration::from_millis(20)).await;

    let prober = RadioProber::new();
    let result = prober.probe_protocol(&mut probe_stream, protocol).await;

    // Clean up
    drop(probe_stream);
    task.abort();

    result
}

/// Run the post-connection setup and read loop for any radio connection
///
/// This function handles CI-V address configuration, initial settle delay,
/// model ID query, initial state query, auto-info enablement, and the read loop.
/// It's used by both COM and virtual radio connections to ensure consistent behavior.
async fn run_radio_connection<T>(
    mut conn: AsyncRadioConnection<T>,
    handle: RadioHandle,
    port_display: String,
    model_name: String,
    civ_address: Option<u8>,
    bg_tx: std::sync::mpsc::Sender<BackgroundMessage>,
    cmd_rx: tokio_mpsc::Receiver<RadioTaskCommand>,
) where
    T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send,
{
    // Set CI-V address for Icom radios
    if let Some(civ_addr) = civ_address {
        conn.set_civ_address(civ_addr);
    }

    // Small delay to let the radio settle
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Query radio ID to get actual model name
    let actual_model_name = conn.query_id().await.unwrap_or(model_name);

    // Query initial state
    if let Err(e) = conn.query_initial_state().await {
        tracing::warn!("Failed to query initial state on {}: {}", port_display, e);
    }

    // Enable auto-info mode
    if let Err(e) = conn.enable_auto_info().await {
        tracing::warn!("Failed to enable auto-info on {}: {}", port_display, e);
    }

    // Notify UI of successful connection
    let _ = bg_tx.send(BackgroundMessage::RadioConnected {
        handle,
        model: actual_model_name,
        port: port_display,
    });

    // Start read loop (runs until error or shutdown)
    conn.run_read_loop(cmd_rx).await;
}

impl CatapultApp {
    /// Allocate a new correlation_id for pending registrations
    pub(super) fn allocate_correlation_id(&mut self) -> u64 {
        let id = self.next_correlation_id;
        self.next_correlation_id += 1;
        id
    }

    /// Register a COM port radio with the mux actor
    /// Returns correlation_id - the RadioHandle will arrive via BackgroundMessage::RadioRegistered
    /// The async radio task is spawned when the handle is received
    /// Caller must store the panel index in pending_registrations with correlation_id as key
    pub(super) fn register_com_radio(&mut self, config: ComRadioConfig, panel_index: usize) -> u64 {
        // Allocate a correlation_id
        let correlation_id = self.allocate_correlation_id();

        // Create metadata for the radio channel
        let meta = RadioChannelMeta::new_real(
            config.model_name.clone(),
            config.port.clone(),
            config.protocol,
            config.civ_address,
        );

        // Create oneshot for receiving the handle
        let (resp_tx, resp_rx) = oneshot::channel();

        // Send RegisterRadio to mux actor
        self.send_mux_command(
            MuxActorCommand::RegisterRadio {
                meta,
                response: resp_tx,
            },
            "RegisterRadio",
        );

        // Spawn a task to await the handle and send it back via BackgroundMessage
        let bg_tx = self.bg_tx.clone();
        self.rt_handle.spawn(async move {
            if let Ok(handle) = resp_rx.await {
                let _ = bg_tx.send(BackgroundMessage::RadioRegistered {
                    correlation_id,
                    handle,
                });
            }
        });

        // Store the config so we can spawn the task when the handle arrives
        self.pending_radio_configs
            .insert(correlation_id, RadioConnectionConfig::Com(config));

        // Store the panel index for when the handle arrives
        self.pending_registrations
            .insert(correlation_id, panel_index);

        correlation_id
    }

    /// Spawn a radio connection task (unified for both COM and Virtual radios)
    ///
    /// This method handles both physical COM port radios and virtual radios,
    /// creating the appropriate I/O stream and running the common initialization.
    pub(super) fn spawn_radio_connection(
        &mut self,
        handle: RadioHandle,
        config: RadioConnectionConfig,
    ) {
        match config {
            RadioConnectionConfig::Com(com_config) => {
                self.spawn_com_radio_connection(handle, com_config);
            }
            RadioConnectionConfig::Virtual { sim_id, config } => {
                self.spawn_virtual_radio_connection(handle, sim_id, config);
            }
        }
    }

    /// Spawn connection task for a physical COM port radio
    fn spawn_com_radio_connection(&mut self, handle: RadioHandle, config: ComRadioConfig) {
        let bg_tx = self.bg_tx.clone();
        let mux_tx = self.mux_cmd_tx.clone();
        let event_tx = self.mux_event_tx.clone();
        let rt = self.rt_handle.clone();

        let port = config.port;
        let baud_rate = config.baud_rate;
        let protocol = config.protocol;
        let civ_address = config.civ_address;
        let model_name = config.model_name;

        // Create channel for sending commands to the task
        let (cmd_tx, cmd_rx) = tokio_mpsc::channel::<RadioTaskCommand>(32);

        // Store the sender so we can send commands to this radio
        self.radio_task_senders.insert(
            handle,
            super::RadioTaskSender {
                port_name: port.clone(),
                task_cmd_tx: cmd_tx,
                virtual_cmd_tx: None,
            },
        );

        // Spawn the async connection task
        rt.spawn(async move {
            match AsyncRadioConnection::connect(
                handle,
                &port,
                baud_rate,
                protocol,
                event_tx.clone(),
                mux_tx,
            ) {
                Ok(conn) => {
                    run_radio_connection(
                        conn,
                        handle,
                        port,
                        model_name,
                        civ_address,
                        bg_tx,
                        cmd_rx,
                    )
                    .await;
                }
                Err(e) => {
                    let _ = event_tx
                        .send(MuxEvent::Error {
                            source: format!("Radio {}", port),
                            message: format!("Connection failed: {}", e),
                        })
                        .await;
                    let _ = event_tx.send(MuxEvent::RadioDisconnected { handle }).await;
                }
            }
        });
    }

    /// Spawn connection task for a virtual/simulated radio
    fn spawn_virtual_radio_connection(
        &mut self,
        handle: RadioHandle,
        sim_id: String,
        config: cat_sim::VirtualRadioConfig,
    ) {
        let name = config.id.clone();
        let protocol = config.protocol;
        let civ_address = config.civ_address;

        // Create the VirtualRadio from config
        let radio = VirtualRadio::from_config(config);
        let model_name = radio.model_name().to_string();

        // Create duplex stream pair for communication
        let (connection_stream, radio_stream) = tokio::io::duplex(1024);

        // Create UI command channel for SimulationPanel -> actor
        let (ui_cmd_tx, ui_cmd_rx) = tokio_mpsc::channel::<VirtualRadioCommand>(32);

        // Create channel for task control commands (shutdown)
        let (task_cmd_tx, task_cmd_rx) = tokio_mpsc::channel::<RadioTaskCommand>(32);

        // Store the sender so we can send commands to this radio
        self.radio_task_senders.insert(
            handle,
            super::RadioTaskSender {
                port_name: sim_id.clone(),
                task_cmd_tx,
                virtual_cmd_tx: Some(ui_cmd_tx.clone()),
            },
        );

        // Register with SimulationPanel for UI display and commands
        self.simulation_panel
            .register_radio(sim_id.clone(), name.clone(), protocol, ui_cmd_tx);

        // Spawn the virtual radio actor task
        self.rt_handle.spawn(async move {
            if let Err(e) = run_virtual_radio_task(radio_stream, radio, ui_cmd_rx).await {
                tracing::warn!("Virtual radio actor task error: {}", e);
            }
        });

        // Spawn the AsyncRadioConnection task
        let bg_tx = self.bg_tx.clone();
        let mux_tx = self.mux_cmd_tx.clone();
        let event_tx = self.mux_event_tx.clone();
        let port_display = format!("Virtual ({})", sim_id);
        self.rt_handle.spawn(async move {
            let conn = AsyncRadioConnection::new(
                handle,
                sim_id,
                connection_stream,
                protocol,
                event_tx,
                mux_tx,
            );
            run_radio_connection(
                conn,
                handle,
                port_display,
                model_name,
                civ_address,
                bg_tx,
                task_cmd_rx,
            )
            .await;
        });
    }

    /// Restore configured COM radios from settings
    pub(super) fn restore_configured_radios(&mut self) {
        let available_ports: std::collections::HashSet<_> = self
            .available_ports
            .iter()
            .map(|p| p.port.clone())
            .collect();

        for config in self.settings.configured_radios.clone() {
            let port_available = available_ports.contains(&config.port);

            // Create ComRadioConfig
            let com_config = ComRadioConfig {
                port: config.port.clone(),
                protocol: config.protocol,
                baud_rate: config.baud_rate,
                civ_address: config.civ_address,
                model_name: config.model_name.clone(),
            };

            if port_available {
                // Create RadioPanel with no handle (will be updated when handle arrives)
                let panel = RadioPanel::new_from_config(None, &config);
                self.radio_panels.push(panel);
                let panel_index = self.radio_panels.len() - 1;

                // Register with mux actor (handle will arrive via RadioRegistered message)
                let _correlation_id = self.register_com_radio(com_config, panel_index);
            } else {
                // Port not available - create panel without registering
                let mut panel = RadioPanel::new_from_config(None, &config);
                panel.unavailable = true;
                self.radio_panels.push(panel);
                self.report_warning("Radio", format!("{} not available", config.port));
            }
        }
    }

    /// Add a new virtual radio - creates duplex stream, spawns actor, registers with mux
    ///
    /// Returns the sim_id for the new radio.
    pub(super) fn add_virtual_radio(&mut self, name: &str, protocol: Protocol) -> String {
        let radio = VirtualRadio::new(name, protocol);
        self.add_virtual_radio_internal(radio)
    }

    /// Add a virtual radio from configuration (used when restoring from settings)
    pub(super) fn add_virtual_radio_from_config(
        &mut self,
        config: cat_sim::VirtualRadioConfig,
    ) -> String {
        let radio = VirtualRadio::from_config(config);
        self.add_virtual_radio_internal(radio)
    }

    /// Internal implementation for adding a virtual radio
    ///
    /// Registers the radio with the mux actor and stores config for later spawning.
    /// The actual connection task is spawned when the handle arrives (via events.rs).
    fn add_virtual_radio_internal(&mut self, radio: VirtualRadio) -> String {
        let sim_id = format!("sim-{}", self.next_sim_id);
        self.next_sim_id += 1;

        let name = radio.id().to_string();
        let protocol = radio.protocol();

        // Create config for later spawning
        let virtual_config = cat_sim::VirtualRadioConfig {
            id: name.clone(),
            protocol,
            model_name: Some(radio.model_name().to_string()),
            initial_frequency_hz: radio.frequency_hz(),
            initial_mode: radio.mode(),
            civ_address: radio.civ_address(),
        };

        // Allocate a correlation_id for tracking the registration
        let correlation_id = self.allocate_correlation_id();

        // Create metadata for the virtual radio channel
        let meta = RadioChannelMeta::new_virtual(name.clone(), sim_id.clone(), protocol);

        // Create oneshot for receiving the handle
        let (resp_tx, resp_rx) = oneshot::channel();

        // Send RegisterRadio to mux actor
        self.send_mux_command(
            MuxActorCommand::RegisterRadio {
                meta,
                response: resp_tx,
            },
            "RegisterRadio (virtual)",
        );

        // Store the config so we can spawn the task when the handle arrives
        self.pending_radio_configs.insert(
            correlation_id,
            RadioConnectionConfig::Virtual {
                sim_id: sim_id.clone(),
                config: virtual_config,
            },
        );

        // Create a RadioPanel with no handle (will be updated when handle arrives)
        self.radio_panels.push(RadioPanel::new_virtual(
            None,
            name.clone(),
            protocol,
            sim_id.clone(),
        ));
        let panel_idx = self.radio_panels.len() - 1;

        // Store the pending registration
        self.pending_registrations.insert(correlation_id, panel_idx);

        // Spawn a task to await the handle and notify via BackgroundMessage
        let bg_tx = self.bg_tx.clone();
        self.rt_handle.spawn(async move {
            if let Ok(handle) = resp_rx.await {
                let _ = bg_tx.send(BackgroundMessage::RadioRegistered {
                    correlation_id,
                    handle,
                });
            }
        });

        self.set_status(format!("Virtual radio added: {}", sim_id));

        sim_id
    }

    /// Remove a radio by handle (unified for both COM and Virtual radios)
    ///
    /// This method handles shutdown, mux unregistration, and persistence for both
    /// physical COM radios and virtual/simulated radios.
    pub(super) fn remove_radio(&mut self, handle: RadioHandle) {
        // Find the panel to determine if it's virtual and get sim_id if applicable
        let panel_info = self
            .radio_panels
            .iter()
            .find(|p| p.handle == Some(handle))
            .map(|p| (p.is_virtual(), p.sim_id().map(String::from)));

        let Some((is_virtual, sim_id)) = panel_info else {
            tracing::warn!("remove_radio: no panel found for handle {:?}", handle);
            return;
        };

        // Shutdown the radio task via unified sender map
        if let Some(sender) = self.radio_task_senders.remove(&handle) {
            Self::send_radio_task_command(
                &sender.task_cmd_tx,
                RadioTaskCommand::Shutdown,
                "Shutdown",
            );
        }

        // Unregister from mux actor
        self.send_mux_command(
            MuxActorCommand::UnregisterRadio { handle },
            "UnregisterRadio",
        );

        // Handle type-specific cleanup
        if is_virtual {
            // Virtual radio: unregister from SimulationPanel
            if let Some(ref sim_id) = sim_id {
                self.simulation_panel.unregister_radio(sim_id);
            }
            // Remove from radio_panels
            self.radio_panels.retain(|p| p.handle != Some(handle));
            // Save virtual radios config
            self.save_virtual_radios();
            self.set_status(format!(
                "Virtual radio removed: {}",
                sim_id.unwrap_or_default()
            ));
        } else {
            // COM radio: remove from radio_panels
            self.radio_panels.retain(|p| p.handle != Some(handle));
            // Save configured radios
            self.save_configured_radios();
            self.set_status("Radio removed".to_string());
        }
    }

    /// Probe the selected port for radio model detection using the user-selected protocol
    pub(super) fn probe_selected_port(&mut self) {
        if self.add_radio_port.is_empty() || self.probing {
            return;
        }

        self.probing = true;
        self.set_status(format!(
            "Detecting model on {} using {} protocol...",
            self.add_radio_port,
            self.add_radio_protocol.name()
        ));

        let port = self.add_radio_port.clone();
        let baud_rate = self.add_radio_baud;
        let protocol = self.add_radio_protocol;
        let tx = self.bg_tx.clone();
        let rt_handle = self.rt_handle.clone();

        // Check if this is a virtual port (VSIM:name format)
        if port.starts_with("VSIM:") {
            // Virtual port - use the user-selected protocol for probing
            std::thread::spawn(move || {
                let result = rt_handle.block_on(probe_virtual_port(protocol));
                let _ = tx.send(BackgroundMessage::ProbeComplete {
                    port,
                    baud_rate: 0,
                    result,
                });
            });
        } else {
            // Real COM port - use the user-selected protocol
            std::thread::spawn(move || {
                let result = rt_handle
                    .block_on(async { probe_port_with_protocol(&port, baud_rate, protocol).await });
                let _ = tx.send(BackgroundMessage::ProbeComplete {
                    port,
                    baud_rate,
                    result,
                });
            });
        }
    }

    /// Add a new COM radio with the current add_radio_* settings
    pub(super) fn add_com_radio(&mut self) {
        if self.add_radio_port.is_empty() {
            return;
        }

        let civ_address = if self.add_radio_protocol == Protocol::IcomCIV {
            Some(self.add_radio_civ_address)
        } else {
            None
        };
        // Use detected model name if available, otherwise generate from protocol
        let model_name = if self.add_radio_model.is_empty() {
            format!("{} Radio", self.add_radio_protocol.name())
        } else {
            self.add_radio_model.clone()
        };

        let config = ComRadioConfig {
            port: self.add_radio_port.clone(),
            protocol: self.add_radio_protocol,
            baud_rate: self.add_radio_baud,
            civ_address,
            model_name: model_name.clone(),
        };

        // Create RadioPanel with no handle (will be updated when handle arrives)
        let panel = RadioPanel::new_com(
            None,
            model_name,
            self.add_radio_port.clone(),
            self.add_radio_protocol,
            self.add_radio_baud,
            civ_address,
        );
        self.radio_panels.push(panel);
        let panel_index = self.radio_panels.len() - 1;

        // Register with mux actor (handle will arrive via RadioRegistered)
        let _correlation_id = self.register_com_radio(config, panel_index);

        // If this port was selected as amp port, clear it
        if self.amp_port == self.add_radio_port {
            self.amp_port.clear();
            if self.amp_data_tx.is_some() {
                self.disconnect_amplifier();
            }
            self.save_amplifier_settings();
        }

        // Save to config
        self.save_configured_radios();

        // Clear the add_radio_port for next addition
        self.add_radio_port.clear();
    }

    /// Add a radio from the selected port (handles both real and virtual ports)
    ///
    /// Checks if the selected port is virtual (starts with "VSIM:") and either
    /// calls add_virtual_radio() or add_com_radio() accordingly.
    pub(super) fn add_radio_from_port(&mut self) {
        if self.add_radio_port.is_empty() {
            return;
        }

        // Check if this is a virtual port (VSIM:name format)
        if let Some(name) = self.add_radio_port.strip_prefix("VSIM:") {
            // Find the virtual port config by name
            if let Some(vport) = self
                .settings
                .virtual_ports
                .iter()
                .find(|v| v.name == name)
                .cloned()
            {
                // Add virtual radio with the configured protocol
                self.add_virtual_radio(&vport.name, vport.protocol);
                // Clear selection for next addition
                self.add_radio_port.clear();
            } else {
                self.report_warning("Radio", format!("Virtual port '{}' not found", name));
            }
        } else {
            // Real COM port - use existing add_com_radio logic
            self.add_com_radio();
        }
    }
}
