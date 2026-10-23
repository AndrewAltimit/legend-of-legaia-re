//! The pause-menu runtime over a running `World`: the menu state machine's
//! host ([`menu_runtime::MenuRuntime`]), the field-menu sub-session
//! dispatcher ([`field_menu_dispatch`]), the out-of-battle action
//! validator host ([`menu_validator`]), the memory-card Save writer
//! ([`card_write`]) and the developer menu's `World` seam
//! ([`dev_menu_host`]).
//!
//! The menu rules and screens are `legaia-engine-menus`, which names no
//! `World`; the `World` they edit is `legaia-engine-core`'s. This crate is the
//! seam between the two, above `engine-core` and below the play hosts.

pub mod bag_rows;
pub mod card_write;
pub mod dev_menu_host;
pub mod field_menu_dispatch;
pub mod menu_runtime;
pub mod menu_validator;
