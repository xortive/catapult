# Virtual Radios

Virtual radios simulate real radio behavior for testing purposes.

## Creating Virtual Radios

In the Simulation panel:

1. Enter a name (optional - defaults to "Radio N")
2. Select a protocol
3. Click **+ Add**

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

- They appear in the Radios panel with a `[SIM]` badge
- They participate in automatic switching
- Their state changes trigger amplifier commands

This means you can test switching logic with virtual radios before connecting real hardware.

## Removing Virtual Radios

Click a virtual radio to expand it, then click **Remove**. The radio is unregistered from the multiplexer.

## Limitations

Virtual radios don't simulate:
- Actual CAT responses (they only generate output)
- Timing or delays
- Hardware errors or disconnections

For testing CAT response handling, you'll need real hardware or a more sophisticated simulator.
