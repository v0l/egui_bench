---
name: egui-bench
description: Build egui UI with the egui_bench crate so every bench-suite app (waveshark, cycler, antenna-toolbox, printfarm, gcad, agentee, thermview, battery-control, bench-suite) looks identical. Use whenever writing or editing egui code that depends on `egui_bench`, adding a panel, card, dialog or modal, a button, a form row, a readout, a table or a plot, picking a colour or font, or reviewing UI code for drift from the house design. Covers the colour grammar, the component to reach for in each case, the app shell, and the modal recipe used in waveshark and printfarm.
---

# egui_bench

One crate owns the look of every bench-suite app. Do not invent a variation. If the screen needs something the crate does not have, compose it from the crate's parts and colours first; add a component to `egui_bench` only when two apps need it.

```rust
egui_bench::install(&cc.egui_ctx);          // once, in the eframe creator
use egui_bench::prelude::*;                  // everything below comes from here
```

`install` sets fonts, dark visuals, 2 px corners and text styles. After it, stock egui widgets (checkbox, combo box, scroll area, drag value, slider) are already on-brand. Never call `set_visuals`, `set_style`, `set_fonts` or `set_theme` afterwards, and never ship a light theme.

Crate features: `markdown`, `agent` (coding-agent pane), `sandbox`, `viewer3d`, `update`. Enable only what the app uses. Read `~/git/egui_bench/README.md` for the full API and run `cargo run --example gallery` (in `~/git/egui_bench`) to see every component. Sources worth copying from: `~/git/super-radio/crates/app/src/ui/settings.rs` (modals), `~/git/printfarm/src/app.rs` (modals, header, keys), `~/git/cycler/crates/ui/src/main.rs` (cards, comb, stage rail), `~/git/bench-suite/src/view.rs` (shell).

## Colour grammar

| constant | meaning | use for |
|---|---|---|
| `READOUT` amber | what the user **sets** | setpoints, limits, thresholds, tuned value, selected tab, primary action, focus ring |
| `TRACE` cyan | what the instrument **measured** | live levels, voltages, received signal, anything read back from hardware |
| `OK` / `SAFE` green | state is good | lamps, status dots, done stages |
| `FAULT` red | state is bad, or a destructive action | alarms, errors, delete/remove/stop keys |
| `WARN` | caution | irreversible but not destructive notes, thin margins |
| `VALUE` off-white | neutral readback and body text | default text, outline key labels |
| `LEGEND` grey | labels and prose | legends, notes, hints, disabled |

Surfaces step one way: `WELL` (recessed: headers, fields, inactive keys) < `CHASSIS` (the case, window and panel fill) < `PANEL` (proud: cards) < `ETCH` (engraved edges and rules). `BAND` is the hover fill. `READOUT_DIM` is amber at rest.

Rules:
- A number is amber if the user can change it, cyan if it came off the device, white if it is just a fact. Never colour by decoration.
- Never write `Color32::from_rgb` for chrome, text, borders or state. Use the constants. A literal colour is acceptable only for a data series in a plot that needs many distinguishable hues, and even then start from `TRACE`, `READOUT`, `OK`, `FAULT`.
- Derive tints with `.gamma_multiply(a)` (fills 0.14 to 0.2, dim borders 0.4 to 0.6), not new literals.
- Do not redefine `GREEN`, `GOLD`, `ACCENT` or similar in an app. Add nothing to a local `theme` module that shadows these.

## Fonts and text

Two faces, both embedded IBM Plex. **Legend** (condensed semibold, uppercase, tracked) is for every label, title, tab and key. **Figure** (mono semibold, tabular) is for every number and every value read back.

- Labels: `legend("tuned")` returns a themed `RichText` that uppercases itself. Write source strings in lower case.
- Own painting: `legend_font(size)`, `figure(size)`, `mono(size)`. Never `FontId::proportional` for a label or number. Never `RichText::new(..).strong()` or `.heading()`.
- Window or app title in a header: `RichText::new("PRINTFARM").font(legend_font(16.0 to 17.0)).color(VALUE)`, optionally `.extra_letter_spacing(2.0..3.0)`.
- Never put a legend and a value in two `ui.label` calls inside `ui.horizontal`; they sit a pixel apart. Use `Line`:

