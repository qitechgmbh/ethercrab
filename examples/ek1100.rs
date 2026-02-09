//! Demonstrate setting outputs using a Beckhoff EK1100/EK1501 and modules.
//!
//! Run with e.g.
//!
//! Linux
//!
//! ```bash
//! RUST_LOG=debug cargo run --example ek1100 --release -- eth0
//! ```
//!
//! Windows
//!
//! ```ps
//! $env:RUST_LOG="debug" ; cargo run --example ek1100 --release -- '\Device\NPF_{FF0ACEE6-E8CD-48D5-A399-619CD2340465}'
//! ```

use env_logger::Env;
use ethercrab::{
    EtherCrabWireWrite, MainDevice, MainDeviceConfig, PduStorage, SubIndex, Timeouts, error::Error, std::ethercat_now
};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::time::MissedTickBehavior;

/// Maximum number of SubDevices that can be stored. This must be a power of 2 greater than 1.
const MAX_SUBDEVICES: usize = 16;
/// Maximum PDU data payload size - set this to the max PDI size or higher.
const MAX_PDU_DATA: usize = PduStorage::element_size(1100);
/// Maximum number of EtherCAT frames that can be in flight at any one time.
const MAX_FRAMES: usize = 16;
/// Maximum total PDI length.
const PDI_LEN: usize = 64;
static PDU_STORAGE: PduStorage<MAX_FRAMES, MAX_PDU_DATA> = PduStorage::new();

#[derive(Debug, Clone, EtherCrabWireWrite)]
#[wire (bits = 144)]
pub struct EL30XXChannelConfiguration {
    // 80n0:01 User Scaling is Active
    #[wire (bits = 1)]
    pub enable_user_scale: bool,
    #[wire (bits = 2)]
    pub presentation: u8,
    #[wire (bits = 1)]
    pub siemens_bits: bool,
    #[wire (bits = 1)]
    pub enable_filter: bool,
    #[wire (bits = 1)]
    pub enable_limit_1: bool,
    #[wire (bits = 1)]
    pub enable_limit_2: bool,
    #[wire (bits = 1)]
    pub enable_user_calibration: bool,
    #[wire (bits = 1)]
    pub enable_vendor_calibration: bool,
    #[wire (bits = 1)]
    pub swap_limit_bits: bool,
    
    #[wire (pre_skip = 6, bits = 16)]
    pub user_scale_offset: i16,
    
    #[wire (bits = 32)]
    pub user_scale_gain: i32,
    
    #[wire (bits = 16)]
    pub limit_1: i16,
    
    #[wire (bits = 16)]
    pub limit_2: i16,

    #[wire (bits = 16)]
    pub filter_settings: u16,

    #[wire (bits = 16)]
    pub user_calibration_offset: i16,
    #[wire (bits = 16)]
    pub user_calibration_gain: i16,
}

#[derive(Debug, Clone, Copy)]
pub enum EL30XXFilterSettings {
    FIR50Hz,
    FIR60Hz,
    IIR1,
    IIR2,
    IIR3,
    IIR4,
    IIR5,
    IIR6,
    IIR7,
    IIR8,
}

impl From<EL30XXFilterSettings> for u16 {
    fn from(filter_settings: EL30XXFilterSettings) -> Self {
        match filter_settings {
            EL30XXFilterSettings::FIR50Hz => 0,
            EL30XXFilterSettings::FIR60Hz => 1,
            EL30XXFilterSettings::IIR1 => 2,
            EL30XXFilterSettings::IIR2 => 3,
            EL30XXFilterSettings::IIR3 => 4,
            EL30XXFilterSettings::IIR4 => 5,
            EL30XXFilterSettings::IIR5 => 6,
            EL30XXFilterSettings::IIR6 => 7,
            EL30XXFilterSettings::IIR7 => 8,
            EL30XXFilterSettings::IIR8 => 9,
        }
    }
}


