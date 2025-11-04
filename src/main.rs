use btleplug::api::{Central, Manager as _, Peripheral as _, ScanFilter};
use btleplug::platform::Manager;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tokio::time::sleep;

const APPLE_MANUFACTURER_ID: u16 = 0x004c; // Apple Inc.
const AIRPODS_DATA_LENGTH: usize = 27;
const SCAN_TIMEOUT_SECS: u64 = 3;
const POLL_INTERVAL_MS: u64 = 100; // Check for new devices every 100ms

// Byte positions in the 27-byte manufacturer data
const BYTE_MODEL_HIGH: usize = 3;
const BYTE_MODEL_LOW: usize = 4;
const BYTE_FLIP: usize = 5;
const BYTE_BATTERY_PODS: usize = 6;
const BYTE_BATTERY_CASE_AND_CHARGING: usize = 7;

// Bit masks
const MASK_FLIP_BIT: u8 = 0x02;
const MASK_CHARGING_LEFT: u8 = 0x01;
const MASK_CHARGING_RIGHT: u8 = 0x02;
const MASK_CHARGING_CASE: u8 = 0x04;
const BATTERY_DISCONNECTED: u8 = 15;

/// Battery status for in-ear AirPods (standard AirPods and AirPods Pro)
#[derive(Debug, Serialize, Deserialize)]
struct InEarStatus {
    #[serde(skip_serializing_if = "Option::is_none")]
    left: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    right: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    case: Option<u8>,
    charging_left: bool,
    charging_right: bool,
    charging_case: bool,
}

/// Battery status for AirPods Max (over-ear headphones)
#[derive(Debug, Serialize, Deserialize)]
struct MaxStatus {
    battery: u8,
    charging: bool,
}

/// Main AirPods status with device-specific battery information
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum AirPodsStatus {
    InEar {
        model: String,
        #[serde(flatten)]
        status: InEarStatus,
    },
    OverEar {
        model: String,
        #[serde(flatten)]
        status: MaxStatus,
    },
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    match scan_for_airpods().await {
        Ok(Some(status)) => {
            println!("{}", serde_json::to_string_pretty(&status)?);
            Ok(())
        }
        Ok(None) => {
            eprintln!("{{\"error\": \"AirPods not found\"}}");
            std::process::exit(1);
        }
        Err(e) => {
            let error_json = serde_json::json!({"error": e.to_string()});
            eprintln!("{}", serde_json::to_string(&error_json)?);
            std::process::exit(1);
        }
    }
}

/// Extract the high nibble (4 bits) from a byte
#[inline]
fn high_nibble(byte: u8) -> u8 {
    (byte >> 4) & 0x0f
}

/// Extract the low nibble (4 bits) from a byte
#[inline]
fn low_nibble(byte: u8) -> u8 {
    byte & 0x0f
}

async fn scan_for_airpods() -> Result<Option<AirPodsStatus>, Box<dyn std::error::Error>> {
    let manager = Manager::new().await?;
    let adapters = manager.adapters().await?;

    let adapter = adapters
        .into_iter()
        .next()
        .ok_or("No Bluetooth adapters found")?;

    // Start scan, providing helpful error message if already in progress
    if let Err(e) = adapter.start_scan(ScanFilter::default()).await {
        if e.to_string().contains("already in progress") {
            return Err(
                "Bluetooth scan already in progress. Try: sudo systemctl restart bluetooth".into(),
            );
        }
        return Err(e.into());
    }

    // Wait for scan to start
    sleep(Duration::from_millis(200)).await;

    // Poll for AirPods up to SCAN_TIMEOUT_SECS seconds
    let start = std::time::Instant::now();
    let timeout = Duration::from_secs(SCAN_TIMEOUT_SECS);
    let poll_interval = Duration::from_millis(POLL_INTERVAL_MS);

    while start.elapsed() < timeout {
        let peripherals = adapter.peripherals().await?;

        for peripheral in peripherals {
            let properties = peripheral.properties().await?;

            if let Some(props) = properties {
                if let Some(data) = props.manufacturer_data.get(&APPLE_MANUFACTURER_ID) {
                    if data.len() == AIRPODS_DATA_LENGTH {
                        if let Some(status) = parse_airpods_data(data) {
                            adapter.stop_scan().await?;
                            return Ok(Some(status));
                        }
                    }
                }
            }
        }

        sleep(poll_interval).await;
    }

    adapter.stop_scan().await?;
    Ok(None)
}

