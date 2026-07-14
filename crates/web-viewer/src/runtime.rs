//! Runtime engine bindings for the browser: the **simulation** half of the
//! play page (`site/play.html`).
//!
//! [`LegaiaRuntime`] wraps a real [`legaia_engine_core::scene::SceneHost`] -
//! the same host the native `legaia-engine play-window` drives - so the browser
//! runs the ported engine itself, not a re-implementation of it: the field /
//! event VM, the free-movement controller with its per-scene walkability grid,
//! floor-height sampling, NPC motion VMs, the interaction probe, and the
//! inline-script dialogue runner. The page's job each frame is only to hand it
//! a pad word, tick it, and draw what [`crate::play`] reports.
//!
//! ### Minimal mode (no disc)
//! `new()` constructs a bare `World` + `MenuRuntime` - enough to prove the
//! engine VMs compile to `wasm32-unknown-unknown` and the tick path is callable
//! from JS.
//!
//! ### Disc mode (after `load_disc`)
//! `load_disc` builds a `SceneHost` from the user's own image, in memory, in
//! their browser (nothing is uploaded). `enter_field(name)` then boots a named
//! CDNAME scene exactly as the native shell's `enter_field_scene` does, and
//! assembles the render state [`crate::play`] serves to the page.

#[cfg(target_arch = "wasm32")]
use legaia_engine_audio::WebAudioOut;
use legaia_engine_core::menu_runtime::MenuRuntime;
use legaia_engine_core::scene::{SceneHost, SceneTickEvent};
use legaia_engine_core::world::{SceneMode, World};
use legaia_engine_vm::menu::{MenuInput, open as menu_open};
use wasm_bindgen::prelude::*;

use crate::play::{FieldRender, NpcRender, PlayerRig};

/// Bridge object the play page instantiates once. Holds a `World` +
/// `MenuRuntime` for the disc-free path, and - once `load_disc` has run - a
/// `SceneHost` plus the render state for the scene it is running.
#[wasm_bindgen]
pub struct LegaiaRuntime {
    pub(crate) world: World,
    pub(crate) menu: MenuRuntime,
    pub(crate) scene_host: Option<SceneHost>,
    /// Assembled static map for the current scene.
    pub(crate) field: Option<FieldRender>,
    /// Lead party member's field-form mesh.
    pub(crate) player: Option<PlayerRig>,
    /// The scene's MAN-placed NPC catalog.
    pub(crate) npcs: Option<NpcRender>,
    /// The scene's ANM bundle (the pose source for scene NPCs **and** placed
    /// props), resolved once per scene the way the native window's
    /// `find_scene_anm_bundle` does: entry-major, descriptor-count seed
    /// `[3, 5, 6, 7]` minor.
    pub(crate) scene_anm: Option<legaia_asset::player_anm::PlayerAnmBundle>,
    /// The PROT 0874 §1 party locomotion bundle - the pose source for the
    /// global-pool specials (save point / party heads).
    pub(crate) locomotion_anm: Option<legaia_asset::player_anm::PlayerAnmBundle>,
    #[cfg(target_arch = "wasm32")]
    audio_out: Option<WebAudioOut>,
}

#[wasm_bindgen]
impl LegaiaRuntime {
    #[wasm_bindgen(constructor)]
    pub fn new() -> LegaiaRuntime {
        console_error_panic_hook::set_once();
        let mut world = World::default();
        world.spawn_actor(0).default_pos = legaia_engine_vm::Position::new(0, 0);
        world.mode = SceneMode::Title;
        let menu = MenuRuntime::new("/saves");
        Self {
            world,
            menu,
            scene_host: None,
            field: None,
            player: None,
            npcs: None,
            scene_anm: None,
            locomotion_anm: None,
            #[cfg(target_arch = "wasm32")]
            audio_out: None,
        }
    }

