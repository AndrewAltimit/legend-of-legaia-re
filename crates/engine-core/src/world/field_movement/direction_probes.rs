//! Directional blocking probes: the player's pad-direction remap and slide
//! resolve, and the actor, NPC, prop and interaction probes.

use super::*;

/// Result of one direction's prop-collision probe
/// ([`World::field_prop_dir_probe`]): whether a solid prop box blocks the
/// step, and - for a static-class (auto-touch) hit - which prop-bank entry
/// the contact posts.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct PropDirProbe {
    /// A solid prop box overlaps a probe point: the 2-unit step is refused
    /// (retail result bits `1`/`4` both gate the commit).
    pub blocked: bool,
    /// Static-class hit with a bank entry: the anchor whose record the
    /// contact auto-posts (`None` for interact-class or unbound props).
    pub touch: Option<(u8, u8)>,
}

impl World {
    /// Retail's static-wall direction test: from the CURRENT position
    /// `(x, z)`, probe the three leading-edge points of `FIELD_WALL_PROBES`
    /// row `dir` (`0` = Z-, `1` = X-, `2` = Z+, `3` = X+) through
    /// [`Self::field_tile_is_wall`]; the direction is blocked when any probe
    /// lands on a wall sub-cell.
    ///
    /// PORT: FUN_801cfe4c
    /// REF: FUN_801cfc40
    ///
    /// This is the static-wall arm of `FUN_801cfe4c` (result bit `2`): the
    /// probes are taken at the player's pre-step position, so a step commits
    /// while the edge is still clear and the next step from the deeper
    /// position blocks - the player rests 47-48 units off the wall plane,
    /// step-exact (pinned by the `rimelm_wall_press_left`/`_down` captures).
    /// The actor-collision arm (result bits `1`/`4`) is
    /// [`Self::field_actor_dir_blocked`].
    pub fn field_dir_blocked(&self, x: i16, z: i16, dir: usize) -> bool {
        FIELD_WALL_PROBES[dir & 3]
            .iter()
            .any(|&(dx, dz)| self.field_tile_is_wall(x.saturating_add(dx), z.saturating_sub(dz)))
    }

    /// Camera-relative pad-direction remap: rotate a held direction mask by
    /// the field camera's rotation index `rot` (0..7 eighth-turns) around the
    /// eight-direction ring [`FIELD_DIR_RING`], so "screen up" always walks
    /// away from the camera regardless of azimuth. Returns the input mask
    /// unchanged when `rot == 0` (identity camera) or no direction is held.
    ///
    /// PORT: FUN_800467e8
    ///
    /// Faithful to the retail path (`func_0x800467e8(&_DAT_8007b850)`, called
    /// at the top of the free-movement controller `FUN_801d01b0` before the
    /// wall-slide resolve): retail finds the held direction's index in the
    /// ring, adds the camera step (`gp+0x2d8`), wraps `& 7`, and writes the
    /// rotated mask back over the direction nibble (`held & 0xffff0fff | new`).
    /// This is a **45°** remap - it rotates diagonals as first-class ring
    /// entries, unlike [`World::decode_field_direction`]'s 90°-quantised
    /// screen-vector rotation, which the two agree on for even `rot` (the
    /// axis-aligned cameras every retail field scene actually uses).
    ///
    /// The live pad path calls this every frame through
    /// [`Self::decode_field_direction`], with the rotation index
    /// [`Self::field_pad_ring_rotation`] derives from the port's own camera
    /// azimuth. `precise_movement` keeps its continuous decode, which no ring
    /// index can express.
    ///
    /// **Where retail's `gp+0x2D8` comes from.** Not from the camera. Its
    /// disc-wide writers are a field-VM arm and the tile-board walker: the
    /// `0x4C` outer-nibble-`2` arm at `0x801E0EB8` stores `sub_op & 7` there
    /// (and, when `0x8007B6B0` reads `-1000`, turns the player with it -
    /// `actor[+0x26] += (new - old) * 0x200`), and the tile-board walker
    /// derives it from the terrain type. So the index is **authored**, per
    /// scene, to match the camera the same script installed; there is no
    /// azimuth-to-octant arithmetic anywhere in retail. A port whose camera
    /// free-orbits has no authored index to read, so it derives one from the
    /// azimuth instead - which is a port decision, not a retail one, and is
    /// why `field_pad_ring_rotation` is separate from this routine.
    pub fn remap_pad_direction(held: u16, rot: u32) -> u16 {
        if rot == 0 {
            return held;
        }
        let dir = held & 0xF000;
        if dir == 0 {
            return held;
        }
        // Locate the held direction in the ring (retail's linear scan; a valid
        // direction mask is always one of the eight entries).
        let idx = FIELD_DIR_RING.iter().position(|&m| m == dir).unwrap_or(8);
        let rotated = FIELD_DIR_RING[(idx as u32).wrapping_add(rot) as usize & 7];
        // Retail rewrites the 0xf000 direction nibble in place (32-bit
        // `held & 0xffff0fff`); on the 16-bit mask that clears the top nibble.
        (held & 0x0FFF) | rotated
    }