```rust
Line::new().legend("squelch").column(ui, 98.0).set("-78 dB").show(ui);
Line::new().legend("heard").measured("-93.4 dBm").show(ui);
Line::new().legend("mode").value("NFM").gap(10.0).note("auto").show(ui);
```

`.set()` amber, `.measured()` cyan, `.value()` neutral, `.note()` grey prose. End with `.show()`, `.elided(ui)` (one row, ellipsis) or `.wrapped(ui)`. `.size(11.0)` applies to the last span. Typical sizes: 11 for values beside legends and right-aligned card notes (10.5), 12 for notes. Long value under a label column: `Line::new().legend(l).hanging(ui, col, Line::new().value(v))`, or just `reading(ui, label, text)`.
- Prose: `note(ui, text, LEGEND)` wraps to the pane; tint `WARN` or `FAULT` when it is a warning. `hint(ui, text)` is smaller and quieter, for explanations under a group. `help(ui, text)` is the "?" with hover text. Prefer `row_help` or `help` over long inline explanation.
- Copy style: lower case legends, short sentences, no em or en dashes, no exclamation marks. Say what the control does to the world ("allow transmit"), not what the widget is.

## Pick the component

| need | use | never |
|---|---|---|
| group of related controls or readings | `section(ui, "label", "what it is for", body)` | `ui.group`, `CollapsingHeader` as a box, bare `Frame` |
| card with custom header or state colour | `card(ui, Some(rail), header, body)` rail is `READOUT` (setting), `TRACE` (live), `OK`/`FAULT` (state), `None` or `ETCH` (idle) | hand-rolled `Frame::NONE.fill(PANEL)` |
| heading between card groups | `heading(ui, "label")` | `ui.heading` |
| button | `ui.add(Key::new("save").solid(READOUT))`, secondary `.outline(VALUE)`, destructive `.solid(FAULT)` or `.outline(FAULT)`, menu `Key::more()`; `.enabled(b)`, `.min_width(100.0)` | `ui.button` for primary actions, `egui::Button` with custom fill |
| small on/off pill | `toggle(ui, "monitor", on)` (clickable), `lamp(ui, "armed", on, fault)` (display), `chip(ui, "usb", tint, lit)` (card-corner tag) | coloured `ui.label` |
| validation or state sentence | `status(ui, ok, "text")` | red `ui.label` |
| tabs | `tabs(ui, &mut tab, &[(Tab::A, "a"), ...])` | `selectable_value` rows, `TopBottomPanel` of buttons |
| filter bar with counts | `segments(ui, &mut cur, &[(v, "label", Some(n))])` | |
| big number | `hero(ui, "centre", "145.5250", "MHz", READOUT)` | `RichText::size(40.0)` |
| captioned figures | `readouts(ui, &[("mode", "NFM".into(), READOUT), ...])` | grids of labels |
| label + value row | `reading(ui, "serial", text)` or `Line` | `ui.label` pairs, `egui::Grid` |
| sequence of stages | `stage_rail(ui, &stages, Some(i), tint, 22.0, false)` + `exit_note` | |
| progress | `progress(ui, "file", done, Some(total), "said")`, `bar`, `busy(ui, rect, Some(f), "label", tint)` to turn a key into its own bar | `ui.add(ProgressBar)` |
| level meter / volume / squelch | `vu`, `Fader::new(&mut v, peak)`, `Threshold::new(&mut v, lo, hi, measured, open)` | `Slider` where the value has a live peak |
| history line | `Trace::new(&hist).size(v).range(lo, hi)`; `.ratio(1.0, 3.0)` for margins; `.rule(at)`, `.filled(true)` | `egui_plot` for a sparkline |
| N similar channels with limits | `Comb::new(&channels).rule("ceiling", x).fault_above(x)` | |
| many rows | `Table::new(&columns, n).show(ui, row_fn)`, painting each field with `cell(p, row, at(0), width, text, VALUE)` | a widget per cell per row |
| text input | `field`, `secret`, `prose(ui, &mut s, hint, rows)`, `field_then` | `ui.text_edit_singleline`, bare `TextEdit` |
| dropdown | `choice(ui, "id", &mut v, [(v, label), ...])` | raw `ComboBox` |
| checkbox with explanation | `switch(ui, "label", &mut b, "text", "why")` | |
| file open | `Browser::new(title, dir).suffixes(&[".gcad"]).folders(false)`, call `.show(ctx)` per frame, match `Picked` | `rfd` or a hand-made file list |
| files dragged over the window | `drop::hovering(ctx)` + `drop::overlay(ctx, None, "drop to open", "note", READOUT)` | |
| markdown | `markdown::show` (feature) | |

