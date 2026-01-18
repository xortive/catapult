# Protocol Overview

Catapult supports multiple CAT (Computer Aided Transceiver) protocols used by different radio manufacturers.

## Supported Protocols

| Protocol | Manufacturer | Format | Example Radios |
|----------|--------------|--------|----------------|
| [Kenwood](./kenwood.md) | Kenwood, Elecraft | ASCII | TS-590, TS-890, K3 |
| [Icom CI-V](./icom.md) | Icom | Binary | IC-7300, IC-7610, IC-705 |
| [Yaesu Binary](./yaesu.md) | Yaesu | Binary | FT-450, FT-950, FTDX-3000 |
| [Yaesu ASCII](./yaesu-ascii.md) | Yaesu | ASCII | FT-991, FTDX-101D, FTDX-10, FT-710 |
| [Elecraft](./elecraft.md) | Elecraft | Extended ASCII | K3, K4, KX2 |
| [FlexRadio](./flexradio.md) | FlexRadio | TCP/Text | FLEX-6400, FLEX-6600 |

## Protocol Translation

Catapult can translate between any supported protocols. This enables:

- Icom radio → Kenwood amplifier
- Yaesu radio → Icom amplifier
- Mixed radio fleet → single amplifier protocol

### How Translation Works

1. Radio sends CAT command in its native protocol
2. Catapult parses the command into an internal representation
3. The command is encoded into the target protocol
4. Encoded bytes are sent to the amplifier

### Supported Commands for Translation

| Command | Description | Translated? |
|---------|-------------|-------------|
| Set Frequency | Change VFO frequency | Yes |
| Set Mode | Change operating mode | Yes |
| Set PTT | Key/unkey transmitter | Yes |
| Query Frequency | Read current frequency | No (queries not forwarded) |
| Query Mode | Read current mode | No |

Query commands are not forwarded to the amplifier - only set commands that change state.

## Protocol Auto-Detection

When connecting a radio, Catapult can often detect the protocol by:

1. USB Vendor/Product ID (known radio models)
2. Initial handshake responses
3. User selection (fallback)

## Common Issues

### Baud Rate Mismatch
Each protocol may have a default baud rate. Ensure your radio and Catapult agree on the rate.

| Protocol | Common Baud Rates |
|----------|-------------------|
| Kenwood | 9600, 19200, 38400 |
| Icom CI-V | 9600, 19200 |
| Yaesu Binary | 4800, 9600, 38400 |
| Yaesu ASCII | 4800, 9600, 38400 |

### CI-V Address Conflicts
Icom radios use addresses on the CI-V bus. Ensure each radio has a unique address if multiple are on the same bus.
