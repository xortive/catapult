# Building from Source

Catapult is written in Rust and builds on Windows, macOS, and Linux.

## Prerequisites

### Rust Toolchain

Install Rust via [rustup](https://rustup.rs/):

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

Minimum supported Rust version: 1.70

### Platform Dependencies

#### Linux

```bash
# Debian/Ubuntu
sudo apt install build-essential pkg-config libxcb-render0-dev libxcb-shape0-dev \
    libxcb-xfixes0-dev libxkbcommon-dev libssl-dev libudev-dev

# Fedora
sudo dnf install gcc pkg-config xcb-util-renderutil-devel xcb-util-devel \
    libxkbcommon-devel openssl-devel systemd-devel
```

#### macOS

Xcode Command Line Tools:
```bash
xcode-select --install
```

#### Windows

Install Visual Studio Build Tools with C++ workload.

## Clone and Build

```bash
# Clone the repository
git clone https://github.com/your-org/catapult.git
cd catapult

# Build in release mode
cargo build --release

# Run tests
cargo test --workspace

# Run the application
cargo run --release -p cat-desktop
```

## Build Artifacts

After building, find binaries at:

```
target/release/catapult     # Main application (Unix)
target/release/catapult.exe # Main application (Windows)
```

## Development Build

For faster iteration during development:

```bash
# Debug build (faster compile, slower runtime)
cargo build -p cat-desktop

# Run with debug output
RUST_LOG=debug cargo run -p cat-desktop
```

## Workspace Structure

The project is organized as a Cargo workspace:

```
catapult/
├── Cargo.toml          # Workspace root
├── cat-desktop/        # GUI application
├── crates/
│   ├── cat-protocol/   # Protocol library
│   ├── cat-mux/        # Multiplexer library
│   ├── cat-detect/     # Detection library
│   └── cat-sim/        # Simulation library
└── cat-bridge/         # ESP32 firmware
```

## Building Individual Crates

```bash
# Build just the protocol library
cargo build -p cat-protocol

# Run tests for a specific crate
cargo test -p cat-mux

# Check a crate without building
cargo check -p cat-sim
```

## Documentation

Generate API documentation:

```bash
cargo doc --workspace --open
```
