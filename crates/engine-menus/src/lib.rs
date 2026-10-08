//! Menu, title and memory-card front-end kernels: the save / title / glyph
//! atlases, the SCUS list-row model, the equipment and item catalogs, the
//! item bag, the inventory-use, spell-menu and Equip sessions, the pause
//! root, pause screens and shop screens, the save screen family, the menu
//! overlay's widget
//! choreography, category / arrange tables and open sequence, name entry,
//! the title and publisher-logo phases, the card write flow and `bu` I/O,
//! the debug character editor, key rebinding, game over, the inn, and the
//! field dialog pager's row window, pacing, picker slide and text balloon:
//! the `World`-free half of the engine's menu layer.
//!
//! Every module's whole dependency closure inside the engine is in this
//! crate or below it, so it sits strictly below `legaia-engine-core`,
//! which re-exports each module at its old path. Doc links that pointed
//! back up at `engine-core` are plain code spans, since rustdoc cannot
//! resolve a link into a dependent crate. See the crate README for the
//! module map.

#![forbid(unsafe_code)]

// Modules these files name as `crate::...`, which engine-core re-exports at
// its root; binding them here keeps the moved files' paths unchanged.
use legaia_engine_battle::battle_stats;
use legaia_engine_battle::spells;
use legaia_engine_system::input;

use legaia_engine_vm::menu_input;

pub mod card_bu_io;
pub mod card_flow;
pub mod debug_char_editor;
pub mod dialog_pacing;
pub mod dialog_picker_slide;
pub mod dialog_window;
pub mod equip_session;
pub mod equipment;
pub mod field_menu;
pub mod game_over;
pub mod inn;
pub mod inventory_use;
pub mod item_bag;
pub mod items;
pub mod key_rebind;
pub mod list_order;
pub mod menu_arrange;
pub mod menu_cues;
pub mod menu_glyph_atlas;
pub mod menu_item_category;
pub mod menu_list_rows;
pub mod menu_open_sequence;
pub mod menu_widget;
pub mod name_entry;
pub mod pause_screens;
pub mod publisher_logos;
pub mod save_menu_atlas;
pub mod save_screen;
pub mod save_select;
pub mod save_subscreen;
pub mod shop;
pub mod shop_catalog;
pub mod spell_menu;
pub mod spell_party_broadcast;
pub mod status_screen;
pub mod text_balloon;
pub mod title;
pub mod title_screen_atlas;
