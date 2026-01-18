//! CAT Bridge - ESP32-S3 Dual USB Serial Bridge Firmware
//!
//! This firmware allows a host computer to appear as a USB serial device
//! to an amplifier. Since Windows/Linux/macOS cannot natively act as USB
//! gadgets, the ESP32-S3 acts as a proxy.
//!
//! # Architecture
//!
//! ```text
//! Host Computer <--USB-Serial-JTAG--> ESP32-S3 <--USB OTG (CDC)--> Amplifier
//!   (USB host)                                   (USB device)      (USB host)
//! ```
//!
//! The ESP32-S3 has two USB interfaces:
//! - **USB-Serial-JTAG**: Built-in USB that appears as a serial port to the
//!   host computer when you plug in the "programming" USB port
//! - **USB OTG**: Configured as a CDC ACM device that plugs into the
//!   amplifier's USB host port
//!
//! Data flows bidirectionally between these two interfaces.
//!
//! # Hardware Setup (ESP32-S3-DevKitC)
//! - **USB-UART port** (usually labeled "UART"): Connect to host computer
//! - **USB OTG port** (usually labeled "USB"): Connect to amplifier
//! - **Status LED**: GPIO48 shows activity
//!
//! # LED Indicators
//! - Slow blink (1Hz): Waiting for connections
//! - Fast blink (4Hz): Both USB interfaces active, bridging data
//! - Solid: Data transfer in progress

#![no_std]
#![no_main]

use embassy_executor::Spawner;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::Channel;
use embassy_time::{Duration, Timer};
use embassy_usb::class::cdc_acm::{CdcAcmClass, State};
use embassy_usb::driver::EndpointError;
use embassy_usb::Builder;
use esp_backtrace as _;
use esp_hal::gpio::{Level, Output, OutputConfig};
use esp_hal::otg_fs::asynch::{Config, Driver};
use esp_hal::otg_fs::Usb;
use esp_hal::timer::timg::TimerGroup;
use esp_hal::usb_serial_jtag::UsbSerialJtag;
use log::{info, warn};
use static_cell::StaticCell;

/// Buffer size for data transfer
const BUFFER_SIZE: usize = 64;

/// Channel for Host -> Amplifier data (USB-Serial-JTAG -> USB OTG)
static HOST_TO_AMP: Channel<CriticalSectionRawMutex, DataPacket, 8> = Channel::new();

/// Channel for Amplifier -> Host data (USB OTG -> USB-Serial-JTAG)
static AMP_TO_HOST: Channel<CriticalSectionRawMutex, DataPacket, 8> = Channel::new();

/// Data packet with length information
struct DataPacket {
    data: [u8; BUFFER_SIZE],
    len: usize,
}

impl DataPacket {
    fn new(src: &[u8]) -> Self {
        let mut data = [0u8; BUFFER_SIZE];
        let len = src.len().min(BUFFER_SIZE);
        data[..len].copy_from_slice(&src[..len]);
        Self { data, len }
    }

    fn as_slice(&self) -> &[u8] {
        &self.data[..self.len]
    }
}

/// USB Device Descriptor configuration for the amplifier-facing port
const USB_VID: u16 = 0x1209; // pid.codes VID for open source projects
const USB_PID: u16 = 0xCA71; // Unique PID for catapult (valid hex)

