# Switching Modes

Catapult offers three switching modes to control how it selects the active radio.

## Automatic Mode

**Best for:** Casual operation, monitoring multiple bands

In Automatic mode, the active radio switches whenever:
- A radio starts transmitting (PTT)
- A radio changes frequency
- A radio changes mode

This is the most "hands-off" mode - Catapult follows your activity.

### Behavior
- PTT takes priority over frequency changes
- Rapid switching is prevented with a short lockout period
- The amplifier is always synced to the active radio

## Frequency Triggered Mode

**Best for:** SO2R contesting where you tune between QSOs

In Frequency Triggered mode, switching only happens when a radio *changes* frequency. PTT alone does not cause a switch.

### Behavior
- Tune a radio to change the active selection
- PTT on an inactive radio does **not** switch (prevents accidental amplifier switching)
- Good for run/search-and-pounce alternation
- Polling responses with unchanged frequencies do **not** trigger switching

### Use Case
You're running on Radio 1. You tune Radio 2 to find a multiplier. Catapult switches the amplifier to Radio 2. When you're done, tune Radio 1 and it switches back.

## Manual Mode

**Best for:** Full control, preventing accidental switches

In Manual mode, the active radio only changes when you explicitly click **Select**.

### Behavior
- No automatic switching whatsoever
- You must manually select which radio controls the amplifier
- PTT and frequency changes are ignored for switching purposes

### Use Case
You want to lock the amplifier to a specific radio regardless of what the other radios are doing.

## Auto-Information Mode

For automatic switching to work, Catapult needs to know when your radios change state. Rather than constantly polling each radio, Catapult enables **Auto-Information** (also called **Transceive** mode) on each connected radio at startup.

With auto-info enabled, your radio automatically sends status updates whenever:
- You tune the VFO
- You change modes
- You key the transmitter
- Other state changes occur

### Protocol Support

| Protocol | Command | Notes |
|----------|---------|-------|
| Kenwood | `AI1;` | Standard auto-info |
| Elecraft | `AI1;` | Kenwood-compatible |
| FlexRadio | `AI1;` | Kenwood-compatible |
| Yaesu ASCII | `AI1;` | Same as Kenwood |
| Icom CI-V | Transceive (0x1A) | Unsolicited updates |
| Yaesu Binary | N/A | No auto-info; polled on PTT |

This happens automatically when you connect a radio - no configuration needed.

## Switching Lockout

To prevent rapid switching (which could stress amplifier relays), Catapult implements a brief lockout period after each switch. During lockout:
- Further automatic switches are blocked
- Manual switches are still allowed
- A visual indicator shows lockout status

## Comparison Table

| Feature | Automatic | Frequency Triggered | Manual |
|---------|-----------|---------------------|--------|
| Switch on PTT | Yes | No | No |
| Switch on Frequency | Yes | Yes | No |
| Switch on Mode | Yes | No | No |
| Manual Select | Yes | Yes | Yes |