## App shell

Every app is a header, an optional sub-bar, a body and an optional footer, all flat on `CHASSIS`:

```rust
egui::Panel::top("header")
    .frame(egui::Frame::NONE.fill(CHASSIS).inner_margin(egui::Margin { left: 12, right: 12, top: 8, bottom: 6 }))
    .show(ui, |ui| header(ui));
egui::Panel::left("side").exact_size(330.0).resizable(false)
    .frame(egui::Frame::NONE.fill(CHASSIS).inner_margin(8))
    .show(ui, |ui| egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 8.0;
        /* cards */
    }));
egui::CentralPanel::no_frame().show(ui, |ui| {
    let r = ui.max_rect();
    ui.painter().rect_filled(r, 0.0, CHASSIS);
    ui.painter().line_segment([r.left_top(), r.right_top()], Stroke::new(1.0, ETCH));
    /* body in a Frame::NONE.inner_margin(Margin::same(10)) */
});
```

- Header: title in legend font (`VALUE`), then `tabs`, then `ui.with_layout(right_to_left, ..)` holding `Key`s (primary first = rightmost). A secondary strip under it is filled `PANEL`. Bottom notice bar is `CHASSIS` with `status(ui, ok, text)`.
- Fills are only `CHASSIS`, `WELL`, `PANEL`. Never `Frame::default()` for a panel.
- Gaps: 8 px between cards, 4 to 6 inside, 12 between groups in a dialog. `Margin` 8 to 12 at panel edges.
- Cards have 2 px corners and an `ETCH` stroke already. Never call `.corner_radius` with anything else, and never use shadows or gradients.
- Request repaints only while something animates (`request_repaint_after`).

## Inline controls in a row

`choice` and `field` fill the width on offer, which is what a form row wants. Beside a legend or in a toolbar, give them a box:

```rust
ui.allocate_ui(egui::vec2(150.0, KEY_H), |ui| {
    choice(ui, "cable", &mut self.cable, CABLES.iter().enumerate().map(|(i, c)| (i, c.name.to_string())));
});
field_then(ui, &mut self.path, "path to a file", 90.0, |ui| {
    load = ui.add(Key::new("load").outline(VALUE)).clicked();
});
```

`field_then(ui, text, hint, reserve, after)` sizes the field to the row minus `reserve` px and draws `after` beside it, so a search box with its key never overflows.

- A selectable chip is `chip(ui, text, tint, lit).interact(egui::Sense::click())`, not a coloured `egui::Button`.
- A key in a popup menu is `Key::new(label).outline(VALUE).min_width(ui.available_width())`, `FAULT` for the destructive entry.
- A tiny dismiss or remove control is a `Key` labelled for what it does ("dismiss", "remove"), or `Key::more()` for a menu. Never `small_button("x")`.
- `Key` and `egui::Key` collide under `use egui::*`; import `egui_bench::key::Key` by path in files that use both.
- `Key` paints its label upper case, so a UI test that looks a button up by text searches for "OPEN", not "open".
- Pin `egui_bench` to the same version across every app (`0.5.4` at the time of writing). A stale pin is the usual reason a component is missing.

## Modals

Every dialog, confirmation and settings page is an `egui::Modal` with the same bones: unique id, black backdrop at alpha 150, fixed width, `modal_title`, content as `section` cards and `row`s, `footer` with keys. Waveshark's settings and printfarm's confirmations are the reference.

