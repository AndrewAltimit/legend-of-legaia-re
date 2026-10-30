//! The expansion workbench's edit-test loop on the play page: hot-reload a
//! pack into the running map, and the editor overlay the workbench's 3D
//! preview draws over it.
//!
//! The workbench's 3D preview **is** this page (embedded, `?xwpreview=1`),
//! and its *Play here* button talks to an ordinary play tab over a
//! `BroadcastChannel`; both hand the working pack's `.lxpack` bytes to
//! [`LegaiaRuntime::reload_expansion`]. That is the activation path of
//! [`LegaiaRuntime::load_expansion`] followed by the map entry of
//! [`LegaiaRuntime::enter_expansion_map`] with the player seated where they
//! stood (`SceneHost::enter_expansion_map_seated`), so what the preview
//! draws is the scene build the play page makes for the same pack - there
//! is no second builder. The pack's quest progress survives the reload: a
//! re-activation adopts the progress the deactivation left dormant.
//!
//! [`LegaiaRuntime::expansion_overlay_json`] is the active map read back
//! for the overlay pass (`site/js/play-view-editor-overlay.js`): the tile
//! rectangle on the field grid, each tile's wall quarters and floor height
//! from the engine's own floor sampler, and the spawns, exits, trigger and
//! encounter zones, props and NPCs of the definition the engine runs.

use legaia_engine_scene_host::scene_host::ExpansionReloadSeat;
use serde_json::{Value, json};
use wasm_bindgen::prelude::*;

use crate::runtime::LegaiaRuntime;

#[wasm_bindgen]
impl LegaiaRuntime {
    /// Hot-reload: activate the `.lxpack` `bytes` as [`Self::load_expansion`]
    /// does and, when the player is on a map of the pack that the new
    /// version still has, re-enter it with the player where they stood.
    /// JSON `{ ok, pack, errors, warnings, map, reentered, ms }`: `map` the
    /// map the player is on afterwards, `reentered` whether the map was
    /// rebuilt. A pack with errors is refused and the running one stays;
    /// so is any pack while the player is on a pack map but off its field
    /// (a fight, a card screen), answered with `busy: true`.
    pub fn reload_expansion(&mut self, bytes: &[u8]) -> String {
        self.reload_expansion_core(bytes).to_string()
    }

    /// Enter pack maps without their place-name banner and arrival cutscene
    /// (`SceneHost::quiet_expansion_arrivals`): the workbench's 3D preview
    /// sets it, and clears it to watch the arrival as a player would.
    pub fn expansion_set_quiet_arrivals(&mut self, on: bool) {
        if let Some(h) = self.scene_host.host_mut() {
            h.quiet_expansion_arrivals = on;
        }
    }

    /// The active pack map for the editor overlay (JSON, `null` off a pack
    /// map): `{ map, origin, width, height, unit, floor, walls, void,
    /// spawns, exits, triggers, encounters, props, npcs, player }`. `floor`
    /// is each tile centre's floor height (retail Y, down positive) from the
    /// world's floor sampler, `walls` its wall-quarter mask.
    pub fn expansion_overlay_json(&self) -> String {
        self.expansion_overlay().to_string()
    }

    /// Hide the field HUD (the party readout and the passive badges) while
    /// the workbench's 3D preview has an edit mode on, and show it again
    /// with `false`. The play page never sets it.
    pub fn set_editor_hud_hidden(&mut self, hidden: bool) {
        self.editor_hud_hidden = hidden;
    }

    /// Stand the player on map-local tile `(col, row)` of the active pack
    /// map, on its floor (the workbench's "walk from here"). `false` off a
    /// pack map or off its rectangle.
    pub fn expansion_seat_player(&mut self, col: u32, row: u32) -> bool {
        self.expansion_seat_player_core(col, row)
    }

    /// Move pack NPC `n` of the active map to map-local tile `(col, row)`,
    /// facing compass sector `facing`, without rebuilding the map: the
    /// anchor the draw, the collision box and the talk probe read, and its
    /// heading - what the workbench's 3D preview does for a moved NPC.
    /// `false` (the caller rebuilds instead) off a pack map, off its
    /// rectangle, for a scenery NPC or one that patrols (its walk legs are
    /// built from its tile at entry).
    pub fn expansion_place_npc(&mut self, n: u32, col: u32, row: u32, facing: u32) -> bool {
        self.expansion_place_npc_core(n, col, row, facing)
    }

    /// Move prop `i` of the active map to map-local tile `(col, row)` with
    /// offset `(ox, oz)` and yaw `yaw` degrees, without rebuilding the map:
    /// the definition the overlay, the prop's light and the floor read,
    /// its standable top, and the collision grid under the tile it left
    /// and the tile it closes (a solid prop with no top closes its tile).
    /// The draw is the page's (`play-expansion-live.js` moves it). `false`
    /// off a pack map, off its rectangle or for an index the map lacks.
    #[allow(clippy::too_many_arguments)]
    pub fn expansion_place_prop(
        &mut self,
        i: u32,
        col: u32,
        row: u32,
        ox: i32,
        oz: i32,
        yaw: f32,
    ) -> bool {
        self.expansion_place_prop_core(i, col, row, ox, oz, yaw)
    }

