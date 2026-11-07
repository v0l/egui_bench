use crate::markdown;
use crate::prelude::*;
use egui::{Align, Align2, Layout, Margin, RichText, Sense, Stroke, Ui, vec2};

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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Model<'a> {
    None,
    Loading(&'a str),
    Ready(&'a str),
    Failed(&'a str),
}

pub type Strip<'a> = &'a mut dyn FnMut(&mut Ui, usize);

const OPEN_THINK: &str = "<think>";
const CLOSE_THINK: &str = "</think>";

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

pub fn split_thinking(text: &str) -> (Option<&str>, &str) {
    let lead = text.trim_start();
    let Some(rest) = lead.strip_prefix(OPEN_THINK) else { return (None, text) };
    match rest.find(CLOSE_THINK) {
        Some(end) => {
            let thought = rest[..end].trim();
            let reply = rest[end + CLOSE_THINK.len()..].trim_start();
            ((!thought.is_empty()).then_some(thought), reply)
        }
        None => (Some(rest.trim()), ""),
    }
}

pub fn context_bar(ui: &mut Ui, used: usize, limit: usize) {
    let frac = used as f32 / limit.max(1) as f32;
    ui.horizontal(|ui| {
        Line::new().legend("context").measured(format!("{used} / {limit}")).size(11.0).show(ui);
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 6.0), Sense::hover());
        bar(ui.painter(), rect, frac, if frac > 0.85 { WARN } else { TRACE });
    });
}

pub fn header(ui: &mut Ui, model: Model, busy: bool, right: impl FnOnce(&mut Ui)) {
    ui.horizontal(|ui| {
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            right(ui);
            ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                match model {
                    Model::None => {
                        lamp(ui, "no model", false, false);
                    }
                    Model::Loading(name) => {
                        lamp(ui, "loading", true, false);
                        chip(ui, name, READOUT_DIM, false);
                    }
                    Model::Ready(name) => {
                        lamp(ui, if busy { "generating" } else { "ready" }, true, false);
                        chip(ui, name, READOUT, true);
                    }
                    Model::Failed(name) => {
                        lamp(ui, "failed", false, true);
                        chip(ui, name, FAULT, false);
                    }
                }
            });
        });
    });
}

pub fn ask_box(ui: &mut Ui, draft: &mut String, hint: &str, busy: bool, ready: bool) -> Vec<Event> {
    ask_box_with(ui, draft, hint, busy, ready, |_| {})
}

pub fn ask_box_with(
    ui: &mut Ui,
    draft: &mut String,
    hint: &str,
    busy: bool,
    ready: bool,
    status: impl FnOnce(&mut Ui),
) -> Vec<Event> {
    let mut events = Vec::new();
    let edit = ui.add_enabled_ui(ready, |ui| prose(ui, draft, hint, 3)).inner;
    let enter = edit.has_focus()
        && ui.input(|i| i.key_pressed(egui::Key::Enter) && !i.modifiers.shift);
    let mut send = false;
    ui.horizontal(|ui| {
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if busy {
                if ui.add(Key::new("stop").solid(FAULT).min_width(100.0).id_salt("ask-stop")).clicked() {
                    events.push(Event::Stop);
                }
            } else {
                let can = ready && !draft.trim().is_empty();
                send = ui
                    .add(Key::new("send").solid(READOUT).enabled(can).min_width(100.0).id_salt("ask-send"))
                    .clicked();
            }
            ui.with_layout(Layout::left_to_right(Align::Center), status);
        });
    });
    if (enter || send) && ready && !busy {
        if let Some(text) = submit(draft, true) {
            events.push(Event::Sent(text));
        }
    } else if enter && draft.ends_with('\n') {
        draft.pop();
    }
    events
}