/// Parse AirPods manufacturer data from BLE advertisement
///
/// # BLE Packet Structure (27 bytes)
/// Based on reverse engineering from OpenPods project:
/// - Byte 3-4: Device model identifier
/// - Byte 5: Flip bit (determines left/right orientation)
/// - Byte 6: Left and right pod battery levels (4 bits each)
/// - Byte 7: Case battery + charging status
///   - High nibble (bits 4-7): Charging flags
///   - Low nibble (bits 0-3): Case battery level
fn parse_airpods_data(data: &[u8]) -> Option<AirPodsStatus> {
    if data.len() != AIRPODS_DATA_LENGTH {
        return None;
    }

    // Check if left/right are flipped
    let flip = (data[BYTE_FLIP] & MASK_FLIP_BIT) == 0;

    // Detect model from 2-byte identifier
    let model_byte = low_nibble(data[BYTE_MODEL_HIGH]);
    let model_full = ((data[BYTE_MODEL_HIGH] as u16) << 8) | (data[BYTE_MODEL_LOW] as u16);

    // See: https://github.com/d4rken-org/capod/blob/5860bbffb6b2e59feca450bc234595314e842366/app/src/main/java/eu/darken/capod/pods/core/apple/airpods/AirPodsGen4.kt#L78
    let model = match model_full {
        0x0220 => "AirPods 1",
        0x0F20 => "AirPods 2",
        0x1320 => "AirPods 3",
        0x1920 => "AirPods 4",
        0x0E20 => "AirPods Pro",
        0x1420 | 0x2420 => "AirPods Pro 2",
        0x2720 => "AirPods Pro 3",
        0x0A20 | 0x1F20 => "AirPods Max",
        _ => "AirPods",
    };

    // Check if this is a single-battery device (AirPods Max)
    let is_max_device = model_byte == 0x0A;

    let battery_byte = data[BYTE_BATTERY_PODS];

    let case_charge_byte = data[BYTE_BATTERY_CASE_AND_CHARGING];
    let case_battery_raw = low_nibble(case_charge_byte);
    let charging_flags = high_nibble(case_charge_byte);

    if is_max_device {
        // For single-battery devices (AirPods Max), use low nibble of byte 6
        let single_raw = low_nibble(battery_byte);
        let battery = battery_level(single_raw)?;
        let charging = (charging_flags & MASK_CHARGING_LEFT) != 0;

        Some(AirPodsStatus::OverEar {
            model: model.into(),
            status: MaxStatus { battery, charging },
        })
    } else {
        // For dual-pod devices (AirPods, AirPods Pro), extract left and right
        let (left_raw, right_raw) = if flip {
            (low_nibble(battery_byte), high_nibble(battery_byte))
        } else {
            (high_nibble(battery_byte), low_nibble(battery_byte))
        };

        let left = battery_level(left_raw);
        let right = battery_level(right_raw);
        let case = battery_level(case_battery_raw);

        // Parse charging flags (respecting flip bit)
        let (left_mask, right_mask) = if flip {
            (MASK_CHARGING_RIGHT, MASK_CHARGING_LEFT)
        } else {
            (MASK_CHARGING_LEFT, MASK_CHARGING_RIGHT)
        };
        let charging_left = (charging_flags & left_mask) != 0;
        let charging_right = (charging_flags & right_mask) != 0;
        let charging_case = (charging_flags & MASK_CHARGING_CASE) != 0;

        Some(AirPodsStatus::InEar {
            model: model.into(),
            status: InEarStatus {
                left,
                right,
                case,
                charging_left,
                charging_right,
                charging_case,
            },
        })
    }
}

/// Convert raw battery value (0-10) to percentage (5-100%)
/// Returns None if the device is disconnected (value 15)
fn battery_level(raw: u8) -> Option<u8> {
    match raw {
        10 => Some(100),
        0..=9 => Some(raw * 10 + 5),
        BATTERY_DISCONNECTED => None,
        _ => None,
    }
}
