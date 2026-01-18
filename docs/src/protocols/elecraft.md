# Elecraft Protocol

Elecraft radios use an extended Kenwood protocol with additional commands.

## Overview

- **Format:** ASCII text (Kenwood-compatible)
- **Terminator:** Semicolon (`;`)
- **Extensions:** Additional K-series specific commands

## Kenwood Compatibility

Elecraft radios support standard Kenwood commands:
- `FA` / `FB` - Frequency
- `MD` - Mode
- `TX` / `RX` - PTT

See the [Kenwood Protocol](./kenwood.md) documentation for these commands.

## Elecraft Extensions

### K3/K4 Extended Commands

| Command | Description |
|---------|-------------|
| `K3` | Extended K3 mode |
| `AP` | Audio peaking |
| `BN` | Band select |
| `DS` | Display |
| `FW` | Filter bandwidth |
| `GT` | AGC time |
| `LN` | Link VFOs |
| `PA` | Pre-amp |
| `PC` | Power control |
| `RA` | Attenuator |

### Power Output

```
PC<power>;
```

Example: `PC050;` sets 50 watts.

### Identification

```
K3;
```

Response indicates extended command support.

## Catapult Usage

Catapult treats Elecraft as Kenwood-compatible:
- Standard frequency/mode/PTT commands work
- Extended commands are passed through if using Elecraft-to-Elecraft

### Protocol Selection

For best compatibility:
- Select **Elecraft** for Elecraft-specific features
- Select **Kenwood** if connecting to generic Kenwood-compatible devices

## Compatible Radios

- K3 / K3S
- K4 / K4D / K4HD
- KX2 / KX3
- P3 Panadapter

## Example Session

```
# Query K3 mode
K3;

# Response (extended mode enabled)
K31;

# Set frequency
FA00014074000;

# Set mode to USB
MD2;

# Set power to 100W
PC100;
```

## Tips

- Enable K3 extended mode for additional features
- The K4 has enhanced commands beyond K3
- KX2/KX3 support a subset of K3 commands
