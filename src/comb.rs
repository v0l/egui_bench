use crate::theme::{self, BAND, FAULT, LEGEND, READOUT, TRACE, VALUE, WELL};
use egui::{Color32, Pos2, Rect, Response, Sense, Stroke, Ui, Vec2, Widget};

#[derive(Clone, Copy, Default)]
pub struct Channel {
    pub value: f32,
    pub marked: bool,
}

impl Channel {
    pub fn new(value: f32) -> Self {
        Self { value, marked: false }
    }

    pub fn marked(value: f32, marked: bool) -> Self {
        Self { value, marked }
    }
}

pub struct Comb<'a> {
    channels: &'a [Channel],
    shown: Option<&'a [f32]>,
    rules: Vec<(&'a str, f32, f32)>,
    height: f32,
    fault_above: Option<f32>,
    prefix: &'a str,
    format: Box<dyn Fn(f32) -> String + 'a>,
    empty: &'a str,
}

const GUTTER: f32 = 58.0;
const FOOT: f32 = 28.0;
const HEAD: f32 = 16.0;

impl<'a> Comb<'a> {
    pub fn new(channels: &'a [Channel]) -> Self {
        Self {
            channels,
            shown: None,
            rules: Vec::new(),
            height: 150.0,
            fault_above: None,
            prefix: "c",
            format: Box::new(|v| format!("{v:.0}")),
            empty: "NO CHANNELS REPORTED",
        }
    }

    pub fn animated(mut self, shown: &'a [f32]) -> Self {
        self.shown = Some(shown);
        self
    }

    pub fn rule(mut self, name: &'a str, at: f32) -> Self {
        let weight = if self.rules.is_empty() { 0.85 } else { 0.5 };
        self.rules.push((name, at, weight));
        self
    }

    pub fn height(mut self, h: f32) -> Self {
        self.height = h;
        self
    }

    pub fn fault_above(mut self, at: f32) -> Self {
        self.fault_above = Some(at);
        self
    }

    pub fn prefix(mut self, p: &'a str) -> Self {
        self.prefix = p;
        self
    }

    pub fn format(mut self, f: impl Fn(f32) -> String + 'a) -> Self {
        self.format = Box::new(f);
        self
    }

    pub fn empty(mut self, text: &'a str) -> Self {
        self.empty = text;
        self
    }
}

