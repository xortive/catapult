# Virtual Radios

Virtual radios simulate real radio behavior for testing purposes.

## Creating Virtual Radios

Virtual radios are created in two steps:

1. **Configure a virtual port in Settings:**
   - Open Settings
   - In the **Virtual Ports** section, enter a name and select a protocol
   - Click **Add** to create the virtual port

2. **Add the radio from the port dropdown:**
   - In the Add Radio section, select your virtual port from the dropdown
   - Virtual ports appear as "Name [SIM - Protocol]" (e.g., "IC-7300 [SIM - Icom CI-V]")
   - Click **Add Radio**

Each virtual radio gets a unique ID (sim-1, sim-2, etc.).

## Protocol Selection

Choose the protocol that matches what you want to test:

| Protocol | Output Format | Use Case |
|----------|---------------|----------|
| Kenwood | ASCII commands | Testing Kenwood-compatible amplifiers |
| Icom CI-V | Binary frames | Testing CI-V to other protocol translation |
| Yaesu | Binary 5-byte | Testing Yaesu integration |
| Elecraft | Extended ASCII | Testing K3/K4 specific features |

## Radio Controls

### Frequency

**Band Presets:** Quick buttons for common bands:
- 160m (1.9 MHz)
- 80m (3.75 MHz)
- 40m (7.15 MHz)
- 20m (14.25 MHz)
- 15m (21.25 MHz)
- 10m (28.5 MHz)
- And more...

**Direct Entry:** Type a frequency in Hz (e.g., "14250000") and click Set.

**Fine Tuning:** Buttons for ±100 Hz, ±1 kHz, ±10 kHz adjustments.

### Mode

Click mode buttons to change:
- LSB / USB (sideband)
- CW / CW-R (Morse)
- AM / FM (broadcast modes)
- DIG / RTTY (digital modes)

### PTT

Click **TX OFF** to simulate keying the transmitter. The button turns red and shows **TX ON**. Click again to release.

## Multiplexer Integration

Virtual radios are registered with the multiplexer just like real radios:

- They appear in the Radios panel (no visual distinction from real radios)
- They participate in automatic switching
- Their state changes trigger amplifier commands
- They follow the same 100ms settle delay before sending commands
- They use the same switching rules as real radios

This unified behavior means you can test switching logic with virtual radios and expect identical behavior when you connect real hardware.

## Managing Virtual Ports

Virtual ports are managed in Settings:

**Adding a Virtual Port:**
1. Open Settings
2. Scroll to the **Virtual Ports** section
3. Enter a name for the port (e.g., "IC-7300", "K3")
4. Select the protocol
5. Click **Add**

**Removing a Virtual Port:**
1. Open Settings
2. Find the port in the Virtual Ports list
3. Click **Remove**

Note: Removing a virtual port will disconnect any radio using that port.

## Removing Virtual Radios

Virtual radios are removed the same way as real radios: click the radio to expand it, then click **Disconnect**. The radio is unregistered from the multiplexer.

To remove the underlying virtual port, use Settings as described above.

## Limitations

Virtual radios don't simulate:
- Actual CAT responses (they only generate output)
- Timing or delays
- Hardware errors or disconnections

For testing CAT response handling, you'll need real hardware or a more sophisticated simulator.
