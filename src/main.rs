use btleplug::api::{Central, Manager as _, Peripheral as _, ScanFilter};
use btleplug::platform::Manager;
use serde::{Deserialize, Serialize};
use std::env;
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
    Max {
        model: String,
        #[serde(flatten)]
        status: MaxStatus,
    },
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();
    let json_output = args.len() > 1 && args[1] == "--json";

    match scan_for_airpods().await {
        Ok(Some(status)) => {
            if json_output {
                println!("{}", serde_json::to_string_pretty(&status)?);
            } else {
                print_plain_text(&status);
            }
            Ok(())
        }
        Ok(None) => {
            if json_output {
                println!("{{\"error\": \"AirPods not found\"}}");
            } else {
                eprintln!("AirPods not found");
            }
            std::process::exit(1);
        }
        Err(e) => {
            if json_output {
                println!("{{\"error\": \"{}\"}}", e);
            } else {
                eprintln!("Error: {}", e);
            }
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

    if adapters.is_empty() {
        return Err("No Bluetooth adapters found".into());
    }

    let adapter = adapters.into_iter().next().unwrap();

    // Start scanning
    adapter.start_scan(ScanFilter::default()).await?;

    // Give the scan a moment to start capturing broadcasts
    sleep(Duration::from_millis(200)).await;

    // Poll for AirPods up to SCAN_TIMEOUT_SECS seconds
    let start = std::time::Instant::now();
    let timeout = Duration::from_secs(SCAN_TIMEOUT_SECS);
    let poll_interval = Duration::from_millis(POLL_INTERVAL_MS);

    loop {
        // Check if we've exceeded the timeout
        if start.elapsed() >= timeout {
            adapter.stop_scan().await?;
            return Ok(None);
        }

        // Get all discovered peripherals so far
        let peripherals = adapter.peripherals().await?;

        // Look for AirPods in the discovered devices
        for peripheral in peripherals {
            let properties = peripheral.properties().await?;

            if let Some(props) = properties {
                let manufacturer_data = props.manufacturer_data;
                if let Some(data) = manufacturer_data.get(&APPLE_MANUFACTURER_ID) {
                    if data.len() == AIRPODS_DATA_LENGTH {
                        if let Some(status) = parse_airpods_data(data) {
                            adapter.stop_scan().await?;
                            return Ok(Some(status));
                        }
                    }
                }
            }
        }

        // Wait before checking again
        sleep(poll_interval).await;
    }
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

    let model = match model_full {
        0x0220 => "AirPods 1",
        0x0F20 => "AirPods 2",
        0x1320 => "AirPods 3",
        0x0E20 => "AirPods Pro",
        0x1420 | 0x2420 => "AirPods Pro 2",
        0x2720 => "AirPods Pro 3",
        _ if model_byte == 0x0A => "AirPods Max",
        _ => "AirPods",
    };

    // Check if this is a single-battery device (AirPods Max, Beats, etc.)
    let is_single_device = model_byte == 0x0A // AirPods Max
        || model_byte == 0x0B // Powerbeats Pro (uses dual pods though)
        || matches!(model_full, 0x0520 | 0x1020 | 0x0620 | 0x0320) // BeatsX, BeatsFlex, BeatsSolo3, Powerbeats3
        || (model_full >> 8) == 0x09; // BeatsStudio3

    // Extract battery levels from byte 6
    let battery_byte = data[BYTE_BATTERY_PODS];

    // Extract case battery and charging status from byte 7
    let case_charge_byte = data[BYTE_BATTERY_CASE_AND_CHARGING];
    let case_raw = low_nibble(case_charge_byte);
    let charging_status = high_nibble(case_charge_byte);

    if is_single_device {
        // For single-battery devices (AirPods Max, Beats), use low nibble of byte 6
        let single_raw = low_nibble(battery_byte);
        let battery = battery_level(single_raw)?;
        let charging = (charging_status & MASK_CHARGING_LEFT) != 0;

        Some(AirPodsStatus::Max {
            model: model.to_string(),
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
        let case = battery_level(case_raw);

        // Parse charging flags (respecting flip bit)
        let charging_left = if flip {
            (charging_status & MASK_CHARGING_RIGHT) != 0
        } else {
            (charging_status & MASK_CHARGING_LEFT) != 0
        };
        let charging_right = if flip {
            (charging_status & MASK_CHARGING_LEFT) != 0
        } else {
            (charging_status & MASK_CHARGING_RIGHT) != 0
        };
        let charging_case = (charging_status & MASK_CHARGING_CASE) != 0;

        Some(AirPodsStatus::InEar {
            model: model.to_string(),
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

fn print_plain_text(status: &AirPodsStatus) {
    match status {
        AirPodsStatus::Max { model, status } => {
            println!("{}", model);
            let charging_suffix = if status.charging { " (charging)" } else { "" };
            println!("Battery: {}%{}", status.battery, charging_suffix);
        }
        AirPodsStatus::InEar { model, status } => {
            println!("{}", model);
            print_component("Left", status.left, status.charging_left);
            print_component("Right", status.right, status.charging_right);
            print_component("Case", status.case, status.charging_case);
        }
    }
}

/// Print a single component's battery status
fn print_component(name: &str, battery: Option<u8>, charging: bool) {
    if let Some(level) = battery {
        let charging_suffix = if charging { " (charging)" } else { "" };
        println!("{}: {}%{}", name, level, charging_suffix);
    }
}
