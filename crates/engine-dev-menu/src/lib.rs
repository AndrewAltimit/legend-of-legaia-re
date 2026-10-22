//! The field overlay's developer-menu bodies: the row model and list picker
//! (`dev_menu_leaves`), the value-step kernels (`dev_menu_adjust`), the
//! EVENT FLAG editor (`dev_menu_flags`), the per-slot equip applier
//! (`dev_equip_commit`) and the row-action dispatcher (`dev_menu_action`).
//!
//! They sit below both `legaia-engine-field`, whose op-`0x49` submode hub and
//! panel code run the same leaves, and `legaia-engine-menus`, whose dev menu
//! screen and pause-menu equip session call them. Their only engine
//! dependency is `legaia-engine-system`'s packed pad layout. See the crate
//! README for the module map.

#![forbid(unsafe_code)]

pub mod dev_equip_commit;
pub mod dev_menu_action;
pub mod dev_menu_adjust;
pub mod dev_menu_flags;
pub mod dev_menu_leaves;