    /// Load a disc image from raw in-memory bytes.
    ///
    /// `raw_bytes` may be either a Mode2/2352 full disc image (`.bin`) - PROT.DAT
    /// and CDNAME.TXT are extracted via an ISO9660 walk - or the raw contents of
    /// `PROT.DAT`. `cdname_text` overrides any CDNAME.TXT found on the disc; pass
    /// an empty string to use the disc's own.
    ///
    /// Returns the number of PROT entries parsed. Nothing leaves the browser.
    pub fn load_disc(&mut self, raw_bytes: Vec<u8>, cdname_text: String) -> Result<u32, JsValue> {
        use crate::disc::{extract_cdname_txt, extract_prot_dat, extract_scus, is_mode2_2352_disc};

        let (prot_bytes, auto_cdname, scus) = if is_mode2_2352_disc(&raw_bytes) {
            let prot = extract_prot_dat(&raw_bytes)
                .ok_or_else(|| JsValue::from_str("load_disc: PROT.DAT not found in disc image"))?;
            let cdname = extract_cdname_txt(&raw_bytes);
            let scus = extract_scus(&raw_bytes);
            (prot, cdname, scus)
        } else {
            (raw_bytes, None, None)
        };

        let cdname_resolved = if !cdname_text.is_empty() {
            Some(cdname_text.as_str())
        } else {
            auto_cdname.as_deref()
        };
        let mut host = SceneHost::from_prot_bytes(prot_bytes, cdname_resolved)
            .map_err(|e| JsValue::from_str(&format!("load_disc: {e}")))?;
        // Retail new-game defaults from the disc's own executable: a cold
        // scene entry (the page's scene picker, no save imported) seeds the
        // template party + starting bag, so the engine never runs a zeroed
        // scaffold roster. Best-effort - a PROT.DAT-only load has no SCUS and
        // keeps the old behaviour.
        if let Some(scus) = scus {
            if let Some(party) = legaia_asset::new_game::StartingParty::from_scus(&scus) {
                host.new_game_defaults = Some(legaia_engine_core::new_game::NewGameDefaults {
                    party,
                    inventory: legaia_asset::new_game::StartingInventory::from_scus(&scus),
                });
            }
        }

        let count = host.index.entry_count() as u32;
        self.scene_host = Some(host);
        self.field = None;
        self.player = None;
        self.npcs = None;
        Ok(count)
    }

    /// `true` if a disc has been loaded.
    pub fn disc_loaded(&self) -> bool {
        self.scene_host.is_some()
    }

    /// Boot a named CDNAME scene (e.g. `"town01"`) and assemble everything the
    /// page draws. This is the real field entry: the scene's assets, the
    /// walkability grid + elevation overrides, the MAN system script, the player
    /// install, the encounter session. World-map labels (`map01`..`map03`) route
    /// through the world-map entry, which installs the overworld controller
    /// instead.
    ///
    /// Returns the same JSON as [`Self::state_json`]. Throws when the disc isn't
    /// loaded or the label is unknown.
    pub fn enter_field(&mut self, name: &str) -> Result<String, JsValue> {
        let host = self
            .scene_host
            .as_mut()
            .ok_or_else(|| JsValue::from_str("enter_field: call load_disc first"))?;
        // Faithful-play arming, matching the native play-window's flags:
        // dialogue through the field VM (so branch handlers - flag sets,
        // GIVE_ITEM, scene changes - actually execute), retail's leading-edge
        // wall footprint, solid NPC bodies, per-step terrain follow, and NPCs
        // walking their MAN-authored routes.
        host.world.use_vm_dialogue = true;
        host.world.follow_terrain_height = true;
        host.world.leading_edge_wall_probes = true;
        host.world.solid_field_npcs = true;
        host.world.animate_field_npcs = true;
        if legaia_engine_core::scene::is_world_map_scene(name) {
            host.enter_world_map_scene(name)
                .map_err(|e| JsValue::from_str(&format!("enter_field({name}): {e:#}")))?;
        } else {
            host.enter_field_scene(name, 0)
                .map_err(|e| JsValue::from_str(&format!("enter_field({name}): {e:#}")))?;
        }
        self.rebuild_render_state()?;
        self.seat_player();
        Ok(self.state_json())
    }

    /// Route this frame's pad word into the engine. Bit layout is the PSX digital
    /// pad ([`legaia_engine_core::input::PadButton`]): `0x0008` Start, `0x0010`
    /// Up, `0x0020` Right, `0x0040` Down, `0x0080` Left, `0x1000` Triangle,
    /// `0x2000` Circle, `0x4000` Cross, `0x8000` Square. Edge detection is the
    /// engine's - just hand it the held set each frame.
    pub fn set_pad(&mut self, mask: u16) {
        match self.scene_host.as_mut() {
            Some(h) => h.world.set_pad(mask),
            None => self.world.set_pad(mask),
        }
    }