    /// The eighth-turn index [`Self::decode_field_direction`] rotates the held
    /// d-pad mask by: the camera azimuth
    /// ([`crate::world::FieldLocomotion::camera_azimuth`], 12-bit, `0` = the
    /// follow default) rounded to the nearest of eight compass steps.
    ///
    /// This stands in for retail's `gp+0x2D8`, which is authored per scene by
    /// a field-VM arm rather than computed (see
    /// [`Self::remap_pad_direction`]). The port's camera free-orbits, so the
    /// authored index would be stale the moment the player drags the view;
    /// deriving it from the live azimuth keeps "screen up walks away from the
    /// camera" true at every orbit, which is the law the hosts' compass
    /// oracles measure.
    ///
    /// It is a strict refinement of the 90-degree quantisation it replaced:
    /// `rot` is even exactly where the old `quadrant` was defined, and
    /// `rot == 2 * quadrant` there, so every axis-aligned camera - which is
    /// every camera a retail field scene installs - decodes bit-identically.
    /// The odd steps are the ones that used to snap up to 45 degrees away.
    pub(crate) fn field_pad_ring_rotation(&self) -> u32 {
        ((self.locomotion.camera_azimuth as u32).wrapping_add(0x100) >> 9) & 7
    }

    /// Retail's wall-slide direction resolver: given the post-remap held mask
    /// and the player's current position, return the mask the per-axis step
    /// loop actually walks on - the held direction, plus a perpendicular
    /// **slide** bit when the held direction is blocked but there is open
    /// space to one side, so the player skids along a wall instead of
    /// sticking to it.
    ///
    /// PORT: FUN_80046494
    /// REF: FUN_801d56c4
    ///
    /// Two short-circuits return the raw mask untouched, exactly as retail:
    /// the no-clip pad bit (`held & 0x2`), and any of the four pure diagonals
    /// (`0x9000`/`0xc000`/`0x3000`/`0x6000`) - a diagonal already offers two
    /// axes for the collision step to resolve independently. Otherwise, for
    /// each held cardinal ([`FIELD_SLIDE_DIRS`]):
    ///
    /// 1. **Three-point block test** at the ±62-unit candidate point: the
    ///    point offset `±FIELD_SLIDE_LATERAL` perpendicular to travel, plus
    ///    dead centre, sampled through [`World::field_tile_is_wall`] (retail's
    ///    walkability probe `func_0x801d56c4`). Clear on all three -> just OR
    ///    the direction bit and move on.
    /// 2. **Slide search**: sweep the perpendicular axis over
    ///    [`FIELD_SLIDE_SWEEP`], summing the offsets that come back walkable.
    /// 3. **Sign picks the slide**: a negative sum ORs the row's negative
    ///    slide bit, a positive sum the positive one ([`FIELD_SLIDE_BITS`]); a
    ///    sum of exactly zero (symmetric dead end) adds nothing. The original
    ///    direction bit is ORed in regardless.
    ///
    /// Live on the pad path: [`Self::step_field_locomotion`] resolves the
    /// camera-remapped mask through this function and feeds the result to
    /// [`Self::advance_with_collision`], the same order retail uses
    /// (`jal 0x80046494` at `0x801D03EC` inside `FUN_801D01B0`, result kept
    /// in `s0` at `0x801D0404`). Only the step reads the resolved mask - the
    /// heading and the diagonal speed cut stay on the held one. The precise
    /// free-angle path keeps its own vector step and does not resolve.
    ///
    /// The "wiring it moves every pinned wall rest" worry was measured and
    /// is false for the oracle that carried it: on the live grid of both
    /// wall-press captures the resolver hands back exactly the held cardinal
    /// at the captured rest position, so the pinned legs of
    /// `parity/tests/field_collision_discriminator.rs` are
    /// slide-neutral. The same grid carries thousands of sliding positions,
    /// and `wall_slide_wire_skids_where_the_bare_stepper_sticks` drives one
    /// of them through this path against the bare stepper.
    pub fn resolve_field_slide(&self, held: u16, x: i16, z: i16) -> u16 {
        // No-clip pad bit: pass the raw mask through untouched.
        if held & 0x2 != 0 {
            return held;
        }
        // Pure diagonals are never slide-resolved.
        if matches!(held & 0xF000, 0x9000 | 0xC000 | 0x3000 | 0x6000) {
            return held;
        }
        let px = x as i32;
        let pz = z as i32;
        // The walkability probe: retail `func_0x801d56c4` returns "blocked"
        // for a wall sub-cell, which is exactly `field_tile_is_wall` (same
        // `+2` Z / `ceil-1` X biased sub-cell derivation). Coords stay well
        // inside i16 for field-sized worlds; clamp defensively.
        let wall = |wx: i32, wz: i32| -> bool {
            self.field_tile_is_wall(
                wx.clamp(i16::MIN as i32, i16::MAX as i32) as i16,
                wz.clamp(i16::MIN as i32, i16::MAX as i32) as i16,
            )
        };
        let mut out = 0u16;
        for (i, &(mask, dx, dy)) in FIELD_SLIDE_DIRS.iter().enumerate() {
            if held & mask == 0 {
                continue;
            }
            // Lateral offset perpendicular to the travel axis.
            let (latx, latz) = if dx != 0 {
                (0, FIELD_SLIDE_LATERAL)
            } else {
                (FIELD_SLIDE_LATERAL, 0)
            };
            let (cx, cz) = (px + dx, pz + dy);
            let blocked = wall(cx + latx, cz + latz) || wall(cx - latx, cz - latz) || wall(cx, cz);
            if !blocked {
                out |= mask;
                continue;
            }
            // Sweep the perpendicular axis; sum the walkable offsets.
            let mut total = 0i32;
            for &off in FIELD_SLIDE_SWEEP.iter() {
                if dx == 0 {
                    // Z-travel -> sweep in X.
                    if !wall(cx + off, cz) {
                        total += off;
                    }
                }
                if dy == 0 {
                    // X-travel -> sweep in Z.
                    if !wall(cx, cz + off) {
                        total += off;
                    }
                }
            }
            let (neg_bit, pos_bit) = FIELD_SLIDE_BITS[i];
            if total < 0 {
                out |= neg_bit;
            }
            if total > 0 {
                out |= pos_bit;
            }
            out |= mask;
        }
        out
    }

