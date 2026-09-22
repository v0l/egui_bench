use crate::text::{self, Line};
use crate::theme::{
    self, ETCH, FAULT, LEGEND, LEGEND_SIZE, OK, PANEL, RADIUS, RAIL_W, READOUT, VALUE, WELL,
};
use egui::{Color32, InnerResponse, Pos2, Rect, Response, Sense, Stroke, StrokeKind, Ui, Vec2};

const PAD_X: i8 = 10;

pub fn card<R>(
    ui: &mut Ui,
    rail: Option<Color32>,
    header: impl FnOnce(&mut Ui),
    body: impl FnOnce(&mut Ui) -> R,
) -> InnerResponse<R> {
    let outer = egui::Frame::NONE.fill(PANEL).stroke(Stroke::new(1.0, ETCH)).corner_radius(RADIUS);
    let framed = outer.show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        let head = egui::Frame::NONE.fill(WELL).inner_margin(egui::Margin {
            left: PAD_X,
            right: PAD_X,
            top: 4,
            bottom: 4,
        });
        let h = head.show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| header(ui));
        });
        let r = h.response.rect;
        ui.painter().line_segment(
            [Pos2::new(r.left(), r.bottom()), Pos2::new(r.right(), r.bottom())],
            Stroke::new(1.0, ETCH),
        );
        egui::Frame::NONE
            .inner_margin(egui::Margin { left: PAD_X, right: PAD_X, top: 6, bottom: 8 })
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 4.0;
                ui.set_width(ui.available_width());
                body(ui)
            })
            .inner
    });
    if let Some(c) = rail {
        let r = framed.response.rect;
        ui.painter().rect_filled(
            Rect::from_min_max(r.left_top(), Pos2::new(r.left() + RAIL_W, r.bottom())),
            0.0,
            c,
        );
    }
    framed
}

pub fn section<R>(
    ui: &mut Ui,
    label: &str,
    note: &str,
    body: impl FnOnce(&mut Ui) -> R,
) -> InnerResponse<R> {
    card(
        ui,
        None,
        |ui| {
            Line::new().legend(label).show(ui);
            if !note.is_empty() {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    Line::new().note(note).size(10.5).elided(ui);
                });
            }
        },
        body,
    )
}

pub fn lamp(ui: &mut Ui, text: &str, on: bool, fault: bool) -> Response {
    let tint = if fault {
        FAULT
    } else if on {
        OK
    } else {
        LEGEND
    };
    let galley =
        ui.painter().layout_no_wrap(text.to_uppercase(), theme::legend_font(LEGEND_SIZE), tint);
    let size = galley.size() + Vec2::new(14.0, 6.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::hover());
    if ui.is_rect_visible(rect) {
        let p = ui.painter();
        p.rect_filled(rect, RADIUS as f32, if on { tint.gamma_multiply(0.18) } else { WELL });
        p.rect_stroke(
            rect,
            RADIUS as f32,
            Stroke::new(1.0, if on || fault { tint } else { ETCH }),
            StrokeKind::Inside,
        );
        p.galley(rect.center() - galley.size() / 2.0, galley, tint);
    }
    resp
}

pub fn toggle(ui: &mut Ui, text: &str, on: bool) -> Response {
    let tint = if on { READOUT } else { LEGEND };
    let galley =
        ui.painter().layout_no_wrap(text.to_uppercase(), theme::legend_font(LEGEND_SIZE), tint);
    let size = galley.size() + Vec2::new(26.0, 7.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let lit = on || resp.hovered();
    if ui.is_rect_visible(rect) {
        let p = ui.painter();
        let fill = if on {
            tint.gamma_multiply(0.18)
        } else if resp.hovered() {
            ETCH.gamma_multiply(0.6)
        } else {
            WELL
        };
        p.rect_filled(rect, RADIUS as f32, fill);
        p.rect_stroke(
            rect,
            RADIUS as f32,
            Stroke::new(1.0, if lit { tint } else { ETCH }),
            StrokeKind::Inside,
        );
        let tick = Rect::from_center_size(
            Pos2::new(rect.left() + 11.0, rect.center().y),
            Vec2::splat(8.0),
        );
        p.rect_stroke(
            tick,
            1.0,
            Stroke::new(1.0, if lit { tint } else { LEGEND }),
            StrokeKind::Inside,
        );
        if on {
            p.rect_filled(tick.shrink(2.0), 0.0, tint);
        }
        p.galley(
            Pos2::new(tick.right() + 6.0, rect.center().y - galley.size().y / 2.0),
            galley,
            tint,
        );
    }
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
}

pub fn status(ui: &mut Ui, ok: bool, text: &str) {
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(Vec2::new(10.0, 18.0), Sense::hover());
        let c = rect.center();
        let col = if ok { OK } else { FAULT };
        ui.painter().circle_filled(c, 3.0, col);
        ui.painter().circle_stroke(c, 4.5, Stroke::new(1.0, col.gamma_multiply(0.4)));
        Line::new().value(text).tint(col).size(11.0).wrapped(ui);
    });
}

