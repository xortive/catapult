# Amplifier Integration

Catapult sends CAT commands to your amplifier to keep it synchronized with the active radio.

## Supported Amplifiers

Any amplifier that accepts CAT control should work. Tested with:

- **ACOM** - S-series (500S, 600S, 700S, 1200S, 1400S, 2020S)
- **Elecraft** - KPA500, KPA1500
- **SPE Expert** - 1.3K-FA, 1.5K-FA, 2K-FA
- **OM Power** - OM2000A+, OM2500A, OM3501A
- **Ameritron** - ALS-1306, ALS-606 (via serial CAT)

### Supported Protocols

- **Kenwood** - Most common, ASCII-based commands (FA, MD, TX/RX)
- **Icom CI-V** - Binary protocol with configurable address
- **Yaesu** - Binary 5-byte commands
- **Elecraft** - Kenwood-compatible with extensions

## Connecting Your Amplifier

### Serial Connection (Most Common)

1. Connect the amplifier's CAT input to a serial port
2. In the Amplifier panel, select:
   - **Protocol**: Match your amplifier's expected protocol
   - **Port**: The serial port connected to your amplifier
   - **Baud Rate**: Common rates are 4800, 9600, 19200, 38400, 115200
   - **CI-V Address**: (Icom only) The amplifier's CI-V address in hex
3. Click **Connect**

## Virtual Amplifier (Simulation)

For testing without physical amplifier hardware:

1. Set **Connection** to "Simulated"
2. Select the **Protocol** (Kenwood, Elecraft, or Icom CI-V)
3. Choose the **Mode**:
   - **Auto-Info** (recommended): Receives pushed state updates from the active radio
   - **Polling**: Queries the radio for frequency every 500ms
4. Click **Connect**

The simulated amplifier displays received frequency, mode, and PTT state in the Traffic Monitor.

### USB Connection

Some amplifiers (like the Elecraft KPA1500) have a built-in USB-to-serial adapter. Simply connect a standard USB cable from your computer to the amplifier's USB port. The amplifier appears as a serial port (COM port) on your computer.

### RS232 Connection

Many amplifiers (like ACOM S-series) use traditional RS232 serial ports. You'll need:

1. **USB-to-RS232 adapter** if your computer lacks a serial port (most modern computers)
2. **Null modem cable** between the adapter and the amplifier (crosses TX/RX lines)

**Note:** Some ACOM amplifiers require a specific null modem wiring—avoid "full" null modem cables that connect all pins. Use a simple 3-wire null modem (TX↔RX, RX↔TX, GND↔GND).

### Common Baud Rates by Amplifier

| Amplifier | Typical Baud Rate |
|-----------|-------------------|
| ACOM S-series | 9600 |
| Elecraft KPA1500 | 38400 or 230400 |
| SPE Expert | 9600 - 115200 |
| OM Power | 9600 |

## Protocol Translation

Catapult automatically translates between protocols. For example:
- Radio (Icom) sends CI-V frequency command
- Catapult translates to Kenwood format
- Amplifier receives Kenwood ASCII command

This means your Icom radio can control a Kenwood amplifier seamlessly.

## What Gets Sent to the Amplifier

When the active radio changes state, Catapult sends:

| Radio Action | Amplifier Command |
|-------------|-------------------|
| Frequency change | Set frequency |
| Mode change | Set mode |
| PTT on | Set TX state |
| PTT off | Set RX state |

## Amplifier Queries

When the amplifier queries for information, Catapult responds from its cached state:

| Amplifier Query | Response |
|----------------|----------|
| ID query (ID;) | ID022; (TS-990S identification) |
| Frequency query (FA;) | Current frequency from active radio |
| Mode query (MD;) | Current mode from active radio |
| Control Band (CB;) | VFO with front panel control (0=Main, 1=Sub) |
| Transmit Band (TB;) | VFO selected for TX (0=Main, 1=Sub) |

### Radio Identification

Catapult always identifies as a **Kenwood TS-990S** (ID022) to amplifiers, regardless of the actual connected radios. This ensures maximum compatibility with amplifiers expecting a high-end transceiver.

### VFO/Split Tracking

For radios with dual VFOs (like TS-990S, IC-7610), Catapult tracks which VFO is selected for receive (Control Band) and transmit (Transmit Band). This is critical for split operation where you receive on one VFO and transmit on another.

For radios that don't natively report CB/TB (most radios), Catapult infers this from VFO selection and split mode commands:

| Radio State | Control Band | Transmit Band |
|-------------|--------------|---------------|
| VFO A selected, no split | 0 (Main) | 0 (Main) |
| VFO B selected, no split | 1 (Sub) | 1 (Sub) |
| VFO A + Split mode | 0 (Main) | 1 (Sub) |
| VFO B + Split mode | 1 (Sub) | 0 (Main) |

## AI2 Heartbeat

Catapult sends `AI2;` (enable auto-information mode) to all connected Kenwood and Elecraft radios every second. This ensures:

- Radios continue sending automatic frequency/mode updates
- Recovery if a radio restarts or loses its auto-info setting
- Consistent behavior across reconnections

## Band Data vs CAT

Some amplifiers use band data (voltage levels or BCD) instead of CAT. Catapult currently only supports CAT control. For band data, you'll need a separate band decoder.

## Multiple Amplifiers

Currently, Catapult supports one amplifier output. For multiple amplifiers, you could use a serial port splitter, but ensure all amplifiers expect the same protocol.

## Troubleshooting

### Amplifier not following frequency
- Check the protocol matches what your amplifier expects
- Verify the serial connection (correct port, baud rate)
- Enable the Traffic Monitor to see what commands are being sent

### Wrong frequency displayed
- Some amplifiers expect frequency in different units
- Check your amplifier's CAT documentation

### Commands being ignored
- Ensure the amplifier is in "remote" or "CAT" mode
- Some amplifiers need CAT control enabled in their menus