fn link(ui: &mut Ui, label: &str, salt: (&str, usize)) -> bool {
    let r = ui.push_id(salt, |ui| {
        ui.add(
            egui::Label::new(RichText::new(label.to_uppercase()).font(legend_font(10.0)).extra_letter_spacing(1.0).color(LEGEND))
                .sense(Sense::click()),
        )
    });
    let r = r.inner.on_hover_cursor(egui::CursorIcon::PointingHand);
    if r.hovered() {
        ui.painter().line_segment([r.rect.left_bottom(), r.rect.right_bottom()], Stroke::new(1.0, LEGEND));
    }
    let name = label.to_string();
    r.widget_info(move || egui::WidgetInfo::labeled(egui::Role::Button, true, name.clone()));
    r.clicked()
}

fn thinking(ui: &mut Ui, text: &str, index: usize, live: bool) {
    let id = ui.make_persistent_id(("thinking", index));
    let mut state = egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, live);
    rail_block(ui, ETCH, egui::Color32::TRANSPARENT, |ui| {
        let open = state.is_open();
        let label = if open { "v thinking" } else { "> thinking" };
        if link(ui, label, ("think-toggle", index)) {
            state.toggle(ui);
        }
        if open {
            ui.add(egui::Label::new(RichText::new(text).italics().size(12.0).color(LEGEND)).wrap());
        }
    });
    state.store(ui.ctx());
}

fn reply(ui: &mut Ui, cache: &mut markdown::Cache, text: &str, index: usize, live: bool) {
    let (thought, answer) = split_thinking(text);
    if let Some(t) = thought {
        thinking(ui, t, index, live && answer.is_empty());
        ui.add_space(4.0);
    }
    if !answer.is_empty() {
        egui::Frame::NONE.inner_margin(Margin { left: 13, right: 8, top: 2, bottom: 2 }).show(ui, |ui| {
            ui.set_width(ui.available_width());
            markdown::show(ui, cache, answer, index);
        });
    }
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
    match message.role {
        Role::User | Role::System => {
            let (who, rail, fill) = match message.role {
                Role::User => ("you", READOUT, PANEL),
                _ => ("system", LEGEND, egui::Color32::TRANSPARENT),
            };
            ui.add_space(6.0);
            rail_block(ui, rail, fill, |ui| {
                Line::new().legend(who).size(10.0).show(ui);
                ui.add_space(2.0);
                markdown::show(ui, cache, &message.text, index);
            });
        }
        Role::Assistant => {
            reply(ui, cache, &message.text, index, false);
            ui.horizontal(|ui| {
                ui.add_space(13.0);
                ui.vertical(|ui| {
                    if let Some(strip) = strip {
                        strip(ui, index);
                    }
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 14.0;
                        if link(ui, "copy", ("copy", index)) {
                            let (_, answer) = split_thinking(&message.text);
                            ui.ctx().copy_text(answer.to_string());
                            events.push(Event::Copy(index));
                        }
                        if last && !busy && link(ui, "regenerate", ("regen", index)) {
                            events.push(Event::Regenerate(index));
                        }
                    });
                });
            });
        }
    }
    events
}

pub fn live(ui: &mut Ui, cache: &mut markdown::Cache, text: &str) {
    reply(ui, cache, text, usize::MAX, true);
    ui.horizontal(|ui| {
        ui.add_space(13.0);
        ui.add(egui::Spinner::new().size(12.0).color(TRACE));
        ui.add_space(4.0);
        let what = if text.trim().is_empty() { "waiting for the first token" } else { "generating" };
        Line::new().legend(what).size(10.5).show(ui);
    });
    ui.ctx().request_repaint();
}

pub fn fault(ui: &mut Ui, title: &str, detail: &str) {
    rail_block(ui, FAULT, FAULT.gamma_multiply(0.08), |ui| {
        Line::new().legend(title).tint(FAULT).size(10.0).show(ui);
        if !detail.is_empty() {
            ui.add(egui::Label::new(RichText::new(detail).font(mono(11.5)).color(FAULT)).wrap());
        }
    });
}

