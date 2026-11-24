//! The rows of the session's state: the core's `row_rules` plus the two rules that read the [`Snapshot`].

pub use buds_core::row_rules::*;

use std::collections::{BTreeSet, HashMap};

use crate::session::Snapshot;

/// The bitmap the rules read: the buds' own, or [`every_command`] before they answer.
pub fn bitmap(s: &Snapshot) -> &[u16] { s.caps.0.as_deref().unwrap_or_else(|| every_command()) }

/// The rows of the session's state. The desktop keeps no reply between runs, so before the buds answer, an unread
/// reply counts as the ask list and an unread bitmap as every command: the pages show what the model can have.
pub fn of(s: &Snapshot) -> BTreeSet<&'static str> {
    let reply: HashMap<u8, u8> = if s.features.is_empty() { ask_ids(s.model).into_iter().map(|id| (id, 0)).collect() }
        else { s.features.iter().copied().collect() };
    rows(&Input {
        model: s.model, reply: Some(&reply), bitmap: Some(bitmap(s)), product_anc: !s.modes.is_empty(), phone_spatial_ok: false,
        manual: s.manual, gestures: Some(!crate::gesture_logic::Model::of(s).rows.is_empty()),
        firmware: s.firmware.as_deref(),
    })
}