    /// Retail's actor-collision direction test: from the CURRENT position
    /// `(x, z)`, take the three probe points of `FIELD_ACTOR_PROBES` row
    /// `dir` (same `(x + dx, z - dz)` convention as the wall probes) and
    /// box-test each against every field NPC's position
    /// ([`crate::world::FieldNpcState::positions`]); the direction is blocked when any
    /// probe lands within `FIELD_NPC_BOX_HALF` (40 units) of an NPC on
    /// both axes (strict).
    ///
    /// PORT: FUN_801cfc40
    /// REF: FUN_801cfe4c
    ///
    /// Covers both entity classes of `FUN_801cfc40`:
    ///
    /// - the **moving-actor arm** (result bit `1`) - the class village NPCs
    ///   belong to, capture-pinned by `rimelm_npc_press_tetsu` (the sparring
    ///   partner's `flags+0x10 = 0x08020884` carries the `0x20000` class
    ///   bit, and the mutual `+0x98` collision link is live in-frame). The
    ///   positions are LIVE: `Self::tick_field_npc_motions` walks
    ///   scripted NPCs through the motion VM and writes back into
    ///   [`crate::world::FieldNpcState::positions`], so a moving NPC's
    ///   ±`FIELD_NPC_BOX_HALF` (40) box follows it, exactly as retail
    ///   probes the live `+0x14`/`+0x18`.
    /// - the **static-entity arm** (result bit `4`) - placed `.MAP` props,
    ///   box ±`FIELD_PROP_BOX_HALF` (80) around the record-derived
    ///   footprint centre ([`crate::world::FieldPropState::colliders`]).
    ///
    /// The locomotion-path touch dispatches are modelled alongside: the
    /// button-press interact (facing probe + event + face-the-NPC,
    /// `Self::tick_field_interaction_probe`) and the no-button prop
    /// walk-touch event post (`Self::check_field_walk_touch`, the
    /// `FUN_801d5b5c` analogue for the decoded script classes). Not
    /// modelled: the mutual `+0x98` partner-link bookkeeping itself and the
    /// `_DAT_8007b6b8 == 0x20` full-table delegation to `FUN_801cf9f4`.
    /// Faithful quirk kept: the probe has no near-side clamp, so a position
    /// already deep inside a box (past the probe reach) reads clear -
    /// exactly as retail's forward-only probe behaves.
    pub fn field_actor_dir_blocked(&self, x: i16, z: i16, dir: usize) -> bool {
        self.field_npc_dir_blocked(x, z, dir) || self.field_prop_dir_probe(x, z, dir).blocked
    }

