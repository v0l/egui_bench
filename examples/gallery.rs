use egui_bench::prelude::*;
use egui_bench::trace::Trace;

#[derive(PartialEq, Clone, Copy)]
enum Tab {
    Panel,
    Meters,
    Data,
}

#[derive(PartialEq, Clone)]
enum Mode {
    Standard,
    TopBalance,
    BulkOnly,
}

struct Gallery {
    tab: Tab,
    gain: f32,
    squelch: f32,
    armed: bool,
    lit: bool,
    mode: Mode,
    host: String,
    key: String,
    notes: String,
    history: Vec<f32>,
    levels: Vec<f32>,
    t: f32,
}

impl Default for Gallery {
    fn default() -> Self {
        let tab = match std::env::args().nth(1).unwrap_or_default().as_str() {
            "meters" => Tab::Meters,
            "data" => Tab::Data,
            _ => Tab::Panel,
        };
        Self {
            tab,
            gain: 0.62,
            squelch: -78.0,
            armed: true,
            lit: false,
            mode: Mode::Standard,
            host: "10.100.2.249".into(),
            key: "hunter2".into(),
            notes: String::new(),
            history: vec![1.0; 120],
            levels: vec![0.0; 6],
            t: 0.0,
        }
    }
}

fn wobble(t: f32, seed: f32) -> f32 {
    0.5 + (t * 0.7 + seed * 1.7).sin() * 0.35 + (t * 1.9 + seed * 3.1).sin() * 0.15
}

impl eframe::App for Gallery {
    fn ui(&mut self, ui: &mut egui::Ui, _f: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.t += ctx.input(|i| i.stable_dt).min(0.1);
        let t = self.t;
        for (i, l) in self.levels.iter_mut().enumerate() {
            *l = wobble(t, i as f32 * 0.6);
        }
        self.history.remove(0);
        self.history.push(1.6 + (t * 0.8).sin() * 0.7 + (t * 3.1).sin() * 0.15);
        ctx.request_repaint();

        egui::Panel::left("controls").exact_size(320.0).show(ui, |ui| {
            ui.add_space(10.0);
            modal_title(ui, "receiver setup");

            section(ui, "link", "where the radio is", |ui| {
                row(ui, "host", |ui| {
                    field(ui, &mut self.host, "address or name");
                });
                row_help(ui, "key", "Kept in the OS keyring, never written to the config.", |ui| {
                    secret(ui, &mut self.key);
                });
                row(ui, "mode", |ui| {
                    choice(
                        ui,
                        "mode",
                        &mut self.mode,
                        [
                            (Mode::Standard, "standard".to_string()),
                            (Mode::TopBalance, "top balance".to_string()),
                            (Mode::BulkOnly, "bulk only".to_string()),
                        ],
                    );
                });
                switch(
                    ui,
                    "arm",
                    &mut self.armed,
                    "allow transmit",
                    "Nothing is keyed until this is set, whatever the pane says.",
                );
                hint(ui, "The link is opened when the first pane asks for a sample, not here.");
            });

            ui.add_space(8.0);
            section(ui, "notes", "", |ui| {
                prose(ui, &mut self.notes, "what this run is for", 3);
            });

            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if toggle(ui, "monitor", self.lit).clicked() {
                    self.lit = !self.lit;
                }
                lamp(ui, "armed", self.armed, false);
                lamp(ui, "fault", false, true);
            });

            ui.add_space(8.0);
            status(ui, true, "Front end locked, 2.4 ppm off reference.");
            status(ui, false, "No GPS fix: timestamps are host clock only.");

            footer(ui, |ui| {
                let _ = ui.button(action("close"));
                let _ = ui.button(action("apply"));
            });
        });

        egui::CentralPanel::default().show(ui, |ui| {
            ui.add_space(6.0);
            tabs(
                ui,
                &mut self.tab,
                &[(Tab::Panel, "panel"), (Tab::Meters, "meters"), (Tab::Data, "data")],
            );
            match self.tab {
                Tab::Panel => self.panel(ui),
                Tab::Meters => self.meters(ui),
                Tab::Data => self.data(ui),
            }
        });
    }
}