    /// Tell the engine where the camera is looking, so the free-movement
    /// controller remaps the d-pad camera-relative ("up" walks away from the
    /// camera). PSX 12-bit angle units (`4096` = a full turn); the field
    /// controller quantises it to the nearest quarter-turn, as retail does.
    pub fn set_camera_azimuth(&mut self, units: u16) {
        if let Some(h) = self.scene_host.as_mut() {
            h.world.field_camera_azimuth = units % 4096;
        }
    }

    /// Advance the engine one frame. Returns `""` normally, or the label of the
    /// scene the engine just walked into (a door / warp) - the page rebuilds its
    /// render state whenever the return is non-empty.
    pub fn tick_frame(&mut self) -> Result<String, JsValue> {
        let Some(host) = self.scene_host.as_mut() else {
            self.world.tick();
            return Ok(String::new());
        };
        let event = host
            .tick()
            .map_err(|e| JsValue::from_str(&format!("tick: {e:#}")))?;
        if let SceneTickEvent::SceneEntered { name } = event {
            self.rebuild_render_state()?;
            return Ok(name);
        }
        Ok(String::new())
    }

    /// One-line engine state for the HUD:
    /// ```text
    /// { "scene": "town01", "frame": 421, "mode": "Field",
    ///   "actors": 12, "npcs": 9,
    ///   "player": { "x": 2688, "y": -256, "z": 2432, "facing": 2048,
    ///               "walking": true },
    ///   "dialog": { "text": "...", "options": ["Yes", "No"], "cursor": 0 } }
    /// ```
    /// `dialog` is `null` when no box is up.
    pub fn state_json(&self) -> String {
        let Some(h) = self.scene_host.as_ref() else {
            return serde_json::json!({
                "scene": serde_json::Value::Null,
                "frame": self.world.frame,
                "mode": format!("{:?}", self.world.mode),
                "actors": 0,
                "npcs": 0,
                "player": serde_json::Value::Null,
                "dialog": serde_json::Value::Null,
            })
            .to_string();
        };
        let w = &h.world;
        let player = w
            .player_actor_slot
            .and_then(|s| w.actors.get(s as usize))
            .map(|a| {
                serde_json::json!({
                    "x": a.move_state.world_x,
                    "y": a.move_state.world_y,
                    "z": a.move_state.world_z,
                    "facing": a.move_state.render_26,
                    "walking": w.field_player_anim.as_ref().is_some_and(|f| f.walking),
                })
            })
            .unwrap_or(serde_json::Value::Null);
        serde_json::json!({
            "scene": h.scene.as_ref().map(|s| s.name.clone()),
            "frame": w.frame,
            "mode": format!("{:?}", w.mode),
            "actors": w.actors.iter().filter(|a| a.active).count(),
            "npcs": self.npcs.as_ref().map(|n| n.pack.entries.len()).unwrap_or(0),
            "player": player,
            "dialog": self.dialog_value(),
        })
        .to_string()
    }

    /// Attempt to start the WebAudio backend. Must be called from a user-gesture
    /// handler (browser autoplay policy). `true` on success.
    pub fn audio_init(&mut self) -> bool {
        #[cfg(target_arch = "wasm32")]
        {
            match WebAudioOut::new() {
                Ok(out) => {
                    self.audio_out = Some(out);
                    true
                }
                Err(e) => {
                    web_sys::console::error_1(&format!("audio_init: {e}").into());
                    false
                }
            }
        }
        #[cfg(not(target_arch = "wasm32"))]
        false
    }

    /// Frame counter.
    pub fn frame(&self) -> u64 {
        match self.scene_host.as_ref() {
            Some(h) => h.world.frame,
            None => self.world.frame,
        }
    }

    /// Active scene mode as a stable enum string (`Field`, `WorldMap`, ...).
    pub fn scene_mode(&self) -> String {
        match self.scene_host.as_ref() {
            Some(h) => format!("{:?}", h.world.mode),
            None => format!("{:?}", self.world.mode),
        }
    }

    /// Open the disc-free scaffold menu (the headless [`MenuRuntime`] - the
    /// retail pause menu's screens are a native-only draw path today).
    pub fn open_menu(&mut self) {
        menu_open(&mut self.menu.ctx);
    }

    pub fn menu_is_open(&self) -> bool {
        self.menu.is_open()
    }

    pub fn menu_label(&self) -> String {
        self.menu.current_label().to_string()
    }

