//! The brand-neutral state that every driver fills (docs/CORE-API.md, "The feature model"). The widgets, the tiles,
//! the notification and the home screen read only this. A driver keeps its own brand state beside it.
//!
//! Feature keys: the row keys of `oppo::row_rules` (`anc`, `game`, `eq`, `controls` ...), the same keys as the
//! Android `RowRules.kt`. A new brand reuses a key when the feature means the same. The list only grows.

use std::collections::BTreeSet;

/// One battery level.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Level {
    /// From 0 to [`max`](Self::max).
    pub value: u8,
    /// 100 for a percent. Some brands report steps (Soundcore: 5).
    pub max: u8,
    pub charging: bool,
}

/// Where a bud is.
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub enum Wear {
    #[default]
    Unknown,
    InEar,
    /// Out of the ear and out of the case.
    Out,
    InCase,
}

#[derive(Clone, PartialEq, Debug, Default)]
pub struct Common {
    pub connected: bool,
    /// The device name, or the model name for an unknown device name.
    pub name: String,
    /// The `models.json` id of the model in use.
    pub model_id: Option<String>,
    /// Left, right, case.
    pub battery: [Option<Level>; 3],
    /// Left, right.
    pub wear: [Wear; 2],
    /// The noise control mode key; None until the buds report it.
    pub noise: Option<String>,
    /// The noise control modes these buds offer, in the UI's order.
    pub noise_modes: Vec<String>,
    pub low_latency: Option<bool>,
    /// The feature keys of the connected buds: a control shows only when its key is here.
    pub features: BTreeSet<&'static str>,
}
