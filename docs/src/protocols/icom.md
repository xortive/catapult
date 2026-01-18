# Icom CI-V Protocol

CI-V (Communication Interface - V) is Icom's binary protocol used across their radio lineup.

## Overview

- **Format:** Binary frames
- **Start:** `0xFE 0xFE` (preamble)
- **End:** `0xFD` (end of message)
- **Addressing:** Source and destination addresses

## Frame Format

```
FE FE <to> <from> <cmd> [<sub>] [<data>...] FD
```

| Field | Size | Description |
|-------|------|-------------|
| Preamble | 2 bytes | Always `0xFE 0xFE` |
| To | 1 byte | Destination address |
| From | 1 byte | Source address |
| Command | 1 byte | Command code |
| Sub-command | 0-1 bytes | Optional sub-command |
| Data | 0+ bytes | Command-specific data |
| End | 1 byte | Always `0xFD` |

## Addresses

- `0x00` - Broadcast
- `0xE0` - Controller (computer)
- `0x94` - Default radio address (varies by model)

Check your radio's CI-V settings for its address.

## Common Commands

### Frequency (Command 0x00/0x05)

Set frequency uses BCD encoding, 5 bytes, least-significant first:

```
FE FE 94 E0 05 <freq-bcd> FD
```

Example for 14.250 MHz (14250000 Hz):
```
FE FE 94 E0 05 00 00 25 14 00 FD
```

### Mode (Command 0x06)

```
FE FE 94 E0 06 <mode> <filter> FD
```

Mode values:
- `0x00` - LSB
- `0x01` - USB
- `0x03` - CW
- `0x05` - FM
- `0x02` - AM

### PTT (Command 0x1C 0x00)

```
FE FE 94 E0 1C 00 <state> FD
```

State: `0x00` = RX, `0x01` = TX

### Transceive / Auto-Information (Command 0x1A 0x05)

Enable automatic updates from the radio:

```
FE FE 94 E0 1A 05 01 FD  (Enable transceive)
FE FE 94 E0 1A 05 00 FD  (Disable transceive)
```

When enabled, the radio sends frequency/mode changes automatically without polling. Catapult enables this on connection for real-time state tracking.

## Catapult Usage

### As Radio Protocol
Catapult parses CI-V frames and extracts:
- Frequency from BCD data
- Mode from mode command
- PTT state

### As Amplifier Protocol
Catapult encodes commands into CI-V format:
- Converts frequency to BCD
- Uses configured target address

### CI-V Address Configuration
Set the CI-V address in Catapult to match your radio's setting.

## Compatible Radios

- IC-7300, IC-7610, IC-7851
- IC-705, IC-905
- IC-9700
- Most modern Icom transceivers
