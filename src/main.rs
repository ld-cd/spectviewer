use egui_plot::{Line, Plot, PlotBounds, PlotPoints};
use num_complex::Complex32;
use serialport::SerialPort;

use std::io::{BufRead, BufReader};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Duration;

/// The structure of this GUI application is built around egui
/// which is a wonderfully easy to use immediate mode GUI framework
/// [egui](https://github.com/emilk/egui), this actual file is based
/// off of a videogame system emulator which I have not yet had a chance
/// to clean up and place on github.
///
/// This struct represents the render thread state that is maintained
/// between frames.
pub struct SpecViewer {
    /// channel provides a way for the render thread to recieve new data from
    /// the comms thread.
    channel: Receiver<Vec<Complex32>>,
    /// A place to stash old data until new data is ready
    data: Vec<Complex32>,
}

/// Define the sample rate
const FS: f64 = 96000.;
/// Define our FFT Size
const N: usize = 8192;

impl eframe::App for SpecViewer {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Check for a data update from the FFT+Comms thread
        if let Ok(d) = self.channel.try_recv() {
            self.data = d;
        }

        // Boilerplate File Quit and Light/Dark theme pane
        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("Quit").clicked() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });
                ui.add_space(16.0);

                egui::widgets::global_theme_preference_buttons(ui);
            });
        });

        // Central Pane with all our plots
        egui::CentralPanel::default().show(ctx, |ui| {
            // Rescale our FFT from complex amplitude to dBFS, such that
            // 0 dBFS is a full scale sine wave. We intentionally do not scale
            // by frequency to get a PSD because we do not know the precisce
            // frequency response of the microphone so it would be meaningless.
            let points: PlotPoints = self
                .data
                .iter()
                .enumerate()
                .map(|(i, p)| {
                    [
                        (i as f64) * FS / (N as f64),
                        10. * (p / (2048. * (N as f32))).norm().log10() as f64,
                    ]
                })
                .collect();

            // Create and format our plot
            let line = Line::new(points);
            Plot::new("Data")
                .allow_zoom([true, false])
                .auto_bounds([true, false])
                .x_axis_label("Frequency (Hz)")
                .y_axis_label("Power (dBFS)")
                .show(ui, |plot_ui| {
                    // Have plot bounds adjust in a way that is reminiscint of
                    // most spectrum analyzers, and makes sense for the input
                    // data range.
                    plot_ui.line(line);
                    let bounds = plot_ui.plot_bounds();
                    if *bounds.range_x().start() < 0.
                        || *bounds.range_x().end() > (FS / 2.)
                        || *bounds.range_y().end() > 0.
                        || *bounds.range_y().start() < -60.
                    {
                        let bounds = PlotBounds::from_min_max(
                            [bounds.range_x().start().max(0.), -60.],
                            [bounds.range_x().end().min(FS / 2.), 0.],
                        );
                        plot_ui.set_plot_bounds(bounds);
                    }
                });
        });
        ctx.request_repaint();
    }
}

/// This function runs a loop that reads from the ADC and performs an FFT
fn reader(mut port: Box<dyn SerialPort>, channel: Sender<Vec<Complex32>>) {
    // There's no no timeout option, so set it very high
    port.set_timeout(Duration::from_secs(8192)).unwrap();
    // Clear anything already recieved by the OS
    port.clear(serialport::ClearBuffer::All).unwrap();
    // Send the p comand, which forms the Computer->Device framing boundary
    port.write_all(b"p").unwrap();

    // Alocate a resizable buffer for our bufreader on the heap;
    let mut buf = Vec::new();
    // Allocate a resizable buffer for the data
    let mut d: Vec<u16> = vec![];

    // Main recieve/fft loop
    loop {
        // Recieve from the device until we get a \xff which forms the
        // Device->Computer framing boundary
        let mut bufreader = BufReader::new(port);
        bufreader.read_until(0xff, &mut buf).unwrap();

        // Turn our bufreader back into a serial port and request a new
        // buffer of ADC data while we process this one
        port = bufreader.into_inner();
        port.write_all(b"p").unwrap();
        // Pop the \xff out of our data as it isn't part of the text we parse
        buf.pop().unwrap();

        // Parse the string data recieved from the device, crashing if it is
        // malformed
        let string = String::from_utf8(buf.clone()).unwrap();
        {
            for line in string.lines() {
                d.push(line.parse().unwrap_or_else(|_e| panic!("{}", line)));
            }

            // Subtract out the zero Hz bin (as the ADC input is single ended
            // and biased to almost but not quite VCC/2), and then perform
            // the FFT
            let mean = d.iter().map(|i| *i as usize).sum::<usize>() as f32 / d.len() as f32;
            let mut fftbuf = [0f32; N];
            for i in 0..fftbuf.len() {
                fftbuf[i] = d[i] as f32 - mean;
            }
            let v = microfft::real::rfft_8192(&mut fftbuf);

            // Send the data off to our render thread
            channel.send(Vec::from(v)).unwrap();
            d = Vec::with_capacity(N);
        }
        buf.clear();
    }
}

fn main() -> eframe::Result {
    // Initilize our GUI window with normalish settings
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([400.0, 300.0])
            .with_min_inner_size([300.0, 220.0]),
        ..Default::default()
    };

    // Create a channel which the GUI thread and COMs thread
    // can chat through.
    let (sender, reciever) = channel();

    // Open the "serial" port to the device and launch the COMs
    // thread. This is actually a pure USB serial device on both ends
    // , so the baud rate can be quite high and is fairly arbitrary
    let port = "/dev/cu.usbmodemSPECT1";
    let port = serialport::new(port, 115200 * 32).open().unwrap();
    std::thread::spawn(move || {
        reader(port, sender);
    });

    // Start rendering on this thread
    eframe::run_native(
        "eframe template",
        native_options,
        Box::new(|_cc| {
            Ok(Box::new(SpecViewer {
                data: vec![],
                channel: reciever,
            }))
        }),
    )
}
