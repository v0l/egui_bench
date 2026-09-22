use crate::theme::{ETCH, LEGEND, TRACE, WELL};
use egui::{Color32, Pos2, Rect, Response, Sense, Stroke, StrokeKind, Ui, Vec2, Widget};

#[derive(Clone, Copy)]
pub enum Scale {
    Linear { lo: f32, hi: f32 },
    Ratio { centre: f32, octaves: f32 },
}

impl Scale {
    fn place(&self, v: f32, r: Rect) -> f32 {
        match *self {
            Scale::Linear { lo, hi } => {
                let t = ((v - lo) / (hi - lo)).clamp(0.0, 1.0);
                r.bottom() - t * r.height()
            }
            Scale::Ratio { centre, octaves } => {
                let t = ((v.max(1e-3) / centre).log2() / octaves).clamp(-1.0, 1.0);
                r.center().y - t * r.height() / 2.0
            }
        }
    }
}

pub struct Trace<'a> {
    points: &'a [f32],
    size: Vec2,
    tint: Color32,
    scale: Scale,
    rule: Option<f32>,
    fill: bool,
}

impl<'a> Trace<'a> {
    pub fn new(points: &'a [f32]) -> Self {
        Self {
            points,
            size: Vec2::new(120.0, 28.0),
            tint: TRACE,
            scale: Scale::Linear { lo: 0.0, hi: 1.0 },
            rule: None,
            fill: false,
        }
    }

    pub fn size(mut self, size: Vec2) -> Self {
        self.size = size;
        self
    }

    pub fn tint(mut self, c: Color32) -> Self {
        self.tint = c;
        self
    }

    pub fn range(mut self, lo: f32, hi: f32) -> Self {
        self.scale = Scale::Linear { lo, hi };
        self
    }

    pub fn ratio(mut self, centre: f32, octaves: f32) -> Self {
        self.scale = Scale::Ratio { centre, octaves };
        self.rule = Some(centre);
        self
    }

    pub fn rule(mut self, at: f32) -> Self {
        self.rule = Some(at);
        self
    }

    pub fn filled(mut self, on: bool) -> Self {
        self.fill = on;
        self
    }
}

impl Widget for Trace<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        let size = Vec2::new(self.size.x.min(ui.available_width()), self.size.y);
        let (rect, resp) = ui.allocate_exact_size(size, Sense::hover());
        if !ui.is_rect_visible(rect) {
            return resp;
        }
        let p = ui.painter_at(rect);
        p.rect_filled(rect, 1.0, WELL);
        p.rect_stroke(rect, 1.0, Stroke::new(1.0, ETCH), StrokeKind::Inside);

        let plot = rect.shrink(2.0);
        if let Some(at) = self.rule {
            let y = self.scale.place(at, plot);
            let mut x = plot.left();
            while x < plot.right() {
                p.line_segment(
                    [Pos2::new(x, y), Pos2::new((x + 2.0).min(plot.right()), y)],
                    Stroke::new(1.0, LEGEND.gamma_multiply(0.7)),
                );
                x += 4.0;
            }
        }

        if self.points.len() > 1 {
            let step = plot.width() / (self.points.len() - 1) as f32;
            let pts: Vec<Pos2> = self
                .points
                .iter()
                .enumerate()
                .map(|(i, v)| {
                    Pos2::new(
                        plot.left() + i as f32 * step,
                        self.scale.place(*v, plot).clamp(plot.top(), plot.bottom()),
                    )
                })
                .collect();
            if self.fill {
                let shade = self.tint.gamma_multiply(0.12);
                for w in pts.windows(2) {
                    p.add(egui::Shape::convex_polygon(
                        vec![
                            w[0],
                            w[1],
                            Pos2::new(w[1].x, plot.bottom()),
                            Pos2::new(w[0].x, plot.bottom()),
                        ],
                        shade,
                        Stroke::NONE,
                    ));
                }
            }
            p.add(egui::Shape::line(pts, Stroke::new(1.0, self.tint)));
        }
        resp
    }
}
