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

In Frequency Triggered mode, switching only happens when a radio changes frequency. PTT alone does not cause a switch.

### Behavior
- Tune a radio to change the active selection
- PTT on an inactive radio does **not** switch (prevents accidental amplifier switching)
- Good for run/search-and-pounce alternation

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
