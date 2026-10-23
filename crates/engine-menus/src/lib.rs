//! Menu and memory-card front-end kernels: the save and glyph
//! atlases, the SCUS list-row model, the equipment and item catalogs, the
//! item bag, the inventory-use, spell-menu and Equip sessions, the pause
//! root, pause screens and shop screens, the save screen family, the menu
//! overlay's widget
//! choreography, category / arrange tables and open sequence, name entry,
//! the card write flow and `bu` I/O,
//! the debug character editor, key rebinding, game over and the inn: the
//! `World`-free half of the engine's menu layer. The field dialog pager and
//! the inline-dialogue / cutscene-timeline context state live one crate
//! down, in `legaia-engine-dialog`; the publisher-logo and title phases
//! are `legaia-engine-title`, a sibling.
//!
//! Every module's whole dependency closure inside the engine is in this
//! crate or below it, so it sits strictly below `legaia-engine-core`. Doc
//! comments name `engine-core` items in plain code spans, since rustdoc
//! cannot resolve a link into a dependent crate. See the crate README for
//! the module map.

#![forbid(unsafe_code)]

pub mod card_bu_io;
pub mod card_flow;
pub mod debug_char_editor;
pub mod dev_menu_host;
pub mod equip_session;
pub mod equipment;
pub mod field_menu;
pub mod game_over;
pub mod gameover_banner;
pub mod inn;
pub mod inventory_use;
pub mod item_bag;
pub mod items;
pub mod key_rebind;
pub mod list_order;
pub mod menu;
pub mod menu_arrange;
pub mod menu_cues;
pub mod menu_glyph_atlas;
pub mod menu_item_category;
pub mod menu_list_rows;
pub mod menu_open_sequence;
pub mod menu_widget;
pub mod name_entry;
pub mod new_game;
pub mod options;
pub mod party_state;
pub mod pause_screens;
pub mod save_menu_atlas;
pub mod save_screen;
pub mod save_select;
pub mod save_subscreen;
pub mod shop;
pub mod shop_catalog;
pub mod shop_cues;
pub mod shop_state;
pub mod spell_menu;
pub mod spell_party_broadcast;
pub mod status_screen;