impl Gallery {
    fn panel(&mut self, ui: &mut egui::Ui) {
        card(
            ui,
            Some(READOUT),
            |ui| {
                Line::new().legend("tuned").show(ui);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    Line::new().note("what you set is amber").size(10.5).elided(ui);
                });
            },
            |ui| {
                ui.horizontal(|ui| {
                    hero(ui, "centre", "145.5250", "MHz", READOUT);
                    ui.add_space(28.0);
                    hero(ui, "heard", "-93.4", "dBm", TRACE);
                });
                ui.add_space(6.0);
                readouts(
                    ui,
                    &[
                        ("mode", "NFM".into(), READOUT),
                        ("span", "2.40 MHz".into(), READOUT),
                        ("rate", "2.048 MS/s".into(), READOUT),
                        ("offset", "+2.4 ppm".into(), TRACE),
                        ("squelch", format!("{:.0} dB", self.squelch), READOUT),
                        ("agc", "auto".into(), READOUT),
                    ],
                );
            },
        );

        ui.add_space(8.0);
        card(
            ui,
            Some(TRACE),
            |ui| {
                Line::new().legend("charge").show(ui);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    Line::new().measured("4.7 Ah").size(11.0).show(ui);
                });
            },
            |ui| {
                stage_rail(
                    ui,
                    &["precharge", "bulk", "absorb", "float", "done"],
                    Some(2),
                    READOUT,
                    22.0,
                    false,
                );
                exit_note(
                    ui,
                    "Ends when the tail current holds under 1.2 A for five minutes.",
                    LEGEND,
                );
                ui.add_space(6.0);
                progress(ui, "elapsed", 0.62, Some(1.0), "2 h 14 m of an estimated 3 h 36 m");
            },
        );

        ui.add_space(8.0);
        section(ui, "readings", "one galley a line, so the column stays true", |ui| {
            reading(ui, "serial", "SR-0001-A");
            reading(ui, "firmware", "1.4.2");
            reading(ui, "uptime", "6 d 04:11:52");
            note(
                ui,
                "Prose wraps to the pane and is set apart from every reading above it.",
                LEGEND,
            );
        });
    }

    fn meters(&mut self, ui: &mut egui::Ui) {
        section(ui, "audio", "the track is its own meter", |ui| {
            for (i, name) in ["left", "right", "monitor"].iter().enumerate() {
                ui.horizontal(|ui| {
                    ui.add_sized([70.0, 18.0], egui::Label::new(legend(*name)));
                    ui.add(Fader::new(&mut self.gain, self.levels[i]).width(200.0));
                    Line::new().value(format!("{:.0}%", self.gain * 100.0)).size(11.0).show(ui);
                });
            }
            let measured = -120.0 + self.levels[3] * 100.0;
            let open = measured > self.squelch;
            ui.horizontal(|ui| {
                ui.add_sized([70.0, 18.0], egui::Label::new(legend("squelch")));
                ui.add(Threshold::new(&mut self.squelch, -120.0, -20.0, measured, open));
                Line::new().set(format!("{:.0} dB", self.squelch)).size(11.0).show(ui);
            });
        });

        ui.add_space(8.0);
        section(ui, "bare bars", "painted into a rectangle you own", |ui| {
            let w = ui.available_width().min(260.0);
            for (i, name) in ["rf", "if", "af"].iter().enumerate() {
                ui.horizontal(|ui| {
                    ui.add_sized([70.0, 18.0], egui::Label::new(legend(*name)));
                    let (r, _) = ui.allocate_exact_size(egui::vec2(w, VU_H), egui::Sense::hover());
                    vu(ui.painter(), r, self.levels[i + 1]);
                });
            }
            ui.horizontal(|ui| {
                ui.add_sized([70.0, 18.0], egui::Label::new(legend("buffer")));
                let (r, _) = ui.allocate_exact_size(egui::vec2(w, 8.0), egui::Sense::hover());
                bar(ui.painter(), r, self.levels[0], TRACE);
            });
        });

        ui.add_space(8.0);
        section(ui, "traces", "margin against a rule, not a lamp", |ui| {
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    Line::new().legend("speed").show(ui);
                    ui.add(
                        Trace::new(&self.history)
                            .size(egui::vec2(240.0, 44.0))
                            .ratio(1.0, 3.0)
                            .tint(OK),
                    );
                });
                ui.add_space(16.0);
                ui.vertical(|ui| {
                    Line::new().legend("level").show(ui);
                    ui.add(
                        Trace::new(&self.history)
                            .size(egui::vec2(240.0, 44.0))
                            .range(0.0, 3.0)
                            .filled(true),
                    );
                });
            });
        });
    }

    fn data(&mut self, ui: &mut egui::Ui) {
        let cells: Vec<Channel> = (0..15)
            .map(|i| {
                let v = 3320.0 + wobble(self.t, i as f32) * 60.0;
                Channel::marked(v, i % 7 == 3)
            })
            .collect();

        section(ui, "cells", "every rule amber, every bar cyan", |ui| {
            ui.add(
                Comb::new(&cells)
                    .rule("ceiling", 3400.0)
                    .rule("target", 3380.0)
                    .rule("floor", 3000.0)
                    .fault_above(3395.0)
                    .height(190.0)
                    .format(|v| format!("{v:.0}")),
            );
        });

        ui.add_space(8.0);
        section(ui, "heard", "painted rows, not widgets", |ui| {
            let columns: &[(&str, f32)] =
                &[("time", 90.0), ("freq", 100.0), ("mode", 60.0), ("what", 200.0)];
            Table::new(columns, 12).show(ui, |i, p, row, at| {
                let secs = 42 - i as i64;
                cell(p, row, at(0), 90.0, &format!("12:04:{secs:02}"), LEGEND);
                cell(p, row, at(1), 100.0, &format!("{:.4}", 145.0 + i as f32 * 0.0125), TRACE);
                cell(p, row, at(2), 60.0, if i % 3 == 0 { "NFM" } else { "DMR" }, VALUE);
                cell(
                    p,
                    row,
                    at(3),
                    200.0,
                    if i % 4 == 0 { "voice, 3.1 s" } else { "data burst" },
                    VALUE,
                );
            });
        });
    }
}

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1180.0, 800.0]),
        ..Default::default()
    };
    eframe::run_native(
        "egui_bench gallery",
        options,
        Box::new(|cc| {
            egui_bench::install(&cc.egui_ctx);
            Ok(Box::new(Gallery::default()))
        }),
    )
}