    /// The actor / prop sweep at **one exact world point** - retail
    /// `FUN_801cfc40` invoked with a single offset pair, which is how the
    /// ledge classifier calls it.
    ///
    /// PORT: FUN_801cfc40
    /// REF: FUN_801d1878
    ///
    /// `FUN_801cfc40` takes no footprint. It stages **one** probe point into
    /// scratchpad `0x1F800020` - `+0 = actor.x + a2`, `+2 = actor.y`,
    /// `+4 = actor.z - a3` (`0x801CFCA0..0x801CFCCC`; the `a3` subtraction is
    /// why callers pass the Z offset negated) - and box-tests every candidate
    /// against that single point. The half-extent is `0x40` widened by the
    /// caller's two stack words (`0x801CFD6C..0x801CFDD0`), and the ledge
    /// classifier passes `0` for both at each of its two call sites
    /// (`sw zero, 0x10(sp)` / `sw zero, 0x14(sp)` at
    /// `0x801D1A88`+`0x801D1A90` and `0x801D1AAC`+`0x801D1AB4`, the second of
    /// each pair in the `jal`'s delay slot) - the same zero-extent call the
    /// walk controller makes, so the boxes are the established
    /// [`FIELD_NPC_BOX_HALF`] / [`FIELD_PROP_BOX_HALF`].
    ///
    /// [`Self::field_actor_dir_blocked`] is the *walk controller's* shape of
    /// the same routine - three points off a compass table, taken at the
    /// actor's own position - and the two are not interchangeable.
    pub(crate) fn field_actor_point_blocked(&self, px: i32, pz: i32) -> bool {
        if self.npcs.solid
            && self.npcs.positions.values().any(|&(ax, az)| {
                (px - ax as i32).abs() < FIELD_NPC_BOX_HALF
                    && (pz - az as i32).abs() < FIELD_NPC_BOX_HALF
            })
        {
            return true;
        }
        self.props.colliders.iter().any(|c| {
            if !c.solid || self.collider_arrival_exempt(c) || self.prop_collider_exempt(c) {
                return false;
            }
            let ((cx, cz), half) = if c.moving_box {
                (c.live, FIELD_NPC_BOX_HALF)
            } else {
                (c.center, FIELD_PROP_BOX_HALF)
            };
            (px - cx).abs() < half && (pz - cz).abs() < half
        })
    }

    /// The **moving-NPC arm** of the actor-collision direction test (retail
    /// result bit `1` for the village-NPC class): the three probe points
    /// against every live NPC position at ±[`FIELD_NPC_BOX_HALF`].
    ///
    /// PORT: FUN_801cfc40
    pub(crate) fn field_npc_dir_blocked(&self, x: i16, z: i16, dir: usize) -> bool {
        if self.npcs.positions.is_empty() {
            return false;
        }
        FIELD_ACTOR_PROBES[dir & 3].iter().any(|&(dx, dz)| {
            let px = x.saturating_add(dx) as i32;
            let pz = z.saturating_sub(dz) as i32;
            self.npcs.positions.values().any(|&(ax, az)| {
                (px - ax as i32).abs() < FIELD_NPC_BOX_HALF
                    && (pz - az as i32).abs() < FIELD_NPC_BOX_HALF
            })
        })
    }

    /// The **placed-prop arms** of the actor-collision direction test: the
    /// three `FIELD_ACTOR_PROBES` points of `dir` box-tested against every
    /// solid prop collider. A static-class hit (`+0x10` clear of the
    /// `0x40020000` interact bits - retail contact result bit `4`) also
    /// surfaces the touched prop's bank anchor, which the locomotion
    /// auto-posts (`FUN_801D01B0` `0x801d0800` -> `FUN_801D5B5C`); an
    /// interact-class hit (bit `1`) blocks silently - only the button-gated
    /// facing probe fires it.
    ///
    /// Box classes per collider (see [`FieldPropCollider`]): static = ±80
    /// around the footprint centre; moving-box = ±40 around the live
    /// position. A non-solid collider (script ran `31 00`) is skipped
    /// entirely, exactly as `FUN_801CF754`'s `flags & 3` filter drops the
    /// opened door from the candidate list.
    ///
    /// PORT: FUN_801cfc40
    /// REF: FUN_801CF754, FUN_801D5B5C
    pub(crate) fn field_prop_dir_probe(&self, x: i16, z: i16, dir: usize) -> PropDirProbe {
        let mut out = PropDirProbe::default();
        if self.props.colliders.is_empty() {
            return out;
        }
        for &(dx, dz) in &FIELD_ACTOR_PROBES[dir & 3] {
            let px = x.saturating_add(dx) as i32;
            let pz = z.saturating_sub(dz) as i32;
            for c in &self.props.colliders {
                if !c.solid || self.collider_arrival_exempt(c) || self.prop_collider_exempt(c) {
                    continue;
                }
                let ((cx, cz), half) = if c.moving_box {
                    (c.live, FIELD_NPC_BOX_HALF)
                } else {
                    (c.center, FIELD_PROP_BOX_HALF)
                };
                if (px - cx).abs() < half && (pz - cz).abs() < half {
                    out.blocked = true;
                    if !c.interact && out.touch.is_none() {
                        out.touch = c.anchor;
                    }
                }
            }
        }
        out
    }