    /// Tick the scaffold menu with a packed button mask
    /// (`cross | circle<<1 | triangle<<2 | square<<3 | up<<4 | down<<5 |
    /// left<<6 | right<<7`).
    pub fn menu_tick(&mut self, button_mask: u8) -> JsValue {
        let input = MenuInput {
            cross: button_mask & 0x01 != 0,
            circle: button_mask & 0x02 != 0,
            triangle: button_mask & 0x04 != 0,
            square: button_mask & 0x08 != 0,
            up: button_mask & 0x10 != 0,
            down: button_mask & 0x20 != 0,
            left: button_mask & 0x40 != 0,
            right: button_mask & 0x80 != 0,
        };
        let event = self.menu.tick(&mut self.world, input);
        JsValue::from_str(&format!("{event:?}"))
    }
}

impl LegaiaRuntime {
    /// Decode the live dialogue box (the field VM's inline-script runner) into
    /// the JSON the HUD prints. Glyph bytes are ASCII-compatible from `0x20`.
    fn dialog_value(&self) -> serde_json::Value {
        let Some(h) = self.scene_host.as_ref() else {
            return serde_json::Value::Null;
        };
        let Some(id) = h.world.inline_dialogue.as_ref() else {
            return serde_json::Value::Null;
        };
        let ascii = |bytes: &[u8]| -> String {
            bytes
                .iter()
                .map(|&b| {
                    if (0x20..=0x7E).contains(&b) {
                        b as char
                    } else {
                        ' '
                    }
                })
                .collect::<String>()
                .trim_end()
                .to_string()
        };
        let text = ascii(&id.page_bytes());
        if text.trim().is_empty() {
            return serde_json::Value::Null;
        }
        let options: Vec<String> = id
            .menu_active()
            .then(|| id.picker())
            .flatten()
            .map(|p| p.options.iter().map(|o| ascii(&o.label)).collect())
            .unwrap_or_default();
        serde_json::json!({
            "text": text,
            "options": options,
            "cursor": id.picker_cursor(),
        })
    }

    /// Rebuild the page-facing render state for the scene the host now holds:
    /// the assembled map, the lead's posed mesh, the NPC catalog. Runs on scene
    /// entry and on every door the engine walks through.
    fn rebuild_render_state(&mut self) -> Result<(), JsValue> {
        self.field = None;
        self.player = None;
        self.npcs = None;
        self.scene_anm = None;
        self.locomotion_anm = None;
        let Some(host) = self.scene_host.as_ref() else {
            return Ok(());
        };
        let (Some(scene), Some(res)) = (host.scene.as_ref(), host.resources.as_ref()) else {
            return Err(JsValue::from_str(
                "enter: the scene loaded but built no resources",
            ));
        };
        let name = scene.name.clone();
        let is_world_map = legaia_engine_core::scene::is_world_map_scene(&name);
        self.field = Some(crate::play::build_field_render(
            &host.index,
            scene,
            res,
            is_world_map,
        ));
        // Pose sources, resolved the way the native window's
        // `find_scene_anm_bundle` does (entry-major, desc-seed minor). The
        // scene bundle poses the MAN NPCs and the bound placed props; the
        // locomotion bundle poses the global-pool specials.
        self.scene_anm = scene.entries.iter().find_map(|e| {
            [3usize, 5, 6, 7].into_iter().find_map(|desc| {
                legaia_asset::player_anm::find_in_entry(&e.bytes, desc)
                    .into_iter()
                    .next()
            })
        });
        self.locomotion_anm = host
            .index
            .entry_bytes(legaia_asset::character_pack::PROT_ENTRY_INDEX)
            .ok()
            .and_then(|b| legaia_asset::character_pack::field_locomotion_anm(&b).ok());
        // The NPC catalog resolves against the same TMD pool + VRAM, plus the
        // world's global pool for the `model >= 0xF0` specials - everything
        // the native play-window draws.
        match crate::field_npc::build_npc_catalog_play(
            &host.index,
            &name,
            res,
            &host.world.global_tmd_pool,
        ) {
            Ok(pack) => self.npcs = Some(NpcRender { pack }),
            Err(e) => crate::console_log(&format!("play: NPC catalog for {name}: {e}")),
        }
        self.build_player_rig();
        Ok(())
    }

