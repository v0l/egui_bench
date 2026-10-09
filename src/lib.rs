#![doc = include_str!("../README.md")]

pub mod comb;
pub mod form;
pub mod meter;
pub mod panel;
pub mod readout;
pub mod table;
pub mod text;
pub mod theme;
pub mod trace;
#[cfg(feature = "viewer3d")]
pub mod viewer3d;

pub use theme::install;

pub mod prelude {
    pub use crate::comb::{Channel, Comb};
    pub use crate::form::{
        choice, clipboard_menu, field, field_then, footer, modal_title, prose, row, row_help,
        secret, switch,
    };
    pub use crate::meter::{FADER_H, Fader, Threshold, VU_H, bar, progress, vu};
    pub use crate::panel::{card, exit_note, lamp, section, stage_rail, status, tabs, toggle};
    pub use crate::readout::{LABEL_W, READOUT_W, hero, reading, readout, readouts};
    pub use crate::table::{ROW_H, Table, cell};
    pub use crate::text::{Line, action, help, hint, legend, legend_job, note, value};
    pub use crate::theme::{
        self, BAND, CHASSIS, ETCH, FAULT, LEGEND, OK, PANEL, READOUT, READOUT_DIM, SAFE, TRACE,
        VALUE, WARN, WELL, figure, install, legend_font, mono,
    };
    pub use crate::trace::{Scale, Trace};
}
