//! **Live scene preview**: a field or overworld scene entered through the
//! real [`SceneHost`] and ticked headless (no pad), so a viewer animates
//! exactly what the play hosts animate on the same scene.
//!
//! A static full-map view ([`crate::scene_assembly`]) bakes the draw lists
//! once, against the floor-height ladder the scene's MAN header ships. The
//! running game moves several things the bake cannot see, all of them driven
//! by the scene's own scripts on the world tick:
//!
//! - the **floor-height ladder** (field-VM op `0x4C` nibble 9): `jouina`'s
//!   pulsing path, `concnow`'s flesh pits - and `concnow`'s entry script
//!   replaces the whole ladder (`4C 9E`), so its baked terrain heights are not
//!   the heights the scene is ever shown at (its placed objects keep theirs:
//!   they were spawned before the install);
//! - **placed-prop clips** (the prop bank's cursors, retail `FUN_800204F8`):
//!   the Rim Elm windmill's sails;
//! - the **MAN-placed actors' clips** - on the overworld these play out of
//!   the kingdom bundle's slot-4 ANM bank
//!   (`docs/formats/world-map-overlay.md`);
//! - the **ambient move-VM tree** and the scripted VRAM effects (palette
//!   cyclers, lightning, VDF vertex morphs).
//!
//! This type owns no second implementation of any of them: it is a
//! [`SceneHost`] entered the way the play pages' scene picker enters it
//! ([`crate::world::World::stage_picker_entry`] then
//! [`SceneHost::enter_field_scene`], or
//! [`SceneHost::enter_world_map_scene`] for a kingdom map), stepped by
//! [`SceneHost::tick`], and read through the same kernels the play hosts
//! call - [`FloorWave`] for the terrain / decoration cells (the live rungs)
//! and [`crate::world::World::placed_floor_offsets`] for placed objects (the
//! ladder their actors spawned on),
//! [`crate::field_ground::live_render_positions`] for the walk ground,
//! [`crate::field_env::PropAnimBank::pose_key`] for props and
//! [`crate::world::World::step_field_vram_effects`] for VRAM.
//!
//! # Rooms
//!
//! A full map shows every room of a scene at once, but the ladder is one
//! scratchpad array that holds only the room the player stands in. A scene's
//! system script can re-install it per room: `concnow`'s loop tests the
//! region-type mask (op `0x42` mode 0 over `_DAT_8007B8F4`) and, on a room
//! change, stops the oscillators (`4C 9F`), installs that room's ladder
//! (`4C 9E`) and spawns its own (`4C 90`). Drawing every room on the entry
//! room's ladder lifts rooms the player is not in onto rungs they never take.
//!
//! So the preview keeps one ladder per room ([`RoomLadder`]), keyed by the
//! region-type mask a tile carries (the `.MAP` region table,
//! [`crate::field_regions::refresh_region_attributes`] - the same mask the
//! script tests). At entry a throwaway probe host seats a standing player in
//! each room in turn ([`SceneHost::debug_seat_standing`]) and lets the
//! scene's own script react; whatever it installs is that room's ladder,
//! and the oscillators it spawned are stepped on
//! [`crate::world::step_floor_ladder`], the world's own kernel. The room the
//! live player stands in reads the live world itself. Nothing here reads the
//! script: a scene whose rooms share one ladder probes to identical ladders,
//! and a scene with no ladder op at all is not probed.
//!
//! A viewer never follows the scene out: a door, warp or scripted battle the
//! headless world reaches re-enters the viewed scene instead (its entry
//! script runs again, as on a fresh visit), and after
//! [`MAX_RESTARTS`] such re-entries the preview stops ticking and the view
//! holds its last frame.
//!
//! The overworld's ground does not follow the field ladder (its bulk
//! continent is a separate pass), so a kingdom map reports no floor wave and
//! has no rooms; its palette animation is the CLUT walker, which a viewer
//! drives on its own ([`crate::clut_walk_anim`]).

use std::sync::Arc;

