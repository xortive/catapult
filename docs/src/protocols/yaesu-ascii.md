# Yaesu ASCII Protocol

Modern Yaesu radios use an ASCII-based protocol similar to Kenwood, with semicolon-terminated commands.

## Overview

- **Format:** ASCII text commands
- **Terminator:** Semicolon (`;`)
- **Frequency:** 9 digits (1 Hz resolution)
- **Mode:** 2-digit format with hex codes for digital modes

## Command Format

Commands follow the pattern: `<CMD><params>;`

### Frequency (FA/FB)

9-digit frequency in Hz:

| Command | Description | Example |
|---------|-------------|---------|
| `FA` | Set/query VFO A frequency | `FA014250000;` (14.250 MHz) |
| `FB` | Set/query VFO B frequency | `FB007074000;` (7.074 MHz) |

Note: Yaesu ASCII uses 9 digits (1 Hz resolution), while Kenwood uses 11 digits.

### Mode (MD)

Format: `MD<receiver><mode>;` where receiver is `0` (main) or `1` (sub).

| Mode | Code | Mode | Code |
|------|------|------|------|
| LSB | 1 | RTTY-LSB | 6 |
| USB | 2 | CW-L | 7 |
| CW-U | 3 | DATA-LSB | 8 |
| FM | 4 | RTTY-USB | 9 |
| AM | 5 | DATA-FM | A |
| FM-N | B | DATA-USB | C |
| AM-N | D | C4FM | E |

Example: `MD02;` sets the main receiver to USB.

### Auto-Information (AI)

| Command | Description |
|---------|-------------|
| `AI;` | Query AI status |
| `AI0;` | Disable auto-info |
| `AI1;` | Enable auto-info |

When enabled, the radio sends unsolicited frequency/mode updates when parameters change. Catapult enables this on connection for real-time tracking.

### PTT (TX)

| Command | Description |
|---------|-------------|
| `TX;` or `TX1;` | Transmit on |
| `TX0;` | Transmit off |
| `TX2;` | Tune mode |

### Information Query (IF)

The `IF;` command returns comprehensive status including frequency, mode, VFO, and TX state.

## Radio Identification

| Model | ID Response |
|-------|-------------|
| FT-991 | ID0570; |
| FT-991A | ID0670; |
| FTDX-101D | ID0681; |
| FTDX-101MP | ID0682; |
| FTDX-10 | ID0761; |
| FT-710 | ID0800; |

Catapult uses the 4-digit ID response to auto-detect Yaesu ASCII radios.

## Catapult Usage

### As Radio Protocol
When connecting a Yaesu ASCII radio:
- Catapult parses incoming `FA`, `MD`, `TX` commands
- Auto-Information is enabled for real-time updates
- State changes trigger amplifier updates

### As Amplifier Protocol
When sending to a Yaesu ASCII-compatible amplifier:
- Frequency changes send `FA` commands (9-digit format)
- Mode changes send `MD` commands

## Compatible Radios

- FT-991 / FT-991A
- FTDX-101D / FTDX-101MP
- FTDX-10
- FT-710

## Differences from Yaesu Binary

| Feature | Yaesu ASCII | Yaesu Binary |
|---------|-------------|--------------|
| Format | ASCII text | Binary (5-byte) |
| Frequency | 9 digits in Hz | 4-byte BCD |
| Mode codes | Hex (1-E) | Binary (0x00-0x08) |
| Terminator | Semicolon | Fixed length |
| Digital modes | Yes (DATA-USB, C4FM) | Limited |

## Notes

- Some radios support both ASCII and binary protocols (check CAT menu)
- Baud rate is configurable (commonly 4800, 9600, or 38400)
- Catapult auto-detects Yaesu ASCII by the 4-digit ID response format
