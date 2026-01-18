# Kenwood Protocol

The Kenwood protocol is an ASCII-based CAT protocol used by Kenwood radios and many compatible devices.

## Overview

- **Format:** ASCII text commands
- **Terminator:** Semicolon (`;`)
- **Case:** Commands are typically uppercase

## Command Format

```
<command><parameters>;
```

Commands can be strung together:
```
FA00014250000;MD2;
```

## Common Commands

### Frequency

| Command | Description | Example |
|---------|-------------|---------|
| `FA` | Set/query VFO A frequency | `FA00014250000;` (14.250 MHz) |
| `FB` | Set/query VFO B frequency | `FB00007150000;` (7.150 MHz) |
| `IF` | Information query | Returns comprehensive status |

Frequency format: 11 digits in Hz, zero-padded.

### Mode

| Command | Description | Example |
|---------|-------------|---------|
| `MD` | Set/query mode | `MD2;` (USB) |

Mode values:
- `1` - LSB
- `2` - USB
- `3` - CW
- `4` - FM
- `5` - AM
- `6` - FSK (RTTY)
- `7` - CW-R
- `9` - FSK-R

### PTT

| Command | Description | Example |
|---------|-------------|---------|
| `TX` | Transmit | `TX;` |
| `RX` | Receive | `RX;` |

### Auto-Information (AI)

| Command | Description | Example |
|---------|-------------|---------|
| `AI` | Query/set auto-info | `AI;` (query), `AI0;` (off), `AI1;` (on) |

When enabled, the radio automatically sends frequency and mode updates without polling. Catapult enables AI on connection for real-time state tracking.

## Catapult Usage

### As Radio Protocol
When connecting a Kenwood radio:
- Catapult parses incoming `FA`, `MD`, `TX` commands
- State changes trigger amplifier updates

### As Amplifier Protocol
When sending to a Kenwood-compatible amplifier:
- Frequency changes send `FA` commands
- Mode changes send `MD` commands

## Compatible Radios

- Kenwood TS-590, TS-890, TS-990
- Elecraft K3, K4 (extended Kenwood)
- Some SDR software

## Example Session

```
Radio → Catapult: FA00014250000;
(Radio changed to 14.250 MHz)

Catapult → Amplifier: FA00014250000;
(Amplifier receives frequency command)

Radio → Catapult: MD2;
(Radio changed to USB)

Catapult → Amplifier: MD2;
(Amplifier receives mode command)
```