impl Widget for Comb<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        let w = ui.available_width();
        let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, self.height), Sense::hover());
        if !ui.is_rect_visible(rect) {
            return resp;
        }
        let p = ui.painter_at(rect);
        p.rect_filled(rect, 2.0, WELL);

        if self.channels.is_empty() {
            let g = p.layout_no_wrap(
                self.empty.to_uppercase(),
                theme::legend_font(theme::LEGEND_SIZE),
                LEGEND,
            );
            p.galley(rect.center() - g.size() / 2.0, g, LEGEND);
            return resp;
        }

        let plot = Rect::from_min_max(
            Pos2::new(rect.left() + 8.0, rect.top() + 8.0 + HEAD),
            Pos2::new(rect.right() - GUTTER, rect.bottom() - 6.0 - FOOT),
        );
        if plot.width() < 20.0 || plot.height() < 20.0 {
            return resp;
        }

        let lo = self.channels.iter().map(|c| c.value).fold(f32::INFINITY, f32::min);
        let hi = self.channels.iter().map(|c| c.value).fold(f32::NEG_INFINITY, f32::max);
        let spread = (hi - lo).max(0.0);
        let ceiling = self.rules.first().map(|r| r.1).unwrap_or(hi);
        let pad = (spread * 0.6).max((hi.abs().max(1.0)) * 0.01);
        let band_hi = (ceiling + pad * 0.5).max(hi + pad * 0.5);
        let band_lo = (lo - pad).min(band_hi - pad.max(1e-3));
        let y_of = |v: f32| plot.bottom() - (v - band_lo) / (band_hi - band_lo) * plot.height();

        for (name, at, weight) in &self.rules {
            let y = y_of(*at);
            if !(plot.top() - 1.0..=plot.bottom() + 1.0).contains(&y) {
                continue;
            }
            p.add(egui::Shape::dashed_line(
                &[Pos2::new(plot.left(), y), Pos2::new(plot.right(), y)],
                Stroke::new(1.0, READOUT.gamma_multiply(weight * 0.7)),
                5.0,
                4.0,
            ));
            let g = p.layout_no_wrap(
                (self.format)(*at),
                theme::figure(11.0),
                READOUT.gamma_multiply(*weight),
            );
            p.galley(Pos2::new(plot.right() + 6.0, y - g.size().y / 2.0), g, READOUT);
            let g = p.layout_no_wrap(
                name.to_uppercase(),
                theme::legend_font(9.5),
                LEGEND.gamma_multiply(0.8),
            );
            p.galley(Pos2::new(plot.right() + 6.0, y + 5.0), g, LEGEND);
        }

        let n = self.channels.len();
        let col = plot.width() / n as f32;
        let tooth_w = (col - 6.0).clamp(2.0, 42.0);
        let label_channels = col >= 18.0;
        let label_values = col >= 34.0;
        let width = n.saturating_sub(1).to_string().len();
        for (i, c) in self.channels.iter().enumerate() {
            let v = self.shown.and_then(|s| s.get(i).copied()).unwrap_or(c.value);
            let cx = plot.left() + col * (i as f32 + 0.5);
            let y = y_of(v).clamp(plot.top() + 1.0, plot.bottom());
            let governs = c.value == hi || c.value == lo;
            let tint: Color32 = match self.fault_above {
                Some(at) if c.value >= at => FAULT,
                _ => TRACE,
            };
            let half = tooth_w / 2.0;
            p.line_segment(
                [Pos2::new(cx, plot.top()), Pos2::new(cx, plot.bottom())],
                Stroke::new(1.0, BAND.gamma_multiply(0.55)),
            );
            p.rect_filled(
                Rect::from_min_max(Pos2::new(cx - half, y), Pos2::new(cx + half, plot.bottom())),
                0.0,
                tint.gamma_multiply(if governs { 0.16 } else { 0.09 }),
            );
            p.rect_filled(
                Rect::from_min_max(
                    Pos2::new(cx - half, y),
                    Pos2::new(cx + half, y + if governs { 4.0 } else { 3.0 }),
                ),
                0.0,
                tint.gamma_multiply(if governs { 1.0 } else { 0.62 }),
            );
            if c.marked {
                p.rect_filled(
                    Rect::from_center_size(Pos2::new(cx, y - 6.0), Vec2::splat(4.0)),
                    0.0,
                    READOUT,
                );
            }
            if governs && n > 1 {
                let tag = if c.value == hi { "HIGH" } else { "LOW" };
                let g = p.layout_no_wrap(
                    format!("{tag} {}", (self.format)(c.value)),
                    theme::legend_font(10.5),
                    tint,
                );
                let x = (cx - g.size().x / 2.0).clamp(plot.left(), plot.right() - g.size().x);
                p.galley(Pos2::new(x, (y - 18.0).max(rect.top() + 2.0)), g, tint);
            }
            if label_channels || governs {
                let ink = if governs { tint } else { LEGEND.gamma_multiply(0.75) };
                let g =
                    p.layout_no_wrap(format!("{}{i:0width$}", self.prefix), theme::mono(9.5), ink);
                p.galley(Pos2::new(cx - g.size().x / 2.0, plot.bottom() + 3.0), g, ink);
                if label_values {
                    let g = p.layout_no_wrap(
                        (self.format)(c.value),
                        theme::figure(10.0),
                        if governs { tint } else { VALUE.gamma_multiply(0.8) },
                    );
                    p.galley(Pos2::new(cx - g.size().x / 2.0, plot.bottom() + 14.0), g, VALUE);
                }
            }
        }
        resp
    }
}
