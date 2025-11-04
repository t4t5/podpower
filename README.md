# airpods-status

A simple, Unix-philosophy command-line tool to check AirPods battery status on Linux.

## Why This Exists

The Python implementation ([AirStatus](https://github.com/SleepyScribe/AirStatus)) works great but:
- Requires Python runtime + dependencies (bleak)
- Does more than one thing (continuous monitoring, fancy UI, file logging)
- Not following the Unix philosophy of "do one thing well"

This Rust implementation:
- ✅ Does one thing: outputs battery status
- ✅ Plain text output (scriptable)
- ✅ Single compiled binary (no runtime dependencies)
- ✅ Fast (~3 second scan time)

## How It Works

AirPods broadcast their battery information via Bluetooth Low Energy (BLE) advertising packets. Specifically, they include manufacturer-specific data in these broadcasts:

1. **Manufacturer ID**: `0x004c` (Apple Inc.)
2. **Data Length**: 27 bytes
3. **Data Structure**:
   - Byte 7: Model identifier
     - `0x0e` = AirPods Pro
     - `0x03` = AirPods 3
     - `0x0f` = AirPods 2
     - `0x02` = AirPods 1
     - `0x0a` = AirPods Max
   - Byte 10: Flip flag (determines left/right orientation)
   - Bytes 12-13: Left and right earbud battery (nibble encoded, may be flipped)
   - Byte 14: Charging status flags
   - Byte 15: Case battery (nibble encoded)

### Battery Encoding

The battery level is stored as a single hex digit (0-F):
- `10` (0xA) = 100%
- `0-9` = (value × 10) + 5%
- `15` (0xF) = Not available/unknown

### Why Scanning Is Needed

AirPods don't maintain a persistent BLE connection when idle - they just broadcast advertising packets periodically. This tool:
1. Scans for BLE devices for 3 seconds
2. Filters for Apple manufacturer data
3. Parses the 27-byte packet
4. Outputs the battery information

## Requirements

- Linux with BlueZ
- Bluetooth adapter (BLE capable)
- Rust toolchain (for building)

## Installation

```bash
# Build release binary
cd airpods-status-rs
cargo build --release

# Install to your local bin
ln -sf "$(pwd)/target/release/airpods-status" ~/dotfiles/bin/airpods-status
```

## Usage

```bash
# Plain text output (default)
$ airpods-status
AirPods Pro
Left: 85%
Right: 90%
Case: 45%

# JSON output (for scripting)
$ airpods-status --json
{
  "model": "AirPods Pro",
  "left": 85,
  "right": 90,
  "case": 45,
  "charging_left": false,
  "charging_right": false,
  "charging_case": false
}
```

## Exit Codes

- `0` - Success (AirPods found and data retrieved)
- `1` - AirPods not found or error occurred

## Integration Examples

### Waybar

```json
"custom/airpods": {
    "exec": "airpods-status --json",
    "return-type": "json",
    "interval": 30,
    "format": " {}%"
}
```

### Shell Script

```bash
#!/bin/bash
if battery=$(airpods-status 2>/dev/null); then
    echo "AirPods connected: $battery"
else
    echo "AirPods not found"
fi
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
- Try increasing `SCAN_DURATION_SECS` in `src/main.rs` if the scan is too short

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
- Various reverse engineering efforts of Apple's BLE protocol

## License

MIT
