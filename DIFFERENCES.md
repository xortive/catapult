# Virtual vs Real Radio Differences

This document summarizes how virtual radios (simulating COM ports) are treated differently from real radios throughout the Catapult codebase.

## Port Naming and Identification

**Virtual radios** use a `VSIM:` prefix for port names (e.g., `VSIM:sim-001`), while **real radios** use actual system port names (e.g., `/dev/ttyUSB0`, `COM3`).

Helper functions in `crates/cat-mux/src/channel.rs:9-25`:
- `is_virtual_port()` - checks if port name starts with `VSIM:`
- `virtual_port_name()` - creates virtual port name from sim ID
- `sim_id_from_port()` - extracts sim ID from port name

## I/O Backend

| Type | Backend | Location |
|------|---------|----------|
| Real radios | `SerialStream` from tokio_serial | `crates/cat-mux/src/async_radio.rs:52-76` |
| Virtual radios | `DuplexStream` from `tokio::io::duplex()` | `cat-desktop/src/app/radio.rs:252` |

Both implement `AsyncRead + AsyncWrite + Unpin + Send`, allowing unified handling after initialization via generic `AsyncRadioConnection<T>`.

## Registration and Task Spawning

### Real Radios (`cat-desktop/src/app/radio.rs:28-70`)
- `register_com_radio()` creates `RadioChannelMeta::new_real()`
- Opens serial port with `tokio_serial::new(port_name, baud_rate)`
- Spawns **one** async task via `spawn_radio_task()`

### Virtual Radios (`cat-desktop/src/app/radio.rs:201-347`)
- `add_virtual_radio()` creates `RadioChannelMeta::new_virtual()`
- Creates duplex stream pair for bidirectional communication
- Spawns **two** concurrent tasks:
  - Virtual radio actor task via `run_virtual_radio_task()`
  - AsyncRadioConnection task connecting to actor

### On RadioRegistered Event (`cat-desktop/src/app/events.rs:72-91`)
Control flow differs after mux actor confirms registration:

**Virtual radios** (line 76-78):
```rust
if let Some(sim_id) = panel.sim_id() {
    self.sim_radio_ids.insert(sim_id.to_string(), handle);
}
```
- Store handle in `sim_radio_ids` HashMap for lookup
- Task already running (spawned during `add_virtual_radio`)

**Real radios** (line 80-91):
```rust
if let Some(config) = self.pending_radio_configs.remove(&correlation_id) {
    self.spawn_radio_task(handle, config.port, ...);
}
```
- Spawn the radio task **after** registration confirmed
- Task not started until this point

## UI Differences

### Background Colors (`cat-desktop/src/app/ui_panels.rs:313-329`)
| State | Virtual | Real |
|-------|---------|------|
| PTT | RGB(80, 40, 20) - red-orange | RGB(80, 30, 30) - darker red |
| Active | RGB(60, 50, 30) | RGB(40, 60, 40) - greenish |
| Idle | RGB(40, 35, 25) | RGB(30, 30, 30) - dark |

### Badge Display (`cat-desktop/src/app/ui_panels.rs:340-346`)
- **Virtual**: Shows `[SIM]` badge with orange color
- **Real**: No badge

### Detail Display (`cat-desktop/src/app/ui_panels.rs:391-394`)
- **Virtual**: Shows protocol name (e.g., "Kenwood")
- **Real**: Shows port name (e.g., "/dev/ttyUSB0")

### Port Dropdown Data (`cat-desktop/src/app/ui_panels.rs:85-93`)
When building the port selection dropdown, `is_virtual()` is called to collect metadata:
```rust
(p.port_name(), p.display_label(), p.is_virtual(), p.virtual_protocol())
```
This enables auto-selecting protocol for virtual ports (line 136) since they have pre-configured protocols.

### Expanded Controls (`cat-desktop/src/app/ui_panels.rs:404-522`)
- **Virtual expanded**: Band presets, tune buttons (-10k/-1k/+1k/+10k), mode buttons, PTT toggle
- **Real expanded**: Only remove button

## Radio Removal Logic

### UI Removal Path (`cat-desktop/src/app/ui_panels.rs:541-583`)

**Virtual radio removal** (lines 555-559):
- Calls `remove_virtual_radio(&sim_id)`

**Real radio removal** (lines 560-581):
- Shutdown async task
- Send UnregisterRadio to mux actor
- Remove from panels
- Save configured radios to config

