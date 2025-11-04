use btleplug::api::{Central, Manager as _, Peripheral as _, ScanFilter};
use btleplug::platform::Manager;
use serde::{Deserialize, Serialize};
use std::env;
use std::time::Duration;
use tokio::time::sleep;

const APPLE_MANUFACTURER_ID: u16 = 0x004c; // Apple Inc.
const AIRPODS_DATA_LENGTH: usize = 27;
const SCAN_DURATION_SECS: u64 = 3;

#[derive(Debug, Serialize, Deserialize)]
struct AirPodsStatus {
    model: String,
    left: Option<u8>,
    right: Option<u8>,
    case: Option<u8>,
    charging_left: bool,
    charging_right: bool,
    charging_case: bool,
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

async fn scan_for_airpods() -> Result<Option<AirPodsStatus>, Box<dyn std::error::Error>> {
    let manager = Manager::new().await?;
    let adapters = manager.adapters().await?;

    if adapters.is_empty() {
        return Err("No Bluetooth adapters found".into());
    }

    let adapter = adapters.into_iter().next().unwrap();

    // Start scanning
    adapter
        .start_scan(ScanFilter::default())
        .await?;

    // Scan for specified duration
    sleep(Duration::from_secs(SCAN_DURATION_SECS)).await;

    // Get all discovered peripherals
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

    adapter.stop_scan().await?;
    Ok(None)
}

fn parse_airpods_data(data: &[u8]) -> Option<AirPodsStatus> {
    if data.len() != AIRPODS_DATA_LENGTH {
        return None;
    }

    // Check if data is flipped (bit 1 of byte 10)
    let flip = (data[10] & 0x02) == 0;

    // Detect model from byte 7
    let model = match data[7] {
        0x0e => "AirPods Pro",
        0x03 => "AirPods 3",
        0x0f => "AirPods 2",
        0x02 => "AirPods 1",
        0x0a => "AirPods Max",
        _ => "AirPods",
    };

    // Parse battery levels (accounting for flip)
    let (left_pos, right_pos) = if flip { (13, 12) } else { (12, 13) };

    let left_raw = (data[left_pos] & 0x0f) as i8;
    let right_raw = (data[right_pos] & 0x0f) as i8;
    let case_raw = (data[15] & 0x0f) as i8;

    let left = battery_level(left_raw);
    let right = battery_level(right_raw);
    let case = battery_level(case_raw);

    // Parse charging status from byte 14
    let charging_status = data[14];
    let charging_left = if flip {
        (charging_status & 0x02) != 0
    } else {
        (charging_status & 0x01) != 0
    };
    let charging_right = if flip {
        (charging_status & 0x01) != 0
    } else {
        (charging_status & 0x02) != 0
    };
    let charging_case = (charging_status & 0x04) != 0;

    Some(AirPodsStatus {
        model: model.to_string(),
        left,
        right,
        case,
        charging_left,
        charging_right,
        charging_case,
    })
}

fn battery_level(raw: i8) -> Option<u8> {
    match raw {
        10 => Some(100),
        0..=9 => Some((raw as u8) * 10 + 5),
        _ => None,
    }
}

fn print_plain_text(status: &AirPodsStatus) {
    println!("{}", status.model);

    if let Some(left) = status.left {
        print!("Left: {}%", left);
        if status.charging_left {
            print!(" (charging)");
        }
        println!();
    }

    if let Some(right) = status.right {
        print!("Right: {}%", right);
        if status.charging_right {
            print!(" (charging)");
        }
        println!();
    }

    if let Some(case) = status.case {
        print!("Case: {}%", case);
        if status.charging_case {
            print!(" (charging)");
        }
        println!();
    }
}