use legaia_engine_vm::field_actor_timers::FloorTierBob;

use crate::field_env::{EnvDraw, FloorWave, PlacedWindowKey, PropPoseKey};
use crate::scene::{ProtIndex, SceneHost, SceneTickEvent};
use crate::world::SceneMode;

/// Re-entries of the viewed scene a preview performs before it stops
/// ticking (see the module docs).
pub const MAX_RESTARTS: u32 = 3;

/// Ticks the room probe lets the entry script run before the first seat, so
/// its install slice has run and it sits in its per-frame loop.
const PROBE_SETTLE_TICKS: u32 = 30;

/// Ticks the room probe runs after seating the player in a room - enough for
/// the system loop to make a pass and react to the new region mask.
const PROBE_ROOM_TICKS: u32 = 12;

/// Tiles per side of the field map grid.
const GRID: i32 = 0x80;

/// One room's floor-height ladder (see the module docs' **Rooms**).
#[derive(Debug, Clone)]
pub struct RoomLadder {
    /// The region-type mask the room's tiles carry.
    pub mask: u32,
    /// The room's live rungs (scratchpad frame, `0x1F80035C`), stepped by
    /// [`Self::bobs`].
    pub lut: [i16; 16],
    /// The ladder a window-owned placed object in this room was spawned on:
    /// the probe's last window re-plan.
    pub placed_lut: [i16; 16],
    /// The oscillators the room's own block spawned.
    pub bobs: Vec<FloorTierBob>,
}

/// A headless, live field or overworld scene (see the module docs).
pub struct LiveScene {
    /// The engine scene host the preview runs. Public so a viewer can read
    /// any further world state through the host's own accessors.
    pub host: SceneHost,
    name: String,
    world_map: bool,
    /// The ladder the scene's MAN header ships (MAN frame) - the one a static
    /// assembly resolved every draw against, and the base of [`FloorWave`].
    scene_lut: Option<[i16; 16]>,
    /// Region-type mask per tile (`row * 0x80 + col`); empty for a scene
    /// without rooms.
    tile_masks: Vec<u32>,
    rooms: Vec<RoomLadder>,
    restarts: u32,
    halted: bool,
}

impl LiveScene {
    /// Enter `name` (a CDNAME field scene, or one of the three kingdom
    /// overworld maps) as a scene-picker visit. `Err` when the host refuses
    /// the scene.
    pub fn enter(index: Arc<ProtIndex>, name: &str) -> Result<Self, String> {
        let world_map = crate::scene::is_world_map_scene(name);
        let mut host = SceneHost::new(index.clone());
        Self::enter_host(&mut host, name, world_map)?;
        let scene_lut = host
            .scene
            .as_ref()
            .and_then(|s| s.field_floor_height_lut(&host.index).ok().flatten());
        let (tile_masks, rooms) = if world_map {
            (Vec::new(), Vec::new())
        } else {
            probe_rooms(index, name, &host)
        };
        Ok(Self {
            host,
            name: name.to_string(),
            world_map,
            scene_lut,
            tile_masks,
            rooms,
            restarts: 0,
            halted: false,
        })
    }

    fn enter_host(host: &mut SceneHost, name: &str, world_map: bool) -> Result<(), String> {
        // The scene picker's free-roam staging, the one rule both play hosts
        // apply to a picker entry.
        host.world.stage_picker_entry(name, false);
        host.world.npcs.animate = true;
        let entered = if world_map {
            host.enter_world_map_scene(name)
        } else {
            host.enter_field_scene(name, 0)
        };
        entered.map_err(|e| format!("enter {name}: {e:#}"))
    }

    /// The scene this preview shows.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Whether the preview is a kingdom overworld map.
    pub fn is_world_map(&self) -> bool {
        self.world_map
    }

    /// Whether the preview still ticks.
    pub fn is_live(&self) -> bool {
        !self.halted
    }

    /// How many times the headless world left the scene and was re-entered.
    pub fn restarts(&self) -> u32 {
        self.restarts
    }

