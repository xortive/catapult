# Connecting Radios

Catapult supports connecting multiple radios simultaneously, each with potentially different protocols.

## Supported Connection Types

### USB Serial
Most modern radios use USB for CAT control. The radio appears as a virtual serial port (COM port on Windows, /dev/ttyUSB* on Linux, /dev/cu.* on macOS).

### RS-232 Serial
Older radios may require a traditional RS-232 serial connection. You may need a USB-to-serial adapter.

### CI-V Level Converters
Icom radios using CI-V may need a level converter. Many USB-CI-V cables include this.

## Adding a Radio

Open **Settings** to find the **Add Radio** section:
1. Select the serial port from the dropdown (virtual ports also appear here as "Name [SIM - Protocol]")
2. Catapult auto-suggests the protocol for known USB radio IDs (Icom, Kenwood, FlexRadio, Yaesu)
3. Adjust protocol and baud rate if the suggestion is incorrect
4. Click **Add Radio**

## Model Detection

After adding a radio, you can use the **Detect Model** button to identify the specific radio model. This sends a model identification query using the currently selected protocol.

The detected model helps Catapult optimize settings for your specific radio. If detection fails:
1. Verify the protocol matches your radio's CAT settings
2. Check that the baud rate is correct
3. Ensure the radio is powered on and ready

## Protocol Selection

Choose the correct protocol for your radio:
- **Kenwood** - Kenwood, some Elecraft models
- **Icom CI-V** - All Icom radios
- **Yaesu Binary** - Older Yaesu radios (FT-450, FT-950, FTDX-3000)
- **Yaesu ASCII** - Modern Yaesu radios (FT-991, FTDX-101D, FTDX-10, FT-710)
- **Elecraft** - K3, K4, KX series
- **FlexRadio** - FlexRadio SDRs via SmartSDR CAT

For Icom radios, you may need to set the CI-V address (default: 0x94).

## Virtual Ports

For testing without hardware, you can create virtual ports in Settings:

1. Open **Settings**
2. Scroll to the **Virtual Ports** section
3. Enter a name and select a protocol
4. Click **Add**

Virtual ports appear in the port dropdown alongside real serial ports. See [Virtual Radios](./simulation/virtual-radios.md) for more details.

## Persistent Configuration

Catapult automatically saves your radio configurations. When you restart:
- All configured radios are restored
- The application attempts to reconnect to each radio
- If a port is unavailable, the radio shows as disconnected

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