Widths: 380 to 400 for a confirmation, 460 to 520 for a form or settings page, 560 to 640 for a list or table. Pick one, never `auto`.

Keep the open state as a field (`Option<State>`, `bool`, or an enum). Draw every modal from the root `ui`/`update` after the panels, each one returning early when closed. Always honour `should_close()` (Escape and a click on the backdrop), or the dialog cannot be dismissed.

### Form dialog (waveshark style)

```rust
fn feed_modal(&mut self, ctx: &egui::Context) {
    if !self.feed.open {
        return;
    }
    let (mut close, mut apply) = (false, false);
    let r = egui::Modal::new(egui::Id::new("feed"))
        .backdrop_color(Color32::from_black_alpha(150))
        .show(ctx, |ui| {
            ui.set_width(520.0);
            modal_title(ui, "Feed example.net");
            let f = &mut self.feed;
            section(ui, "account", "from example.net/account", |ui| {
                row_help(ui, "API name", "Not the name you log in with.", |ui| {
                    field(ui, &mut f.name, "AID0000");
                });
                row_help(ui, "API token", "Stored in plain text on this machine.", |ui| {
                    secret(ui, &mut f.token);
                });
                match f.name.trim().is_empty() || f.token.trim().is_empty() {
                    true => status(ui, false, "no account: nothing can be uploaded"),
                    false => status(ui, true, &format!("uploading as {}", f.name.trim())),
                }
            });
            ui.add_space(8.0);
            section(ui, "upload", "what leaves this machine", |ui| {
                if switch(ui, "upload", &mut f.on, "while receiving", "Uploaded as CSV.") {
                    apply = true;
                }
                reading(ui, "waiting", "12 rows in 2 files");
            });
            footer(ui, |ui| {
                if ui.add(Key::new("apply").solid(READOUT).min_width(100.0)).clicked() {
                    apply = true;
                }
                if ui.add(Key::new("close").outline(VALUE).min_width(100.0)).clicked() {
                    close = true;
                }
            });
        });
    if r.should_close() {
        close = true;
    }
    if apply {
        self.apply_feed();
    }
    if close {
        self.feed.open = false;
    }
}
```

Layout rules inside the body:
- `modal_title` first. It is a legend, not a heading: lower case or sentence case text is fine, it uppercases.
- Group settings in `section(ui, label, note, ..)` cards with `ui.add_space(8.0)` between them. Inside, one `row`/`row_help`/`switch`/`reading` per setting. `row_help` puts the explanation in a "?" so the dialog stays a column of settings.
- A setting that needs more than a sentence goes in `hint(ui, ..)` under the row, or in the `section` note, not in a paragraph.
- Validation and live state use `status(ui, ok, text)` next to the field it concerns, not a dialog-level error box. Warnings that apply to the whole action use `note(ui, text, WARN)`.
- A long list or table scrolls inside the modal. Take the width first, because inside a scroll area the width on offer is the screen's:

```rust
let w = ui.available_width();
egui::ScrollArea::vertical()
    .auto_shrink([false, true])
    .max_height((ui.ctx().content_rect().height() * 0.7).clamp(320.0, 760.0))
    .show(ui, |ui| {
        ui.set_max_width(w);
        /* cards */
    });
```

- Do not nest a modal inside a modal. Close one, open the other, or use `Browser` (which is itself a modal) from a flag.
- Do not put `ui.separator()` above the buttons; `footer` draws its own `ETCH` rule.

### Footer buttons

`footer(ui, |ui| ..)` lays out right to left, so the **first key added is the rightmost**: the primary action, then the way out to its left. Both get `.min_width(100.0)` (120 to 140 when the labels are long).

| action | key |
|---|---|
| confirm, save, apply, start | `Key::new("save").solid(READOUT)` |
| destructive confirm (remove, delete, stop, cancel a print) | `Key::new("remove").solid(FAULT)` |
| irreversible but not destructive (writes to EEPROM) | `.solid(WARN)` |
| cancel, close, keep | `Key::new("cancel").outline(VALUE)` |
| stop a running operation | `Key::new("stop").outline(FAULT)` |

