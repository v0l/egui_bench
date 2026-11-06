use crate::markdown;
use crate::prelude::*;
use egui::{Align2, Margin, RichText, Sense, Stroke, Ui, vec2};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    System,
    User,
    Assistant,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Message {
    pub role: Role,
    pub text: String,
}

impl Message {
    pub fn new(role: Role, text: impl Into<String>) -> Self {
        Self { role, text: text.into() }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    Sent(String),
    Stop,
    Regenerate(usize),
    Copy(usize),
    Select(usize),
    Delete(usize),
    New,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub title: String,
    pub subtitle: String,
}

#[derive(Default)]
pub struct State {
    pub draft: String,
    pub markdown: markdown::Cache,
}

pub type Strip<'a> = &'a mut dyn FnMut(&mut Ui, usize);

pub fn submit(draft: &mut String, enter: bool) -> Option<String> {
    if !enter {
        return None;
    }
    if draft.ends_with('\n') {
        draft.pop();
    }
    let text = draft.trim().to_string();
    if text.is_empty() {
        return None;
    }
    draft.clear();
    Some(text)
}

pub fn context_bar(ui: &mut Ui, used: usize, limit: usize) {
    let frac = used as f32 / limit.max(1) as f32;
    ui.horizontal(|ui| {
        Line::new().legend("context").measured(format!("{used} / {limit}")).size(11.0).show(ui);
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 10.0), Sense::hover());
        bar(ui.painter(), rect, frac, if frac > 0.85 { WARN } else { TRACE });
    });
}

pub fn ask_box(ui: &mut Ui, draft: &mut String, hint: &str, busy: bool, ready: bool) -> Vec<Event> {
    let mut events = Vec::new();
    let mut enter = false;
    ui.horizontal(|ui| {
        let w = ui.available_width() - 120.0;
        let r = ui.add_sized(
            [w, 56.0],
            egui::TextEdit::multiline(draft).hint_text(hint).desired_rows(2).font(figure(13.0)),
        );
        enter = r.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter) && !i.modifiers.shift);
        ui.vertical(|ui| {
            if busy {
                if ui.add(Key::new("stop").solid(FAULT).min_width(110.0)).clicked() {
                    events.push(Event::Stop);
                }
            } else if ui.add(Key::new("send").solid(READOUT).enabled(ready).min_width(110.0)).clicked() {
                enter = true;
            }
        });
    });
    if enter && ready && !busy {
        if let Some(text) = submit(draft, true) {
            events.push(Event::Sent(text));
        }
    } else if enter && draft.ends_with('\n') {
        draft.pop();
    }
    events
}

pub fn bubble(
    ui: &mut Ui,
    cache: &mut markdown::Cache,
    message: &Message,
    index: usize,
    last: bool,
    busy: bool,
    strip: Option<Strip>,
) -> Vec<Event> {
    let mut events = Vec::new();
    let (name, rail, fill) = match message.role {
        Role::User => ("YOU", READOUT, PANEL),
        Role::Assistant => ("ASSISTANT", TRACE, CHASSIS),
        Role::System => ("SYSTEM", LEGEND, CHASSIS),
    };
    egui::Frame::NONE
        .fill(fill)
        .stroke(Stroke::new(1.0, ETCH))
        .corner_radius(2)
        .inner_margin(Margin::symmetric(12, 8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new(name).font(legend_font(10.5)).extra_letter_spacing(1.5).color(rail));
            ui.add_space(2.0);
            if message.role == Role::Assistant {
                markdown::show(ui, cache, &message.text, index);
            } else {
                ui.label(RichText::new(&message.text).font(figure(13.0)).color(VALUE));
            }
            if message.role != Role::Assistant {
                return;
            }
            ui.add_space(4.0);
            if let Some(strip) = strip {
                strip(ui, index);
            }
            ui.horizontal(|ui| {
                if ui.add(Key::new("copy").outline(VALUE).min_width(80.0).id_salt(("copy", index))).clicked() {
                    ui.ctx().copy_text(message.text.clone());
                    events.push(Event::Copy(index));
                }
                if last
                    && !busy
                    && ui
                        .add(Key::new("regenerate").outline(VALUE).min_width(130.0).id_salt(("regen", index)))
                        .clicked()
                {
                    events.push(Event::Regenerate(index));
                }
            });
        });
    events
}

pub fn transcript(
    ui: &mut Ui,
    cache: &mut markdown::Cache,
    messages: &[Message],
    streaming: Option<&str>,
    mut strip: Option<Strip>,
) -> Vec<Event> {
    let mut events = Vec::new();
    egui::ScrollArea::vertical().auto_shrink([false, false]).stick_to_bottom(true).show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 10.0;
        let shown: Vec<(usize, &Message)> =
            messages.iter().enumerate().filter(|(_, m)| m.role != Role::System).collect();
        let tail = shown.last().map(|(i, _)| *i);
        for (i, m) in shown {
            let strip = strip.as_mut().map(|s| &mut **s as Strip);
            events.extend(bubble(ui, cache, m, i, Some(i) == tail, streaming.is_some(), strip));
        }
        if let Some(text) = streaming {
            let live = Message::new(Role::Assistant, if text.is_empty() { "..." } else { text });
            ui.scope(|ui| {
                bubble_frame_only(ui, cache, &live);
            });
        }
    });
    events
}