    /// The mode the viewed scene runs in.
    fn home_mode(&self) -> SceneMode {
        if self.world_map {
            SceneMode::WorldMap
        } else {
            SceneMode::Field
        }
    }

    /// Advance one retail vsync (one [`SceneHost::tick`]), and every other
    /// room's oscillators with it. Returns `false` once the preview has
    /// stopped ticking.
    pub fn tick(&mut self) -> bool {
        if self.halted {
            return false;
        }
        let event = self.host.tick();
        let left = match &event {
            Err(_) => true,
            Ok(SceneTickEvent::SceneEntered { .. }) => true,
            Ok(_) => {
                self.host.world.mode != self.home_mode()
                    || self.host.scene.as_ref().map(|s| s.name.as_str()) != Some(self.name.as_str())
            }
        };
        if left {
            self.restarts += 1;
            let name = self.name.clone();
            if self.restarts > MAX_RESTARTS
                || Self::enter_host(&mut self.host, &name, self.world_map).is_err()
            {
                self.halted = true;
                return false;
            }
        }
        let delta = self
            .host
            .world
            .clock
            .display_frame_step
            .min(u16::from(u8::MAX)) as u8;
        for room in &mut self.rooms {
            crate::world::step_floor_ladder(&mut room.bobs, &mut room.lut, delta);
        }
        true
    }

    /// The ladder the scene's MAN header ships (MAN frame).
    pub fn scene_floor_lut(&self) -> Option<[i16; 16]> {
        self.scene_lut
    }

    /// The live floor-height ladder (scratchpad frame, `0x1F80035C`) - the
    /// room the live player stands in.
    pub fn live_floor_lut(&self) -> [i16; 16] {
        self.host.world.terrain.floor_height_lut
    }

    /// The rooms the probe found a ladder for, besides the one the live
    /// player stands in.
    pub fn rooms(&self) -> &[RoomLadder] {
        &self.rooms
    }

    /// The region-type mask of tile `(col, row)`; `None` off the grid or in
    /// a scene without rooms.
    pub fn tile_mask(&self, tile: (i32, i32)) -> Option<u32> {
        let (c, r) = tile;
        if !(0..GRID).contains(&c) || !(0..GRID).contains(&r) {
            return None;
        }
        self.tile_masks.get((r * GRID + c) as usize).copied()
    }

    /// The room ladder tile `(col, row)` draws on, when it is not the room
    /// the live player stands in.
    fn room_at(&self, tile: (i32, i32)) -> Option<&RoomLadder> {
        let mask = self.tile_mask(tile)?;
        if mask == self.host.world.flags.extra_flags {
            return None;
        }
        self.rooms.iter().find(|r| r.mask == mask)
    }

    /// Whether tile `(col, row)` draws on the live world's ladder - the room
    /// the live player stands in, or a tile the probe found no room for.
    /// What a play host shows is exactly these tiles' ladder.
    pub fn in_live_room(&self, tile: (i32, i32)) -> bool {
        self.room_at(tile).is_none()
    }

    /// The live ladder (scratchpad frame) tile `(col, row)` draws on: its
    /// room's, or the live world's for the player's own room and any tile
    /// the probe found no room for.
    pub fn floor_lut_at(&self, tile: Option<(i32, i32)>) -> &[i16; 16] {
        match tile.and_then(|t| self.room_at(t)) {
            Some(r) => &r.lut,
            None => &self.host.world.terrain.floor_height_lut,
        }
    }

    /// The wave between the shipped ladder and the live one; `None` while
    /// they agree (and always on the overworld).
    pub fn floor_wave(&self) -> Option<FloorWave> {
        if self.world_map {
            return None;
        }
        FloorWave::from_scene_and_world(self.scene_lut, &self.host.world.terrain.floor_height_lut)
    }

