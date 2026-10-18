//! `FieldTerrainHost` for [`FieldHostImpl`].

use super::*;

impl<'a> FieldTerrainHost for FieldHostImpl<'a> {
    /// Op `0x4D`'s tile derivation (`0x801DFADC`): scratchpad flag bit
    /// `0x20000` of `_DAT_1F800394` selects the `(world << 16) >> 23` form
    /// over `(world - 0x40) >> 7`.
    fn world_to_tile_use_alt(&self) -> bool {
        self.world.flags.story_flags & 0x0002_0000 != 0
    }

    /// Field-VM op `0x4C` outer-nibble-7 - rectangular collision-grid wall
    /// paint. Writes the high-nibble wall bits of the per-scene collision
    /// grid (`*(_DAT_1F8003EC) + 0x4000`), the same grid
    /// [`World::step_field_locomotion`] reads. The VM dispatcher has
    /// already turned the op operands into half-open tile ranges; we just
    /// apply the per-byte mutation. See [`World::paint_field_collision`].
    fn paint_collision_walls(&mut self, sub: u8, x_range: (u8, u8), z_range: (u8, u8), mask: u8) {
        self.world
            .paint_field_collision(sub, x_range, z_range, mask);
    }

    /// Op `4C 83` (arm `0x801E20A8`): rewrite the kind-2 elevation records
    /// of a tile rectangle. See [`World::fill_elevation_rect`].
    fn rect_tile_fill(
        &mut self,
        col_start: u8,
        row_start: u8,
        col_end: u8,
        row_end: u8,
        value: u8,
    ) {
        self.world
            .fill_elevation_rect(col_start, row_start, col_end, row_end, value);
    }

    fn retire_floor_ladder_oscillators(&mut self) {
        // `4C 9F` is `FUN_8003CF40(_DAT_8007C34C, LAB_801DA930)`, a **retire
        // sweep** over the `0x801F27EC` handler - so it cancels every live
        // floor-height-ladder oscillator this scene spawned (sub-`0..2`),
        // and registers nothing. Retiring the engine's records is the whole
        // of that half; the VM advances past the op either way.
        self.world.terrain.floor_tier_bobs.clear();
        self.world.retire_floor_ladder_oscillators();
    }

    /// Op `0x4C` nibble-9 sub-`0xE` - install all sixteen rungs of the scene
    /// floor-height ladder at once.
    ///
    /// The paired install of the animator below, and the op that identifies
    /// the whole nibble-9 family: retail writes `-words[i]` into
    /// `0x1F800314 + 0x48 + i*2` (and `words[i]` into the MAN-header mirror
    /// at `_DAT_8007B898 + 2`), which is the same array
    /// [`legaia_engine_scene_host::scene_host::SceneHost`] seeds from the MAN header's sixteen
    /// **negated** shorts on scene entry. `jou`'s entry script installs the
    /// linear ramp `i * 0x20` here and then sets rungs `4..` oscillating -
    /// the undulating organic floor.
    ///
    /// Both stores are in the arm's one loop (`0x801E24F8..0x801E2538`:
    /// `sh v0, 2(MAN + i*2)` then `sh -v0, 0x48(0x1F800314 + i*2)`), so the
    /// MAN-header ladder the camera composer swaps in around its floor
    /// sample follows the install too. Only the oscillators write the live
    /// rungs alone. Updating the live rungs without the mirror left the
    /// follow camera framing `concnow`'s pre-install ladder while the player
    /// walked the installed one - the eye sank under the raised floor.
    fn install_floor_height_ladder(&mut self, words: [i16; 16]) {
        let terrain = &mut self.world.terrain;
        for (i, w) in words.into_iter().enumerate() {
            terrain.floor_height_lut[i] = w.wrapping_neg();
            terrain.floor_height_lut_static[i] = w.wrapping_neg();
        }
    }

    /// Op `0x4C` nibble-9 sub-`0..2` - one rung of the scene floor-height
    /// ladder starts oscillating (`FUN_801DDE34` allocating `0x801F27EC`,
    /// tick `FUN_801DA930`).
    ///
    /// `b1` is the rung (`0..16`, the low nibble of a collision byte);
    /// `words` are the half-period, the amplitude and the burst-arm word.
    /// Retail seeds both the position and the rest height from the rung's
    /// current value, which is `World::terrain.floor_height_lut` here.
    fn start_floor_ladder_oscillator(&mut self, sub: u8, b1: u8, words: [i16; 3]) {
        let seed = self
            .world
            .terrain
            .floor_height_lut
            .get(b1 as usize)
            .copied()
            .unwrap_or(0);
        self.world.terrain.floor_tier_bobs.push(
            legaia_engine_field::field_actor_timers::FloorTierBob::spawn(
                u16::from(b1),
                sub,
                words[0],
                words[1],
                words[2],
                seed,
            ),
        );
    }

    fn region_attributes_refresh_at_player(&mut self) {
        self.world
            .push_camera_zone_request(CameraZoneRequest::RefreshAttributes);
    }

    /// Op `4C C1` - re-derive every fog region's enable byte from its story
    /// flag: for each record of the `_DAT_80073ED8` table (count
    /// `DAT_80073EDC`, stride `0xB`), `FUN_8003CE64(rec[9] | rec[10] << 8)`
    /// and `rec[0] = flag set ? 0 : 1` (`0x801E2674..0x801E26EC`). The fog
    /// spawner ends its region search on the first box holding the tile and
    /// spawns nothing when that record's byte 0 is clear, so this is what
    /// keeps a region's fog off once its story beat has passed: retail's
    /// `retock` inn (flag `0x51C`), `rikuroa` (`0x007`) and `garmel`
    /// (`0x007`) all hold the reset result, with an empty pool where every
    /// covering region is off.
    ///
    /// PORT: FUN_801DE840 (the nibble-C sub-`1` arm)
    fn reset_fog_region_enables(&mut self, _flags: &[u8]) {
        let enables: Vec<bool> = self
            .world
            .fog
            .regions
            .iter()
            .map(|r| !self.world.system_flag_test(r.flag_index))
            .collect();
        for (r, on) in self.world.fog.regions.iter_mut().zip(enables) {
            r.enabled = on;
        }
    }
}