#[esp_rtos::main]
async fn main(spawner: Spawner) {
    // Initialize ESP-HAL
    let peripherals = esp_hal::init(esp_hal::Config::default());

    // Initialize logging
    esp_println::logger::init_logger_from_env();

    info!("CAT Bridge starting...");
    info!("Architecture: Host <--USB-JTAG--> ESP32 <--USB-OTG--> Amplifier");

    // Initialize timer for embassy
    let timg0 = TimerGroup::new(peripherals.TIMG0);
    esp_rtos::start(timg0.timer0);

    // Configure status LED (GPIO48 on ESP32-S3-DevKitC)
    let led = Output::new(peripherals.GPIO48, Level::High, OutputConfig::default());

    // =========================================================================
    // USB-Serial-JTAG: Connection to host computer
    // =========================================================================
    let usb_serial_jtag = UsbSerialJtag::new(peripherals.USB_DEVICE).into_async();
    let (jtag_rx, jtag_tx) = usb_serial_jtag.split();

    // =========================================================================
    // USB OTG: CDC device that plugs into the amplifier
    // =========================================================================
    let usb = Usb::new(peripherals.USB0, peripherals.GPIO20, peripherals.GPIO19);

    // Create the driver from the HAL
    static EP_OUT_BUFFER: StaticCell<[u8; 1024]> = StaticCell::new();
    let ep_out_buffer = EP_OUT_BUFFER.init([0u8; 1024]);
    let config = Config::default();
    let driver = Driver::new(usb, ep_out_buffer, config);

    // USB device configuration - this is what the amplifier sees
    let mut usb_config = embassy_usb::Config::new(USB_VID, USB_PID);
    usb_config.manufacturer = Some("Catapult");
    usb_config.product = Some("CAT Bridge");
    usb_config.serial_number = Some("001");
    usb_config.max_power = 100;
    usb_config.max_packet_size_0 = 64;
    // Required for Windows compatibility
    usb_config.device_class = 0xEF;
    usb_config.device_sub_class = 0x02;
    usb_config.device_protocol = 0x01;
    usb_config.composite_with_iads = true;

    // Create USB device
    static STATE: StaticCell<State> = StaticCell::new();
    let state = STATE.init(State::new());

    static CONFIG_DESCRIPTOR: StaticCell<[u8; 256]> = StaticCell::new();
    static BOS_DESCRIPTOR: StaticCell<[u8; 256]> = StaticCell::new();
    static CONTROL_BUF: StaticCell<[u8; 64]> = StaticCell::new();

    let mut builder = Builder::new(
        driver,
        usb_config,
        CONFIG_DESCRIPTOR.init([0; 256]),
        BOS_DESCRIPTOR.init([0; 256]),
        &mut [],
        CONTROL_BUF.init([0; 64]),
    );

    // Create CDC ACM class (serial port) for the amplifier
    let cdc_class = CdcAcmClass::new(&mut builder, state, 64);
    let (cdc_sender, cdc_receiver) = cdc_class.split();

    // Build USB device
    let mut usb_device = builder.build();

    // =========================================================================
    // Spawn tasks
    // =========================================================================
    spawner.spawn(host_rx_task(jtag_rx)).unwrap();
    spawner.spawn(host_tx_task(jtag_tx)).unwrap();
    spawner.spawn(led_task(led)).unwrap();
    spawner.spawn(amp_rx_task(cdc_receiver)).unwrap();
    spawner.spawn(amp_tx_task(cdc_sender)).unwrap();

    info!("CAT Bridge ready!");
    info!("Connect 'UART' USB port to host computer");
    info!("Connect 'USB' OTG port to amplifier");

    // Run USB device
    usb_device.run().await;
}

/// Host RX task - receives data from host computer via USB-Serial-JTAG
#[embassy_executor::task]
async fn host_rx_task(mut rx: esp_hal::usb_serial_jtag::UsbSerialJtagRx<'static, esp_hal::Async>) {
    info!("Host RX task started");
    let mut buf = [0u8; BUFFER_SIZE];

    loop {
        match embedded_io_async::Read::read(&mut rx, &mut buf).await {
            Ok(n) if n > 0 => {
                let packet = DataPacket::new(&buf[..n]);
                HOST_TO_AMP.send(packet).await;
            }
            Ok(_) => {}
            Err(e) => {
                warn!("Host RX error: {:?}", e);
                Timer::after(Duration::from_millis(100)).await;
            }
        }
    }
}

/// Host TX task - sends data to host computer via USB-Serial-JTAG
#[embassy_executor::task]
async fn host_tx_task(mut tx: esp_hal::usb_serial_jtag::UsbSerialJtagTx<'static, esp_hal::Async>) {
    info!("Host TX task started");

    loop {
        let packet = AMP_TO_HOST.receive().await;
        if packet.len > 0 {
            if let Err(e) = embedded_io_async::Write::write_all(&mut tx, packet.as_slice()).await {
                warn!("Host TX error: {:?}", e);
            }
        }
    }
}

/// Amplifier RX task - receives data from amplifier via USB OTG CDC
#[embassy_executor::task]
async fn amp_rx_task(
    mut receiver: embassy_usb::class::cdc_acm::Receiver<'static, Driver<'static>>,
) {
    info!("Amplifier RX task started");
    let mut buf = [0u8; BUFFER_SIZE];

    loop {
        receiver.wait_connection().await;
        info!("Amplifier USB connected");

        loop {
            match receiver.read_packet(&mut buf).await {
                Ok(n) if n > 0 => {
                    let packet = DataPacket::new(&buf[..n]);
                    AMP_TO_HOST.send(packet).await;
                }
                Ok(_) => {}
                Err(EndpointError::BufferOverflow) => {
                    warn!("Amplifier RX buffer overflow");
                }
                Err(EndpointError::Disabled) => {
                    info!("Amplifier USB disconnected");
                    break;
                }
            }
        }
    }
}

/// Amplifier TX task - sends data to amplifier via USB OTG CDC
#[embassy_executor::task]
async fn amp_tx_task(
    mut sender: embassy_usb::class::cdc_acm::Sender<'static, Driver<'static>>,
) {
    info!("Amplifier TX task started");

    loop {
        sender.wait_connection().await;

        loop {
            let packet = HOST_TO_AMP.receive().await;
            if packet.len > 0 {
                match sender.write_packet(packet.as_slice()).await {
                    Ok(_) => {}
                    Err(EndpointError::BufferOverflow) => {
                        warn!("Amplifier TX buffer overflow");
                    }
                    Err(EndpointError::Disabled) => {
                        break;
                    }
                }
            }
        }
    }
}

/// LED task - indicates status
#[embassy_executor::task]
async fn led_task(mut led: Output<'static>) {
    loop {
        led.toggle();
        Timer::after(Duration::from_millis(500)).await;
    }
}