    /// Move lamp `i` of the active map's lighting to map-local tile
    /// `(col, row)` at `offset` (world units, `y` up), without rebuilding:
    /// the light is read from the definition every frame. `false` off a
    /// pack map, off its rectangle or for an index the map lacks.
    pub fn expansion_place_lamp(
        &mut self,
        i: u32,
        col: u32,
        row: u32,
        ox: i32,
        oy: i32,
        oz: i32,
    ) -> bool {
        self.expansion_place_lamp_core(i, col, row, [ox, oy, oz])
    }

    /// Move spawn `i` of the active map to map-local tile `(col, row)`,
    /// facing compass sector `facing`, without rebuilding: what the overlay
    /// and the next arrival read. `false` off a pack map, off its
    /// rectangle or for an index the map lacks.
    pub fn expansion_place_spawn(&mut self, i: u32, col: u32, row: u32, facing: u32) -> bool {
        self.expansion_place_spawn_core(i, col, row, facing)
    }

    /// Play cutscene `id` of the active pack now (the workbench's timeline
    /// "Play in 3D"), through the trigger path a map zone takes.
    pub fn expansion_preview_cutscene(&mut self, id: &str) -> bool {
        self.scene_host
            .host_mut()
            .is_some_and(|h| h.world.expansion_play_cutscene(id))
    }

    /// Start a fight now (the workbench's "Fight in 3D"): `kind` is
    /// `"formation"` or `"boss"`, through the paths a map trigger's `battle`
    /// and `boss` events take, so the monsters draw and act as they would in
    /// play.
    pub fn expansion_preview_fight(&mut self, kind: &str, id: &str) -> bool {
        let Some(h) = self.scene_host.host_mut() else {
            return false;
        };
        match kind {
            "formation" => h.world.start_expansion_battle(id),
            "boss" => h.world.start_expansion_boss(id),
            _ => false,
        }
    }

    /// Put one of every pack card the preview's collection lacks into it
    /// and open the binder (the workbench's card face preview: the engine's
    /// own card faces, portraits included). The preview's world is the
    /// workbench's own, not a player's save. The number of cards added.
    pub fn expansion_preview_cards(&mut self) -> u32 {
        let Some(h) = self.scene_host.host_mut() else {
            return 0;
        };
        let w = &mut h.world;
        let ids: Vec<String> = w
            .expansion
            .pack
            .as_deref()
            .map(|p| p.cards.cards.iter().map(|c| c.id.clone()).collect())
            .unwrap_or_default();
        let coll = &mut w.expansion.quests.progress.cards.collection;
        let mut added = 0;
        for id in &ids {
            if coll.count(id) == 0 {
                coll.add(id, 1);
                added += 1;
            }
        }
        if !ids.is_empty() {
            w.open_card_binder();
        }
        added
    }
}

impl LegaiaRuntime {
    /// JsValue-free body of [`Self::reload_expansion`].
    pub(crate) fn reload_expansion_core(&mut self, bytes: &[u8]) -> Value {
        let t0 = now_ms();
        let before = self
            .scene_host
            .host()
            .map(|h| h.expansion_reload_seat())
            .unwrap_or(ExpansionReloadSeat::OffPack);
        // On a pack map but not on its field - a fight, a card screen, a
        // mode round: activation would take the map out from under it, so
        // the reload waits until the field is back.
        if let ExpansionReloadSeat::Busy { map } = &before {
            return json!({
                "ok": false,
                "busy": true,
                "map": map,
                "reentered": false,
                "errors": ["the game is in a fight or a screen: the pack reloads once it is back on the field"],
            });
        }
        let mut v: Value =
            serde_json::from_str(&self.load_expansion_core(bytes)).unwrap_or_default();
        if v["ok"] != json!(true) {
            v["map"] = json!(
                self.scene_host
                    .host()
                    .and_then(|h| h.active_expansion_map())
            );
            v["reentered"] = json!(false);
            return v;
        }
        let mut reentered = false;
        if let ExpansionReloadSeat::Field { map, seat } = before {
            let has = self
                .scene_host
                .host()
                .and_then(|h| h.world.expansion.pack.as_ref())
                .is_some_and(|p| p.maps.contains_key(&map));
            if has {
                // The map entry resets the follow camera; a reload keeps the
                // framing the player had.
                let camera = self.scene_host.camera().clone();
                match self.enter_expansion_map_seated_core(&map, "", Some(seat)) {
                    Ok(_) => {
                        reentered = true;
                        *self.scene_host.camera_mut() = camera;
                        // A reload is not an arrival: no place-name banner.
                        if let Some(h) = self.scene_host.host_mut() {
                            h.world.cutscene.text_balloon = None;
                        }
                    }
                    Err(e) => {
                        if let Some(errs) = v["errors"].as_array_mut() {
                            errs.push(json!(e));
                        }
                        v["ok"] = json!(false);
                    }
                }
            }
        }
        v["map"] = json!(
            self.scene_host
                .host()
                .and_then(|h| h.active_expansion_map())
        );
        v["reentered"] = json!(reentered);
        v["ms"] = json!((now_ms() - t0).round());
        v
    }