pub fn transcript(
    ui: &mut Ui,
    cache: &mut markdown::Cache,
    messages: &[Message],
    streaming: Option<&str>,
    strip: Option<Strip>,
) -> Vec<Event> {
    transcript_with(ui, cache, messages, streaming, strip, |_| {})
}

pub fn transcript_with(
    ui: &mut Ui,
    cache: &mut markdown::Cache,
    messages: &[Message],
    streaming: Option<&str>,
    mut strip: Option<Strip>,
    below: impl FnOnce(&mut Ui),
) -> Vec<Event> {
    let mut events = Vec::new();
    egui::ScrollArea::vertical().auto_shrink([false, false]).stick_to_bottom(true).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.spacing_mut().item_spacing.y = 6.0;
        let shown: Vec<(usize, &Message)> =
            messages.iter().enumerate().filter(|(_, m)| m.role != Role::System).collect();
        let tail = shown.last().map(|(i, _)| *i);
        for (i, m) in shown {
            let strip = strip.as_mut().map(|s| &mut **s as Strip);
            events.extend(bubble(ui, cache, m, i, Some(i) == tail, streaming.is_some(), strip));
        }
        if let Some(text) = streaming {
            live(ui, cache, text);
        }
        below(ui);
        ui.add_space(8.0);
    });
    cache.sweep();
    events
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
            let title = one_line(ui, &e.title, rect.width() - 20.0, if on { VALUE } else { LEGEND });
            let p = ui.painter();
            p.galley(rect.left_top() + vec2(12.0, 12.0 - title.size().y / 2.0), title, VALUE);
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

fn one_line(ui: &Ui, text: &str, room: f32, tint: egui::Color32) -> std::sync::Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::simple(text.to_string(), legend_font(12.0), tint, room);
    job.wrap.max_rows = 1;
    job.wrap.overflow_character = Some('…');
    ui.fonts_mut(|f| f.layout_job(job))
}

pub fn welcome(ui: &mut Ui, title: &str, text: &str) {
    ui.add_space(40.0);
    Line::new().legend("chat").size(12.0).show(ui);
    ui.add_space(6.0);
    ui.label(RichText::new(title).font(legend_font(22.0)).color(VALUE));
    ui.add_space(8.0);
    Line::new().note(text).size(13.0).wrapped(ui);
    ui.add_space(10.0);
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
    fn thinking_is_split_from_the_reply() {
        assert_eq!(split_thinking("<think>\nhmm\n</think>\n\nHi"), (Some("hmm"), "Hi"));
        assert_eq!(split_thinking("<think>\n\n</think>\n\nHi"), (None, "Hi"));
        assert_eq!(split_thinking("<think>still going"), (Some("still going"), ""));
        assert_eq!(split_thinking("plain <think> later"), (None, "plain <think> later"));
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
            Message::new(Role::Assistant, "<think>a</think>```rs\nfn main() {}\n```"),
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
    fn the_header_list_and_context_bar_draw_without_events() {
        let ctx = egui::Context::default();
        crate::install(&ctx);
        let entries = vec![Entry { title: "a long title that does not fit in the list".into(), subtitle: "2 messages".into() }];
        let mut events = Vec::new();
        frame(&ctx, |ui| {
            events = conversations(ui, &entries, 0);
            header(ui, Model::Ready("Qwen3-0.6B"), false, |_| {});
            header(ui, Model::None, false, |_| {});
            context_bar(ui, 10, 100);
            fault(ui, "load failed", "model not found");
        });
        assert!(events.is_empty());
    }

    #[test]
    fn send_is_refused_while_no_model_is_ready() {
        let ctx = egui::Context::default();
        crate::install(&ctx);
        let mut draft = "hi".to_string();
        let mut events = Vec::new();
        frame(&ctx, |ui| events = ask_box(ui, &mut draft, "Load a model to chat", false, false));
        assert!(events.is_empty());
        assert_eq!(draft, "hi");
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
