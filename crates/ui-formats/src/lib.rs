//! The formats the game's 2D interface is drawn from: the boot `init.pak`
//! and title packs, the system-UI TIM bundle and its interior page, the menu
//! glyph atlas, the save-slot portrait sheet, the screen-element and
//! widget-class tables in `SCUS_942.54`, the battle screen's label strings,
//! and the palette a given texture is actually drawn with.
//!
//! Sits on `legaia-container-formats` (for the TIM scanner) and names no
//! scene, minigame or battle-model format.

#![forbid(unsafe_code)]

pub mod battle_ui_strings;
pub mod init_pak;
pub mod interior_page;
pub mod menu_glyph_atlas;
pub mod name_entry_prompts;
pub mod save_icon;
pub mod screen_elements;
pub mod system_ui_bundle;
pub mod tim_palette_context;
pub mod title_pak;
pub mod ui_widgets;
