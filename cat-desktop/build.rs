use std::fs;
use std::path::Path;

fn main() {
    let firmware_path = Path::new("assets/cat-bridge.bin");

    println!("cargo:rerun-if-changed=assets/cat-bridge.bin");

    if cfg!(feature = "bundle-firmware") {
        // Feature enabled: require valid firmware binary
        if firmware_path.exists() {
            let metadata = fs::metadata(firmware_path).expect("Failed to read firmware metadata");
            if metadata.len() == 0 {
                panic!(
                    "bundle-firmware feature requires valid firmware binary. \
                     The file assets/cat-bridge.bin exists but is empty."
                );
            }
        } else {
            panic!(
                "bundle-firmware feature requires firmware binary at assets/cat-bridge.bin. \
                 Build cat-bridge first or disable the feature."
            );
        }
    } else {
        // Feature disabled: create empty placeholder if needed
        if !firmware_path.exists() {
            fs::create_dir_all("assets").expect("Failed to create assets directory");
            fs::write(firmware_path, b"").expect("Failed to create firmware placeholder");
            println!(
                "cargo:warning=Created empty firmware placeholder. \
                 Enable bundle-firmware feature for firmware flashing support."
            );
        }
    }
}
