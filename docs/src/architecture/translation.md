# Protocol Translation

Catapult translates CAT commands between different protocols, enabling cross-vendor compatibility.

## Translation Pipeline

```
Radio Output     Parse to        Encode to       Amplifier Input
(bytes)     →   RadioCommand  →  Target Protocol  →  (bytes)
```

## RadioCommand Abstraction

All protocols are parsed into a common representation:

```rust
pub enum RadioCommand {
    SetFrequency { hz: u64 },
    SetMode { mode: OperatingMode },
    SetPtt { active: bool },
    QueryFrequency,
    QueryMode,
    StatusReport { frequency_hz: u64, mode: OperatingMode },
    // ...
}
```

## Translation Matrix

| Source | Target | Supported |
|--------|--------|-----------|
| Kenwood | Icom CI-V | Yes |
| Kenwood | Yaesu | Yes |
| Kenwood | Elecraft | Yes |
| Icom CI-V | Kenwood | Yes |
| Icom CI-V | Yaesu | Yes |
| Yaesu | Kenwood | Yes |
| Yaesu | Icom CI-V | Yes |
| Elecraft | Kenwood | Yes |
| Any | Any | Yes |

## What Gets Translated

### Frequency

All protocols represent frequency, but formats differ:

| Protocol | Format | Example (14.250 MHz) |
|----------|--------|---------------------|
| Kenwood | ASCII, 11 digits | `FA00014250000;` |
| Icom | BCD, 5 bytes, LSB first | `00 00 25 14 00` |
| Yaesu | BCD, 4 bytes, MSB first | `14 25 00 00` |

Translation handles format conversion.

### Mode

Mode values differ between protocols:

| Mode | Kenwood | Icom | Yaesu |
|------|---------|------|-------|
| LSB | 1 | 0x00 | 0x00 |
| USB | 2 | 0x01 | 0x01 |
| CW | 3 | 0x03 | 0x02 |
| AM | 5 | 0x02 | 0x04 |
| FM | 4 | 0x05 | 0x08 |

### PTT

PTT is straightforward - all protocols have TX on/off states.

## What's NOT Translated

- **Queries**: Not forwarded (amplifier doesn't need to respond)
- **Vendor-specific commands**: Passed through only if same protocol
- **Responses**: The multiplexer sends commands, doesn't forward responses

## Example Translation

Radio (Icom) sends frequency update:
```
FE FE 94 E0 00 00 00 25 14 00 FD
```

Catapult parses to:
```rust
RadioCommand::SetFrequency { hz: 14_250_000 }
```

Encodes to Kenwood for amplifier:
```
FA00014250000;
```

## Precision

Frequency translation maintains full precision:
- Internal representation: Hz (u64)
- No floating point - exact integer math
- Supports frequencies from 0 to 18 GHz+

## Error Handling

If translation fails:
- Invalid commands are logged
- No output sent to amplifier
- Radio continues to function