fn bubble_frame_only(ui: &mut Ui, cache: &mut markdown::Cache, message: &Message) {
    egui::Frame::NONE
        .fill(CHASSIS)
        .stroke(Stroke::new(1.0, ETCH))
        .corner_radius(2)
        .inner_margin(Margin::symmetric(12, 8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new("ASSISTANT").font(legend_font(10.5)).extra_letter_spacing(1.5).color(TRACE));
            ui.add_space(2.0);
            markdown::show(ui, cache, &message.text, usize::MAX);
        });
}

pub fn conversations(ui: &mut Ui, entries: &[Entry], current: usize) -> Vec<Event> {
    let mut events = Vec::new();
    if ui.add(Key::new("new chat").solid(READOUT).min_width(ui.available_width())).clicked() {
        events.push(Event::New);
    }
    ui.add_space(6.0);
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        for (i, e) in entries.iter().enumerate() {
            let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 44.0), Sense::click());
            let p = ui.painter();
            let on = i == current;
            if on {
                p.rect_filled(rect, 2.0, PANEL);
                p.rect_filled(egui::Rect::from_min_size(rect.min, vec2(3.0, rect.height())), 0.0, READOUT);
            } else if resp.hovered() {
                p.rect_filled(rect, 2.0, BAND);
            }
            p.text(
                rect.left_top() + vec2(12.0, 12.0),
                Align2::LEFT_CENTER,
                &e.title,
                legend_font(12.0),
                if on { VALUE } else { LEGEND },
            );
            p.text(rect.left_top() + vec2(12.0, 30.0), Align2::LEFT_CENTER, &e.subtitle, figure(10.0), LEGEND);
            if resp.clicked() {
                events.push(Event::Select(i));
            }
            resp.context_menu(|ui| {
                if ui.add(Key::new("delete chat").outline(FAULT).min_width(ui.available_width())).clicked() {
                    events.push(Event::Delete(i));
                    ui.close();
                }
            });
        }
    });
    events
}

pub fn welcome(ui: &mut Ui, title: &str, text: &str) {
    ui.add_space(80.0);
    ui.vertical_centered(|ui| {
        ui.label(RichText::new(title).font(legend_font(18.0)).extra_letter_spacing(3.0).color(LEGEND));
        note(ui, text, LEGEND);
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RunFrame;

    fn frame(ctx: &egui::Context, f: impl FnMut(&mut Ui)) {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, vec2(800.0, 600.0))),
            ..Default::default()
        };
        ctx.run_frame(input, f);
    }

    #[test]
    fn enter_sends_the_trimmed_draft_and_clears_it() {
        let mut draft = "hello\n".to_string();
        assert_eq!(submit(&mut draft, true), Some("hello".to_string()));
        assert!(draft.is_empty());
    }

    #[test]
    fn a_blank_draft_or_no_enter_sends_nothing() {
        let mut blank = "  \n".to_string();
        assert_eq!(submit(&mut blank, true), None);
        let mut draft = "hi".to_string();
        assert_eq!(submit(&mut draft, false), None);
        assert_eq!(draft, "hi");
    }

    #[test]
    fn the_stats_hook_runs_once_per_assistant_reply_only() {
        let ctx = egui::Context::default();
        crate::install(&ctx);
        let messages = vec![
            Message::new(Role::System, "be brief"),
            Message::new(Role::User, "hi"),
            Message::new(Role::Assistant, "hello"),
            Message::new(Role::User, "again"),
            Message::new(Role::Assistant, "```rs\nfn main() {}\n```"),
        ];
        let mut seen = Vec::new();
        let mut cache = markdown::Cache::default();
        frame(&ctx, |ui| {
            let mut hook = |_: &mut Ui, i: usize| seen.push(i);
            transcript(ui, &mut cache, &messages, None, Some(&mut hook));
        });
        assert_eq!(seen, vec![2, 4]);
    }

    #[test]
    fn the_conversation_list_and_context_bar_draw_without_events() {
        let ctx = egui::Context::default();
        crate::install(&ctx);
        let entries = vec![Entry { title: "a".into(), subtitle: "2 messages".into() }];
        let mut events = Vec::new();
        frame(&ctx, |ui| {
            events = conversations(ui, &entries, 0);
            context_bar(ui, 10, 100);
        });
        assert!(events.is_empty());
    }

    #[test]
    fn a_streaming_reply_draws_a_live_bubble() {
        let ctx = egui::Context::default();
        crate::install(&ctx);
        let mut cache = markdown::Cache::default();
        let mut events = Vec::new();
        frame(&ctx, |ui| {
            events = transcript(ui, &mut cache, &[Message::new(Role::User, "hi")], Some("par"), None);
        });
        assert!(events.is_empty());
        assert!(cache.len() >= 1);
    }
}
