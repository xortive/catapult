# Connecting Radios

Catapult supports connecting multiple radios simultaneously, each with potentially different protocols.

## Supported Connection Types

### USB Serial
Most modern radios use USB for CAT control. The radio appears as a virtual serial port (COM port on Windows, /dev/ttyUSB* on Linux, /dev/cu.* on macOS).

### RS-232 Serial
Older radios may require a traditional RS-232 serial connection. You may need a USB-to-serial adapter.

### CI-V Level Converters
Icom radios using CI-V may need a level converter. Many USB-CI-V cables include this.

## Auto-Detection

Click **Scan Ports** to enumerate available serial ports. Catapult will attempt to identify:
- Port name and path
- USB vendor/product ID (if USB)
- Suggested protocol based on known radio IDs

## Manual Configuration

If auto-detection doesn't identify your radio:

1. Select the serial port from the dropdown
2. Choose the correct protocol:
   - **Kenwood** - Kenwood, some Elecraft models
   - **Icom CI-V** - All Icom radios
   - **Yaesu** - Yaesu radios (older CAT protocol)
   - **Elecraft** - K3, K4, KX series

3. For Icom radios, you may need to set the CI-V address (default: 0x94)

## Multiple Radios

To add more radios:

1. Each radio needs its own serial port
2. Configure each with the appropriate protocol
3. All radios appear in the Radios panel

The **first radio connected** becomes the active radio by default.

## Radio Status Display

Each radio in the list shows:
- **Frequency** - Current VFO frequency in MHz
- **Mode** - Operating mode (USB, LSB, CW, etc.)
- **TX indicator** - Red "TX" when transmitting
- **Active indicator** - Green dot for the radio controlling the amplifier

## Switching Active Radio

In **Manual** mode, click **Select** on any radio to make it active.

In **Automatic** or **Frequency Triggered** modes, the active radio switches automatically based on activity.

## Disconnecting

Click the radio name to expand controls, then click **Disconnect** to remove it.