pub fn stage_rail(
    ui: &mut Ui,
    stages: &[&str],
    at: Option<usize>,
    now: Color32,
    height: f32,
    muted: bool,
) -> Response {
    let w = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, height), Sense::hover());
    if !ui.is_rect_visible(rect) {
        return resp;
    }
    let p = ui.painter();
    let n = stages.len().max(1);
    let gap = 5.0;
    let seg_w = ((rect.width() - gap * (n as f32 - 1.0)) / n as f32).max(8.0);
    for (i, name) in stages.iter().enumerate() {
        let x = rect.left() + i as f32 * (seg_w + gap);
        let seg = Rect::from_min_size(Pos2::new(x, rect.top()), Vec2::new(seg_w, rect.height()));
        let (fill, edge, ink) = match at {
            Some(a) if i == a => (if muted { WELL } else { now.gamma_multiply(0.20) }, now, now),
            Some(a) if i < a => (WELL, ETCH, OK.gamma_multiply(0.85)),
            _ => (WELL, ETCH, LEGEND.gamma_multiply(0.65)),
        };
        p.rect_filled(seg, RADIUS as f32, fill);
        p.rect_stroke(seg, RADIUS as f32, Stroke::new(1.0, edge), StrokeKind::Inside);
        let size = if height < 22.0 { LEGEND_SIZE - 1.5 } else { LEGEND_SIZE };
        let galley = p.layout_no_wrap(name.to_uppercase(), theme::legend_font(size), ink);
        p.galley(seg.center() - galley.size() / 2.0, galley, ink);
    }
    resp
}

pub fn exit_note(ui: &mut Ui, text: &str, tint: Color32) {
    if text.is_empty() {
        return;
    }
    ui.add_space(3.0);
    ui.label(egui::RichText::new(text).font(theme::legend_font(12.0)).color(tint));
}

pub fn tabs<T: PartialEq + Copy>(ui: &mut Ui, current: &mut T, options: &[(T, &str)]) -> bool {
    const TAB_H: f32 = 20.0;
    const MARK_H: f32 = 2.0;
    const GAP: f32 = 20.0;

    let width = ui.available_width();
    let (strip, _) = ui.allocate_exact_size(Vec2::new(width, TAB_H + MARK_H), Sense::hover());
    let mut changed = false;
    let mut marks = Vec::with_capacity(options.len());
    let mut x = strip.left();
    for (i, (value, label)) in options.iter().enumerate() {
        let galley = ui.painter().layout_job(text::legend_job(label));
        let w = galley.size().x;
        let hit = Rect::from_min_size(Pos2::new(x, strip.top()), Vec2::new(w, TAB_H));
        let r = ui.interact(hit, ui.id().with(("tab", i)), Sense::click());
        let on = *current == *value;
        if r.clicked() && !on {
            *current = *value;
            changed = true;
        }
        let colour = if on || r.hovered() { VALUE } else { LEGEND };
        let at = Pos2::new(x, strip.top() + (TAB_H - galley.size().y) * 0.5);
        ui.painter().galley(at, galley, colour);
        marks.push((x, w, on));
        x += w + GAP;
    }
    let rule = Rect::from_min_size(
        Pos2::new(strip.left(), strip.bottom() - MARK_H),
        Vec2::new(width, 1.0),
    );
    ui.painter().rect_filled(rule, 0.0, ETCH);
    for (x, w, on) in marks {
        if on {
            let mark =
                Rect::from_min_size(Pos2::new(x, strip.bottom() - MARK_H), Vec2::new(w, MARK_H));
            ui.painter().rect_filled(mark, 0.0, READOUT);
        }
    }
    ui.add_space(10.0);
    changed
}
