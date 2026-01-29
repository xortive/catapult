# Troubleshooting

Common issues and solutions for Catapult.

## Connection Issues

### Port not appearing in dropdown

**Symptoms:** Expected serial port not listed in the port dropdown

**Solutions:**
1. Unplug and replug the USB cable
2. Check if another application is using the port (close logging software, etc.)
3. On Linux, verify permissions: `sudo usermod -a -G dialout $USER` (logout/login after)
4. On macOS, check System Preferences > Security for blocked drivers
5. Try a different USB port or cable

### Radio not connecting

**Symptoms:** Port appears but radio doesn't connect or respond

**Solutions:**
1. Check USB cable is connected
2. Verify radio is powered on
3. Ensure correct protocol is selected (Catapult auto-suggests for known radios)
4. Verify baud rate matches radio's CAT settings
5. Try changing flow control (Settings > Flow Control):
   - Default is Hardware (RTS/CTS)
   - Try "None" if radio doesn't support hardware flow control
6. Check the connection indicator color (see below)

### Connection indicator shows yellow/red

**Symptoms:** Radio connected but indicator is yellow (unresponsive) or red (disconnected)

**Yellow (Unresponsive):**
- Radio hasn't sent data in 2+ seconds
- Check radio's CAT/AI settings are enabled
- Verify baud rate matches
- Try power cycling the radio

**Red (Disconnected):**
- Serial port error (cable disconnected, radio powered off)
- Catapult will auto-reconnect when port becomes available
- Check USB cable connection
- Verify radio is powered on

### Wrong protocol detected

**Symptoms:** Commands not recognized, garbled data

**Solutions:**
1. Manually select the correct protocol
2. Check radio's CAT settings match (baud rate, protocol mode)
3. For Icom, verify CI-V address matches

### Serial port permission denied (Linux)

**Symptoms:** Error opening port, permission denied

**Solution:**
```bash
sudo usermod -a -G dialout $USER
# Log out and back in
```

## Switching Issues

### Amplifier not following radio

**Symptoms:** Change frequency on radio, amplifier stays on old frequency

**Solutions:**
1. Verify amplifier is connected and in CAT mode
2. Check protocol matches amplifier's expected format
3. Enable Traffic Monitor to see what's being sent
4. Verify the radio is the active one (green dot)

### Wrong radio becomes active

**Symptoms:** Switching to unintended radio

**Solutions:**
1. Check switching mode (use Manual for full control)
2. In Automatic mode, any activity causes a switch
3. Use Frequency Triggered to ignore PTT

### Switching too fast / relay clicking

**Symptoms:** Rapid switching between radios

**Solutions:**
1. Increase lockout duration in settings
2. Use Frequency Triggered mode instead of Automatic
3. Check for noise/interference on serial connection

## Protocol Issues

### Icom CI-V address mismatch

**Symptoms:** No response from radio, commands ignored

**Solutions:**
1. Check radio's CI-V address in its menu
2. Match the address in Catapult's settings
3. Common addresses: 0x94 (IC-7300), 0x98 (IC-7610)

### Kenwood commands not recognized

**Symptoms:** Radio doesn't respond to commands

**Solutions:**
1. Verify baud rate matches radio setting
2. Check radio is in correct CAT mode
3. Some radios need CAT enabled in menu

### Yaesu frequency wrong

**Symptoms:** Frequency off by factor of 10 or 100

**Solutions:**
1. This is usually a BCD encoding issue
2. Verify you're using the correct Yaesu protocol variant (Binary for older radios, ASCII for FT-991/FTDX series)

### Radio not updating in real-time

**Symptoms:** Frequency/mode changes on radio don't appear in Catapult without manual refresh

**Solutions:**
1. Verify radio supports Auto-Information (AI) or Transceive mode
2. Check radio's CAT menu for AI/Transceive setting (should be enabled)
3. Some older radios don't support automatic updates
4. Enable Traffic Monitor to verify AI commands are being sent on connection
5. Try disconnecting and reconnecting the radio
6. Catapult sends AI2 heartbeat every second to Kenwood/Elecraft radios to maintain auto-info mode

### No incoming data despite connection

**Symptoms:** Radio shows as connected but no frequency/mode data appears, Traffic Monitor shows no incoming traffic

**Solutions:**
1. **Flow control mismatch** is the most common cause:
   - Go to Settings and change Flow Control to "None"
   - Some radios don't support hardware (RTS/CTS) flow control
2. Enable debug logging (`RUST_LOG=debug`) to see if bytes are arriving at serial port level
3. Verify the correct protocol is selected
4. Check baud rate matches radio's CAT settings

### Frequency display out of sync after rapid tuning

**Symptoms:** After spinning the VFO quickly, displayed frequency doesn't match radio

**Solutions:**
1. Wait a moment - Catapult auto-polls idle radios every 500ms to resync
2. Change frequency slightly to trigger an update
3. This is normal behavior when updates arrive faster than they can be processed

## GUI Issues

### Window doesn't appear

**Symptoms:** Application starts but no window

**Solutions:**
1. Check if window is off-screen (multi-monitor setups)
2. Delete config file to reset window position
3. On Linux, ensure X11/Wayland is working

### UI is slow / laggy

**Symptoms:** Delayed response to clicks

**Solutions:**
1. Close unused panels
2. Disable Traffic Monitor if not needed
3. Check CPU usage for other processes

## Simulation Issues

### Virtual port not appearing in dropdown

**Symptoms:** Created a virtual port in Settings but it doesn't appear in the port dropdown

**Solutions:**
1. Verify the virtual port was saved (check Settings > Virtual Ports list)
2. Close and reopen the port dropdown to refresh
3. Ensure you gave the virtual port a unique name

### Simulation panel not visible

**Symptoms:** Can't find simulation controls

**Solutions:**
1. Enable Debug Mode in Settings
2. Simulation panel appears at bottom of window
3. Note: You must have at least one virtual radio added to see its controls

### Virtual radio not triggering switch

**Symptoms:** Changing virtual radio frequency doesn't switch active

**Solutions:**
1. Verify switching mode is Automatic or Frequency Triggered
2. Check that the virtual radio was registered (appears in Radios panel)
3. Virtual radios follow the same switching rules as real radios, including the 100ms settle delay

## Getting Help

If you're still stuck:

1. Enable verbose logging: `RUST_LOG=debug cargo run`
2. Check the Traffic Monitor for command/response issues
3. Open an issue on GitHub with:
   - Steps to reproduce
   - Radio/amplifier models
   - Log output