### Internal Virtual Removal (`cat-desktop/src/app/radio.rs:385-407`)

`remove_virtual_radio()` uses `sim_id()` to find panels:
```rust
.find(|p| p.sim_id() == Some(sim_id))  // line 389
// ...
self.radio_panels.retain(|p| p.sim_id() != Some(sim_id));  // line 403
```

Steps:
1. Find panel by sim_id
2. Unregister handle from mux actor
3. Remove from `sim_radio_ids` HashMap
4. Remove from `radio_panels`
5. Save virtual radio config

## Error Handling

### Radio Connection (`crates/cat-mux/src/async_radio.rs:326-342`)
- `WouldBlock` is **silently ignored** for virtual radios (expected behavior)
- `ConnectionAborted` is **expected** for virtual radios (duplex stream closure), logged as debug not error
- Other errors treated as fatal for both types

### Amplifier Connection (`crates/cat-mux/src/async_amp.rs:100-109`)
- Both `WouldBlock` AND `TimedOut` are **silently allowed** for virtual amplifiers
- Virtual amplifiers use non-blocking I/O semantics

## Amplifier-Specific Differences

### Connection Type Enum (`cat-desktop/src/app/mod.rs:42-48`)
```rust
pub enum AmplifierConnectionType {
    ComPort,      // Physical amplifier via serial port
    Simulated,    // Virtual amplifier (goes to traffic monitor)
}
```

### Connection Methods (`cat-desktop/src/app/amplifier.rs`)
| Aspect | Real (`connect_amplifier_com`) | Virtual (`connect_amplifier_virtual`) |
|--------|-------------------------------|--------------------------------------|
| Lines | 206-280 | 284-339 |
| I/O | `SerialStream` | `VirtualAmplifier` |
| Port name | Actual (e.g., `/dev/ttyUSB1`) | `[VIRTUAL]` |
| Baud rate | Configurable | 0 (unused) |
| Metadata | `AmplifierChannelMeta::new_real()` | `AmplifierChannelMeta::new_virtual()` |

### Operating Modes

Virtual amplifiers support two modes, selected at connection time:

| Mode | Behavior |
|------|----------|
| AutoInfo | Sends AI enable at startup, receives pushed updates |
| Polling | Sends FA query every 500ms |

The mode cannot be changed while connected.

### UI Controls (`cat-desktop/src/app/amplifier.rs:89-184`)
- **ComPort**: Shows port, baud rate, CI-V address dropdowns; connect/disconnect buttons
- **Simulated**: Read-only "Simulated" display in blue; "Commands appear in Traffic Monitor" message

### Protocol Support (`crates/cat-sim/src/amplifier.rs:81-89`)
Virtual amplifiers only support:
- Kenwood
- Elecraft
- IcomCIV

Unsupported protocols (Yaesu, YaesuAscii, FlexRadio) are silently skipped with error log.

## Traffic Monitor Source Tracking

### Radio Sources (`cat-desktop/src/traffic_monitor/ingest.rs:230-289`)
Different `TrafficSource` enum variants:
- `SimulatedRadio { id: String }` - for virtual radios
- `RealRadio { handle: RadioHandle, port: String }` - for real radios
- `ToSimulatedRadio { id: String }` - commands to virtual
- `ToRealRadio { handle: RadioHandle, port: String }` - commands to real

### Amplifier Sources (`cat-desktop/src/traffic_monitor/ingest.rs:351-398`)
- `SimulatedAmplifier` - for virtual amplifier
- `RealAmplifier { port: String }` - for real amplifier

Detection: `amp_is_virtual = self.amp_data_tx.is_none()` (no separate task for virtual amps)

## Port Enumeration (`cat-desktop/src/app/ports.rs:54-98`)

### Separate In-Use Tracking (`cat-desktop/src/app/ports.rs:57-78`)
Virtual and real ports are tracked separately for availability:

```rust
// Real ports in use (line 57)
let in_use: HashSet<String> = self.radio_panels.iter()
    .filter(|p| !p.is_virtual())
    .map(|p| p.port.clone())
    .collect();

// Virtual ports in use (line 73-78)
let virtual_in_use: HashSet<String> = self.radio_panels.iter()
    .filter(|p| p.is_virtual())
    .map(|p| p.port.clone())
    .collect();
```

