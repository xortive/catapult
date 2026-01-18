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

1. Connect the amplifier's CAT input to a serial port
2. In the Amplifier panel, select:
   - **Protocol**: Match your amplifier's expected protocol
   - **Port**: The serial port connected to your amplifier
   - **Baud Rate**: Common rates are 4800, 9600, 19200, 38400, 115200
   - **CI-V Address**: (Icom only) The amplifier's CI-V address in hex
3. Click **Connect**

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
