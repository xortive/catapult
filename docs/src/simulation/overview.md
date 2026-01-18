# Simulation Mode

Simulation mode lets you test Catapult's functionality without physical hardware. This is useful for:

- Learning how Catapult works before connecting real equipment
- Testing switching behavior with virtual radios
- Debugging and development

## Enabling Simulation

1. Open **Settings**
2. Enable **Debug Mode**
3. A new **Simulation** panel appears in the main window

## What's Simulated

### Virtual Radios
Create virtual radios with any protocol. They behave like real radios:
- Have frequency, mode, and PTT state
- Generate proper CAT protocol output
- Integrate with the multiplexer for switching

### Traffic Monitor
See the CAT commands that would be sent, displayed in the Traffic Monitor with a `[SIM]` badge.

## What's NOT Simulated

- Actual serial port communication
- Real amplifier responses
- Hardware timing characteristics

## Using Simulation

### Add a Virtual Radio

1. In the Simulation panel, enter a name (e.g., "IC-7300")
2. Select the protocol (e.g., "Icom CI-V")
3. Click **+ Add**

The virtual radio appears in both:
- The Simulation panel (for control)
- The Radios panel in the sidebar (shows active status)

### Control a Virtual Radio

Click a virtual radio to expand its controls:
- **Band presets** - Quick frequency buttons (40m, 20m, etc.)
- **Frequency input** - Enter exact frequency
- **Tune buttons** - Fine-tune up/down
- **Mode buttons** - Select operating mode
- **TX button** - Simulate PTT

### Test Switching

1. Add two virtual radios
2. Set switching mode to **Automatic**
3. Change frequency on the non-active radio
4. Watch the green dot move - the radio becomes active

## Example: SO2R Test

1. Add "Radio 1" (Kenwood) at 14.250 MHz
2. Add "Radio 2" (Kenwood) at 7.150 MHz
3. Enable Automatic switching
4. Click TX on Radio 2
5. Observe Radio 2 becomes active and the amplifier would receive the 7.150 MHz command