Real ports are enumerated from `serialport::available_ports()`, while virtual ports come from `settings.virtual_ports`. Both are merged in `available_radio_ports()` returning `PortInfo` enum:

```rust
pub enum PortInfo {
    Real(SerialPortInfo),
    Virtual(VirtualPortConfig),
}
```

## Filtering Real Radios Only

Used in `cat-desktop/src/app/ports.rs:57` and `status.rs:85`:
```rust
.filter(|p| !p.is_virtual())
```

Applied when:
- Getting ports in use by radios
- Getting available amplifier ports (amplifiers can only use real COM ports)

## Metadata Differences

### RadioChannelMeta (`crates/cat-mux/src/channel.rs:44-81`)
| Method | Real | Virtual |
|--------|------|---------|
| Constructor | `new_real()` | `new_virtual()` |
| Port name | Actual port | `VSIM:{sim_id}` |
| CI-V address | Configured | None |

### RadioState (`crates/cat-mux/src/state.rs:44-83`)
| Field | Real | Virtual |
|-------|------|---------|
| `is_simulated` | `false` | `true` |
| `port` | Actual port | `"[SIM]"` |

### AmplifierChannelMeta (`crates/cat-mux/src/amplifier.rs:11-66`)
| Field | Real | Virtual |
|-------|------|---------|
| `amp_type` | `AmplifierType::Real` | `AmplifierType::Virtual` |
| `port_name` | `Some(...)` | `None` |
| `baud_rate` | Configured | `0` |

## SimulationPanel Updates

Only virtual radios update the SimulationPanel display state (`cat-desktop/src/app/events.rs:168-172`):
```rust
if let Some(sim_id) = panel.sim_id() {
    self.simulation_panel.update_radio_state(sim_id, freq, mode, ptt);
}
```

## Repaint Triggering (`cat-desktop/src/app/mod.rs:403-409`)

Virtual and COM radios are checked separately to determine if continuous repaint is needed:

```rust
let has_virtual_radios = self.radio_panels.iter().any(|p| p.is_virtual());
let has_com_radios = !self.radio_task_senders.is_empty();
let has_amplifier = self.amp_data_tx.is_some();

if has_virtual_radios || has_com_radios || has_amplifier {
    ctx.request_repaint();
}
```

Both trigger repaint, but detection mechanism differs:
- **Virtual radios**: Checked via `is_virtual()` on panels
- **COM radios**: Checked via `radio_task_senders` HashMap (only contains real radio handles)

## Summary Table

| Aspect | Real Radio | Virtual Radio |
|--------|-----------|---------------|
| Port Naming | `/dev/ttyUSB0`, `COM3` | `VSIM:sim-001` |
| I/O Backend | `SerialStream` | `DuplexStream` + Actor task |
| Registration | `new_real()` | `new_virtual()` |
| Task Count | 1 (AsyncRadioConnection) | 2 (Actor + AsyncRadioConnection) |
| On RadioRegistered | Spawn task | Store in `sim_radio_ids` |
| Handle Lookup | `radio_task_senders` | `sim_radio_ids` |
| UI Background | Green/Red tints | Orange/Brown tints |
| UI Badge | None | `[SIM]` orange badge |
| UI Detail | Port name | Protocol name |
| Expanded Controls | Remove button only | Band/Tune/Mode/PTT buttons |
| Port Protocol | Manual selection | Auto-selected from config |
| Removal | Task shutdown + config save | Direct unregister via sim_id |
| In-Use Tracking | Separate HashSet | Separate HashSet |
| SimulationPanel | Not updated | Updated on state change |
| Error Handling | Real errors fatal | WouldBlock/ConnectionAborted expected |

| Aspect | Real Amplifier | Virtual Amplifier |
|--------|---------------|------------------|
| Connection Type | `AmplifierConnectionType::ComPort` | `AmplifierConnectionType::Simulated` |
| I/O Backend | `SerialStream` | `VirtualAmplifier` |
| UI Controls | Port, Baud, Protocol selectors | Read-only "Simulated" display |
| Status | Connected/Disconnected buttons | "Simulated" text |
| Traffic Monitor | `RealAmplifier { port }` | `SimulatedAmplifier` |
| Error Handling | Real errors reported | WouldBlock/TimedOut tolerated |
| Port Name | Actual (e.g., `/dev/ttyUSB1`) | `[VIRTUAL]` |
| Protocol Support | All 6 protocols | Kenwood, Elecraft, IcomCIV only |