    /// Put the player somewhere they can actually stand.
    ///
    /// Scene entry seats them at the retail **cold-boot spawn** - the fixed
    /// camera-window centre `FIELD_COLD_SPAWN_XZ` that `FUN_801D6704` uses on a
    /// non-warp entry. That is the right answer for `town01`, the one scene
    /// retail cold-boots into; every other scene is normally *entered through a
    /// door*, which overrides X/Z with the transition's entry tile. Dropping into
    /// one from the scene picker has no door to supply that, so the cold spawn can
    /// land outside the map entirely (a cave whose floor is nowhere near it).
    ///
    /// So: keep the cold spawn when it is walkable and inside the scene's
    /// populated area, and otherwise fall back to the walkable terrain tile
    /// closest to the middle of that area. Then sample the floor under the final
    /// position - the locomotion step is what normally does that, so without it
    /// the first frame would draw the character sunk into an elevated tier.
    fn seat_player(&mut self) {
        let Some(host) = self.scene_host.as_mut() else {
            return;
        };
        let Some(slot) = host.world.player_actor_slot.map(|s| s as usize) else {
            return;
        };
        // Candidate seats, each `(x, z, drawn_y)`.
        //
        // The **walk-ground heightfield** is the scene's actual floor, so it is
        // the first choice: a cave's terrain-*tile* meshes are its rock walls,
        // not its ground, and seating on one of those buries the player inside a
        // boulder. Scenes with no resolvable floor grid fall back to the tile /
        // placement draws.
        //
        // Each candidate carries the height it is *drawn* at, which is not always
        // what the floor sampler reports (a scene whose floor lives in its
        // meshes rather than in the floor-height LUT samples as 0), so the seat
        // prefers the sampler and falls back to the drawn height.
        let tiles: Vec<(i32, i32, i32)> = self
            .field
            .as_ref()
            .map(|f| match f.ground.as_ref() {
                // Every 16th vertex is plenty: the grid is 128-unit tiles and a
                // 4000-quad heightfield would otherwise cost a needless scan.
                Some(hf) => hf
                    .positions
                    .iter()
                    .step_by(16)
                    .map(|p| (p[0] as i32, p[2] as i32, p[1] as i32))
                    .collect(),
                None => f
                    .terrain
                    .iter()
                    .chain(f.placements.iter())
                    .map(|d| (d.world_x, d.world_z, d.world_y))
                    .collect(),
            })
            .unwrap_or_default();
        let (sx, sz) = match host.world.actors.get(slot) {
            Some(a) => (a.move_state.world_x as i32, a.move_state.world_z as i32),
            None => return,
        };
        let mut x = sx;
        let mut z = sz;
        let mut y = host.world.sample_field_floor_height(sx, sz);
        if !tiles.is_empty() {
            let dist2 = |a: (i32, i32), b: (i32, i32)| {
                let (dx, dz) = ((a.0 - b.0) as i64, (a.1 - b.1) as i64);
                dx * dx + dz * dz
            };
            let n = tiles.len() as i64;
            let cx = (tiles.iter().map(|t| t.0 as i64).sum::<i64>() / n) as i32;
            let cz = (tiles.iter().map(|t| t.1 as i64).sum::<i64>() / n) as i32;
            // "Inside the map" = the cold spawn has floor under it. Measuring
            // that against the *nearest* ground tile, not the map centre, keeps
            // a perfectly good spawn near the edge of a big scene - and moving a
            // player who did not need moving is not free: the relocation target
            // can be a walk-on trigger tile (a town exit), which the engine would
            // fire the moment the first tick crosses onto it.
            let nearest = tiles
                .iter()
                .map(|&t| dist2((t.0, t.1), (sx, sz)))
                .min()
                .unwrap_or(i64::MAX);
            let cold_ok =
                !host.world.field_tile_is_wall(sx as i16, sz as i16) && nearest < 1200 * 1200;
            if !cold_ok {
                let mut best: Option<((i32, i32, i32), i64)> = None;
                for &t in &tiles {
                    if host.world.field_tile_is_wall(t.0 as i16, t.1 as i16) {
                        continue;
                    }
                    // Never seat onto a walk-on trigger tile: the first tick
                    // would fire it, and the scene the player just picked would
                    // warp out from under them (a town exit does exactly this).
                    if host.tile_has_walk_on_trigger(t.0 as i16, t.1 as i16) {
                        continue;
                    }
                    let d = dist2((t.0, t.1), (cx, cz));
                    if best.is_none_or(|(_, bd)| d < bd) {
                        best = Some((t, d));
                    }
                }
                if let Some(((bx, bz, by), _)) = best {
                    x = bx;
                    z = bz;
                    // Prefer the sampler when it has an answer (a town's floor
                    // tiers do come from the LUT); fall back to the height the
                    // tile is actually drawn at.
                    let sampled = host.world.sample_field_floor_height(bx, bz);
                    y = if sampled != 0 { sampled } else { by };
                }
            }
        }
        if let Some(p) = host.world.actors.get_mut(slot) {
            p.move_state.world_x = x as i16;
            p.move_state.world_z = z as i16;
            p.move_state.world_y = y as i16;
        }
    }

