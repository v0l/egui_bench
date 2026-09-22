use crate::theme::{self, ETCH, LEGEND, WELL};
use egui::{Align2, Color32, Pos2, Rect, Ui, Vec2};

pub const ROW_H: f32 = 16.0;

pub fn cell(p: &egui::Painter, row: Rect, x: f32, w: f32, text: &str, col: Color32) {
    let r = Rect::from_min_max(Pos2::new(x, row.top()), Pos2::new(x + w - 6.0, row.bottom()));
    p.with_clip_rect(r.intersect(p.clip_rect())).text(
        Pos2::new(r.left(), r.center().y),
        Align2::LEFT_CENTER,
        text,
        theme::figure(11.0),
        col,
    );
}

pub fn header(p: &egui::Painter, row: Rect, columns: &[(&str, f32)]) {
    p.rect_filled(row, 0.0, WELL);
    p.line_segment(
        [Pos2::new(row.left(), row.bottom()), Pos2::new(row.right(), row.bottom())],
        egui::Stroke::new(1.0, ETCH),
    );
    let mut x = row.left() + 6.0;
    for (name, w) in columns {
        p.with_clip_rect(row).text(
            Pos2::new(x, row.center().y),
            Align2::LEFT_CENTER,
            name.to_uppercase(),
            theme::legend_font(10.5),
            LEGEND,
        );
        x += w;
    }
}

pub struct Table<'a> {
    columns: &'a [(&'a str, f32)],
    rows: usize,
    striped: bool,
}

impl<'a> Table<'a> {
    pub fn new(columns: &'a [(&'a str, f32)], rows: usize) -> Self {
        Self { columns, rows, striped: true }
    }

    pub fn striped(mut self, on: bool) -> Self {
        self.striped = on;
        self
    }

    pub fn show(
        self,
        ui: &mut Ui,
        mut row: impl FnMut(usize, &egui::Painter, Rect, &dyn Fn(usize) -> f32),
    ) {
        let width = ui.available_width();
        let (rect, _) = ui.allocate_exact_size(
            Vec2::new(width, ROW_H + self.rows as f32 * ROW_H),
            egui::Sense::hover(),
        );
        if !ui.is_rect_visible(rect) {
            return;
        }
        let p = ui.painter_at(rect);
        let head = Rect::from_min_size(rect.min, Vec2::new(width, ROW_H));
        header(&p, head, self.columns);

        let starts: Vec<f32> = self
            .columns
            .iter()
            .scan(rect.left() + 6.0, |x, (_, w)| {
                let at = *x;
                *x += w;
                Some(at)
            })
            .collect();
        let at = |i: usize| starts.get(i).copied().unwrap_or(rect.left());

        for i in 0..self.rows {
            let r = Rect::from_min_size(
                Pos2::new(rect.left(), head.bottom() + i as f32 * ROW_H),
                Vec2::new(width, ROW_H),
            );
            if self.striped && i % 2 == 1 {
                p.rect_filled(r, 0.0, WELL.gamma_multiply(0.6));
            }
            row(i, &p, r, &at);
        }
    }
}
