use super::{Light, Lighting, RenderMode, Trace};
use crate::form::{choice, row};
use crate::text::legend;
use egui::{Slider, Ui};

fn angle(ui: &mut Ui, value: &mut f32, range: std::ops::RangeInclusive<f32>) -> bool {
    let mut degrees = value.to_degrees();
    let changed = ui.add(Slider::new(&mut degrees, range).suffix("°").fixed_decimals(0)).changed();
    if changed {
        *value = degrees.to_radians();
    }
    changed
}

fn light(ui: &mut Ui, name: &str, light: &mut Light) -> bool {
    let mut changed = false;
    ui.label(legend(name));
    row(ui, "intensity", |ui| {
        changed |= ui.add(Slider::new(&mut light.intensity, 0.0..=3.0)).changed();
        changed |= ui.color_edit_button_srgba(&mut light.colour).changed();
    });
    row(ui, "azimuth", |ui| changed |= angle(ui, &mut light.azimuth, -180.0..=180.0));
    row(ui, "elevation", |ui| changed |= angle(ui, &mut light.elevation, -90.0..=90.0));
    row(ui, "softness", |ui| changed |= angle(ui, &mut light.spread, 0.0..=30.0));
    changed
}

pub fn lighting_controls(ui: &mut Ui, lighting: &mut Lighting) -> bool {
    let mut changed = light(ui, "key", &mut lighting.key);
    ui.add_space(6.0);
    changed |= light(ui, "fill", &mut lighting.fill);
    ui.add_space(6.0);
    ui.label(legend("environment"));
    row(ui, "ambient", |ui| {
        changed |= ui.add(Slider::new(&mut lighting.ambient, 0.0..=2.0)).changed();
    });
    row(ui, "sky, ground", |ui| {
        changed |= ui.color_edit_button_srgba(&mut lighting.sky).changed();
        changed |= ui.color_edit_button_srgba(&mut lighting.ground).changed();
    });
    row(ui, "exposure", |ui| {
        changed |=
            ui.add(Slider::new(&mut lighting.exposure, 0.25..=4.0).logarithmic(true)).changed();
    });
    row(ui, "", |ui| {
        changed |= ui.checkbox(&mut lighting.follow, "lights turn with the camera").changed();
    });
    row(ui, "", |ui| {
        if ui.button("studio").clicked() {
            *lighting = Lighting::STUDIO;
            changed = true;
        }
    });
    changed
}

pub fn trace_controls(ui: &mut Ui, trace: &mut Trace) -> bool {
    let mut changed = false;
    row(ui, "samples", |ui| {
        changed |= ui.add(Slider::new(&mut trace.samples, 16..=4096).logarithmic(true)).changed();
    });
    row(ui, "bounces", |ui| {
        changed |= ui.add(Slider::new(&mut trace.bounces, 1..=12)).changed();
    });
    row(ui, "per frame", |ui| {
        changed |= ui.add(Slider::new(&mut trace.per_frame, 1..=64)).changed();
    });
    changed
}

pub fn mode_controls(ui: &mut Ui, mode: &mut RenderMode) -> bool {
    let mut changed = false;
    row(ui, "view", |ui| {
        let options = RenderMode::ALL.map(|m| (m, m.name().to_string()));
        changed = choice(ui, "viewer3d render mode", mode, options);
    });
    changed
}
