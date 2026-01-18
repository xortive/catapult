# Catapult

**A CAT Protocol Multiplexer for Amateur Radio**

[![GitHub Release](https://img.shields.io/github/v/release/xortive/catapult?style=flat-square)](https://github.com/xortive/catapult/releases/latest)
[![License](https://img.shields.io/github/license/xortive/catapult?style=flat-square)](https://github.com/xortive/catapult/blob/main/LICENSE)

Catapult allows you to connect multiple radios to a single amplifier, automatically switching based on which radio is transmitting or changing frequency.

<a href="https://github.com/xortive/catapult/releases/latest" class="md-button md-button--primary">Download Latest Release</a>

## Features

- **Multi-Radio Support** - Connect 2+ radios and switch between them seamlessly
- **Automatic Switching** - Switches to the radio that's transmitting or tuning
- **Protocol Translation** - Translates between different CAT protocols (Kenwood, Icom, Yaesu, Elecraft, FlexRadio)
- **Simulation Mode** - Test your setup with virtual radios before connecting real hardware
- **Traffic Monitor** - Debug and inspect CAT protocol traffic in real-time

## Use Cases

### SO2R (Single Operator, Two Radios)
Run two radios in a contest, and Catapult automatically switches the amplifier to whichever radio you're transmitting on.

### Multi-Band Station
Have radios on different bands and let Catapult handle amplifier switching as you move between them.

### Mixed Fleet
Connect radios from different manufacturers - Catapult translates between protocols so your Kenwood amplifier works with your Icom radio.

## Quick Start

1. **Download** the latest release for your platform
2. **Connect** your radios via USB/serial
3. **Configure** the amplifier output port and protocol
4. **Enable** automatic switching mode

See the [Getting Started](./getting-started.md) guide for detailed instructions.

## How It Works

```
┌─────────┐     ┌─────────────┐     ┌───────────┐
│ Radio 1 │────▶│             │     │           │
└─────────┘     │  Catapult   │────▶│ Amplifier │
┌─────────┐     │ Multiplexer │     │           │
│ Radio 2 │────▶│             │     └───────────┘
└─────────┘     └─────────────┘
```

Catapult monitors CAT commands from all connected radios. When it detects activity (frequency change, PTT, mode change), it can automatically switch which radio controls the amplifier.

## Architecture

Catapult is built in Rust for reliability and performance:

- **cat-protocol** - Protocol parsers and encoders for all supported radios
- **cat-mux** - The multiplexer engine with switching logic
- **cat-detect** - Auto-detection of connected radios
- **cat-sim** - Simulation framework for testing
- **cat-desktop** - Cross-platform GUI application
