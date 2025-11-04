# podpower

A simple, Unix-philosophy command-line tool to check AirPods battery status on Linux.

## Why This Exists

The Python implementation ([AirStatus](https://github.com/SleepyScribe/AirStatus)) works pretty well but:
- Requires Python runtime + dependencies (bleak)
- Does more than one thing (continuous monitoring, fancy UI, file logging)

This Rust implementation:
- ✅ Does one thing: outputs battery status
- ✅ JSON output (composable with jq and other Unix tools)
- ✅ Single compiled binary (no runtime dependencies)
- ✅ Fast (~3 second scan time)

## How It Works

AirPods broadcast their battery information via Bluetooth Low Energy (BLE) advertising packets. Specifically, they include manufacturer-specific data in these broadcasts:

1. **Manufacturer ID**: `0x004c` (Apple Inc.)
2. **Data Length**: 27 bytes
3. **Data Structure**:
   - Bytes 3-4: Model identifier (2-byte identifier)
     - `0x0220` = AirPods 1
     - `0x0F20` = AirPods 2
     - `0x1320` = AirPods 3
     - `0x1920` = AirPods 4
     - `0x0E20` = AirPods Pro
     - `0x1420`/`0x2420` = AirPods Pro 2
     - `0x2720` = AirPods Pro 3
     - `0x0A20`/`0x1F20` = AirPods Max
   - Byte 5: Flip flag (bit 5 at 0x20 determines left/right orientation)
   - Byte 6: Left and right earbud battery (nibble encoded, may be flipped)
   - Byte 7: Case battery (low nibble) + Charging status flags (high nibble)

### Battery Encoding

The battery level is stored as a single hex digit (0-F):
- `10` (0xA) = 100%
- `0-9` = (value × 10) + 5%
- `15` (0xF) = Not available/unknown

### Why Scanning Is Needed

AirPods don't maintain a persistent BLE connection when idle - they just broadcast advertising packets periodically. This tool:
1. Scans for BLE devices for up to 3 seconds (polling every 100ms)
2. Filters for Apple manufacturer data (ID `0x004c`)
3. Validates the 27-byte packet length
4. Parses the packet and outputs the battery information
5. Stops scanning as soon as AirPods are found

## Requirements

- Linux with BlueZ
- Bluetooth adapter (BLE capable)
- Rust toolchain (for building)

## Installation

```bash
# Build release binary
cd podpower
cargo build --release
```

## Usage

```bash
# JSON output for in-ear AirPods (standard models and Pro)
$ podpower
{
  "type": "in_ear",
  "model": "AirPods Pro",
  "battery": 85,
  "components": [
    {
      "name": "left",
      "battery": 85,
      "charging": false
    },
    {
      "name": "right",
      "battery": 90,
      "charging": false
    },
    {
      "name": "case",
      "battery": 45,
      "charging": false
    }
  ]
}

# JSON output for AirPods Max (over-ear headphones)
$ podpower
{
  "type": "over_ear",
  "model": "AirPods Max",
  "battery": 95,
  "components": [
    {
      "name": "headphones",
      "battery": 95,
      "charging": false
    }
  ]
}

# Get the main battery level (works for all AirPods types)
$ podpower | jq '.battery'
85

# Pipe through jq for formatted output
$ podpower | jq -r '"\(.model): \(.battery)%"'
AirPods Pro: 85%

# Get individual component battery levels
$ podpower | jq '.components[] | select(.name=="left") | .battery'
85

# Custom format for in-ear with all components
$ podpower | jq -r '"\(.model): L=\(.components[] | select(.name=="left") | .battery)% R=\(.components[] | select(.name=="right") | .battery)% Case=\(.components[] | select(.name=="case") | .battery)%"'
AirPods Pro: L=85% R=90% Case=45%
```

## Exit Codes

- `0` - Success (AirPods found and data retrieved)
- `1` - AirPods not found or error occurred

## Integration Examples

### Waybar

```json
"custom/airpods": {
    "exec": "podpower | jq -r '.battery // empty'",
    "interval": 30,
    "format": " {}%"
}
```

Or with different icons based on type:

```json
"custom/airpods": {
    "exec": "podpower | jq -r 'if .type == \"in_ear\" then \"👂 \" + (.battery | tostring) + \"%\" elif .type == \"over_ear\" then \"🎧 \" + (.battery | tostring) + \"%\" else empty end'",
    "interval": 30
}
```

### Shell Script

Simple version (works for all types):

```bash
#!/bin/bash
if status=$(podpower); then
    battery=$(echo "$status" | jq -r '.battery // "?"')
    model=$(echo "$status" | jq -r '.model')
    echo "$model: $battery%"
else
    echo "AirPods not found"
fi
```

Detailed version (showing all components):

```bash
#!/bin/bash
if status=$(podpower); then
    model=$(echo "$status" | jq -r '.model')
    echo "$model:"
    echo "$status" | jq -r '.components[] | "  \(.name): \(.battery)%\(if .charging then " (charging)" else "" end)"'
else
    echo "AirPods not found"
fi
```

### i3status/i3blocks

```bash
#!/bin/bash
# ~/.config/i3blocks/airpods
podpower | jq -r 'if .type == "in_ear" then "👂 \(.battery)%" elif .type == "over_ear" then "🎧 \(.battery)%" else "" end' || echo ""
```

## Troubleshooting

### Permission Issues

If you get D-Bus permission errors, make sure your user is in the `bluetooth` group:

```bash
sudo usermod -aG bluetooth $USER
# Log out and back in
```

### AirPods Not Found

- Make sure AirPods are out of the case or the case is open
- Ensure they're in range and Bluetooth is enabled
- Try increasing `SCAN_TIMEOUT_SECS` in `src/main.rs` if the scan is too short (default is 3 seconds)

### Bluetooth Scan Already in Progress

If you see an error about "Bluetooth scan already in progress":
```bash
sudo systemctl restart bluetooth
```

### No Bluetooth Adapter

```bash
# Check if BlueZ is running
systemctl status bluetooth

# List adapters
bluetoothctl list
```

## Credits

Inspired by:
- [AirStatus](https://github.com/SleepyScribe/AirStatus) - Python implementation
- [cApod](https://github.com/d4rken-org/capod) - Android implementation with detailed model identifiers
- Various reverse engineering efforts of Apple's BLE protocol

## License

MIT
