//! The staged menu warp drain and the scene-name table it resolves against.

use super::*;

/// The scene a travel art's destination word names, or `None` for a miss.
///
/// A word inside the TOC's header rows (`< RAW_TOC_INDEX_OFFSET`) names a
/// head define - `init_data 0`, `gameover_data 1` - not a scene. Retail's
/// resolve scan would match it, but there is no field to load there, and the
/// scene host's entry fails half-way through its reset. It is the word a
/// region record with an all-zero return triple stores (station3's), so a
/// Door of Light used there drops like any other unresolved word.
fn travel_scene(names: &legaia_prot::cdname::IndexMap, word: u32) -> Option<String> {
    if word < legaia_prot::cdname::RAW_TOC_INDEX_OFFSET {
        return None;
    }
    names.get(&word).cloned()
}

impl World {
    /// Drain a menu-staged Door use into the pause-menu session's travel-art
    /// hand-off ([`Self::begin_pause_session_exit`]), which runs the art and
    /// then issues the named scene transition the scene host consumes
    /// ([`Self::pending_named_scene_transition`]).
    ///
    /// **Door of Wind** ([`crate::world::MenuState::pending_warp`]): the staged triple is
    /// retail's `0x80084628` scene word + `0x80084624`/`0x8008462C` tile
    /// pair (`FUN_801D8B90` phase 3, from quick-travel placement record
    /// bytes `+2/+4/+5`). The scene word is the destination scene's raw
    /// CDNAME TOC index ([`crate::world::DiscTables::scene_toc_names`]), the
    /// same key the art's resolve looks up in the resident define table; the
    /// tile pair seats the party at `(tile << 7) + 0x40`. Exit code `5`, so
    /// the session hands on to Rula (`FUN_801EE328`).
    ///
    /// **Door of Light** ([`crate::world::MenuState::pending_escape`]): exit
    /// code `4` (`FUN_801D8A58`), so the session hands on to Riremito
    /// (`FUN_801EE094`). The three words then hold what the last long-layout
    /// region record the player stood in stored (`region[+9..+0xB]`,
    /// [`legaia_engine_battle::region_encounter::WorldMapReturn`]): a retail capture of a
    /// Door of Light in cave01 reads `0x55 @ (37, 109)` there and seats the
    /// party at `(37 << 7) + 0x40, (109 << 7) + 0x40` on map01. Only when no
    /// region has stored a triple yet (an engine-only state: a scene with no
    /// region table, or a use before the first step) does the drain fall back
    /// to the world-map panel host's last recorded map.
    ///
    /// A target that does not resolve logs retail's `UNFIND MAP NUMBER %d`
    /// diagnostic and drops the use before anything is installed - see
    /// [`crate::world::pause_session`] for why that differs from retail.
    ///
    /// REF: FUN_801D8B90 (stage), FUN_801D8A58 (escape exit code),
    /// FUN_801ED308 (the session), FUN_801EE094 / FUN_801EE328 (the arts)
    pub fn drain_staged_menu_warp(&mut self) {
        use crate::world::pause_session::PauseTravelTarget;
        use legaia_engine_menus::pause_screens::{
            MENU_EXIT_CODE_FIELD_ESCAPE, MENU_EXIT_CODE_WORLD_MAP_WARP,
        };
        if let Some(warp) = self.menu.pending_warp.take() {
            match travel_scene(&self.tables.scene_toc_names, u32::from(warp.scene_id)) {
                Some(name) => {
                    let target = PauseTravelTarget {
                        scene: name,
                        tile_x: warp.menu_x,
                        tile_z: warp.menu_y,
                    };
                    self.begin_pause_session_exit(MENU_EXIT_CODE_WORLD_MAP_WARP, target);
                }
                None => {
                    // Retail's miss arm prints and parks (phase 0x63).
                    log::warn!("menu warp: UNFIND MAP NUMBER {}", warp.scene_id);
                }
            }
        }
        if self.menu.pending_escape {
            self.menu.pending_escape = false;
            // The menu's installer refreshes the region setup before the
            // menu opens (`FUN_801F1278` calls `FUN_801D9E1C(player, 0)` at
            // `0x801F12F8`, with `+0x8E` / `+0x8F` preset to `0xFF`); the
            // player cannot move between that open and this drain, so the
            // refresh here reads the same tile.
            self.apply_region_battle_setup_at_player();
            // Retail's resolve reads the triple the last long-layout region
            // record stored (`FUN_801D9E1C`); the capture has cave01 leave
            // `0x55 @ (37, 109)` - the tile outside the cave mouth, not the
            // tile the party entered from - and the art seats the party there.
            if let Some(ret) = self
                .encounters
                .region_setup
                .and_then(|s| s.world_map_return)
            {
                match travel_scene(&self.tables.scene_toc_names, u32::from(ret.map_word)) {
                    Some(name) => {
                        let target = PauseTravelTarget {
                            scene: name,
                            tile_x: ret.tile_x,
                            tile_z: ret.tile_z,
                        };
                        self.begin_pause_session_exit(MENU_EXIT_CODE_FIELD_ESCAPE, target);
                    }
                    None => {
                        log::warn!("menu escape: UNFIND MAP NUMBER {}", ret.map_word);
                    }
                }
                return;
            }
            let visited = self
                .world_map
                .ctrl
                .as_ref()
                .and_then(|c| c.panels.visited.last().copied());
            match visited {
                Some(v) => {
                    let name = match v.map_id {
                        0 => "map01",
                        1 => "map02",
                        2 => "map03",
                        _ => {
                            log::warn!("menu escape: UNFIND MAP NUMBER {}", v.map_id);
                            return;
                        }
                    };
                    let target = PauseTravelTarget {
                        scene: name.to_string(),
                        tile_x: v.tile_x.clamp(0, 0xFF) as u8,
                        tile_z: v.tile_z.clamp(0, 0xFF) as u8,
                    };
                    self.begin_pause_session_exit(MENU_EXIT_CODE_FIELD_ESCAPE, target);
                }
                None => {
                    log::warn!("menu escape: no visited world-map record to return to");
                }
            }
        }
    }

    /// Install the CDNAME `#define` map (raw TOC index → block name) the
    /// menu-warp drain resolves a quick-travel `scene_id` against. The
    /// scene host wires this once at construction from the same parsed
    /// `CDNAME.TXT` its own scene loads use.
    pub fn install_scene_toc_names(&mut self, map: legaia_prot::cdname::IndexMap) {
        self.tables.scene_toc_names = map;
    }
}