    /// Per-draw Y offsets (retail frame) of terrain / decoration `draws` -
    /// resolved against the shipped ladder - under the live ladder of the
    /// room each draw's cell is in. `None` while every room's ladder sits
    /// where the scene shipped it. Placed objects do not follow the live
    /// rungs; see [`Self::placed_wave_offsets`].
    pub fn floor_wave_offsets(&self, draws: &[EnvDraw]) -> Option<Vec<i32>> {
        if self.world_map {
            return None;
        }
        let live = self.floor_wave();
        if live.is_none() && self.rooms.is_empty() {
            return None;
        }
        let out: Vec<i32> = draws
            .iter()
            .map(|d| {
                let tile = (i32::from(d.cell.0), i32::from(d.cell.1));
                let wave = match self.room_at(tile) {
                    Some(r) => FloorWave::from_scene_and_world(self.scene_lut, &r.lut),
                    None => live,
                };
                wave.map_or(0, |w| w.offset(&d.floor))
            })
            .collect();
        out.iter().any(|&o| o != 0).then_some(out)
    }

    /// Per-draw Y offsets of placed `draws` (with their window keys,
    /// [`crate::field_env::placed_window_key`]): the world's
    /// [`crate::world::World::placed_floor_offsets`], except a window-owned
    /// object in another room, which stands on the ladder that room's window
    /// re-plan spawned it on.
    pub fn placed_wave_offsets(
        &self,
        draws: &[EnvDraw],
        keys: &[Option<PlacedWindowKey>],
    ) -> Vec<i32> {
        let mut out = self.host.world.placed_floor_offsets(
            self.scene_lut,
            draws.iter().map(|d| &d.floor),
            keys,
        );
        if self.world_map || self.rooms.is_empty() {
            return out;
        }
        for (i, d) in draws.iter().enumerate() {
            if !matches!(keys.get(i), Some(Some(_))) {
                continue;
            }
            let tile = (i32::from(d.cell.0), i32::from(d.cell.1));
            if let Some(r) = self.room_at(tile) {
                out[i] = FloorWave::from_scene_and_world(self.scene_lut, &r.placed_lut)
                    .map_or(0, |w| w.offset(&d.floor));
            }
        }
        out
    }

    /// The walk ground's drawn positions, each cell under its room's live
    /// ladder ([`crate::field_ground::live_render_positions_by_cell`]).
    /// `cells` is [`crate::field_ground::vertex_cells`] of `hf`.
    pub fn ground_positions(
        &self,
        hf: &legaia_asset::field_objects::WalkHeightfield,
        cells: &[Option<(i32, i32)>],
    ) -> Vec<[f32; 3]> {
        crate::field_ground::live_render_positions_by_cell(hf, cells, |c| self.floor_lut_at(c))
    }

    /// A key that changes whenever any ladder a ground vertex reads changes
    /// (the live one, or any room's): a host re-uploads the ground when it
    /// moves.
    pub fn ground_key(&self) -> Vec<[i16; 16]> {
        let mut k = vec![self.host.world.terrain.floor_height_lut];
        k.extend(self.rooms.iter().map(|r| r.lut));
        k
    }

    /// The live pose key of a placed draw whose bind names a clip (`None` for
    /// a static prop or one the prop bank does not drive).
    pub fn prop_pose_key(&self, draw: &EnvDraw) -> Option<PropPoseKey> {
        if draw.anim_id == 0 {
            return None;
        }
        self.host.world.props.bank.pose_key(draw.anchor)
    }
}

/// Whether a scene's MAN carries a floor-ladder install or oscillator at all
/// (the byte pair `4C 9E` or `4C 90..92`). A byte scan over-reports - a pair
/// inside an operand matches too - which only costs a probe that finds one
/// room ladder; it cannot miss a real op.
fn man_moves_ladder(man: &[u8]) -> bool {
    man.windows(2)
        .any(|w| w[0] == 0x4C && matches!(w[1], 0x90..=0x92 | 0x9E))
}

