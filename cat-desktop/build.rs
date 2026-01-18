use std::env;
use std::fs;
use std::path::Path;

fn main() {
    let firmware_path = Path::new("assets/cat-bridge.bin");

    // Rerun if the firmware file changes
    println!("cargo:rerun-if-changed=assets/cat-bridge.bin");
    println!("cargo:rerun-if-env-changed=CI");
    println!("cargo:rerun-if-env-changed=SKIP_FIRMWARE_CHECK");

    // Check if we're in CI and if firmware check should be skipped
    let in_ci = env::var("CI").is_ok();
    let skip_firmware_check = env::var("SKIP_FIRMWARE_CHECK")
        .map(|v| v == "true")
        .unwrap_or(false);

    // If skipping firmware check, treat it like local development
    let require_firmware = in_ci && !skip_firmware_check;

    if firmware_path.exists() {
        let metadata = fs::metadata(firmware_path).expect("Failed to read firmware metadata");
        if metadata.len() == 0 && require_firmware {
            panic!(
                "CI build requires valid firmware binary. \
                 The file assets/cat-bridge.bin exists but is empty. \
                 Ensure the firmware job runs before the desktop build."
            );
        }
    } else if require_firmware {
        panic!(
            "CI build requires firmware binary at assets/cat-bridge.bin. \
             Ensure the firmware job runs before the desktop build."
        );
    } else {
        // Local development or skipped firmware check: create empty placeholder
        fs::create_dir_all("assets").expect("Failed to create assets directory");
        fs::write(firmware_path, b"").expect("Failed to create firmware placeholder");
        println!(
            "cargo:warning=Created empty firmware placeholder. \
             Firmware flashing will not work until you build cat-bridge."
        );
    }
}
