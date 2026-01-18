# Yaesu Protocol

Yaesu uses a binary CAT protocol with fixed 5-byte commands.

## Overview

- **Format:** Binary, fixed 5 bytes per command
- **Byte Order:** Big-endian for frequency
- **No Terminator:** Fixed length means no end marker needed

## Command Format

```
<byte1> <byte2> <byte3> <byte4> <opcode>
```

The first 4 bytes contain parameters (frequency, etc.), and the 5th byte is the opcode.

## Common Commands

### Set Frequency (Opcode 0x01)

Frequency is BCD-encoded in the first 4 bytes:

```
<freq-bcd-4-bytes> 01
```

Example for 14.250 MHz:
```
14 25 00 00 01
```

### Read Frequency (Opcode 0x03)

Query current frequency:
```
00 00 00 00 03
```

Response contains frequency in BCD.

### Set Mode (Opcode 0x07)

```
<mode> 00 00 00 07
```

Mode values:
- `0x00` - LSB
- `0x01` - USB
- `0x02` - CW
- `0x03` - CW-R
- `0x04` - AM
- `0x08` - FM

### PTT (Opcode 0x08)

```
<state> 00 00 00 08
```

State: `0x00` = TX off, `0x01` = TX on

### Split/VFO (Opcode 0x01)

Various VFO control commands use opcode 0x01 with different parameters.

## Catapult Usage

### As Radio Protocol
Catapult parses 5-byte commands and extracts state changes.

### As Amplifier Protocol
Commands are encoded as 5-byte sequences.

## BCD Encoding

Yaesu uses a different BCD format than Icom:
- 4 bytes total
- Most significant digit first
- Represents frequency in 10 Hz steps

For 14.250000 MHz:
- 14250000 Hz ÷ 10 = 1425000
- BCD: `14 25 00 00`

## Compatible Radios

- FT-991 / FT-991A
- FT-DX10 / FT-DX101
- FT-710
- FT-450, FT-950
- FTDX-3000, FTDX-5000

## Notes

- Some newer Yaesu radios also support Kenwood protocol
- Check your radio's CAT settings for protocol selection
- Baud rate is configurable (commonly 4800 or 38400)