    /// Body of [`Self::expansion_overlay_json`].
    pub(crate) fn expansion_overlay(&self) -> Value {
        let Some(host) = self.scene_host.host() else {
            return Value::Null;
        };
        let w = &host.world;
        let Some(active) = w.expansion.map.as_ref() else {
            return Value::Null;
        };
        let def = &active.def;
        let grid = &active.grid;
        let (gw, gh) = (grid.width, grid.height);
        let mut floor = Vec::with_capacity(gw * gh);
        let mut walls = Vec::with_capacity(gw * gh);
        let mut void = Vec::with_capacity(gw * gh);
        for row in 0..gh {
            for col in 0..gw {
                let t = &grid.tiles[row * gw + col];
                let (x, z) = def.tile_centre(col as u16, row as u16);
                floor.push(if t.void {
                    0
                } else {
                    w.sample_field_floor_height(x, z)
                });
                walls.push(t.walls);
                void.push(u8::from(t.void));
            }
        }
        let kind = |e: &legaia_expansion_format::map::MapEvent| {
            serde_json::to_value(e)
                .ok()
                .and_then(|v| v.get("kind").and_then(Value::as_str).map(str::to_string))
                .unwrap_or_default()
        };
        let player = w.actors.first().map(|p| {
            [
                i32::from(p.move_state.world_x),
                i32::from(p.move_state.world_y),
                i32::from(p.move_state.world_z),
            ]
        });
        json!({
            "map": active.map_id,
            "origin": def.tiles.origin,
            "width": gw,
            "height": gh,
            "unit": 128,
            "floor": floor,
            "walls": walls,
            "void": void,
            "spawns": def.spawns.iter().map(|s| json!({ "id": s.id, "tile": s.tile, "facing": s.facing })).collect::<Vec<_>>(),
            "exits": def.exits.iter().map(|e| json!({ "id": e.id, "rect": e.rect })).collect::<Vec<_>>(),
            "triggers": def.triggers.iter().map(|t| json!({ "id": t.id, "rect": t.rect, "kind": kind(&t.event), "once": t.once })).collect::<Vec<_>>(),
            "encounters": def.encounters.iter().map(|e| json!({ "id": e.id, "rect": e.rect, "table": e.table })).collect::<Vec<_>>(),
            "props": def.props.iter().map(|p| json!({ "id": p.id, "tile": p.tile, "offset": p.offset, "solid": p.solid, "top": p.top, "footprint": p.footprint })).collect::<Vec<_>>(),
            "npcs": def.npcs.iter().map(|n| json!({ "id": n.id, "tile": n.tile, "scenery": n.scenery, "patrol": n.patrol })).collect::<Vec<_>>(),
            "player": player,
        })
    }

    /// Body of [`Self::expansion_place_npc`].
    pub(crate) fn expansion_place_npc_core(
        &mut self,
        n: u32,
        col: u32,
        row: u32,
        facing: u32,
    ) -> bool {
        let Some(host) = self.scene_host.host_mut() else {
            return false;
        };
        let w = &mut host.world;
        let Some(active) = w.expansion.map.as_mut() else {
            return false;
        };
        let (Ok(n), Ok(c), Ok(r)) = (usize::try_from(n), u16::try_from(col), u16::try_from(row))
        else {
            return false;
        };
        if usize::from(c) >= active.grid.width || usize::from(r) >= active.grid.height {
            return false;
        }
        let Some(npc) = active.def.npcs.get(n) else {
            return false;
        };
        if npc.scenery || !npc.patrol.is_empty() {
            return false;
        }
        let Some(slot) = legaia_expansion_format::map::npc_slot(n) else {
            return false;
        };
        let (x, z) = active.def.tile_centre(c, r);
        let (Ok(x), Ok(z)) = (i16::try_from(x), i16::try_from(z)) else {
            return false;
        };
        let facing = (facing & 7) as u8;
        // The definition the overlay and the talk read follows the move.
        let def = std::sync::Arc::make_mut(&mut active.def);
        def.npcs[n].tile = [c, r];
        def.npcs[n].facing = facing;
        w.npcs.positions.insert(slot, (x, z));
        w.npcs.entry_positions.insert(slot, (x, z));
        w.npcs
            .headings
            .insert(slot, (i16::from(facing) * 0x200 + 0x800) & 0x0FFF);
        true
    }