/// Find each room's ladder (see the module docs' **Rooms**): the per-tile
/// region masks, then one probe host seated standing in a tile of every mask
/// but the live player's, capturing what the scene's script installed.
fn probe_rooms(index: Arc<ProtIndex>, name: &str, live: &SceneHost) -> (Vec<u32>, Vec<RoomLadder>) {
    let none = || (Vec::new(), Vec::new());
    let Some(table) =
        crate::field_regions::RegionTable::parse(&live.world.terrain.map_region_block)
    else {
        return none();
    };
    let moves = live
        .scene
        .as_ref()
        .and_then(|s| s.field_man_payload(&live.index).ok().flatten())
        .is_some_and(|man| man_moves_ladder(&man));
    if !moves {
        return none();
    }
    let mut masks = Vec::with_capacity((GRID * GRID) as usize);
    for r in 0..GRID {
        for c in 0..GRID {
            masks
                .push(crate::field_regions::refresh_region_attributes(Some(&table), c, r, false).0);
        }
    }
    let entry_mask = live.world.flags.extra_flags;
    // One standing seat per room: the first open tile carrying its mask.
    let mut seats: Vec<(u32, (i16, i16))> = Vec::new();
    for r in 0..GRID {
        for c in 0..GRID {
            let mask = masks[(r * GRID + c) as usize];
            if mask == entry_mask || seats.iter().any(|(m, _)| *m == mask) {
                continue;
            }
            let (x, z) = ((c * 128 + 0x40) as i16, (r * 128 + 0x40) as i16);
            if !live.world.field_tile_is_wall(x, z) {
                seats.push((mask, (x, z)));
            }
        }
    }
    if seats.is_empty() {
        return (masks, Vec::new());
    }
    let mut probe = SceneHost::new(index);
    if LiveScene::enter_host(&mut probe, name, false).is_err() {
        return (masks, Vec::new());
    }
    let settled = |p: &mut SceneHost, ticks: u32| -> bool {
        for _ in 0..ticks {
            let left = match p.tick() {
                Err(_) | Ok(SceneTickEvent::SceneEntered { .. }) => true,
                Ok(_) => {
                    p.world.mode != SceneMode::Field
                        || p.scene.as_ref().map(|s| s.name.as_str()) != Some(name)
                }
            };
            if left {
                return false;
            }
        }
        true
    };
    if !settled(&mut probe, PROBE_SETTLE_TICKS) {
        return (masks, Vec::new());
    }
    // Each room is probed from the entry room: a room whose mask the script
    // does not branch on keeps the ladder the player walked in with, and the
    // view's answer for that is the entry room's, not whichever room the
    // probe happened to visit last.
    let entry_seat = probe
        .world
        .player_actor_slot
        .and_then(|s| probe.world.actors.get(s as usize))
        .map(|a| (a.move_state.world_x, a.move_state.world_z));
    let mut rooms = Vec::new();
    for (mask, (x, z)) in seats {
        if let Some((ex, ez)) = entry_seat
            && probe.debug_seat_standing(ex, ez)
            && !settled(&mut probe, PROBE_ROOM_TICKS)
            && (LiveScene::enter_host(&mut probe, name, false).is_err()
                || !settled(&mut probe, PROBE_SETTLE_TICKS))
        {
            break;
        }
        if !probe.debug_seat_standing(x, z) {
            continue;
        }
        if !settled(&mut probe, PROBE_ROOM_TICKS) {
            // The seat left the scene (a scripted exit); start the probe over
            // for the remaining rooms.
            if LiveScene::enter_host(&mut probe, name, false).is_err()
                || !settled(&mut probe, PROBE_SETTLE_TICKS)
            {
                break;
            }
            continue;
        }
        if probe.world.flags.extra_flags != mask {
            continue;
        }
        let t = &probe.world.terrain;
        rooms.push(RoomLadder {
            mask,
            lut: t.floor_height_lut,
            placed_lut: t.static_window.spawn_lut.unwrap_or(t.placed_spawn_lut),
            bobs: t.floor_tier_bobs.clone(),
        });
    }
    (masks, rooms)
}