    /// Retail's interact probe: from the player's position, take the single
    /// [`FIELD_FACING_PROBES`] compass point 64 units ahead along the
    /// current facing and return the NPC whose ±[`FIELD_INTERACT_BOX_HALF`]
    /// (72-unit) box contains it, if any.
    ///
    /// PORT: FUN_801cf9f4
    /// REF: FUN_801d01b0
    ///
    /// The engine's field heading ([`decode_field_direction`]
    /// (Self::decode_field_direction)) stores `0` = Z+ while the retail
    /// facing byte stores `0` = Z- (a Z+ walk writes `0x800` to `+0x26`), so
    /// the sector index adds the half-turn before quantising. On overlapping
    /// NPC boxes retail keeps the *last* actor-list hit (the `+0x98` link is
    /// overwritten per match); the engine's NPC set is a hash map with no
    /// list order, so it picks the hit nearest the probe point instead
    /// (tie-break: lowest slot) - identical whenever NPCs stand more than
    /// 144 units apart, which every authored placement does.
    pub fn field_interact_probe_slot(&self) -> Option<u8> {
        let slot = self.player_actor_slot? as usize;
        if slot >= self.actors.len() || !self.actors[slot].active {
            return None;
        }
        let ms = &self.actors[slot].move_state;
        let (x, z) = (ms.world_x, ms.world_z);
        let sector = (((ms.render_26 as i32 + 0x800) & 0xfff) >> 9) as usize;
        let (dx, dz) = FIELD_FACING_PROBES[sector];
        let px = x.saturating_add(dx) as i32;
        let pz = z.saturating_sub(dz) as i32;
        let mut best: Option<(i32, u8)> = None;
        let consider = |slot: u8, ax: i16, az: i16, best: &mut Option<(i32, u8)>| {
            let (ex, ez) = ((px - ax as i32).abs(), (pz - az as i32).abs());
            if ex < FIELD_INTERACT_BOX_HALF && ez < FIELD_INTERACT_BOX_HALF {
                let d = ex * ex + ez * ez;
                if best.is_none_or(|(bd, bs)| d < bd || (d == bd && slot < bs)) {
                    *best = Some((d, slot));
                }
            }
        };
        for (&npc_slot, &(ax, az)) in &self.npcs.positions {
            consider(npc_slot, ax, az, &mut best);
        }
        // Retail's probe walks the **actor list** and box-tests every placed
        // actor, with no "is this one a talk NPC" filter - the touched actor's
        // own record is what the dialog SM then runs. The engine's
        // `field_npc_positions` is seeded only for text-bearing NPC placements,
        // so a **minigame door** (a casino cabinet, the dance-hall desk) sat
        // outside the probe entirely and pressing the action button at one did
        // nothing at all. Its placement anchor is already carried by
        // `field_walk_touch`; admit exactly the slots that also carry an
        // interaction record and are not already NPC anchors, which is the door
        // set and nothing else.
        // REF: FUN_801cf9f4 (the actor-list walk), FUN_80039B7C
        for (&slot, &((ax, az), _)) in &self.props.walk_touch {
            if self.npcs.positions.contains_key(&slot)
                || !self.npcs.dialog_prologue.contains_key(&slot)
            {
                continue;
            }
            consider(slot, ax, az, &mut best);
        }
        // A talk proxy is box-tested at its own spawn position and answers
        // with the actor whose conversation its touch runs: `concnow`
        // P1[26], one tile in front of the gate, for guard P1[13], who
        // stands in the wall line beyond the probe's reach.
        // REF: FUN_801cf9f4, FUN_801D5B5C
        for &(target, (ax, az)) in self.npcs.talk_proxies.values() {
            if self.npcs.positions.contains_key(&target) {
                consider(target, ax, az, &mut best);
            }
        }
        best.map(|(_, s)| s)
    }
}