Disable the primary with `.enabled(ready)` until the form is valid. Name keys for the verb and the object ("cancel print" / "keep printing", "uninstall" / "keep it"), not "yes" / "no" / "ok". A dialog with only information has a single `close` key.

### Confirmation (printfarm style)

```rust
if let Some(i) = self.removing {
    let name = self.printers[i].title();
    let m = egui::Modal::new(egui::Id::new("remove"))
        .backdrop_color(Color32::from_black_alpha(150))
        .show(ctx, |ui| {
            ui.set_width(380.0);
            modal_title(ui, &format!("Remove {name}?"));
            note(ui, "Its queued jobs go back to any printer. History is kept.", LEGEND);
            footer(ui, |ui| {
                if ui.add(Key::new("remove").solid(FAULT).min_width(100.0)).clicked() {
                    self.remove(i);
                    self.removing = None;
                }
                if ui.add(Key::new("keep it").outline(VALUE).min_width(100.0)).clicked() {
                    self.removing = None;
                }
            });
        });
    if m.should_close() {
        self.removing = None;
    }
}
```

Title is a question naming the object. Body is one `note` saying what happens and what is kept, tinted `LEGEND`, or `WARN`/`FAULT` for the consequence the user cannot undo. State is cleared in every branch, including `should_close()`.

When the action also needs follow-up work after the modal closure (it borrows `self`), set `let mut go = false;` inside, and run the work after `show` returns, as in `wigle_modal`.

### Progress dialog

No way out except an explicit stop, so do not honour `should_close()`:

```rust
egui::Modal::new(egui::Id::new("copying")).backdrop_color(Color32::from_black_alpha(150)).show(ctx, |ui| {
    ui.set_width(420.0);
    modal_title(ui, "Copying to card");
    progress(ui, "file", done as f32, Some(total as f32), &file);
    footer(ui, |ui| {
        if ui.add(Key::new("stop").outline(FAULT).min_width(100.0)).clicked() {
            stop = true;
        }
    });
});
```

### Picking a file or folder

Use `Browser`, it is already the modal in the right shape:

```rust
if let Some(b) = &mut self.browser {
    match b.show(ctx) {
        Picked::Browsing => {}
        Picked::Cancelled => self.browser = None,
        Picked::File(path) | Picked::Folder(path) => {
            self.browser = None;
            self.open(path);
        }
    }
}
```

### Modal anti-patterns

- A `Window` or `Area` used as a dialog. Dialogs are `Modal`.
- Inline `ui.horizontal` with `ui.button("ok")` and `ui.button("cancel")` at the bottom (agentee's older dialogs do this). Use `footer` and `Key`.
- Titles through `ui.heading`, `RichText::size(19.0)`, or no title at all. Use `modal_title`.
- A different backdrop, width per field, or ad hoc `Frame` around the content.
- `Key` buttons without `min_width` in a footer (ragged widths) and mixed `Key` / `ui.button` in the same footer.
- Hand-made close icons (`small_button("x")`). Use a `Key` or `Key::more()`; a notice bar dismissal is the one allowed small `x`.

## Review checklist

Before finishing any UI change, grep the diff for these and fix them:

1. `Color32::from_rgb` / `from_rgba` outside a plot series; `hex`/`#rrggbb`.
2. `ui.heading`, `.strong()`, `FontId::proportional`, `RichText::size` on a label.
3. `ui.button(` or `egui::Button` for an action.
4. `ui.group`, `egui::Frame::group`, `Frame::default()`, `corner_radius` other than 2, `shadow`.
5. `TextEdit` / `text_edit_singleline` outside `form`.
6. `egui::Grid`, two-label `ui.horizontal` rows (use `Line` / `reading` / `row`).
7. `egui::Window` or `Area` for a dialog; a `Modal` without `backdrop_color`, `set_width`, `modal_title`, `footer`, or `should_close`.
8. A value coloured differently from the grammar: a setting that is cyan, a measurement that is amber.
9. Em or en dashes in any string.

Then run the app (or `cargo run --example gallery` for comparison) and look at it next to a sibling app. If a panel does not look like it came from the same instrument, it is wrong.
