use egui_plot::{Line, Plot, PlotBounds, PlotPoints};
use num_complex::Complex32;
use serialport::SerialPort;

use std::io::{BufRead, BufReader};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Duration;

pub struct SpecViewer {
    channel: Receiver<Vec<Complex32>>,
    data: Vec<Complex32>,
}

const FS: f64 = 96000.;
// const FS: f64 = 480_000.;
const N: usize = 8192;

impl eframe::App for SpecViewer {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if let Ok(d) = self.channel.try_recv() {
            self.data = d;
        }

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

        egui::CentralPanel::default().show(ctx, |ui| {
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
            let line = Line::new(points);
            Plot::new("Data")
                .allow_zoom([true, false])
                .auto_bounds([true, false])
                .x_axis_label("Frequency (Hz)")
                .y_axis_label("Power (dBFS)")
                .show(ui, |plot_ui| {
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

fn reader(mut port: Box<dyn SerialPort>, channel: Sender<Vec<Complex32>>) {
    port.set_timeout(Duration::from_secs(8192)).unwrap();
    port.clear(serialport::ClearBuffer::All).unwrap();
    port.write_all(b"p").unwrap();
    let mut buf = Vec::new();
    let mut d: Vec<u16> = vec![];
    loop {
        let mut bufreader = BufReader::new(port);
        bufreader.read_until(0xff, &mut buf).unwrap();
        port = bufreader.into_inner();
        port.write_all(b"p").unwrap();
        buf.pop().unwrap();
        let string = String::from_utf8(buf.clone()).unwrap();
        {
            for line in string.lines() {
                d.push(line.parse().unwrap_or_else(|_e| panic!("{}", line)));
            }
            let mean = d.iter().map(|i| *i as usize).sum::<usize>() as f32 / d.len() as f32;
            let mut fftbuf = [0f32; N];
            for i in 0..fftbuf.len() {
                fftbuf[i] = d[i] as f32 - mean;
            }
            let v = microfft::real::rfft_8192(&mut fftbuf);
            channel.send(Vec::from(v)).unwrap();
            d = Vec::with_capacity(N);
        }
        buf.clear();
    }
}

fn main() -> eframe::Result {
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([400.0, 300.0])
            .with_min_inner_size([300.0, 220.0]),
        ..Default::default()
    };

    let (sender, reciever) = channel();

    let port = "/dev/cu.usbmodemSPECT1";
    let port = serialport::new(port, 115200 * 32).open().unwrap();
    std::thread::spawn(move || {
        reader(port, sender);
    });

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
