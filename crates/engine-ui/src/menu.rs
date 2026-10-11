//! The menu overlay's draw builders: the pause / status / equipment / spell
//! screens ([`screens`]), the window-descriptor dispatch and its per-kind
//! painters, and the equip stat-compare panels. [`screens`],
//! [`window_dispatch`] and [`equip_stat_compare`] are re-exported at the
//! crate root.

pub mod equip_stat_compare;
pub mod screens;
pub mod window_dispatch;
pub mod window_painters;
