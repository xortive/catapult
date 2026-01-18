use std::env;
use std::fs;
use std::path::Path;

fn main() {
    let firmware_path = Path::new("assets/cat-bridge.bin");

    // Rerun if the firmware file changes
    println!("cargo:rerun-if-changed=assets/cat-bridge.bin");
    println!("cargo:rerun-if-env-changed=CI");

    // Check if we're in CI
    let in_ci = env::var("CI").is_ok();

    if firmware_path.exists() {
        let metadata = fs::metadata(firmware_path).expect("Failed to read firmware metadata");
        if metadata.len() == 0 && in_ci {
            panic!(
                "CI build requires valid firmware binary. \
                 The file assets/cat-bridge.bin exists but is empty. \
                 Ensure the firmware job runs before the desktop build."
            );
        }
    } else if in_ci {
        panic!(
            "CI build requires firmware binary at assets/cat-bridge.bin. \
             Ensure the firmware job runs before the desktop build."
        );
    } else {
        // Local development: create empty placeholder
        fs::create_dir_all("assets").expect("Failed to create assets directory");
        fs::write(firmware_path, b"").expect("Failed to create firmware placeholder");
        println!(
            "cargo:warning=Created empty firmware placeholder. \
             Firmware flashing will not work until you build cat-bridge."
        );
    }
}
