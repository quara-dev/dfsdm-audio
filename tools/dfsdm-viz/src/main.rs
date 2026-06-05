// On Windows, suppress the console window that would otherwise flash on launch.
#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

mod app;
mod connection_manager;
mod events;
mod hpf;
mod serial_reader;
mod toolbox;
mod wav;

use clap::Parser;
use crossbeam_channel::unbounded;

/// Real-time DFSDM audio visualiser.
///
/// Connects to a UART serial port streaming dfsdm-audio firmware frames
/// ([0xAA 0x55 0xAA 0x55] sync + 1024×int16 LE per burst) and renders
/// the waveform as a live scrolling plot.
///
/// --port is optional: if omitted, pick a port from the toolbar ComboBox.
#[derive(Parser, Debug)]
#[command(author, version, about)]
struct Args {
    /// Serial port to connect on launch (e.g. /dev/ttyUSB0, COM3).
    /// If omitted, the port can be chosen in the GUI.
    #[arg(short, long)]
    port: Option<String>,

    /// Baud rate
    #[arg(short, long, default_value_t = 921_600)]
    baud: u32,

    /// Audio sample rate in Hz (used for time axis labelling)
    #[arg(short, long, default_value_t = 32_000)]
    sample_rate: u32,

    /// Window width in pixels
    #[arg(long, default_value_t = 1400)]
    width: u32,

    /// Window height in pixels
    #[arg(long, default_value_t = 650)]
    height: u32,
}

fn main() -> eframe::Result<()> {
    env_logger::init();
    let args = Args::parse();

    // Connection-manager ← GUI commands
    let (cmd_tx, cmd_rx) = unbounded::<events::ConnCmd>();
    // Connection-manager → GUI events  (also used for port-scan results)
    let (event_tx, event_rx) = unbounded::<events::AppEvent>();

    // Spawn the long-lived connection manager thread.
    {
        let et = event_tx.clone();
        std::thread::Builder::new()
            .name("conn_mgr".into())
            .spawn(move || connection_manager::connection_manager_thread(cmd_rx, et))
            .expect("Failed to spawn connection_manager");
    }

    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(format!("DFSDM Audio Viz  v{}", env!("CARGO_PKG_VERSION")))
            .with_inner_size([args.width as f32, args.height as f32]),
        ..Default::default()
    };

    eframe::run_native(
        "dfsdm-viz",
        native_options,
        Box::new(move |_cc| {
            Box::new(app::DfsdmApp::new(
                event_rx,
                cmd_tx,
                event_tx,
                args.port,
                args.baud,
                args.sample_rate,
            )) as Box<dyn eframe::App>
        }),
    )
}