    /// Body of [`Self::expansion_place_prop`].
    pub(crate) fn expansion_place_prop_core(
        &mut self,
        i: u32,
        col: u32,
        row: u32,
        ox: i32,
        oz: i32,
        yaw: f32,
    ) -> bool {
        use legaia_scene_formats::field_objects::GRID_DIM;
        let Some(host) = self.scene_host.host_mut() else {
            return false;
        };
        let w = &mut host.world;
        let Some(active) = w.expansion.map.as_mut() else {
            return false;
        };
        let (Ok(i), Ok(c), Ok(r)) = (usize::try_from(i), u16::try_from(col), u16::try_from(row))
        else {
            return false;
        };
        let (gw, gh) = (active.grid.width, active.grid.height);
        if usize::from(c) >= gw || usize::from(r) >= gh || i >= active.def.props.len() {
            return false;
        }
        let clamp = |v: i32| v.clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16;
        let def = std::sync::Arc::make_mut(&mut active.def);
        let old = def.props[i].tile;
        let p = &mut def.props[i];
        p.tile = [c, r];
        p.offset[0] = clamp(ox);
        p.offset[2] = clamp(oz);
        p.rotation[1] = yaw;
        let (solid, top) = (p.solid, p.top.is_some());
        if top {
            active.refresh_floor_tops();
        }
        if !solid || top || old == [c, r] {
            return true;
        }
        // The closed tile follows the prop: the tile it left takes its own
        // walls back (its style's and the terrain's) unless another solid
        // prop still stands there, the tile it reaches is closed. The wall
        // quarters of tile `(gc, gr)` sit one byte row down.
        let def = &active.def;
        let grid = &active.grid;
        let cg = &mut w.terrain.collision_grid;
        let mut terrain: Option<Vec<u8>> = None;
        for t in [old, [c, r]] {
            let (tc, tr) = (usize::from(t[0]), usize::from(t[1]));
            if tc >= gw || tr >= gh {
                continue;
            }
            let closed = def
                .props
                .iter()
                .any(|p| p.tile == t && p.solid && p.top.is_none());
            let walls = if closed {
                0xF
            } else {
                let extra = if def.terrain.is_some() {
                    let tw = terrain.get_or_insert_with(|| {
                        def.terrain_walls(grid).unwrap_or_else(|_| vec![0; gw * gh])
                    });
                    tw[tr * gw + tc]
                } else {
                    0
                };
                grid.tiles[tr * gw + tc].walls | extra
            };
            let gc = usize::from(def.tiles.origin[0]) + tc;
            let gr = usize::from(def.tiles.origin[1]) + tr;
            if let Some(b) = cg.get_mut((gr + 1) * GRID_DIM + gc) {
                *b = (walls << 4) | (*b & 0x0F);
            }
        }
        true
    }

    /// Body of [`Self::expansion_place_lamp`].
    pub(crate) fn expansion_place_lamp_core(
        &mut self,
        i: u32,
        col: u32,
        row: u32,
        offset: [i32; 3],
    ) -> bool {
        let Some(active) = self
            .scene_host
            .host_mut()
            .and_then(|h| h.world.expansion.map.as_mut())
        else {
            return false;
        };
        let (Ok(i), Ok(c), Ok(r)) = (usize::try_from(i), u16::try_from(col), u16::try_from(row))
        else {
            return false;
        };
        if usize::from(c) >= active.grid.width
            || usize::from(r) >= active.grid.height
            || i >= active.def.lighting.lamps.len()
        {
            return false;
        }
        let clamp = |v: i32| v.clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16;
        let def = std::sync::Arc::make_mut(&mut active.def);
        def.lighting.lamps[i].tile = [c, r];
        def.lighting.lamps[i].offset = offset.map(clamp);
        true
    }

    /// Body of [`Self::expansion_place_spawn`].
    pub(crate) fn expansion_place_spawn_core(
        &mut self,
        i: u32,
        col: u32,
        row: u32,
        facing: u32,
    ) -> bool {
        let Some(active) = self
            .scene_host
            .host_mut()
            .and_then(|h| h.world.expansion.map.as_mut())
        else {
            return false;
        };
        let (Ok(i), Ok(c), Ok(r)) = (usize::try_from(i), u16::try_from(col), u16::try_from(row))
        else {
            return false;
        };
        if usize::from(c) >= active.grid.width
            || usize::from(r) >= active.grid.height
            || i >= active.def.spawns.len()
        {
            return false;
        }
        let def = std::sync::Arc::make_mut(&mut active.def);
        def.spawns[i].tile = [c, r];
        def.spawns[i].facing = (facing & 7) as u8;
        true
    }

    /// Body of [`Self::expansion_seat_player`].
    pub(crate) fn expansion_seat_player_core(&mut self, col: u32, row: u32) -> bool {
        let Some(host) = self.scene_host.host_mut() else {
            return false;
        };
        let w = &mut host.world;
        let Some(active) = w.expansion.map.as_ref() else {
            return false;
        };
        let (Ok(c), Ok(r)) = (u16::try_from(col), u16::try_from(row)) else {
            return false;
        };
        if usize::from(c) >= active.grid.width || usize::from(r) >= active.grid.height {
            return false;
        }
        let (x, z) = active.def.tile_centre(c, r);
        let (Ok(x), Ok(z)) = (i16::try_from(x), i16::try_from(z)) else {
            return false;
        };
        let y = w.sample_field_floor_height(i32::from(x), i32::from(z));
        let Some(p) = w.actors.first_mut() else {
            return false;
        };
        p.move_state.world_x = x;
        p.move_state.world_z = z;
        p.move_state.world_y = y.clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16;
        w.recentre_field_window_on_player();
        true
    }
}

