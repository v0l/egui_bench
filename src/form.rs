use crate::readout::LABEL_W;
use crate::text::{help, legend};
use crate::theme::{self, ETCH, LEGEND, RADIUS, VALUE, WELL};
use egui::{Layout, Pos2, Response, Stroke, Ui};

pub fn modal_title(ui: &mut Ui, text: &str) {
    ui.label(legend(text));
    ui.add_space(10.0);
}

pub fn row(ui: &mut Ui, label: &str, add: impl FnOnce(&mut Ui)) {
    ui.horizontal(|ui| {
        ui.add_sized([LABEL_W, 18.0], egui::Label::new(legend(label)));
        add(ui);
    });
}

pub fn row_help(ui: &mut Ui, label: &str, text: &str, add: impl FnOnce(&mut Ui)) {
    ui.horizontal(|ui| {
        ui.add_sized([LABEL_W, 18.0], egui::Label::new(legend(label)));
        ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
            help(ui, text);
            ui.with_layout(Layout::left_to_right(egui::Align::Center), add);
        });
    });
}

pub fn switch(ui: &mut Ui, label: &str, on: &mut bool, text: &str, explain: &str) -> bool {
    let mut changed = false;
    row_help(ui, label, explain, |ui| {
        changed = ui.checkbox(on, text).changed();
    });
    changed
}

pub fn choice<T: PartialEq + Clone>(
    ui: &mut Ui,
    id: impl std::hash::Hash + std::fmt::Debug,
    picked: &mut T,
    options: impl IntoIterator<Item = (T, String)>,
) -> bool {
    let options: Vec<(T, String)> = options.into_iter().collect();
    let shown = options.iter().find(|(v, _)| v == picked).map(|(_, l)| l.clone());
    let mut changed = false;
    egui::ComboBox::from_id_salt(id)
        .selected_text(shown.unwrap_or_default())
        .width(ui.available_width())
        .show_ui(ui, |ui| {
            for (v, label) in options {
                let on = *picked == v;
                if ui.selectable_label(on, label).clicked() && !on {
                    *picked = v;
                    changed = true;
                }
            }
        });
    changed
}

pub fn field(ui: &mut Ui, text: &mut String, hint: &str) -> Response {
    field_of(ui, text, hint, false, 1, f32::INFINITY)
}

pub fn field_then(
    ui: &mut Ui,
    text: &mut String,
    hint: &str,
    reserve: f32,
    after: impl FnOnce(&mut Ui),
) -> Response {
    let w = (ui.available_width() - reserve).max(60.0);
    let r = field_of(ui, text, hint, false, 1, w);
    after(ui);
    r
}

pub fn secret(ui: &mut Ui, text: &mut String) -> Response {
    field_of(ui, text, "", true, 1, f32::INFINITY)
}

pub fn prose(ui: &mut Ui, text: &mut String, hint: &str, rows: usize) -> Response {
    field_of(ui, text, hint, false, rows, f32::INFINITY)
}

fn field_of(
    ui: &mut Ui,
    text: &mut String,
    hint: &str,
    secret: bool,
    rows: usize,
    width: f32,
) -> Response {
    let font = theme::figure(12.5);
    let hint = egui::RichText::new(hint).font(font.clone()).color(LEGEND.gamma_multiply(0.6));
    egui::Frame::NONE
        .fill(WELL)
        .stroke(Stroke::new(1.0, ETCH))
        .corner_radius(RADIUS)
        .inner_margin(egui::Margin { left: 6, right: 6, top: 3, bottom: 3 })
        .show(ui, |ui| {
            let mut edit = if rows > 1 {
                egui::TextEdit::multiline(text).desired_rows(rows)
            } else {
                egui::TextEdit::singleline(text)
            };
            edit = edit
                .frame(egui::Frame::NONE)
                .font(font)
                .text_color(VALUE)
                .hint_text(hint)
                .password(secret)
                .desired_width(if width.is_finite() { width - 12.0 } else { width });
            ui.add(edit)
        })
        .inner
}

pub fn footer(ui: &mut Ui, buttons: impl FnOnce(&mut Ui)) {
    ui.add_space(10.0);
    let r = ui.available_rect_before_wrap();
    ui.painter().line_segment(
        [Pos2::new(r.left(), r.top()), Pos2::new(r.right(), r.top())],
        Stroke::new(1.0, ETCH),
    );
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        ui.with_layout(Layout::right_to_left(egui::Align::Center), buttons);
    });
}
