//! Simulation panel for managing virtual radio command channels
//!
//! Manages command channels and state for virtual radios. The UI for adding
//! virtual radios has been moved to Settings (Virtual Ports), and simulation
//! controls are integrated in the radio panel (ui_panels.rs).

use std::collections::HashMap;

use cat_protocol::{OperatingMode, Protocol, ProtocolId, RadioDatabase, RadioModel};
use tokio::sync::mpsc;

use cat_sim::VirtualRadioCommand;

/// State of a virtual radio for display purposes
#[derive(Debug, Clone)]
pub struct VirtualRadioDisplayState {
    /// Display name
    pub name: String,
    /// Protocol used
    pub protocol: Protocol,
    /// Radio model (if known)
    pub model: Option<RadioModel>,
    /// Current frequency in Hz
    pub frequency_hz: u64,
    /// Current operating mode
    pub mode: OperatingMode,
    /// PTT active state
    pub ptt: bool,
}

impl VirtualRadioDisplayState {
    /// Create a new display state with default values
    pub fn new(name: String, protocol: Protocol) -> Self {
        Self {
            name,
            protocol,
            model: RadioDatabase::default_for_protocol(protocol),
            frequency_hz: 14_250_000, // 20m default
            mode: OperatingMode::Usb,
            ptt: false,
        }
    }
}

/// Simulation panel state - manages command channels to virtual radios
pub struct SimulationPanel {
    /// Display state for each virtual radio (keyed by sim_id)
    radio_states: HashMap<String, VirtualRadioDisplayState>,
    /// Command senders for each virtual radio (keyed by sim_id)
    radio_commands: HashMap<String, mpsc::Sender<VirtualRadioCommand>>,
}

impl Default for SimulationPanel {
    fn default() -> Self {
        Self::new()
    }
}

impl SimulationPanel {
    /// Create a new simulation panel
    pub fn new() -> Self {
        Self {
            radio_states: HashMap::new(),
            radio_commands: HashMap::new(),
        }
    }

    /// Register a virtual radio after it has been added by App
    ///
    /// Called by App::add_virtual_radio() after spawning the actor.
    pub fn register_radio(
        &mut self,
        sim_id: String,
        name: String,
        protocol: Protocol,
        cmd_tx: mpsc::Sender<VirtualRadioCommand>,
    ) {
        self.radio_states.insert(
            sim_id.clone(),
            VirtualRadioDisplayState::new(name, protocol),
        );
        self.radio_commands.insert(sim_id, cmd_tx);
    }

    /// Unregister a virtual radio
    ///
    /// Called by App::remove_virtual_radio().
    pub fn unregister_radio(&mut self, sim_id: &str) {
        self.radio_states.remove(sim_id);
        self.radio_commands.remove(sim_id);
    }

    /// Update a radio's display state from mux events
    pub fn update_radio_state(
        &mut self,
        sim_id: &str,
        frequency_hz: Option<u64>,
        mode: Option<OperatingMode>,
        ptt: Option<bool>,
    ) {
        if let Some(state) = self.radio_states.get_mut(sim_id) {
            if let Some(hz) = frequency_hz {
                state.frequency_hz = hz;
            }
            if let Some(m) = mode {
                state.mode = m;
            }
            if let Some(p) = ptt {
                state.ptt = p;
            }
        }
    }

    /// Update a radio's model
    pub fn update_radio_model(&mut self, sim_id: &str, model: Option<RadioModel>) {
        if let Some(state) = self.radio_states.get_mut(sim_id) {
            state.model = model;
        }
    }

    /// Get the number of registered virtual radios
    pub fn radio_count(&self) -> usize {
        self.radio_states.len()
    }

    /// Check if a sim_id is a registered virtual radio
    pub fn has_radio(&self, sim_id: &str) -> bool {
        self.radio_states.contains_key(sim_id)
    }

    /// Get radio configurations for saving to settings
    ///
    /// Returns an iterator of VirtualRadioConfig from the current display state.
    pub fn get_radio_configs(&self) -> impl Iterator<Item = cat_sim::VirtualRadioConfig> + '_ {
        self.radio_states
            .values()
            .map(|state| cat_sim::VirtualRadioConfig {
                id: state.name.clone(),
                protocol: state.protocol,
                model_name: state.model.as_ref().map(|m| m.model.clone()),
                initial_frequency_hz: state.frequency_hz,
                initial_mode: state.mode,
                civ_address: state.model.as_ref().and_then(|m| {
                    if let ProtocolId::CivAddress(addr) = &m.protocol_id {
                        Some(*addr)
                    } else {
                        None
                    }
                }),
            })
    }

    /// Send a command to a virtual radio
    ///
    /// This can be called from app.rs for the radio panel UI controls.
    pub fn send_command(&self, sim_id: &str, cmd: VirtualRadioCommand) {
        if let Some(tx) = self.radio_commands.get(sim_id) {
            let _ = tx.try_send(cmd);
        }
    }
}