impl Default for EL30XXChannelConfiguration {
    fn default() -> Self {
        Self {
            enable_user_scale: false,
            presentation: 0,
            siemens_bits: false,
            enable_filter: true,
            enable_limit_1: false,
            enable_limit_2: false,
            enable_user_calibration: false,
            enable_vendor_calibration: true,
            swap_limit_bits: false,
            user_scale_offset: 0,
            user_scale_gain: 65538,
            limit_1: 0,
            limit_2: 0,
            filter_settings: EL30XXFilterSettings::FIR50Hz.into(),
            user_calibration_offset: 0,
            user_calibration_gain: 16384,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum EL30XXPresentation {
    Signed,
    Unsigned,
    SignedMagnitude,
}


#[tokio::main]
async fn main() -> Result<(), Error> {
    env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();

    let interface = std::env::args()
        .nth(1)
        .expect("Provide network interface as first argument.");

    log::info!("Starting EK1100/EK1501 demo...");
    log::info!(
        "Ensure an EK1100 or EK1501 is the first SubDevice, with any number of modules connected after"
    );
    log::info!("Run with RUST_LOG=ethercrab=debug or =trace for debug information");

    let (tx, rx, pdu_loop) = PDU_STORAGE.try_split().expect("can only split once");

    let maindevice = Arc::new(MainDevice::new(
        pdu_loop,
        Timeouts {
            wait_loop_delay: Duration::from_millis(2),
            mailbox_response: Duration::from_millis(1000),
            ..Default::default()
        },
        MainDeviceConfig::default(),
    ));

    #[cfg(target_os = "windows")]
    std::thread::spawn(move || {
        ethercrab::std::tx_rx_task_blocking(
            &interface,
            tx,
            rx,
            ethercrab::std::TxRxTaskConfig { spinloop: false },
        )
        .expect("TX/RX task")
    });
    #[cfg(not(target_os = "windows"))]
    tokio::spawn(ethercrab::std::tx_rx_task(&interface, tx, rx).expect("spawn TX/RX task"));

    let group = maindevice
        .init_single_group::<MAX_SUBDEVICES, PDI_LEN>(ethercat_now)
        .await
        .expect("Init");

    log::info!("Discovered {} SubDevices", group.len());



    for subdevice in group.iter(&maindevice) {
        if subdevice.name() == "EL3024" {
            log::info!("Found EL3024. Configuring...");
            let default_config  = EL30XXChannelConfiguration::default();
            
            let res = subdevice.sdo_write(0x8010, SubIndex::Complete,default_config).await;
            println!("{:?}",res);
            let base_index = 0x8010;

            let enable_user_scale: bool = subdevice.sdo_read(base_index, 0x01).await?;
            println!("0x{:04x}:01 enable_user_scale: {}", base_index, enable_user_scale);

            let presentation: u8 = subdevice.sdo_read(base_index, 0x02).await?;
            println!("0x{:04x}:02 presentation: {}", base_index, presentation);

            let siemens_bits: bool = subdevice.sdo_read(base_index, 0x05).await?;
            println!("0x{:04x}:05 siemens_bits: {}", base_index, siemens_bits);

            let enable_filter: bool = subdevice.sdo_read(base_index, 0x06).await?;
            println!("0x{:04x}:06 enable_filter: {}", base_index, enable_filter);

            let enable_limit_1: bool = subdevice.sdo_read(base_index, 0x07).await?;
            println!("0x{:04x}:07 enable_limit_1: {}", base_index, enable_limit_1);

            let enable_limit_2: bool = subdevice.sdo_read(base_index, 0x08).await?;
            println!("0x{:04x}:08 enable_limit_2: {}", base_index, enable_limit_2);

            let enable_user_calibration: bool = subdevice.sdo_read(base_index, 0x0A).await?;
            println!("0x{:04x}:0A enable_user_calibration: {}", base_index, enable_user_calibration);

            let enable_vendor_calibration: bool = subdevice.sdo_read(base_index, 0x0B).await?;
            println!("0x{:04x}:0B enable_vendor_calibration: {}", base_index, enable_vendor_calibration);

            let swap_limit_bits: bool = subdevice.sdo_read(base_index, 0x0E).await?;
            println!("0x{:04x}:0E swap_limit_bits: {}", base_index, swap_limit_bits);

            let user_scale_offset: i16 = subdevice.sdo_read(base_index, 0x11).await?;
            println!("0x{:04x}:11 user_scale_offset: {}", base_index, user_scale_offset);

            let user_scale_gain: i32 = subdevice.sdo_read(base_index, 0x12).await?;
            println!("0x{:04x}:12 user_scale_gain: {}", base_index, user_scale_gain);

            let limit_1: i16 = subdevice.sdo_read(base_index, 0x13).await?;
            println!("0x{:04x}:13 limit_1: {}", base_index, limit_1);

            let limit_2: i16 = subdevice.sdo_read(base_index, 0x14).await?;
            println!("0x{:04x}:14 limit_2: {}", base_index, limit_2);

            let filter_settings: u16 = subdevice.sdo_read(base_index, 0x15).await?;
            println!("0x{:04x}:15 filter_settings: {}", base_index, filter_settings);

            let user_calibration_offset: i16 = subdevice.sdo_read(base_index, 0x17).await?;
            println!("0x{:04x}:17 user_calibration_offset: {}", base_index, user_calibration_offset);

            let user_calibration_gain: i16 = subdevice.sdo_read(base_index, 0x18).await?;
            println!("0x{:04x}:18 user_calibration_gain: {}", base_index, user_calibration_gain);
/*
            subdevice.sdo_write(0x1c12, 0, 0u8).await?;
            subdevice
                .sdo_write_array(0x1c13, &[0x1a00u16, 0x1a02, 0x1a04, 0x1a06])
                .await?;
*/
            // The `sdo_write_array` call above is equivalent to the following
            // subdevice.sdo_write(0x1c13, 0, 0u8).await?;
            // subdevice.sdo_write(0x1c13, 1, 0x1a00u16).await?;
            // subdevice.sdo_write(0x1c13, 2, 0x1a02u16).await?;
            // subdevice.sdo_write(0x1c13, 3, 0x1a04u16).await?;
            // subdevice.sdo_write(0x1c13, 4, 0x1a06u16).await?;
            // subdevice.sdo_write(0x1c13, 0, 4u8).await?;
        }
    }
/*
    let group = group.into_op(&maindevice).await.expect("PRE-OP -> OP");
    for subdevice in group.iter(&maindevice) {
        let io = subdevice.io_raw();

        log::info!(
            "-> SubDevice {:#06x} {} inputs: {} bytes, outputs: {} bytes",
            subdevice.configured_address(),
            subdevice.name(),
            io.inputs().len(),
            io.outputs().len()
        );
    }

    let mut tick_interval = tokio::time::interval(Duration::from_millis(5));
    tick_interval.set_missed_tick_behavior(MissedTickBehavior::Skip);

    let shutdown = Arc::new(AtomicBool::new(false));
    signal_hook::flag::register(signal_hook::consts::SIGINT, Arc::clone(&shutdown))
        .expect("Register hook");

    loop {
        // Graceful shutdown on Ctrl + C
        if shutdown.load(Ordering::Relaxed) {
            log::info!("Shutting down...");

            break;
        }

        group.tx_rx(&maindevice).await.expect("TX/RX");

        // Increment every output byte for every SubDevice by one
        for subdevice in group.iter(&maindevice) {
            let mut o = subdevice.outputs_raw_mut();

            for byte in o.iter_mut() {
                *byte = byte.wrapping_add(1);
            }
        }

        tick_interval.tick().await;
    }

    let group = group
        .into_safe_op(&maindevice)
        .await
        .expect("OP -> SAFE-OP");

    log::info!("OP -> SAFE-OP");

    let group = group
        .into_pre_op(&maindevice)
        .await
        .expect("SAFE-OP -> PRE-OP");

    log::info!("SAFE-OP -> PRE-OP");

    let _group = group.into_init(&maindevice).await.expect("PRE-OP -> INIT");

    log::info!("PRE-OP -> INIT, shutdown complete");
*/
    Ok(())
}
