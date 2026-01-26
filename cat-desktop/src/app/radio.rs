//! Radio management - COM and virtual radio handling

use std::time::Duration;

use cat_detect::{probe_port, ProbeResult, RadioProber};
use cat_mux::{
    AsyncRadioConnection, MuxActorCommand, MuxEvent, RadioChannelMeta, RadioHandle,
    RadioTaskCommand,
};
use cat_protocol::Protocol;
use cat_sim::{run_virtual_radio_task, VirtualRadio};
use tokio::sync::{mpsc as tokio_mpsc, oneshot};

use crate::radio_panel::RadioPanel;

use super::{BackgroundMessage, CatapultApp, ComRadioConfig, VirtualRadioCommand};

/// Probe a virtual port by creating a temporary virtual radio and probing it
///
/// This creates a temporary VirtualRadio with the given protocol, connects to it
/// via a duplex stream, probes it to detect the protocol, then tears everything down.
async fn probe_virtual_port(protocol: Protocol) -> Option<ProbeResult> {
    let radio = VirtualRadio::new("probe-temp", protocol);
    let (mut probe_stream, radio_stream) = tokio::io::duplex(1024);
    let (_cmd_tx, cmd_rx) = tokio_mpsc::channel(1);

    let task = tokio::spawn(run_virtual_radio_task(radio_stream, radio, cmd_rx));

    // Give the virtual radio task time to start
    tokio::time::sleep(Duration::from_millis(20)).await;

    let prober = RadioProber::new();
    let result = prober.probe(&mut probe_stream).await;

    // Clean up
    drop(probe_stream);
    task.abort();

    result
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
        self.pending_radio_configs.insert(correlation_id, config);

        // Store the panel index for when the handle arrives
        self.pending_registrations
            .insert(correlation_id, panel_index);

        correlation_id
    }

    /// Spawn an async task for a radio connection
    #[allow(clippy::too_many_arguments)]
    pub(super) fn spawn_radio_task(
        &mut self,
        handle: RadioHandle,
        port: String,
        baud_rate: u32,
        protocol: Protocol,
        civ_address: Option<u8>,
        model_name: String,
        query_initial_state: bool,
    ) {
        let bg_tx = self.bg_tx.clone();
        let mux_tx = self.mux_cmd_tx.clone();
        let event_tx = self.mux_event_tx.clone();
        let rt = self.rt_handle.clone();

        // Create channel for sending commands to the task
        let (cmd_tx, cmd_rx) = tokio_mpsc::channel::<RadioTaskCommand>(32);

        // Store the sender so we can send commands to this radio (keyed by handle)
        self.radio_task_senders
            .insert(handle.0, (port.clone(), cmd_tx));

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
                Ok(mut conn) => {
                    // Set CI-V address for Icom radios
                    if let Some(civ_addr) = civ_address {
                        conn.set_civ_address(civ_addr);
                    }

                    // Small delay to let the radio settle after port open
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

                    // Query radio ID to get actual model name
                    let actual_model_name = conn.query_id().await.unwrap_or(model_name);

                    // Query initial state if requested
                    if query_initial_state {
                        if let Err(e) = conn.query_initial_state().await {
                            tracing::warn!("Failed to query initial state on {}: {}", port, e);
                        }
                    }

                    // Try to enable auto-info mode
                    if let Err(e) = conn.enable_auto_info().await {
                        tracing::warn!("Failed to enable auto-info on {}: {}", port, e);
                        let _ = event_tx
                            .send(MuxEvent::Error {
                                source: format!("Radio {}", port),
                                message:
                                    "Auto-info not enabled - radio won't send automatic updates"
                                        .to_string(),
                            })
                            .await;
                    }

                    // Notify UI of successful connection
                    let _ = bg_tx.send(BackgroundMessage::RadioConnected {
                        handle,
                        model: actual_model_name,
                        port: port.clone(),
                    });

                    // Start read loop (runs until error or shutdown)
                    conn.run_read_loop(cmd_rx).await;
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
                query_initial_state: false,
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
    /// Takes ownership of the VirtualRadio, creates duplex stream, spawns actor,
    /// and registers with mux actor.
    fn add_virtual_radio_internal(&mut self, radio: VirtualRadio) -> String {
        let sim_id = format!("sim-{}", self.next_sim_id);
        self.next_sim_id += 1;

        let name = radio.id().to_string();
        let protocol = radio.protocol();
        let model_name = radio.model_name().to_string();
        let civ_address = radio.civ_address();

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

        // Create duplex stream pair for communication
        // connection_stream -> AsyncRadioConnection
        // radio_stream -> virtual radio actor task
        let (connection_stream, radio_stream) = tokio::io::duplex(1024);

        // Create UI command channel for SimulationPanel -> actor
        let (ui_cmd_tx, ui_cmd_rx) = tokio_mpsc::channel::<VirtualRadioCommand>(32);

        // Create channel for task control commands (shutdown)
        let (task_cmd_tx, task_cmd_rx) = tokio_mpsc::channel::<RadioTaskCommand>(32);

        // Store the task shutdown sender
        self.virtual_radio_task_senders
            .insert(sim_id.clone(), task_cmd_tx);

        // Register with SimulationPanel for UI display and commands
        self.simulation_panel
            .register_radio(sim_id.clone(), name.clone(), protocol, ui_cmd_tx);

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

        // Spawn the virtual radio actor task
        self.rt_handle.spawn(async move {
            if let Err(e) = run_virtual_radio_task(radio_stream, radio, ui_cmd_rx).await {
                tracing::warn!("Virtual radio actor task error: {}", e);
            }
        });

        // Spawn a task to await the handle and run AsyncRadioConnection
        let bg_tx = self.bg_tx.clone();
        let mux_tx = self.mux_cmd_tx.clone();
        let event_tx = self.mux_event_tx.clone();
        let sim_id_clone = sim_id.clone();
        self.rt_handle.spawn(async move {
            if let Ok(handle) = resp_rx.await {
                // Notify UI of registration
                let _ = bg_tx.send(BackgroundMessage::RadioRegistered {
                    correlation_id,
                    handle,
                });

                // Create the AsyncRadioConnection with the connection stream
                let mut conn = AsyncRadioConnection::new(
                    handle,
                    sim_id_clone.clone(),
                    connection_stream,
                    protocol,
                    event_tx,
                    mux_tx,
                );

                // Set CI-V address for Icom radios
                if let Some(civ_addr) = civ_address {
                    conn.set_civ_address(civ_addr);
                }

                // Small delay to let the virtual radio actor settle
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;

                // Query radio ID to get actual model name
                let actual_model_name = conn.query_id().await.unwrap_or(model_name);

                // Query initial state
                if let Err(e) = conn.query_initial_state().await {
                    tracing::warn!("Failed to query initial state on {}: {}", sim_id_clone, e);
                }

                // Enable auto-info mode
                if let Err(e) = conn.enable_auto_info().await {
                    tracing::warn!("Failed to enable auto-info on {}: {}", sim_id_clone, e);
                }

                // Notify UI of successful connection
                let _ = bg_tx.send(BackgroundMessage::RadioConnected {
                    handle,
                    model: actual_model_name,
                    port: format!("Virtual ({})", sim_id_clone),
                });

                // Start read loop (runs until error or shutdown)
                conn.run_read_loop(task_cmd_rx).await;
            }
        });

        self.set_status(format!("Virtual radio added: {}", sim_id));
        self.save_virtual_radios();

        sim_id
    }

    /// Remove a virtual radio - sends shutdown, unregisters from mux
    pub(super) fn remove_virtual_radio(&mut self, sim_id: &str) {
        // Shutdown the virtual radio task
        if let Some(task_tx) = self.virtual_radio_task_senders.remove(sim_id) {
            let _ = task_tx.try_send(RadioTaskCommand::Shutdown);
        }

        // Unregister from SimulationPanel
        self.simulation_panel.unregister_radio(sim_id);

        // Get the handle from the panel and unregister from mux actor
        if let Some(panel) = self
            .radio_panels
            .iter()
            .find(|p| p.sim_id() == Some(sim_id))
        {
            if let Some(handle) = panel.handle {
                self.send_mux_command(
                    MuxActorCommand::UnregisterRadio { handle },
                    "UnregisterRadio (virtual)",
                );
            }
        }

        // Remove sim_id mapping
        self.sim_radio_ids.remove(sim_id);

        // Remove from radio_panels
        self.radio_panels.retain(|p| p.sim_id() != Some(sim_id));

        self.set_status(format!("Virtual radio removed: {}", sim_id));
        self.save_virtual_radios();
    }

    /// Probe the selected port for radio detection
    pub(super) fn probe_selected_port(&mut self) {
        if self.add_radio_port.is_empty() || self.probing {
            return;
        }

        self.probing = true;
        self.set_status(format!("Probing {}...", self.add_radio_port));

        let port = self.add_radio_port.clone();
        let baud_rate = self.add_radio_baud;
        let tx = self.bg_tx.clone();
        let rt_handle = self.rt_handle.clone();

        // Check if this is a virtual port (VSIM:name format)
        if let Some(name) = port.strip_prefix("VSIM:") {
            // Find the virtual port config to get its protocol
            if let Some(vport) = self
                .settings
                .virtual_ports
                .iter()
                .find(|v| v.name == name)
                .cloned()
            {
                let protocol = vport.protocol;
                std::thread::spawn(move || {
                    let result = rt_handle.block_on(probe_virtual_port(protocol));
                    let _ = tx.send(BackgroundMessage::ProbeComplete {
                        port,
                        baud_rate: 0,
                        result,
                    });
                });
            } else {
                // Virtual port not found in config
                self.probing = false;
                self.report_warning("Probe", format!("Virtual port '{}' not found", name));
            }
        } else {
            // Real COM port - use existing probe_port logic
            std::thread::spawn(move || {
                let result = rt_handle.block_on(async { probe_port(&port, baud_rate).await });
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
            query_initial_state: true,
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
