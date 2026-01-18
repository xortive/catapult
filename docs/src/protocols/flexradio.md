# FlexRadio Protocol

FlexRadio SDR transceivers are supported via SmartSDR's CAT serial port emulation, which provides Kenwood-compatible commands with FlexRadio extensions.

## Overview

- **Format:** ASCII, semicolon-terminated (Kenwood-compatible)
- **Connection:** Virtual serial port created by SmartSDR
- **Baud Rate:** Typically 9600 (configurable in SmartSDR)

## How It Works

Catapult does **not** connect directly to FlexRadio hardware via TCP. Instead, SmartSDR creates a virtual serial port that speaks Kenwood-compatible CAT:

```
┌─────────┐    Serial    ┌──────────┐    TCP     ┌─────────┐
│ Catapult │◄───────────►│ SmartSDR │◄──────────►│  Flex   │
│          │  Virtual    │   CAT    │   Native   │  Radio  │
└─────────┘   COM Port   └──────────┘    API     └─────────┘
```

## Setup

1. In SmartSDR, enable **CAT** under Settings
2. Configure the virtual serial port (e.g., COM3 on Windows, or use a pair via `socat` on Linux/Mac)
3. In Catapult, select the virtual port and choose **FlexRadio** or **Kenwood** protocol

## Command Format

FlexRadio's CAT emulation supports both standard Kenwood commands and extended ZZ commands:

### Standard Kenwood Commands

| Command | Description | Example |
|---------|-------------|---------|
| `FA` | VFO A frequency | `FA00014250000;` |
| `FB` | VFO B frequency | `FB00007074000;` |
| `MD` | Operating mode | `MD2;` (USB) |
| `TX` | Transmit | `TX;` or `TX1;` |
| `RX` | Receive | `RX;` |
| `IF` | Information/status | `IF...;` |
| `ID` | Radio identification | `ID;` |

### FlexRadio Extended Commands (ZZ prefix)

| Command | Description | Example |
|---------|-------------|---------|
| `ZZFA` | VFO A frequency (11-digit) | `ZZFA00014250000;` |
| `ZZFB` | VFO B frequency (11-digit) | `ZZFB00007074000;` |
| `ZZMD` | Mode (2-digit code) | `ZZMD01;` |
| `ZZTX` | Transmit control | `ZZTX1;` / `ZZTX0;` |
| `ZZIF` | Extended status | `ZZIF...;` |
| `ZZAI` | Auto-information mode | `ZZAI1;` (on) / `ZZAI0;` (off) |

## Mode Values

### Standard Kenwood (MD command)

| Mode | Value |
|------|-------|
| LSB | 1 |
| USB | 2 |
| CW | 3 |
| FM | 4 |
| AM | 5 |

### FlexRadio Extended (ZZMD command)

| Mode | Value |
|------|-------|
| LSB | 00 |
| USB | 01 |
| CW-L | 03 |
| CW-U | 04 |
| AM | 06 |
| DIGL | 07 |
| DIGU | 09 |
| FM | 05 |

## Radio Identification

FlexRadio responds to `ID;` with model-specific codes:

| Model | ID Response |
|-------|-------------|
| FLEX-6300 | 907 |
| FLEX-6400 | 908 |
| FLEX-6400M | 910 |
| FLEX-6500 | 905 |
| FLEX-6600 | 909 |
| FLEX-6600M | 911 |
| FLEX-6700 | 904 |
| FLEX-6700R | 906 |
| FLEX-8400 | 912 |
| FLEX-8600 | 913 |

## Protocol Selection

You can use either protocol in Catapult:

- **FlexRadio**: Understands both standard Kenwood and ZZ-extended commands
- **Kenwood**: Works fine if SmartSDR only sends standard commands

If unsure, select **FlexRadio** to handle all possible command formats.

## Troubleshooting

### Radio not detected
- Ensure SmartSDR CAT is enabled and the virtual port is created
- Check that no other application is using the CAT port

### Commands not working
- Verify SmartSDR is running and connected to the radio
- Try **Kenwood** protocol if **FlexRadio** doesn't work

### Frequency not updating
- SmartSDR CAT reports the active slice frequency
- Ensure the correct slice is selected in SmartSDR

## Notes

- SmartSDR must be running for CAT to work
- The virtual serial port is created by SmartSDR, not the radio itself
- Multiple CAT clients may conflict; close other CAT software if issues occur
- For direct network control, use SmartSDR or third-party software with native FlexRadio API support
