use crate::text::{Line, legend};
use crate::theme::{self, LEGEND, LEGEND_SIZE};
use egui::{Color32, Pos2, RichText, Sense, Ui};

pub const READOUT_W: f32 = 104.0;
pub const LABEL_W: f32 = 90.0;
const LABEL_GAP: f32 = 8.0;

pub fn readout(ui: &mut Ui, label: &str, v: impl Into<String>, tint: Color32) {
    let v = v.into();
    ui.allocate_ui_with_layout(
        egui::vec2(READOUT_W, 0.0),
        egui::Layout::top_down(egui::Align::Min),
        |ui| {
            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
            ui.spacing_mut().item_spacing.y = 2.0;
            ui.label(legend(label));
            ui.label(RichText::new(v).font(theme::figure(17.0)).color(tint));
        },
    );
}

pub fn readouts(ui: &mut Ui, items: &[(&str, String, Color32)]) {
    const GAP: f32 = 18.0;
    let per_row = ((ui.available_width() / (READOUT_W + GAP)).floor() as usize).max(1);
    for chunk in items.chunks(per_row) {
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = GAP;
            for (label, v, tint) in chunk {
                readout(ui, label, v.clone(), *tint);
            }
        });
        ui.add_space(4.0);
    }
}

pub fn hero(ui: &mut Ui, label: &str, v: &str, unit: &str, tint: Color32) {
    const SIZE: f32 = 42.0;
    let figure_g = ui.painter().layout_no_wrap(v.to_owned(), theme::figure(SIZE), tint);
    let unit_g = ui.painter().layout_no_wrap(
        unit.to_uppercase(),
        theme::legend_font(14.0),
        tint.gamma_multiply(0.75),
    );
    let label_g =
        ui.painter().layout_no_wrap(label.to_uppercase(), theme::legend_font(LEGEND_SIZE), LEGEND);
    let w = figure_g.size().x + 5.0 + unit_g.size().x;
    let h = figure_g.size().y + 1.0 + label_g.size().y;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, h), Sense::hover());
    if !ui.is_rect_visible(rect) {
        return;
    }
    let base = rect.top() + figure_g.size().y;
    let p = ui.painter();
    let unit_at = Pos2::new(
        rect.left() + figure_g.size().x + 5.0,
        base - unit_g.size().y - figure_g.size().y * 0.14,
    );
    p.galley(rect.left_top(), figure_g, tint);
    p.galley(unit_at, unit_g, tint);
    p.galley(Pos2::new(rect.left(), base + 1.0), label_g, LEGEND);
}

pub fn reading(ui: &mut Ui, label: &str, text: impl Into<String>) {
    Line::new().legend(label).column(ui, LABEL_W + LABEL_GAP).value(text).size(11.0).show(ui);
}