/// Wall-clock milliseconds (the reload's own cost, reported to the page).
fn now_ms() -> f64 {
    #[cfg(target_arch = "wasm32")]
    {
        js_sys::Date::now()
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs_f64() * 1000.0)
            .unwrap_or(0.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rebirth_files() -> legaia_expansion_format::PackFiles {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../packs/rebirth");
        legaia_expansion_format::read_files(std::path::Path::new(dir)).expect("read")
    }

    fn bundle(files: &legaia_expansion_format::PackFiles) -> Vec<u8> {
        legaia_expansion_format::bundle::write_bundle(files).expect("bundle")
    }

    /// What the page uploads for the running field scene: the arrays
    /// `play-view-scene-build.js` reads, in its order.
    fn scene_build(rt: &LegaiaRuntime) -> Vec<Vec<u8>> {
        let f32s = |v: Vec<f32>| v.iter().flat_map(|x| x.to_le_bytes()).collect::<Vec<u8>>();
        let u32s = |v: Vec<u32>| v.iter().flat_map(|x| x.to_le_bytes()).collect::<Vec<u8>>();
        vec![
            rt.field_vram_bytes(),
            f32s(rt.field_ground_positions()),
            u32s(rt.field_ground_indices()),
            u32s(rt.field_terrain_slots()),
            f32s(rt.field_terrain_positions()),
            u32s(rt.field_placement_slots()),
            f32s(rt.field_placement_positions()),
            u32s(rt.field_placement_anim_ids()),
        ]
    }

    /// No disc: a reload refuses activation and re-enters nothing; the
    /// overlay is `null` off a pack map, and seating fails.
    #[test]
    fn reload_without_a_disc_refuses() {
        let mut rt = LegaiaRuntime::new();
        let v = rt.reload_expansion_core(&bundle(&rebirth_files()));
        assert_eq!(v["ok"], false);
        assert_eq!(v["reentered"], false);
        assert!(rt.expansion_overlay().is_null());
        assert!(!rt.expansion_seat_player_core(1, 1));
    }

    /// Disc-gated: the hot reload is the play page's own build. Reloading an
    /// unchanged pack re-enters the map with the player where they stood
    /// and uploads byte-for-byte what a fresh entry of the same map uploads;
    /// the build is deterministic across runs; an edited tile shows in the
    /// overlay the reload reads back; and a pack with errors leaves the
    /// running map alone.
    #[test]
    fn reload_reenters_the_map_with_the_page_build() {
        let Some(disc) = std::env::var_os("LEGAIA_DISC_BIN") else {
            eprintln!("[skip] LEGAIA_DISC_BIN unset");
            return;
        };
        let bytes = std::fs::read(disc).expect("read the disc");
        let files = rebirth_files();
        let pack = bundle(&files);
        let map = "rebirth_tower_1f";

        // A fresh entry: what the play page draws for the map.
        let mut fresh = LegaiaRuntime::new();
        assert!(fresh.load_disc(bytes.clone(), String::new()).is_ok());
        let v: Value = serde_json::from_str(&fresh.load_expansion_core(&pack)).unwrap();
        assert_eq!(v["ok"], true, "{v}");
        fresh.enter_expansion_map_core(map, "").expect("enter");
        let fresh_build = scene_build(&fresh);

        // The same map entered, walked off the spawn, then hot-reloaded.
        let mut rt = LegaiaRuntime::new();
        assert!(rt.load_disc(bytes, String::new()).is_ok());
        let _ = rt.load_expansion_core(&pack);
        rt.enter_expansion_map_core(map, "").expect("enter");
        assert_eq!(scene_build(&rt), fresh_build, "entry is deterministic");
        let ov = rt.expansion_overlay();
        assert_eq!(ov["map"], map);
        let (w, h) = (
            ov["width"].as_u64().unwrap() as u32,
            ov["height"].as_u64().unwrap() as u32,
        );
        assert_eq!(ov["floor"].as_array().unwrap().len() as u32, w * h);
        // A floor tile next to the spawn to stand on.
        let spawn = &ov["spawns"][0]["tile"];
        let (sc, sr) = (
            spawn[0].as_u64().unwrap() as u32,
            spawn[1].as_u64().unwrap() as u32,
        );
        assert!(rt.expansion_seat_player_core(sc, sr));
        let before = rt.player_transform();
        let r = rt.reload_expansion_core(&pack);
        assert_eq!(r["ok"], true, "{r}");
        assert_eq!(r["reentered"], true);
        assert_eq!(r["map"], map);
        let after = rt.player_transform();
        assert_eq!(&before[..3], &after[..3], "the player keeps their seat");
        assert_eq!(scene_build(&rt), fresh_build, "reload == fresh entry");

        // Edit one tile of the map to a wall and reload: the overlay the
        // engine reads back has it.
        let mut edited = legaia_expansion_format::ExpansionPack::from_files(&files).unwrap();
        let def = edited.maps.get_mut(map).unwrap();
        let wall = def
            .tiles
            .legend
            .iter()
            .find(|(_, s)| s.walls.mask() == 0xF)
            .map(|(k, _)| k.clone())
            .expect("a wall legend entry");
        let (ec, er) = (sc as usize + 1, sr as usize);
        let mut row: Vec<char> = def.tiles.rows[er].chars().collect();
        row[ec] = wall.chars().next().unwrap();
        def.tiles.rows[er] = row.into_iter().collect();
        let edited_bytes = bundle(&edited.to_files().unwrap());
        let r = rt.reload_expansion_core(&edited_bytes);
        assert_eq!(r["ok"], true, "{r}");
        let ov = rt.expansion_overlay();
        assert_eq!(ov["walls"][er * w as usize + ec], 0xF);
        assert_ne!(scene_build(&rt), fresh_build, "the edit shows");

        // Off the field (the card binder up) a reload waits: refused as
        // busy, the map left as it is.
        if rt.expansion_preview_cards() > 0 {
            let r = rt.reload_expansion_core(&edited_bytes);
            assert_eq!(r["busy"], true, "{r}");
            assert_eq!(rt.expansion_overlay()["map"], map);
            rt.play_cards_close();
        }

        // An armed fight counts as off the field from its first frame.
        let formation = edited.battle.formations.keys().next().cloned().unwrap();
        assert!(
            rt.expansion_preview_fight("formation", &formation),
            "the fight arms"
        );
        let r = rt.reload_expansion_core(&edited_bytes);
        assert_eq!(r["busy"], true, "{r}");

        // A broken pack is refused; the edited map keeps running.
        let mut broken = edited.clone();
        broken.maps.get_mut(map).unwrap().tiles.rows[0].push('\u{263A}');
        let r = rt.reload_expansion_core(&bundle(&broken.to_files().unwrap()));
        assert_eq!(r["ok"], false);
        assert_eq!(rt.expansion_overlay()["map"], map);
    }

    /// Disc-gated: an NPC moved in place stands where a rebuild with the
    /// edited definition puts it - the anchor and the heading - and one
    /// that patrols is refused (the caller rebuilds).
    #[test]
    fn npc_moved_in_place_matches_a_rebuild() {
        let Some(disc) = std::env::var_os("LEGAIA_DISC_BIN") else {
            eprintln!("[skip] LEGAIA_DISC_BIN unset");
            return;
        };
        let bytes = std::fs::read(disc).expect("read the disc");
        let files = rebirth_files();
        let pack = legaia_expansion_format::ExpansionPack::from_files(&files).unwrap();
        // A map with a standing (no patrol, not scenery) NPC.
        let (map, n) = pack
            .maps
            .iter()
            .find_map(|(id, m)| {
                m.npcs
                    .iter()
                    .position(|x| !x.scenery && x.patrol.is_empty())
                    .map(|n| (id.clone(), n))
            })
            .expect("a standing NPC in Rebirth");
        let def = &pack.maps[&map];
        let [c, r] = def.npcs[n].tile;
        // One tile over, inside the rectangle.
        let w = def
            .tiles
            .rows
            .iter()
            .map(|r| r.chars().count())
            .max()
            .unwrap() as u16;
        let to = [if c + 1 < w { c + 1 } else { c - 1 }, r];
        let facing = (def.npcs[n].facing + 3) & 7;
        let slot = legaia_expansion_format::map::npc_slot(n).unwrap();
        let read = |rt: &LegaiaRuntime| {
            let w = &rt.scene_host.host().unwrap().world;
            (
                w.npcs.positions.get(&slot).copied(),
                w.npcs.headings.get(&slot).copied(),
            )
        };

        let mut rt = LegaiaRuntime::new();
        assert!(rt.load_disc(bytes, String::new()).is_ok());
        let _ = rt.load_expansion_core(&bundle(&files));
        rt.enter_expansion_map_core(&map, "").expect("enter");
        assert!(rt.expansion_place_npc_core(
            n as u32,
            u32::from(to[0]),
            u32::from(to[1]),
            u32::from(facing)
        ));
        let moved = read(&rt);
        assert_eq!(rt.expansion_overlay()["npcs"][n]["tile"], json!(to));

        let mut edited = pack.clone();
        let e = &mut edited.maps.get_mut(&map).unwrap().npcs[n];
        e.tile = to;
        e.facing = facing;
        let r = rt.reload_expansion_core(&bundle(&edited.to_files().unwrap()));
        assert_eq!(r["ok"], true, "{r}");
        assert_eq!(read(&rt), moved, "in place == rebuilt");

        // Off the rectangle and a patrolling NPC are refused.
        assert!(!rt.expansion_place_npc_core(n as u32, 999, 0, 0));
        if let Some(p) = edited.maps[&map]
            .npcs
            .iter()
            .position(|x| !x.patrol.is_empty())
        {
            assert!(!rt.expansion_place_npc_core(p as u32, 0, 0, 0));
        }
    }

    /// Disc-gated: a solid prop, a standable prop, a lamp and a spawn moved
    /// in place leave the world as a rebuild with the edited definition
    /// does - the whole collision grid, the standable tops, the lights and
    /// the overlay's spawns.
    #[test]
    fn props_lamps_spawns_moved_in_place_match_a_rebuild() {
        let Some(disc) = std::env::var_os("LEGAIA_DISC_BIN") else {
            eprintln!("[skip] LEGAIA_DISC_BIN unset");
            return;
        };
        let bytes = std::fs::read(disc).expect("read the disc");
        let files = rebirth_files();
        let pack = legaia_expansion_format::ExpansionPack::from_files(&files).unwrap();
        let mut rt = LegaiaRuntime::new();
        assert!(rt.load_disc(bytes, String::new()).is_ok());
        let mut checked = 0;
        for (map, def) in &pack.maps {
            let solid = def.props.iter().position(|p| p.solid && p.top.is_none());
            let Some(si) = solid else { continue };
            let grid = def.tile_grid().unwrap();
            let _ = rt.load_expansion_core(&bundle(&files));
            rt.enter_expansion_map_core(map, "").expect("enter");
            let grid_before = rt
                .scene_host
                .host()
                .unwrap()
                .world
                .terrain
                .collision_grid
                .clone();
            let o = def.tiles.origin.map(usize::from);
            let at = |t: [u16; 2]| (o[1] + usize::from(t[1]) + 1) * 0x80 + o[0] + usize::from(t[0]);
            // A tile the world leaves open with no prop on it, to move the
            // prop onto.
            let free = (0..grid.height)
                .flat_map(|r| (0..grid.width).map(move |c| [c as u16, r as u16]))
                .find(|&t| {
                    let g = grid.get(usize::from(t[0]), usize::from(t[1])).unwrap();
                    !g.void
                        && grid_before[at(t)] >> 4 == 0
                        && !def.props.iter().any(|p| p.tile == t)
                });
            let Some(to) = free else { continue };
            let mut edited = pack.clone();
            let e = edited.maps.get_mut(map).unwrap();
            e.props[si].tile = to;
            e.props[si].offset[0] = 8;
            e.props[si].rotation[1] = 90.0;
            assert!(rt.expansion_place_prop_core(
                si as u32,
                u32::from(to[0]),
                u32::from(to[1]),
                8,
                i32::from(e.props[si].offset[2]),
                90.0
            ));
            let top = e.props.iter().position(|p| p.top.is_some());
            if let Some(ti) = top {
                let t = e.props[ti].tile;
                let to = [t[0], (t[1] + 1).min(grid.height as u16 - 1)];
                e.props[ti].tile = to;
                let o = e.props[ti].offset;
                assert!(rt.expansion_place_prop_core(
                    ti as u32,
                    u32::from(to[0]),
                    u32::from(to[1]),
                    i32::from(o[0]),
                    i32::from(o[2]),
                    e.props[ti].rotation[1]
                ));
            }
            if !e.lighting.lamps.is_empty() {
                e.lighting.lamps[0].tile = to;
                e.lighting.lamps[0].offset = [4, 80, -4];
                assert!(rt.expansion_place_lamp_core(
                    0,
                    u32::from(to[0]),
                    u32::from(to[1]),
                    [4, 80, -4]
                ));
            }
            e.spawns[0].tile = to;
            e.spawns[0].facing = 3;
            assert!(rt.expansion_place_spawn_core(0, u32::from(to[0]), u32::from(to[1]), 3));
            let read = |rt: &LegaiaRuntime| {
                let w = &rt.scene_host.host().unwrap().world;
                let m = w.expansion.map.as_ref().unwrap();
                let tops: Vec<_> = (0..grid.height as i32 * 8)
                    .flat_map(|z| (0..grid.width as i32 * 8).map(move |x| (x, z)))
                    .map(|(x, z)| {
                        let o = def.tiles.origin.map(i32::from);
                        m.floor_top_at(o[0] * 128 + x * 16, o[1] * 128 + z * 16)
                    })
                    .collect();
                let lights: Vec<_> = w
                    .expansion_map_lights()
                    .iter()
                    .map(|l| l.pos.map(|v| v as i32))
                    .collect();
                (
                    w.terrain.collision_grid.clone(),
                    tops,
                    lights,
                    rt.expansion_overlay()["spawns"].clone(),
                )
            };
            let moved = read(&rt);
            assert_eq!(
                moved.0[at(to)] >> 4,
                0xF,
                "{map}: the prop closes its new tile"
            );
            assert_ne!(moved.0, grid_before, "{map}: the move changed the grid");
            let r = rt.reload_expansion_core(&bundle(&edited.to_files().unwrap()));
            assert_eq!(r["ok"], true, "{r}");
            let rebuilt = read(&rt);
            assert!(
                moved.0 == rebuilt.0,
                "{map}: collision grid in place == rebuilt"
            );
            assert_eq!(moved.1, rebuilt.1, "{map}: standable tops");
            assert_eq!(moved.2, rebuilt.2, "{map}: lights");
            assert_eq!(moved.3, rebuilt.3, "{map}: spawns");
            checked += 1;
        }
        assert!(checked > 0, "Rebirth has a map with a solid prop");
        assert!(!rt.expansion_place_prop_core(9999, 0, 0, 0, 0, 0.0));
    }

    /// Disc-gated, ignored (a measurement, not a check): what a hot reload
    /// of the largest map the format allows costs, split into the engine's
    /// map entry and the page's scene build. The workbench doc's cost table
    /// quotes it: `cargo test -p legaia-web-play --profile release-test
    /// --lib big_map_reload_cost -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn big_map_reload_cost() {
        let Some(disc) = std::env::var_os("LEGAIA_DISC_BIN") else {
            eprintln!("[skip] LEGAIA_DISC_BIN unset");
            return;
        };
        let bytes = std::fs::read(disc).expect("read the disc");
        let files = rebirth_files();
        let mut pack = legaia_expansion_format::ExpansionPack::from_files(&files).unwrap();
        let mut m = pack.maps["rebirth_tower_1f"].clone();
        let wall = m
            .tiles
            .legend
            .iter()
            .find(|(_, s)| s.walls.mask() == 0xF)
            .map(|(k, _)| k.chars().next().unwrap())
            .unwrap();
        let floor = m
            .tiles
            .legend
            .iter()
            .find(|(_, s)| s.walls.mask() == 0 && s.height == 0)
            .map(|(k, _)| k.chars().next().unwrap())
            .unwrap();
        const N: usize = 120;
        m.tiles.origin = [4, 4];
        m.size = None;
        m.tiles.rows = (0..N)
            .map(|r| {
                (0..N)
                    .map(|c| {
                        let edge = r == 0 || c == 0 || r == N - 1 || c == N - 1;
                        if edge || (r % 8 == 4 && c % 8 == 4) {
                            wall
                        } else {
                            floor
                        }
                    })
                    .collect()
            })
            .collect();
        m.spawns.truncate(1);
        m.spawns[0].tile = [60, 60];
        m.exits.clear();
        m.triggers.clear();
        m.encounters.clear();
        m.entrances.clear();
        m.npcs.clear();
        m.props.clear();
        m.enter_cutscenes.clear();
        m.decor.clear();
        m.edges.clear();
        m.decals.clear();
        m.emitters.clear();
        m.scatter.truncate(1);
        let decorated = !m.scatter.is_empty();
        if let Some(b) = m.scatter.first_mut() {
            b.rect = [1, 1, 118, 59];
            b.density = 0.6;
            b.on.clear();
        }
        pack.maps.insert("perf_big".into(), m);
        let bundle = bundle(&pack.to_files().unwrap());

        let mut rt = LegaiaRuntime::new();
        assert!(rt.load_disc(bytes, String::new()).is_ok());
        let t = std::time::Instant::now();
        let v: Value = serde_json::from_str(&rt.load_expansion_core(&bundle)).unwrap();
        assert_eq!(v["ok"], true, "{v}");
        let activate = t.elapsed();
        let t = std::time::Instant::now();
        rt.scene_host
            .host_mut()
            .unwrap()
            .enter_expansion_map("perf_big", "")
            .expect("enter");
        let engine = t.elapsed();
        let t = std::time::Instant::now();
        assert!(rt.rebuild_render_state().is_ok());
        let page = t.elapsed();
        let t = std::time::Instant::now();
        let r = rt.reload_expansion_core(&bundle);
        let reload = t.elapsed();
        assert_eq!(r["reentered"], true);
        let t = std::time::Instant::now();
        let ov = rt.expansion_overlay();
        let overlay = t.elapsed();
        let f = rt.field.as_ref().unwrap();
        eprintln!(
            "big map (120 x 120, scatter {decorated}): activate {activate:?}, engine entry {engine:?}, \
             page build {page:?}, whole reload {reload:?}, overlay read {overlay:?}; \
             {} placements, {} terrain draws, {} overlay tiles",
            f.placements.len(),
            f.terrain.len(),
            ov["floor"].as_array().map_or(0, Vec::len)
        );
    }

    /// Disc-gated, ignored (a measurement, not a check): each Isles map's
    /// entry cost on the play page's path - the engine's map entry, the
    /// page's scene build and the arrays it uploads - with the triangle
    /// count drawn and a hash of the uploaded arrays (a change of output
    /// shows as a change of hash): `cargo test -p legaia-web-play --profile
    /// release-test --lib isles_map_entry_cost -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn isles_map_entry_cost() {
        let Some(disc) = std::env::var_os("LEGAIA_DISC_BIN") else {
            eprintln!("[skip] LEGAIA_DISC_BIN unset");
            return;
        };
        let bytes = std::fs::read(disc).expect("read the disc");
        let pack = bundle(&rebirth_files());
        let mut rt = LegaiaRuntime::new();
        assert!(rt.load_disc(bytes, String::new()).is_ok());
        let v: Value = serde_json::from_str(&rt.load_expansion_core(&pack)).unwrap();
        assert_eq!(v["ok"], true, "{v}");
        for map in [
            "isles_beach",
            "isles_village",
            "isles_grotto",
            "isles_thornwood",
        ] {
            let t = std::time::Instant::now();
            rt.scene_host
                .host_mut()
                .unwrap()
                .enter_expansion_map(map, "")
                .expect("enter");
            let engine = t.elapsed();
            let t = std::time::Instant::now();
            assert!(rt.rebuild_render_state().is_ok());
            let page = t.elapsed();
            let t = std::time::Instant::now();
            let arrays = scene_build(&rt);
            let mut h = std::collections::hash_map::DefaultHasher::new();
            std::hash::Hash::hash(&arrays, &mut h);
            // Each distinct mesh the page uploads, as `ensure` builds it.
            let mut slots: Vec<(u32, u32)> = rt
                .field_terrain_slots()
                .into_iter()
                .map(|s| (s, 0))
                .chain(
                    rt.field_placement_slots()
                        .into_iter()
                        .zip(rt.field_placement_anim_ids()),
                )
                .collect();
            slots.sort_unstable();
            slots.dedup();
            let mut tris = 0;
            for (slot, anim) in slots {
                if rt.field_mesh_posed(slot, anim).is_err() {
                    continue;
                }
                let idx = rt.field_mesh_indices();
                tris += idx.len() / 3;
                let bits: Vec<u32> = rt
                    .field_mesh_positions()
                    .iter()
                    .map(|f| f.to_bits())
                    .collect();
                std::hash::Hash::hash(&bits, &mut h);
                std::hash::Hash::hash(&idx, &mut h);
                std::hash::Hash::hash(&rt.field_mesh_uvs(), &mut h);
                std::hash::Hash::hash(&rt.field_mesh_cba_tsb(), &mut h);
                std::hash::Hash::hash(&rt.field_mesh_flat_rgba(), &mut h);
            }
            let upload = t.elapsed();
            eprintln!(
                "{map}: engine entry {engine:?}, page build {page:?}, arrays {upload:?}; \
                 ~{tris} triangles, output hash {:016x}",
                std::hash::Hasher::finish(&h)
            );
        }
    }
}
