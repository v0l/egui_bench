# egui_bench

A bench-instrument look for [egui](https://github.com/emilk/egui): chassis greys,
engraved legends, amber readouts, cyan traces. Pulled out of two programs that had
grown the same panel twice, [waveshark](https://github.com/v0l/super-radio) and
cycler, and merged into one crate.

![panel](https://raw.githubusercontent.com/v0l/egui_bench/master/docs/panel.png)

## The grammar

Two accents, and they mean something rather than decorate.

- **Amber (`READOUT`) is what you set.** Setpoints, limits, thresholds, the tuned
  frequency, the handle on a fader, the rule across a plot, the tab you are on.
- **Cyan (`TRACE`) is what the instrument measured.** Levels, cell voltages,
  received signal, anything off the air or off the shunt.
- `OK` green and `FAULT` red are states, not readings. `LEGEND` grey labels,
  `VALUE` off-white reads back.

Surfaces step in one direction and only one: `WELL` is recessed, `CHASSIS` is the
case, `PANEL` is proud of it, `ETCH` is the engraved edge. Corners are 2 px,
because an instrument panel is milled, not moulded. Unit tests in `src/theme.rs`
hold the contrast ratios and the surface ordering, so a palette edit that breaks
legibility fails `cargo test`.

Fonts are IBM Plex, embedded, so every machine renders the same: condensed for
panel legends (uppercase, tracked out, silkscreen), mono semibold for figures
(tabular, so a changing digit never shifts the ones beside it).

## Use

```toml
[dependencies]
egui_bench = "0.2"
```

```rust,ignore
// once, at startup
egui_bench::install(&cc.egui_ctx);

use egui_bench::prelude::*;

card(ui, Some(READOUT), |ui| { Line::new().legend("tuned").show(ui); }, |ui| {
    hero(ui, "centre", "145.5250", "MHz", READOUT);
    readouts(ui, &[
        ("mode", "NFM".into(), READOUT),
        ("heard", "-93.4 dBm".into(), TRACE),
    ]);
});
```

`install` sets fonts, visuals, spacing and text styles. Everything else is
opt-in: the components are plain functions and `egui::Widget` impls, so they
compose with `add_sized`, `add_enabled` and the rest of egui's layout.

## What is in it

### `theme` - palette, fonts, style

`install(ctx)`, `fonts(ctx)`, the colour constants, and `legend_font(size)` /
`figure(size)` / `mono(size)` for painting your own galleys in the panel's faces.

### `text` - words on a panel

`legend()`, `value()`, `action()` return themed `RichText`. `note()` and `hint()`
are wrapped prose. `help(ui, text)` is a "?" that carries the explanation on
hover, which keeps a settings dialog a column of settings.

`Line` is the important one. A legend beside a value is two fonts at two sizes,
and two labels in a `ui.horizontal` are two galleys that egui centres against
each other, so the row sits a pixel out everywhere it appears. `Line` lays every
span into one `LayoutJob` and paints it on a fixed baseline:

```rust,ignore
Line::new().legend("squelch").column(ui, 98.0).set("-78 dB").show(ui);
Line::new().legend("heard").measured("-93.4 dBm").show(ui);
```

`.set()` is amber, `.measured()` is cyan, `.value()` is neutral, `.note()` is
prose. `.column(ui, x)` starts the next span at a fixed offset so a stack of
readings aligns. `.show()`, `.elided()` and `.wrapped()` end it, and
`.hanging(ui, x, value)` wraps a long value under its own column.

### `panel` - blocks and state

- `card(ui, rail, header, body)`: recessed header, body under it, optional
  coloured rail down the left carrying the card's state.
- `section(ui, label, note, body)`: a card whose header is a legend and a
  right-aligned sentence saying what it is for.
- `lamp(ui, text, on, fault)`: a key-shaped state lamp, lit or dark.
- `toggle(ui, text, on)`: the same shape, clickable, amber when set.
- `status(ui, ok, text)`: a dot and a sentence, for validation in a dialog.
- `stage_rail(ui, stages, at, now, height, muted)`: a real sequence drawn as one,
  stages behind you dim, the running one lit, the ones ahead outlined.
- `chip(ui, text, tint, lit)`: a small state tag in any colour, for a card corner.
- `heading(ui, label)`: an engraved legend with a rule running to the edge, for
  a group of cards.
- `segments(ui, &mut current, &[(value, label, count)])`: a joined filter bar,
  the chosen segment amber and underlined, each with an optional count.
- `exit_note`, `tabs`.

### `key` - push buttons

`Key::new("launch").solid(VALUE)` is a 30 px button with an engraved label,
`.outline(tint)` the quieter one beside it, `Key::more()` the three-dot menu key.
They take keyboard focus and draw an amber ring when they have it. `ui.add(key)`
lays one out; `key.paint(ui, rect)` puts it in a rect you have measured, with
`.id_salt(..)` keeping its focus when the layout shifts. `busy(ui, rect, done,
label, tint)` is the same shape filling with progress, or sweeping when the end
is unknown, so a key can turn into its own progress bar.

### `drop` - a file over the window

`drop::hovering(ctx)` says whether files are being dragged over the window and
which, and `drop::overlay(ctx, image, headline, note, tint)` dims the window
and says what letting go will do.

### `readout` - figures

- `hero(ui, label, v, unit, tint)`: the one number you read from across the room,
  unit small beside it, legend under it.
- `readout` / `readouts`: captioned figures on a fixed pitch, so three in one
  card line up with three in the card beside it, wrapping when the pane narrows.
- `reading(ui, label, text)`: a label-column row built on `Line`, the value
  wrapping under its column when it is too long for the pane.

### `meter` - levels

- `vu(painter, rect, peak)`: a level bar on a square-root scale, green to amber
  to red, painted into a rect you own.
- `Fader::new(&mut value, peak)`: a volume control whose track is its own meter,
  so what you set and what it is producing read in one glance.
- `Threshold::new(&mut value, lo, hi, measured, open)`: a squelch-style threshold
  drawn over what it is deciding against.
- `bar`, `progress` (determinate or a sweeping stripe when the total is unknown).

### `trace` - one line in a well

`Trace::new(&history).size(v).ratio(1.0, 3.0)` plots against a dashed rule on a
log axis, for margins where 2x above has to look like half below. `.range(lo, hi)`
is the linear version, `.filled(true)` shades under it.

### `comb` - a channel array

`Comb::new(&channels)` draws every channel as a level in the window they are
actually working in, with `.rule(name, at)` limits ruled across in amber. The
highest and lowest are the only two labelled, because they are the only two any
decision is made on. Generalised from cycler's per-cell voltage comb: anything
with N similar channels and a limit works.

### `table` - painted rows

A thousand rows of widgets is a thousand allocations a frame. `Table` paints a
header and striped rows and hands you a painter, a rect and the column offsets;
`cell()` clips one field to its column.

### `form` - dialogs

`row`, `row_help`, `switch`, `choice`, `field`, `secret`, `prose`, `footer`,
`modal_title`. The text fields are set into the panel as wells with an etched
edge and tabular figures, rather than egui's default grey box, which on this
chassis is the one thing that looks pasted on.

### `browser` - an open dialog

`Browser::new(title, dir)` is a modal that walks folders and lists the files
whose names end in one of `.suffixes(...)`. Call `show(ctx)` each frame while it
is up; it returns `Picked::File` or `Picked::Folder` (with `.folders(true)`) once
a choice is made, and `Picked::Cancelled` on cancel or escape.

### `viewer3d` - a 3D viewport (feature `viewer3d`)

```toml
egui_bench = { version = "0.3", features = ["viewer3d"] }
```

The shaded model view out of gcad and agentee: key and fill light, edges drawn at
a fixed pixel width and lifted toward the eye so they never poke through walls,
section cuts with capped solids, see-through and selected parts, and a software
renderer for headless screenshots. Drawing goes through wgpu on egui's own
device, so the app runs `eframe` with `wgpu` and hands the viewer its render
state once at start:

```rust,ignore
if let Some(state) = &cc.wgpu_render_state {
    egui_bench::viewer3d::install(state);
}
```

Each viewport renders into its own 4x multisampled texture, so the app needs no
depth buffer or multisampling of its own. Without `install` the viewer falls
back to the software renderer.

A `Scene` is triangles (`Surface`) and polylines (`Lines`) in groups. Each frame
a `Look` per group places it (a column-major 4x4), fades it, or selects it.

Materials are the app's own type. Anything `Copy + Eq + Hash + Default` becomes
one by saying how it shades, and surfaces that share a group, material and
texture go to the GPU as one draw:

```rust,ignore
use egui_bench::viewer3d::*;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Default)]
enum Finish { #[default] Mask, Copper, Silk }

impl Material for Finish {
    fn shading(&self) -> Shading {
        match self {
            Finish::Copper => Shading { gloss: 0.46, sharpness: 82.0, metal: 0.8, ..Default::default() },
            _ => Shading::default(),
        }
    }
}

let mut viewer = Viewer::new(scene.clone(), self.camera).looks(looks).cut(cut);
viewer.navigate(ui, &response, &mut self.orbit);
self.camera = viewer.camera();
viewer.paint(ui, rect);
let hit = viewer.pick(rect, pointer);
```

`navigate` orbits about the point under the pointer, pans with shift or the
secondary button, and zooms toward the pointer. `Camera::framed` fits a region,
`Projector` maps between world and screen for overlays, and
`Viewer::render_soft(w, h)` gives a `ColorImage` without a GPU.

#### Lighting

`Style::lighting` holds a key and a fill light, each with an azimuth,
elevation, intensity, colour and softness, plus an ambient sky and ground
colour and an exposure. With `follow` set the lights turn with the camera;
without it they stay put in the world, Z up. `Lighting::STUDIO` is the default
look, and `viewer3d::lighting_controls(ui, &mut lighting)` draws a panel of
sliders for all of it.

```rust,ignore
let viewer = Viewer::new(scene, camera).lighting(self.lighting);
```

#### Ray tracing

`Viewer::trace(Some(Trace { samples, bounces, per_frame }))` path traces the
view once the camera stops: soft shadows from both lights, ambient occlusion,
reflections off metal and light bounced between parts. While the view moves it
draws as usual; once still, it adds `per_frame` samples a frame until it has
`samples`, and swaps to the traced picture after the first 16. Edges and
polylines are still drawn on top, cuts are capped, and see-through parts let
light through. `paint` returns a `Painted` saying how far it has got.

On GPUs with ray tracing hardware it uses wgpu's ray queries, which egui's
device does not ask for by default. Request them in the app's options:

```rust,ignore
let options = eframe::NativeOptions {
    wgpu_options: egui_wgpu::WgpuConfiguration {
        wgpu_setup: egui_bench::viewer3d::wgpu_setup(),
        ..Default::default()
    },
    ..Default::default()
};
```

Without them it traces a BVH in a compute shader, which gives the same picture
much more slowly, so keep `per_frame` low there. `viewer3d::hardware_tracing()`
says which one you got, and `Viewer::render_traced(w, h)` renders a still at
any size with every sample taken.

### `update` - GitHub release updates (feature `update`)

An `Updater` per repo asks `/releases/latest` on a thread, compares the tag with
the version you are running, and fetches an asset with a progress bar, checking
the size and the sha256 GitHub publishes for it. What happens to the download is
yours: the closure you pass gets the file. `Updater::cached` keeps the last
answer on disk and asks again with its ETag, so a check that finds nothing new
does not count against GitHub's 60 an hour, and a check made after the limit
is spent still has an answer.

```rust,ignore
let up = Updater::new("v0l/gcad", Some(env!("CARGO_PKG_VERSION").into()));
up.check(ctx);
if let State::Newer(r) = up.state()
    && let Some(asset) = r.asset("gcad-x86_64-unknown-linux-gnu.tar.gz")
{
    up.install(ctx, &r, asset, |file| {
        let dir = file.with_extension("unpacked");
        update::unpack(file, &dir)?;
        update::replace_self(&update::find_file(&dir, "gcad").ok_or("no gcad")?)
    });
}
up.status(ui);
```

`unpack` reads `.tar.gz`, `.zip`, `.dmg` (macOS) or takes a bare binary.
`replace` swaps a file or an `.app` folder by renaming, so a running program can
be replaced, Windows included; `sweep` clears the `.old` copies Windows would not
let go of. `replace_self` and `relaunch` are the self-update pair, and
`probe_version` reads `--version` from a binary you did not build.

## Gallery

```sh
cargo run --example gallery          # panel
cargo run --example gallery meters
cargo run --example gallery data
cargo run --example viewer3d --features viewer3d
```

![meters](https://raw.githubusercontent.com/v0l/egui_bench/master/docs/meters.png)
![data](https://raw.githubusercontent.com/v0l/egui_bench/master/docs/data.png)
![viewer3d](https://raw.githubusercontent.com/v0l/egui_bench/master/docs/viewer3d.png)

## Licence

MIT. The embedded IBM Plex faces are under the SIL Open Font License, see
`assets/OFL.txt`.