    /// Resolve the lead's field-form mesh out of the global TMD pool (PROT 0874
    /// §0, seeded by `enter_field_scene`) and install the idle / walk clip pair
    /// the world ticks into the player actor's `pose_frame`.
    ///
    /// Mirrors the native play-window's player bind: the disc TMD's object table
    /// is truncated to the clip's bone count (retail caps the live object count
    /// at 10 - groups 10/11 are equipment-swap templates and are never drawn), so
    /// bone `i` poses object `i`.
    /// REF: FUN_8001E890
    fn build_player_rig(&mut self) {
        let Some(host) = self.scene_host.as_mut() else {
            return;
        };
        if host.world.mode != SceneMode::Field {
            return;
        }
        let lead = host.world.active_party.first().copied().unwrap_or(0) as usize;
        let Some(g) = host
            .world
            .global_tmd_pool
            .get(lead)
            .and_then(|s| s.as_ref())
            .map(std::sync::Arc::clone)
        else {
            crate::console_log(&format!(
                "play: global TMD pool has no field mesh for roster slot {lead}"
            ));
            return;
        };
        // The party locomotion bundle (PROT 0874 §1) banks the Vahn / Noa / Gala
        // trio only; any other lead renders in its TMD-local rest pose.
        let locomotion = host
            .index
            .entry_bytes(legaia_asset::character_pack::PROT_ENTRY_INDEX)
            .ok()
            .and_then(|b| legaia_asset::character_pack::field_locomotion_anm(&b).ok())
            .filter(|_| lead <= 2);
        let rec = |slot| legaia_asset::character_pack::locomotion_record_index(lead, slot);
        let bones = locomotion.as_ref().and_then(|bundle| {
            let idx = rec(legaia_asset::character_pack::LOCOMOTION_IDLE_SLOT);
            bundle.record(idx).ok().map(|r| r.bone_count as usize)
        });
        let mut tmd = g.tmd.clone();
        if let Some(b) = bones {
            tmd.objects.truncate(b);
        }
        let (base, object_ids, shading) =
            legaia_tmd::mesh::tmd_to_vram_mesh_field_hybrid(&tmd, &g.raw);
        if base.indices.is_empty() {
            crate::console_log("play: the lead's field mesh has no renderable prims");
            return;
        }
        let mut flat = Vec::with_capacity(shading.colors.len() * 4);
        for (c, &t) in shading.colors.iter().zip(shading.textured.iter()) {
            flat.extend_from_slice(&[c[0], c[1], c[2], if t != 0 { 255 } else { 0 }]);
        }
        let posed = Vec::with_capacity(base.positions.len() * 3);
        self.player = Some(PlayerRig {
            base,
            object_ids,
            flat,
            posed,
        });
        // Live idle/walk playback: the world's field tick picks the clip off the
        // locomotion movement flag and folds the pose into the player actor.
        let anim = locomotion.as_ref().and_then(|bundle| {
            let idle = legaia_engine_core::field_anim::FieldClipPlayer::from_record(
                bundle,
                rec(legaia_asset::character_pack::LOCOMOTION_IDLE_SLOT),
            )?;
            let walk = legaia_engine_core::field_anim::FieldClipPlayer::from_record(
                bundle,
                rec(legaia_asset::character_pack::LOCOMOTION_WALK_SLOT),
            )?;
            Some(legaia_engine_core::field_anim::FieldPlayerAnim::new(
                idle, walk,
            ))
        });
        host.world.set_field_player_anim(anim);
    }
}

impl Default for LegaiaRuntime {
    fn default() -> Self {
        Self::new()
    }
}
