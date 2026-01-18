# Getting Started

This guide walks you through setting up Catapult for the first time.

## Requirements

- A computer running Windows, macOS, or Linux
- One or more radios with CAT control (USB or serial)
- An amplifier with CAT input (optional)

## Installation

### Download

Download the latest release for your platform from the releases page.

### Build from Source

If you prefer to build from source:

```bash
# Clone the repository
git clone https://github.com/your-org/catapult.git
cd catapult

# Build the desktop app
cargo build --release -p cat-desktop

# The binary will be at target/release/catapult
```

## First Launch

1. Launch Catapult
2. The main window shows three panels:
   - **Radios** - Connected radios and their status
   - **Amplifier** - Amplifier connection settings
   - **Switching** - Switching mode configuration

## Connecting Your First Radio

1. Connect your radio via USB
2. In the **Add Radio** section, select the serial port from the dropdown
3. Catapult will auto-suggest the protocol for known radios (Icom, Kenwood, FlexRadio, Yaesu)
4. Adjust the protocol and baud rate if needed
5. Click **Add Radio**

Your radio configuration is saved automatically and will restore on next launch.

The radio should appear in the Radios panel with its current frequency and mode.

## Connecting the Amplifier

1. In the Amplifier panel, select the serial port
2. Choose the amplifier's protocol
3. Click **Connect**

## Enabling Automatic Switching

1. In the Switching panel, select **Automatic** mode
2. The multiplexer will now switch to whichever radio changes frequency or keys up

## Testing with Simulation

If you don't have hardware connected yet, enable **Debug Mode** in settings to access the simulation panel. This lets you create virtual radios to test the switching logic.

See [Simulation Mode](./simulation/overview.md) for details.

## Next Steps

- [Connecting Radios](./connecting-radios.md) - Detailed radio connection guide
- [Switching Modes](./switching-modes.md) - Understanding the different switching modes
- [Troubleshooting](./troubleshooting.md) - Common issues and solutions
