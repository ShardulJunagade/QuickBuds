//! QuickBuds core: no I/O. The platform shells own the Bluetooth socket. The only dependency is `serde_json`
//! for the model data (`models.json`).
pub mod cmd;
pub mod feature_sync;
mod json;
pub mod model;
pub mod msg;
pub mod oppo;
pub mod soundcore;
pub mod wire;

/// The OPPO model data and rules, at the crate root as the desktop app names them.
pub use oppo::{profile, protocol, row_rules, types};
