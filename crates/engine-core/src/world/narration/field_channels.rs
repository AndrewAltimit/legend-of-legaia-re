//! Object render queries and the field-channel prologue runners.

use super::*;

impl World {
    /// Step every live per-actor field-VM channel one frame slice.
    ///
    /// Mirrors the retail per-actor script ticker (`FUN_80039B7C`, the
    /// `+0x9C == 0` branch): each context runs ops until it yields, parks on
    /// a conditional hold, **or executes a `0x21` NOP** - the NOP is the
    /// per-frame pacing point in retail (`if (bVar1 == 0x21) break`), which is
    /// why placement idle loops are written `21 21 26 FE FF`. A parked channel
    /// (flag-test `Halt`) retries the same PC next frame; a cross-context poke
    /// (an extended op naming another channel's script id) resolves the target
    /// context and runs against it, parking the caller if the target is
    /// halted - the retail synchronisation primitive between the cutscene
    /// timeline and its vignette actors.
    ///
    /// Channels are seeded two ways. A cutscene timeline spawns the full set
    /// ([`Self::install_cutscene_timeline_record`]) so its cross-context pokes
    /// land on real per-actor contexts (the opening prologue's vignettes). On
    /// **ordinary free-roam scene entry** the scene loader seeds the same set
    /// ([`Self::seed_field_channels`], the non-cutscene half of retail's
    /// `FUN_8003AEB0` spawn loop) so each placement's own init opcodes run -
    /// scripted initial facings, idle/`WAIT`-loop cadence, local-flag setup.
    ///
    /// After stepping, each channel whose context position changed writes
    /// through to [`crate::world::FieldNpcState::positions`] so the field render / probes
    /// follow the scripted move.
    /// The render scale a placement channel's actor draws at - retail's
    /// `actor[+0x72]` fixed-point scalar (`0x1000` = 1.0), seeded by the
    /// allocator (`actor_free`) and rewritten by the spawn prologue's
    /// `4C 40` writes. The per-actor render dispatcher (`FUN_8001ADA4`)
    /// composes it into the GTE model matrix, so **scale 0 = invisible**:
    /// the invisible interaction-trigger records draw their dev-gizmo
    /// marker mesh at scale 0 in retail. Both hosts' NPC draw loops read
    /// this and skip zero-scale actors. `None` when no channel owns the
    /// placement (draw at unit scale).
    // REF: FUN_8001ADA4 (scale-vector compose, disasm 8001b240..8001b28c)
    // REF: FUN_80020de0 (actor_free: +0x72 = 0x1000 at birth)
    pub fn field_npc_render_scale(&self, placement_index: usize) -> Option<u16> {
        self.field_vm
            .channels
            .iter()
            .find(|c| !c.object_bind && c.placement_index == placement_index)
            .map(|c| c.ctx.field_72)
    }

    /// Flat partition-0 record indices whose **object-bind channel** is
    /// story-hidden after its spawn prologue ran: parked at the off-map hide
    /// box (`0x23 MoveTo 0x7F,0x7F` behind a `SysFlag.Test`, e.g. town01's
    /// gate rocks `P0[18..21]` until flag `0x147` sets) or zero render scale
    /// (`0x4C 0x40 0x00`). The placed-object draw resolvers consult this via
    /// [`legaia_engine_scene::field_env::retain_visible_placed_draws`] - the `.MAP` table
    /// alone says where a placed object *can* stand, the bind record's
    /// prologue says whether it currently *does*.
    // REF: FUN_8003A55C (bind-time prologue pre-run seats/parks the actor)
    ///
    /// On a pack map it also holds the props an open mode seat's table hides
    /// ([`Self::pack_mode_hidden_prop_records`]): the draws both hosts skip
    /// per frame through this one set.
    pub fn hidden_object_records(&self) -> std::collections::HashSet<usize> {
        let hide = legaia_engine_field::man_field_scripts::FIELD_OFFMAP_HIDE_XZ as u16;
        self.field_vm
            .channels
            .iter()
            .filter(|c| c.object_bind)
            .filter(|c| {
                (c.ctx.world_x == hide && c.ctx.world_z == hide)
                    || c.ctx.field_72 == 0
                    || c.ctx.flags & legaia_engine_field::field_channels::CONTEXT_RETIRE_BIT != 0
            })
            .map(|c| c.placement_index)
            .chain(self.field_vm.retired_objects.iter().copied())
            .flat_map(|k| self.object_draw_keys(k))
            .chain(self.pack_mode_hidden_prop_records())
            .collect()
    }

    /// The keys a host finds object `key`'s actor state under: the channel
    /// key itself and, when that is the flat record (the first object the
    /// record binds), the object's per-object key too
    /// ([`legaia_engine_field::field_channels::object_instance_key`]). A
    /// host looks a bound placed draw up by the per-object key of its own
    /// cell ([`legaia_engine_scene::field_env::placed_object_keys`]), so
    /// every object reads its own actor; the record key is the id
    /// `FUN_8003C83C` answers with that first object.
    pub(crate) fn object_draw_keys(&self, key: usize) -> impl Iterator<Item = usize> + use<> {
        let instance = self
            .field_vm
            .object_binds_by_key
            .get(&key)
            .map(|b| legaia_engine_field::field_channels::object_instance_key(b.tile))
            .filter(|&i| i != key);
        std::iter::once(key).chain(instance)
    }

    /// The flat record (script id) object-bind channel `key` runs:
    /// the key itself for the first object a record binds.
    pub(crate) fn object_key_record(&self, key: u16) -> u16 {
        self.field_vm
            .object_binds_by_key
            .get(&usize::from(key))
            .map_or(key, |b| b.record as u16)
    }

    /// The prop-bank anchor (the bind's key tile) of object-bind channel
    /// `key`. With no binds installed (a prop run ahead of the channels) the
    /// key is a record, and the first of its props answers.
    pub(crate) fn object_key_anchor(&self, key: usize) -> Option<(u8, u8)> {
        if let Some(b) = self.field_vm.object_binds_by_key.get(&key) {
            return Some(b.key_tile);
        }
        self.props
            .bank
            .props
            .iter()
            .find(|(_, p)| p.record == key)
            .map(|(&a, _)| a)
    }

    /// The flat partition-0 records whose bound actor carries a bit of
    /// `+0x10 & 0x80800000`, which `FUN_8001ADA4` draws with the ordering-table
    /// base `0x28` lower (`0x8001B204..0x8001B21C`), ten buckets nearer. A
    /// spawn prologue raises it with `31 1F`.
    pub fn near_bucket_object_records(&self) -> std::collections::HashSet<usize> {
        self.field_vm
            .channels
            .iter()
            .filter(|c| c.object_bind && c.ctx.flags & 0x8080_0000 != 0)
            .flat_map(|c| self.object_draw_keys(c.placement_index))
            .collect()
    }

    /// The partition-1 placements whose actor carries a bit of
    /// `+0x10 & 0x80800000`: [`Self::near_bucket_object_records`] for an NPC,
    /// or a seated prop the entry poses through one (`deroa`'s slabs, `31 1F`).
    pub fn near_bucket_npcs(&self) -> std::collections::HashSet<usize> {
        self.field_vm
            .channels
            .iter()
            .filter(|c| !c.object_bind && c.ctx.flags & 0x8080_0000 != 0)
            .map(|c| c.placement_index)
            .collect()
    }

    /// Whether the player draws ten buckets nearer: the floor sampler
    /// `FUN_80019278` raises `+0x10 & 0x800000` per step on an object-grid
    /// cell without bit `0x1000` (`0x80019314..0x80019328`).
    pub fn player_near_buckets(&self, x: i32, z: i32) -> bool {
        if !(0..0x4000).contains(&x) || !(0..0x4000).contains(&z) {
            return false;
        }
        let i = ((z >> 7) * 128 + (x >> 7)) as usize;
        let cell = self.terrain.object_cells.get(i).copied().unwrap_or(0);
        cell & 0x1000 == 0
    }

    /// Flat partition-0 record index -> render scale for every **object-bind
    /// channel** whose spawn prologue left `actor[+0x72]` at a non-zero,
    /// non-unit value. Retail's per-actor render dispatcher composes that word
    /// into the model matrix for every placed object it draws
    /// (`FUN_8001ADA4` case 5: `lhu v1,0x72(s0)` / `li v0,0x1000` /
    /// `beq v1,v0` at `0x8001B240`, else `ScaleMatrix` over `(s, s, s)` at
    /// `0x8001B288`), so a prop the prologue shrinks draws shrunk. town01's
    /// horizon backdrop (partition-0 record 26, a `17920 x 9600` plane at
    /// `(3264, 6744)`) is the case that needs it: its actor carries `0x400`
    /// in a retail capture, and at unit scale the plane stands between the
    /// plaza camera and the player. Zero-scale objects are
    /// [`Self::hidden_object_records`]' business and are not listed here.
    // REF: FUN_8001ADA4 (scale-vector compose, disasm 8001b240..8001b28c)
    pub fn object_render_scales(&self) -> std::collections::HashMap<usize, u16> {
        self.field_vm
            .channels
            .iter()
            .filter(|c| c.object_bind && c.ctx.field_72 != 0 && c.ctx.field_72 != 0x1000)
            .flat_map(|c| {
                self.object_draw_keys(c.placement_index)
                    .map(move |k| (k, c.ctx.field_72))
            })
            .collect()
    }

    /// Flat partition-0 record indices whose **object-bind channel** carries
    /// the actor tick's floor-follow law: `+0x10 & 0x20200` up and
    /// `0x20000000` down. The field actor tick `FUN_8003BC08` rewrites such an
    /// actor's Y `+0x16` with the floor sample under it (`FUN_80019278`, the
    /// `jal` at `0x8003BC98`) on every tick the visibility cull leaves it in
    /// view, so the record's own `y_off` - the lift the `.MAP` sweep
    /// `FUN_8003A55C` seats it with (`lut[nibble] + y_off`, `0x8003A640`) -
    /// lasts only until the actor is first seen. A prologue raises the class
    /// bit with `31 11` (`CFlag.Set` bit 17): `rikuroa`'s `P0[0]`, bound to
    /// most of the summit's props, does, and its sky panorama (pack 37,
    /// `y_off` `2080`) draws at the `-480` floor tier in every retail capture,
    /// which puts the cliff ring across the top of the frame.
    ///
    /// The `0x2000` glide variant converges on the same floor and is listed
    /// too; the `0x20000000` law (`-(+0x8E)`) is
    /// [`Self::object_draw_displacements`]' business.
    // REF: FUN_8003BC08 (height arm, 0x8003BC44..0x8003BCF4), FUN_80019278
    pub fn object_floor_follow_records(&self) -> std::collections::HashSet<usize> {
        self.field_vm
            .channels
            .iter()
            .filter(|c| {
                c.object_bind && c.ctx.flags & 0x2000_0000 == 0 && c.ctx.flags & 0x0002_0200 != 0
            })
            .flat_map(|c| self.object_draw_keys(c.placement_index))
            .collect()
    }

    /// The Y a host **bakes** a placed object bound to `record` at, standing
    /// at world `(x, z)`: the floor sample under it when the record follows
    /// the floor ([`Self::object_floor_follow_records`]), else `None` (the
    /// `.MAP` sweep's `lut[nibble] + y_off` stands).
    ///
    /// The sample is taken through the ladder the bound objects were spawned
    /// against ([`crate::world::FieldTerrain::placed_spawn_lut`]), not the
    /// live rungs, so the bake does not depend on when the host builds its
    /// placed layer. The live floor - a script's `4C 9E` ladder or a rung
    /// oscillator - reaches the draw through
    /// [`Self::object_draw_displacements`], which carries the difference
    /// every frame.
    // REF: FUN_8003BC08, FUN_80019278
    pub fn object_floor_follow_y(
        &self,
        follow: &std::collections::HashSet<usize>,
        record: usize,
        x: i32,
        z: i32,
    ) -> Option<i32> {
        follow
            .contains(&record)
            .then(|| self.sample_field_floor_height_spawned(x, z))
    }

    /// Flat partition-0 record index -> how far a script has moved that
    /// **object-bind channel's** actor from where its bind seated it, as
    /// `[dx, dy, dz]` in retail world units (Y-down). Retail draws a placed
    /// object at its actor's live `+0x14 / +0x16 / +0x18`, not at the `.MAP`
    /// record, so a script that seats one with `A3 <id> <tx> <tz>` or lifts it
    /// with op `4C 42` (`+0x8E`, mirrored into world Y while `+0x10 &
    /// 0x20000000` is up) moves the drawn mesh. `chitei2`'s collapse is the
    /// case that needs it: partition-0 records 28..30 are the boulder, born
    /// 700 units up at a parking tile (`31 1D` + `4C 42 BC 02`), and the
    /// boulder beat (P2[17]) seats them at the foot of the escape stairs and
    /// ramps them down to the floor.
    ///
    /// The bind seats the actor at its `.MAP` seat Y (`ladder[nibble] +
    /// y_off`, [`Self::set_object_bind_heights`]) and every height write is
    /// absolute, so `dy` is the actor's Y **less that seat**; hosts add the
    /// displacement to the placement's own transform, which already stands
    /// at the seat. Reading the actor's Y as the displacement itself counted
    /// the seat twice: `suimon`'s water channel (partition-0 records 5..9,
    /// `31 1D` + `4C 42 <260..338>`) sits on the `-320` seat and drew a
    /// further `260..338` units up, level with the gate's lintel instead of
    /// the channel. Parked objects ([`Self::hidden_object_records`]) and
    /// unmoved ones are not listed.
    ///
    /// A floor follower ([`Self::object_floor_follow_records`]) takes its
    /// `dy` from the floor instead: its draw is baked on the floor under its
    /// seat, and the actor tick re-samples the floor under the actor where it
    /// stands, so a script that walks it off the seat carries it onto the
    /// floor there. `town01`'s record 8 (pack 25, the cliff-top sky strip)
    /// is seated at `(3648, 32)` and stands at `(5312, 4032)` in
    /// `rim_elm_zoom_intro`, where retail holds it at the `-256` floor; drawn
    /// on the seat's floor (`16`) it sat a cliff band low across the frame.
    // REF: FUN_8001ADA4 (case 5 draws at the actor position), FUN_8003A55C
    pub fn object_draw_displacements(&self) -> std::collections::HashMap<usize, [i32; 3]> {
        let hide = legaia_engine_field::man_field_scripts::FIELD_OFFMAP_HIDE_XZ as u16;
        let follow = self.object_floor_follow_records();
        self.field_vm
            .channels
            .iter()
            .filter(|c| c.object_bind && !(c.ctx.world_x == hide && c.ctx.world_z == hide))
            .filter_map(|c| {
                let (sx, sz) = self
                    .field_vm
                    .object_binds_by_key
                    .get(&c.placement_index)?
                    .seat;
                let sy = self
                    .field_vm
                    .object_channel_heights
                    .get(&c.placement_index)
                    .copied()
                    .unwrap_or(0);
                let (lx, lz) = (
                    i32::from(c.ctx.world_x as i16),
                    i32::from(c.ctx.world_z as i16),
                );
                // A floor follower's placed draw is baked on the floor under
                // its seat through the spawn ladder
                // (`follow_floor_placed_draws`), and retail's tick samples
                // the floor under the actor where it stands now through the
                // live ladder, so its rise is the floor's between the two -
                // a script that walks it, and one that installs a ladder
                // under it (`rikuroa`'s cave-mouth summit, `4C 9E`).
                let dy = if follow.contains(&c.placement_index) {
                    self.sample_field_floor_height(lx, lz)
                        - self.sample_field_floor_height_spawned(i32::from(sx), i32::from(sz))
                } else {
                    i32::from(c.ctx.world_y as i16) - i32::from(sy)
                };
                let d = [lx - i32::from(sx), dy, lz - i32::from(sz)];
                (d != [0, 0, 0]).then_some((c.placement_index, d))
            })
            .flat_map(|(k, d)| self.object_draw_keys(k).map(move |k| (k, d)))
            // A pack map's animated props, under their own record keys.
            .chain(self.expansion_prop_moves())
            .collect()
    }

    /// Record the scene's object-bind rotations
    /// ([`legaia_engine_field::man_field_scripts::object_script_bind_rots`]). Call before
    /// [`Self::seed_object_channels`], which seats each context's angles from
    /// them.
    pub fn set_object_bind_rots(&mut self, rots: std::collections::HashMap<usize, [u16; 3]>) {
        self.field_vm.object_channel_rots = rots;
    }

    /// Record the scene's object-bind seat heights
    /// ([`legaia_engine_field::man_field_scripts::object_script_bind_heights`]).
    /// Call before [`Self::seed_object_channels`], which seats each
    /// context's Y from them.
    pub fn set_object_bind_heights(&mut self, heights: std::collections::HashMap<usize, i16>) {
        self.field_vm.object_channel_heights = heights;
    }

    /// Seed each object-bind context's `+0x24 / +0x26 / +0x28` from its
    /// object's `.MAP` angles and its `+0x16` from the object's seat Y, as
    /// `FUN_8003A55C` seeds the actor: a later op `0x38` or `4C 48` turns it
    /// from there, and a height write replaces the Y outright.
    // REF: FUN_8003A55C
    pub(crate) fn seat_object_channel_rots(
        &self,
        chans: &mut [legaia_engine_field::field_channels::FieldChannel],
    ) {
        for c in chans.iter_mut().filter(|c| c.object_bind) {
            if let Some(&[rx, ry, rz]) = self.field_vm.object_channel_rots.get(&c.placement_index) {
                c.ctx.field_24 = rx as i16;
                c.ctx.field_26 = ry;
                c.ctx.field_28 = rz as i16;
            }
            // The seat's Y (`+0x16`), so a later height write lands as the
            // absolute Y it is in retail.
            if let Some(&y) = self.field_vm.object_channel_heights.get(&c.placement_index) {
                c.ctx.world_y = y as u16;
            }
        }
    }

    /// Flat record -> the live `(pitch, yaw, roll)` of each object-bind
    /// actor a script has **turned** away from its bind-time angles (op
    /// `0x38`'s compass write, op `4C 48`'s heading write). Retail's case-5
    /// draw composes the actor's `+0x24 / +0x26 / +0x28`, not the `.MAP`
    /// record's, so the drawn mesh turns with it: `town0c`'s two exit rocks
    /// are seated off the path and turned a quarter-plus (`38 83 00` /
    /// `38 87 00`) once the story opens the south road.
    ///
    /// Hosts pair each entry with the bind-time angles
    /// ([`Self::object_bind_rot`]) through
    /// [`legaia_engine_scene::field_env::object_turn_matrix`]. Unturned objects are not
    /// listed.
    // REF: FUN_8001ADA4 (case 5 composes the actor's angles), FUN_8003A55C
    pub fn object_draw_turns(&self) -> std::collections::HashMap<usize, [u16; 3]> {
        self.field_vm
            .channels
            .iter()
            .filter(|c| c.object_bind)
            .filter_map(|c| {
                let seed = *self.field_vm.object_channel_rots.get(&c.placement_index)?;
                let live = [c.ctx.field_24 as u16, c.ctx.field_26, c.ctx.field_28 as u16];
                let turned = live.iter().zip(seed).any(|(&l, s)| (l ^ s) & 0x0FFF != 0);
                turned.then_some((c.placement_index, live))
            })
            .flat_map(|(k, live)| self.object_draw_keys(k).map(move |k| (k, live)))
            .collect()
    }

    /// [`Self::object_draw_turns`] as the rotation each turned record's draws
    /// take about their own origin ([`legaia_engine_scene::field_env::object_turn_matrix`]):
    /// the one table both hosts fold into their placed draws
    /// ([`legaia_engine_scene::field_env::turn_placed_model`]).
    pub fn object_draw_turn_matrices(&self) -> std::collections::HashMap<usize, [f32; 16]> {
        self.object_draw_turns()
            .into_iter()
            .filter_map(|(record, live)| {
                let seed = self.object_bind_rot(record)?;
                Some((
                    record,
                    legaia_engine_scene::field_env::object_turn_matrix(seed, live),
                ))
            })
            .chain(self.expansion_prop_turns())
            .collect()
    }

    /// The bind-time `.MAP` angles of object-bind record `record`.
    pub fn object_bind_rot(&self, record: usize) -> Option<[u16; 3]> {
        self.field_vm.object_channel_rots.get(&record).copied()
    }

    /// The bind-time seat Y (`ladder[nibble] + y_off`, retail Y-down) of
    /// object-bind record `record` - the Y its draw displacement is measured
    /// from ([`Self::object_draw_displacements`]).
    pub fn object_bind_height(&self, record: usize) -> Option<i16> {
        self.field_vm.object_channel_heights.get(&record).copied()
    }

    /// Retail's spawn-install prologue pre-run: `FUN_8003A1E4` runs each
    /// just-spawned placement context through the field VM at scene load, so
    /// the record's story-flag-tested opening ops execute BEFORE the first
    /// rendered frame - the `0x23 MoveTo` park to the off-map sentinel tile
    /// (`0x7F,0x7F`) for actors the current story state despawns, and the
    /// story-relocation `MoveTo`s that seat an actor away from its MAN header
    /// tile. Runtime-pinned against the retail town01 actor list: a share of
    /// the placements stand parked or relocated from frame one, positions the
    /// raw placement header does not carry.
    ///
    /// The engine mirror: run ONE frame slice per channel (the NOP-break
    /// slice of `FUN_80039B7C` - a spawn prologue is written `test / MoveTo / 21`-idle, so its repositioning
    /// lands in the first slice), *unconditionally* - this is load-time
    /// behaviour, not the opt-in free-roam liveliness - but only for a record
    /// whose first opcode is `0x24`/`0x25`, the install loop's own entry gate.
    /// Everything that slice runs is the record's **spawn** section (it stops
    /// at the section's raw `0x21`), so a story-flag write there is a write
    /// every MAN-loading entry performs - `kor5` `P1[2]`'s `SET 0x619` at
    /// `+0x1A` is one. A same-scene reload that does not re-load the MAN
    /// (`FUN_801D6704` passes `a0 = loader-mask & 4` to `FUN_8003AEB0`, which
    /// skips the partition-1 spawn loop at `0x8003B8A0` when it is zero) runs
    /// none of it. Position writes are
    /// surfaced for every repositioned slot, and a slot the prologue parks
    /// drops its glide pace and any in-flight leg. A `4C 51` seat also writes
    /// its LUT heading, on the arm the prologue actually took - over the
    /// first-nibble guess [`Self::seed_field_npc_facings`] made at load.
    ///
    /// Call at scene entry after the carrier/channel install; the resulting
    /// positions snapshot into [`crate::world::FieldNpcState::entry_positions`], the state
    /// a cutscene teardown restores to.
    ///
    /// This load-frame slice is the only time the engine steps a placement
    /// channel's own script. Retail's per-actor tick `FUN_8003BC08` runs a
    /// context (`FUN_80039B7C`) only while its `+0x10 & 0x100` is up, and
    /// the three writers of that bit are the touch post `FUN_801D5B5C`, the
    /// op-`0x44` record spawner `FUN_8003BDE0` (a new partition-2 context -
    /// the engine's helper contexts) and the system SM `FUN_801DA51C`. The
    /// spawn install `FUN_8003A1E4` raises `0x01020000`, not `0x100`, and a
    /// cross-context op runs on the caller's slice against the target's
    /// context without raising it. So a placement a cutscene pokes, walks or
    /// places does not run its talk body: `dolk2`'s Noa (`P1[2]`) and the
    /// `town01` opening's `P1[10]` / `P1[11]` stay parked after their spawn
    /// section. A touch runs the engaged context as the interaction timeline
    /// ([`legaia_engine_field::cutscene_timeline::CutsceneTimeline::interaction_slot`]).
    // PORT: FUN_8003A1E4 (spawn-prologue pre-run -> initial actor positions)
    // REF: FUN_8003AEB0, FUN_80039B7C, FUN_8003BC08
    pub fn pre_run_field_channel_prologues(&mut self) {
        // A record this slice executes sets its heading from the ops on the
        // story arm it takes (`4C 51` seats, `38` compass turns), so the
        // load-time first-nibble guess ([`Self::seed_field_npc_facings`])
        // does not survive it: an arm the flags skip leaves the spawn
        // default. `dolk2`'s King Drake (`P1[41]`) turns to index 4 only
        // while `0x178` is clear, and `dolk2_market_noa` holds him at `0`.
        if let Some(man) = self.field_vm.channels_man.clone() {
            let executed: Vec<u8> = self
                .field_vm
                .channels
                .iter()
                .filter(|c| !c.object_bind && !c.done)
                .filter(|c| matches!(man.get(c.record_offset + c.pc), Some(0x24 | 0x25)))
                .filter_map(|c| u8::try_from(c.placement_index).ok())
                .collect();
            for slot in executed {
                self.npcs.headings.remove(&slot);
            }
            // The spawn loop's own `rand()` draws, one per placement with a
            // move, before its prologue slice (`0x8003A410`, `0x8003A4B8`).
            if let Ok(man_file) = legaia_scene_formats::man_section::parse(&man) {
                self.draw_placement_spawn_phases(&man_file, &man);
            }
        }
        self.field_vm.entry_prerun = true;
        self.step_field_channel_prologues(None);
        self.field_vm.entry_prerun = false;
        self.field_vm.prerun_running_slot = None;
        // A spawn section's story gate retires its actor before the first
        // frame draws (`cave01` P1[3]: `71 5D ..`, `31 03`).
        self.retire_killed_field_channels();
        self.npcs.entry_positions = self.npcs.positions.clone();
        // The ambient motion channels installed with the carriers still hold
        // the raw MAN header tiles; re-seat them on the story-true positions
        // this pre-run just resolved. The `0x18` wander's containment box is
        // absolute world space, so a stale seat silently retires it.
        self.resync_ambient_start_positions();
        // ... and on the headings its seats wrote.
        self.resync_ambient_start_headings();
    }

    /// Whether placement `slot`'s context has retired this visit.
    pub fn field_placement_retired(&self, slot: u8) -> bool {
        self.field_vm
            .retired_placements
            .contains(&usize::from(slot))
    }

    /// Owe the next frame a [`Self::apply_party_mesh_swap`].
    pub fn request_party_mesh_swap(&mut self) {
        self.field_vm.party_mesh_swap_pending = true;
    }

    /// The per-frame half of `FUN_8002519C` over the placement contexts: a
    /// context whose retire bit is up stops existing, and a queued `4C D2`
    /// seats a retired placement again.
    ///
    /// PORT: FUN_8002519C (the retire arm, placement contexts)
    pub(crate) fn sweep_field_channel_lifetimes(&mut self) {
        // Mid-scene the hosts have built their rigs: tell them.
        if std::mem::take(&mut self.field_vm.party_mesh_swap_pending)
            && self.apply_party_mesh_swap()
        {
            self.locomotion.player_rig_dirty = true;
        }
        // The re-seat resolves first: a context killed this frame still
        // answers `FUN_8003C83C`, the node stays linked for the walk after
        // next, so `4C D2` on it does nothing.
        self.run_pending_placement_respawns();
        for target in std::mem::take(&mut self.field_vm.pending_retire_ids) {
            if let Some(ci) =
                legaia_engine_field::field_channels::resolve_target(&self.field_vm.channels, target)
            {
                self.field_vm.channels[ci].ctx.flags |=
                    legaia_engine_field::field_channels::CONTEXT_RETIRE_BIT;
            }
        }
        self.retire_killed_field_channels();
    }

    /// Retire every placement context carrying
    /// [`CONTEXT_RETIRE_BIT`](legaia_engine_field::field_channels::CONTEXT_RETIRE_BIT).
    ///
    /// The channel leaves the set, so `FUN_8003C83C` lookups miss it, and the
    /// slot parks in the hide box, which every placement surface (draw, shadow,
    /// talk probe, collision, lights) already reads as "not here". A retired
    /// object-bind context leaves the set too, its key kept for
    /// [`Self::hidden_object_records`]: nothing re-seats one.
    fn retire_killed_field_channels(&mut self) {
        use legaia_engine_field::field_channels::CONTEXT_RETIRE_BIT;
        // A killed object drops out of the collision and touch lists with its
        // draw; a cross-context `B1 <obj> 03` reached only the channel. The
        // actor leaves the list (`FUN_8002519C` unlinks it), so the next
        // `B1 <obj> 03` reaches the record's next object: `rikuroa`'s summit
        // beat retires six of record 0's props, eight frames apart.
        let killed_objects: Vec<usize> = self
            .field_vm
            .channels
            .iter()
            .filter(|c| c.object_bind && c.ctx.flags & CONTEXT_RETIRE_BIT != 0)
            .map(|c| c.placement_index)
            .collect();
        for key in killed_objects {
            if let Some(anchor) = self.object_key_anchor(key)
                && let Some(prop) = self.props.bank.props.get_mut(&anchor)
            {
                prop.cflags |= CONTEXT_RETIRE_BIT;
            }
            self.field_vm.retired_objects.insert(key);
        }
        self.field_vm
            .channels
            .retain(|c| !c.object_bind || c.ctx.flags & CONTEXT_RETIRE_BIT == 0);
        let killed: Vec<usize> = self
            .field_vm
            .channels
            .iter()
            .filter(|c| !c.object_bind && c.ctx.flags & CONTEXT_RETIRE_BIT != 0)
            .map(|c| c.placement_index)
            .collect();
        if killed.is_empty() {
            return;
        }
        self.field_vm
            .channels
            .retain(|c| c.object_bind || c.ctx.flags & CONTEXT_RETIRE_BIT == 0);
        let hide = legaia_engine_field::man_field_scripts::FIELD_OFFMAP_HIDE_XZ;
        for index in killed {
            self.field_vm.retired_placements.insert(index);
            self.field_vm.pending_engagements.retain(|&p| p != index);
            let Ok(slot) = u8::try_from(index) else {
                continue;
            };
            self.npcs.positions.insert(slot, (hide, hide));
            self.npcs.glide_speeds.remove(&slot);
            self.npcs.motions.remove(&slot);
            self.npcs.glide_legs.remove(&slot);
            self.npcs.face_legs.remove(&slot);
            self.npcs.rotate_legs.remove(&slot);
        }
    }

    /// `4C D2 <channel>`: when `FUN_8003C83C` answers nothing, seat the
    /// placement afresh through `FUN_8003A1E4`, spawn section included
    /// (`0x801E2BC8`). An answered channel is left alone.
    // REF: FUN_801DE840 (0x801E2B7C..0x801E2BD8), FUN_8003A1E4
    fn run_pending_placement_respawns(&mut self) {
        if self.field_vm.pending_placement_respawns.is_empty() {
            return;
        }
        let queue = std::mem::take(&mut self.field_vm.pending_placement_respawns);
        let Some(man) = self.field_vm.channels_man.clone() else {
            return;
        };
        let Ok(man_file) = legaia_scene_formats::man_section::parse(&man) else {
            return;
        };
        let n0 = man_file.header.partition_counts[0].max(0) as usize;
        for channel in queue {
            let Some(index) = usize::from(channel).checked_sub(n0) else {
                continue;
            };
            if !self.field_vm.retired_placements.remove(&index) {
                continue;
            }
            let Some(fresh) = legaia_engine_field::field_channels::spawn_placement_channel(
                &man_file, &man, index,
            ) else {
                continue;
            };
            let Ok(slot) = u8::try_from(index) else {
                continue;
            };
            // Placements stay ahead of the object binds: a flat index at or
            // past `N0` aliases a placement's script id, and the first-match
            // walk has to keep the placement.
            let at = self
                .field_vm
                .channels
                .iter()
                .position(|c| c.object_bind)
                .unwrap_or(self.field_vm.channels.len());
            self.field_vm.channels.insert(at, fresh);
            // A fresh node at the list's tail (`FUN_80020454`), and the
            // spawn's clip-phase draw before the prologue slice.
            self.relink_ambient_at_tail(slot);
            if man_file
                .actor_placement(&man, index)
                .is_some_and(|p| p.anim_id > 0)
            {
                self.draw_placement_spawn_phase(slot);
            }
            self.npcs.positions.remove(&slot);
            self.npcs.headings.remove(&slot);
            self.field_vm.entry_prerun = true;
            self.step_field_channel_prologues(Some(index));
            self.field_vm.entry_prerun = false;
            match self.npcs.positions.get(&slot) {
                Some(&seat) => {
                    self.npcs.entry_positions.insert(slot, seat);
                }
                None => {
                    self.npcs.entry_positions.remove(&slot);
                }
            }
            if let Some(chan) = self.npcs.ambient.get_mut(&slot) {
                chan.live = None;
                chan.vm.pc = 0;
                chan.vm.cursor = 0;
            }
            self.resync_ambient_start_positions();
        }
        // The spawn section runs the same story gates a scene entry does.
        self.retire_killed_field_channels();
    }

    /// The clip-phase `rand()` draw of every placement the spawn loop
    /// (`FUN_8003AEB0` over `FUN_8003A1E4`) seats with a move, in spawn
    /// order: `man_file.actor_placements` is partition order `1..N1`.
    ///
    /// REF: FUN_8003AEB0 (the `0x8003B8D4` loop)
    pub(crate) fn draw_placement_spawn_phases(
        &mut self,
        man_file: &legaia_scene_formats::man_section::ManFile,
        man: &[u8],
    ) {
        for p in man_file.actor_placements(man) {
            if p.anim_id > 0
                && let Ok(slot) = u8::try_from(p.index)
            {
                self.draw_placement_spawn_phase(slot);
            }
        }
    }

    pub(super) fn step_field_channel_prologues(&mut self, only: Option<usize>) {
        if self.field_vm.channels.is_empty() {
            return;
        }
        let Some(man) = self.field_vm.channels_man.clone() else {
            return;
        };
        let mut channels = std::mem::take(&mut self.field_vm.channels);
        // Host hooks resolve cross-context ids against the channel set while
        // one of these is executing; the live vector is moved out for the
        // borrow, so they read this copy (`World::channel_view`).
        self.field_vm.stepping_view = channels.clone();
        let pre_pos: Vec<(u16, u16)> = channels
            .iter()
            .map(|c| (c.ctx.world_x, c.ctx.world_z))
            .collect();
        // Placements whose load slice ended cleanly - on an executed raw
        // `0x21` or in front of a text lead - with the cursor it left.
        let mut talk_cursors: Vec<(usize, usize)> = Vec::new();
        for i in 0..channels.len() {
            if channels[i].done {
                continue;
            }
            if only.is_some_and(|o| channels[i].object_bind || channels[i].placement_index != o) {
                continue;
            }
            // Object-bind channels are cross-context poke targets only: the
            // engine drives their interaction bodies through the
            // touch/interact dispatch, not autonomous stepping (their
            // bind-time `0x24`/`0x25` prologue already ran at install,
            // mirroring `FUN_8003A55C`).
            if channels[i].object_bind {
                continue;
            }
            if man.len() <= channels[i].record_offset {
                channels[i].done = true;
                continue;
            }
            // A halted channel is SUSPENDED - the halt bit persists until an
            // explicit un-halt (the op-0x32 CFLAG_CLR bit-10 carve-out, the
            // one cross-context op allowed against a halted target). This is
            // the timeline's freeze/unfreeze choreography primitive: a
            // `4C 85` halt-acquire suspends the actor, `B3 <id> 0A` verifies
            // the suspension, `B2 <id> 0A` resumes it for its next beat.
            // (Autonomous frame pacing uses the `0x21` NOP break instead of
            // yields, so suspension never races normal idling.)
            if channels[i].ctx.is_halted() {
                continue;
            }
            // The load-frame slice is gated on the record's FIRST opcode:
            // `FUN_8003A1E4` enters its run loop only when the byte at the
            // freshly seated `+0x9E` is `0x24` or `0x25` (`addiu v0,v1,-0x24;
            // sltiu v0,v0,0x2` at `0x8003A480`), so a placement opening on
            // anything else executes nothing inside the load frame.
            if !matches!(
                man.get(channels[i].record_offset + channels[i].pc),
                Some(0x24 | 0x25)
            ) {
                continue;
            }
            self.field_vm.prerun_running_slot = if only.is_none() {
                u8::try_from(channels[i].placement_index).ok()
            } else {
                None
            };
            let mut budget = FIELD_CHANNEL_STEP_BUDGET;
            while budget > 0 {
                budget -= 1;
                let pc = channels[i].pc;
                let record_offset = channels[i].record_offset;
                let bc = &man[record_offset..];
                let Some(&opcode_byte) = bc.get(pc) else {
                    channels[i].done = true;
                    break;
                };
                // `FUN_8003A1E4`'s loop tests the byte at the new PC before
                // each call (`andi 0x7F; sltiu 0x20` at `0x8003A4EC`): a
                // text lead or any other sub-`0x20` byte ends the load slice
                // unexecuted.
                if opcode_byte & 0x7F < 0x20 {
                    if self.field_vm.entry_prerun {
                        talk_cursors.push((channels[i].placement_index, pc));
                    }
                    break;
                }
                // Cross-context poke: resolve the extended target to another
                // channel and run the op against that context.
                let ext = vm::field::peek_extended(bc, pc);
                let target = ext.and_then(|t| {
                    let ci = legaia_engine_field::field_channels::resolve_target(&channels, t)?;
                    (ci != i).then_some(ci)
                });
                let self_target = ext.is_some_and(|t| {
                    legaia_engine_field::field_channels::resolve_target(&channels, t) == Some(i)
                });
                self.field_vm.executing_object = target
                    .filter(|&ci| channels[ci].object_bind)
                    .map(|ci| channels[ci].placement_index as u16);
                self.field_vm.executing_channel = match target {
                    // Object-bind targets carry a flat record index, not a
                    // placement slot - no placement-keyed attribution.
                    Some(ci) if channels[ci].object_bind => None,
                    Some(ci) => Some(channels[ci].placement_index as u8),
                    // An extended op aimed at something that is not a
                    // channel (the player `0xF8`, the system `0xFB`) is not
                    // this placement's to receive.
                    None if ext.is_some() && !self_target => None,
                    None => Some(channels[i].placement_index as u8),
                };
                // A face-at acquire on a placement - the record's own actor
                // (`4C 85 <lo> <hi> <bind>`, `rikuroa`'s Noa turning toward
                // the placement she was seated beside) or one named by the
                // cross-context form (`CC <id> 85 ..`), the record's own id
                // included - `kor3` `P1[11]` runs `CC 14 85 00 00 13` on
                // itself: the target's walk kernel runs the leg.
                // It is armed here and stepped from the next field tick,
                // once the seats this pre-run writes have surfaced; the op
                // itself still runs below, so the pre-run's control flow -
                // an own-context acquire parks the section, as the record's
                // own halt does in retail - is unchanged.
                // REF: FUN_801DE840 (0x801E2148..0x801E21DC), FUN_8003774C (the 0x4C arm)
                if let Some(slot) = self.field_vm.executing_channel
                    && let Some(ramp) = legaia_engine_field::inline_dialogue::TalkFaceRamp::from_own_acquire(
                        bc, pc,
                    )
                    .or_else(|| {
                        legaia_engine_field::inline_dialogue::TalkFaceRamp::from_npc_acquire(bc, pc)
                            .filter(|_| target.is_some() || self_target)
                            .map(|(_, r)| r)
                    })
                {
                    self.npcs.rotate_legs.remove(&slot);
                    self.npcs.face_legs.insert(slot, ramp);
                }
                if let Some(ci) = target.filter(|&ci| channels[ci].object_bind) {
                    let key = channels[ci].placement_index;
                    self.mirror_object_end_latch(key, &mut channels[ci].ctx.local_flags);
                }
                let object_words = target
                    .filter(|&ci| channels[ci].object_bind)
                    .map(|ci| (ci, channels[ci].ctx.local_flags, channels[ci].ctx.field_6a));
                let result = {
                    let mut host = FieldHostImpl { world: self };
                    match target {
                        Some(ci) => {
                            // Two disjoint &mut contexts out of the vec.
                            let (lo, hi) = channels.split_at_mut(i.max(ci));
                            let (target_ctx, caller_ctx) = if ci < i {
                                (&mut lo[ci].ctx, &mut hi[0].ctx)
                            } else {
                                (&mut hi[0].ctx, &mut lo[i].ctx)
                            };
                            let bc = &man[record_offset..];
                            vm::field::step_with_caller(
                                &mut host, target_ctx, caller_ctx, false, bc, pc,
                            )
                        }
                        None => {
                            let bc = &man[record_offset..];
                            field_step_routed(&mut host, &mut channels[i].ctx, bc, pc)
                        }
                    }
                };
                let exec_slot = self.field_vm.executing_channel;
                self.field_vm.executing_channel = None;
                self.field_vm.executing_object = None;
                // A poke at a placed object's clip words reaches the clip it
                // already plays (`World::poke_object_prop_words`).
                if let Some((ci, flags, rate)) = object_words {
                    let key = channels[ci].placement_index;
                    let c = &channels[ci].ctx;
                    self.poke_object_prop_words(key, (flags, c.local_flags), (rate, c.field_6a));
                }
                match result {
                    FieldStepResult::Advance { next_pc } => {
                        let stalled = next_pc == pc;
                        channels[i].pc = next_pc;
                        if self.field_vm.entry_prerun && opcode_byte == 0x21 && !stalled {
                            talk_cursors.push((channels[i].placement_index, next_pc));
                        }
                        // Retail frame-slice pacing: a NOP ends the slice
                        // after executing (FUN_80039B7C `bVar1 == 0x21`), and
                        // a non-advancing PC ends it defensively.
                        if opcode_byte & 0x7F == 0x21 || stalled {
                            break;
                        }
                    }
                    FieldStepResult::Yield { resume_pc } => {
                        // Retail's load slice stops only on an executed
                        // `0x21`, an unmoved PC or a sub-`0x20` byte
                        // (`0x8003A4C4..0x8003A4F4`); a yield op that hands
                        // back the next PC (a `41` axis glide armed on the
                        // actor) runs on into the ops after it. `dolk`
                        // `P1[22]` (`23 63 4F`, `41 06 01`, `38 86 00`) and
                        // `izumi` `P1[1]` stand turned by the `38` past the
                        // glide.
                        //
                        // The glide itself is the walk kernel's: the arm
                        // seats the op on the actor's `+0x94` under the halt
                        // bit, and `FUN_8003BC08` runs `FUN_8003774C` on it
                        // from the next tick until it lands.
                        // REF: FUN_801DE840 (0x801DEE90..0x801DEF1C), FUN_8003774C (the 0x37 / 0x41 arm)
                        if let Some(slot) = exec_slot
                            && let Some((body0, body1, rate)) =
                                compass_walk_operands(&man[record_offset..], pc)
                        {
                            self.npcs.glide_legs.insert(
                                slot,
                                legaia_engine_field::cutscene_timeline::TimelineNpcGlide {
                                    slot,
                                    state: legaia_engine_motion_vm::motion_vm::MotionState::default(
                                    ),
                                    body0,
                                    body1,
                                    rate,
                                    frames: 0,
                                },
                            );
                        }
                        // The same arm aimed at a placed object: the op seats
                        // on the object actor, whose tick walks it.
                        if let Some(ci) = target.filter(|&ci| channels[ci].object_bind)
                            && let Some(leg) = compass_walk_operands(&man[record_offset..], pc)
                        {
                            channels[ci].ctx.flags |= 0x400;
                            self.arm_object_glide_leg(channels[ci].placement_index as u16, leg);
                        }
                        let stalled = resume_pc == pc;
                        channels[i].pc = resume_pc;
                        if opcode_byte & 0x7F == 0x21 || stalled {
                            break;
                        }
                    }
                    FieldStepResult::Halt { final_pc } => {
                        // Parked (conditional hold / halted cross-target):
                        // retry the same PC next frame.
                        channels[i].pc = final_pc;
                        break;
                    }
                    FieldStepResult::Pending { pc, .. } | FieldStepResult::Unknown { pc, .. } => {
                        // An op this port can't advance past inside a channel
                        // script: retire the channel (it stays resolvable as a
                        // cross-context target).
                        channels[i].pc = pc;
                        channels[i].done = true;
                        break;
                    }
                }
            }
        }
        // Write the spawn prologue's moves through to the field NPC
        // render/probe state, unconditionally: the executed branch is the
        // story truth for the slot's initial position. A park also drops the
        // slot's glide pace and any in-flight leg - a despawned actor has
        // nothing left to walk.
        for (c, pre) in channels.iter().zip(pre_pos) {
            if c.object_bind {
                // Flat-record-keyed context; the NPC surfaces are
                // placement-keyed.
                continue;
            }
            let (nx, nz) = (c.ctx.world_x, c.ctx.world_z);
            if (nx, nz) == pre {
                continue;
            }
            let slot = c.placement_index as u8;
            let (nx, nz) = (nx as i16, nz as i16);
            let hide = legaia_engine_field::man_field_scripts::FIELD_OFFMAP_HIDE_XZ;
            if (nx, nz) == (hide, hide) {
                self.npcs.glide_speeds.remove(&slot);
                self.npcs.motions.remove(&slot);
            }
            self.npcs.positions.insert(slot, (nx, nz));
        }
        self.field_vm.channels = channels;
        self.field_vm.stepping_view.clear();
        // The talk resumes the same context the load slice ran (retail's
        // dialog SM starts at `actor[+0x9E]`), so a clean stop is the talk's
        // entry. The static entry - one past the record's first linear `0x21`
        // - names the wrong arm of a flag-dispatched spawn section: the
        // pickup spots (`dolk2` `P1[70]`, the Platinum Card) open `25`,
        // `7x <flag>` (taken -> `31 03`, `21`), `26 05 00`, `4C 40 ..`, `21`,
        // and the talk the static entry started ran into the taken arm's
        // `0x21` and fell through to the "now has" box, past the flag set,
        // the `0x39` grant and the `36 00 80 2C 00` item-get cue.
        // REF: FUN_8003A1E4 (the load slice, `0x8003A4C4..0x8003A4F4`), FUN_80039B7C
        for (placement, pc) in talk_cursors {
            if let Ok(slot) = u8::try_from(placement)
                && let Some(rec) = self.npcs.dialog_prologue.get_mut(&slot)
                && pc < rec.body.len()
            {
                rec.entry_pc = pc;
                rec.resume = true;
            }
        }
    }

    /// Seed the per-actor field-VM channels for **ordinary free-roam** scene
    /// entry: one context per MAN partition-1 placement, exactly as a cutscene
    /// install seeds them, but without a timeline driving cross-context pokes.
    /// The scene loader calls this after the placement-derived carrier / NPC
    /// install so each placement's spawn section runs through
    /// [`Self::pre_run_field_channel_prologues`] - the non-cutscene half of
    /// retail's `FUN_8003AEB0` spawn loop.
    ///
    /// Cutscene scenes (`opdeene` and friends) re-seed the set through
    /// [`Self::install_cutscene_timeline_record`] afterwards, which simply
    /// replaces this set, so the two paths compose. A scene with no placements
    /// seeds an empty set and the pre-run no-ops - but the
    /// MAN is retained regardless: object-bind channels
    /// ([`Self::seed_object_channels`]) and walk-on partition-2 installs
    /// ([`Self::install_gated_p2_record`]) execute out of the same buffer, and
    /// retail binds objects (`FUN_8003A55C`) whether or not partition 1 placed
    /// any actors.
    // PORT: FUN_8003AEB0 (the per-record spawn loop, free-roam entry)
    // REF: FUN_8003A1E4 (per-record context spawn)
    pub fn seed_field_channels(
        &mut self,
        man_file: &legaia_scene_formats::man_section::ManFile,
        man: &[u8],
    ) {
        self.field_vm.pending_engagements.clear();
        self.field_vm.retired_placements.clear();
        self.field_vm.pending_placement_respawns.clear();
        self.field_vm.channels = legaia_engine_field::field_channels::spawn_channels(man_file, man);
        self.field_vm.channels_man = if man.is_empty() {
            None
        } else {
            Some(std::sync::Arc::new(man.to_vec()))
        };
        // A new scene's binds are installed by `seed_object_channels` after
        // the trigger tables resolve; drop the previous scene's.
        self.field_vm.object_channel_binds.clear();
        self.field_vm.object_binds_by_key.clear();
        self.field_vm.retired_objects.clear();
        self.npcs.anim_cues.clear();
        self.npcs.clip_current.clear();
        self.npcs.clip_cursors.clear();
        self.npcs.phase_draws.clear();
        self.npcs.faces.clear();
        self.npcs.face_ramps = legaia_engine_motion_vm::ambient_motion::RampScheduler::new();
        self.npcs.clip_rate_live.clear();
        self.npcs.clip_bones.clear();
    }

    /// Append the `.MAP` **object-bind** channels (retail scene-init
    /// `FUN_8003A55C`): one context per bound object - two objects bound to
    /// one record are two contexts - script id = the gate-0 trigger's
    /// **flat** MAN record index (`actor[+0x50] = trigger[2]`), seated on
    /// the object's own position, angles and height.
    /// This is what makes partition-0 records resolvable cross-context
    /// targets through the `FUN_8003C83C` walk - the `town01` Mei walk-on
    /// beat pokes the Vahn's-house door object as channel `0x01`.
    ///
    /// Mirrors the retail bind-time prologue pre-run: a bound record whose
    /// first opcode is `0x24`/`0x25` runs through the field VM until it
    /// yields, stalls, or reaches a dialog byte (the `FUN_8003A55C` inline
    /// loop), so the object context carries its init state (the door-angle
    /// `4C 41` ramp seed) before the first poke.
    ///
    /// Binds are remembered on [`crate::world::FieldVmState::object_channel_binds`] so a
    /// cutscene-timeline install that has to respawn the channel set
    /// re-appends them. Call after [`Self::seed_field_channels`]; no-op when
    /// that seeded nothing (a scene without a MAN).
    // REF: FUN_8003A55C (object bind: +0x50 flat script id + prologue pre-run)
    pub fn seed_object_channels(
        &mut self,
        man_file: &legaia_scene_formats::man_section::ManFile,
        man: &[u8],
        binds: &[legaia_engine_field::man_field_scripts::ObjectScriptBind],
    ) {
        self.install_object_binds(binds);
        let same_man = self
            .field_vm
            .channels_man
            .as_deref()
            .is_some_and(|m| m.as_slice() == man);
        if !same_man {
            return;
        }
        let mut obj =
            legaia_engine_field::field_channels::spawn_object_channels(man_file, man, binds);
        self.seat_object_channel_rots(&mut obj);
        self.pre_run_object_channel_prologues(&mut obj, man);
        self.field_vm.channels.extend(obj);
    }

    /// Record the scene's object binds ([`Self::seed_object_channels`]):
    /// the list, its channel keys, and each object's own angles and seat
    /// height under every key a host looks its draw up by
    /// ([`Self::object_draw_keys`]).
    pub(crate) fn install_object_binds(
        &mut self,
        binds: &[legaia_engine_field::man_field_scripts::ObjectScriptBind],
    ) {
        self.field_vm.object_channel_binds = binds.to_vec();
        self.field_vm.retired_objects.clear();
        self.field_vm.object_binds_by_key =
            legaia_engine_field::field_channels::object_channel_keys(binds)
                .into_iter()
                .zip(binds.iter().copied())
                .collect();
        for (&key, b) in &self.field_vm.object_binds_by_key {
            let instance = legaia_engine_field::field_channels::object_instance_key(b.tile);
            for k in [key, instance] {
                self.field_vm.object_channel_rots.insert(k, b.rot);
                self.field_vm.object_channel_heights.insert(k, b.seat_y);
            }
        }
    }

    /// The `FUN_8003A55C` bind-time prologue pre-run: for each object-bind
    /// channel whose first opcode is `0x24`/`0x25`, step the field VM until
    /// the op was a `0x21` NOP, the PC stalls, a dialog byte is reached, or
    /// the context yields/halts - the retail inline loop's exact stop set.
    pub(super) fn pre_run_object_channel_prologues(
        &mut self,
        channels: &mut [legaia_engine_field::field_channels::FieldChannel],
        man: &[u8],
    ) {
        for c in channels.iter_mut() {
            let Some(bc) = man.get(c.record_offset..) else {
                c.done = true;
                continue;
            };
            let Some(&first) = bc.get(c.pc) else {
                c.done = true;
                continue;
            };
            if first != 0x24 && first != 0x25 {
                continue;
            }
            let mut budget = FIELD_CHANNEL_STEP_BUDGET;
            while budget > 0 {
                budget -= 1;
                let pc = c.pc;
                let Some(&op) = bc.get(pc) else {
                    break;
                };
                if op & 0x7F < 0x20 {
                    // Dialog byte: the retail pre-run loop stops here.
                    break;
                }
                // The bind's own context is the one executing: an op-`0x4B`
                // here arms the bound object's morph lanes.
                self.field_vm.executing_object = Some(c.placement_index as u16);
                let result = {
                    let mut host = FieldHostImpl { world: self };
                    field_step_routed(&mut host, &mut c.ctx, bc, pc)
                };
                self.field_vm.executing_object = None;
                match result {
                    FieldStepResult::Advance { next_pc } => {
                        if next_pc == pc {
                            break;
                        }
                        c.pc = next_pc;
                        if op & 0x7F == 0x21 {
                            break;
                        }
                    }
                    FieldStepResult::Yield { resume_pc } => {
                        // A compass walk on the object's own actor: the
                        // walk kernel moves it from the next tick.
                        // REF: FUN_8003BC08, FUN_8003774C
                        if let Some(leg) = compass_walk_operands(bc, pc).filter(|_| op & 0x80 == 0)
                        {
                            self.arm_object_glide_leg(c.placement_index as u16, leg);
                        }
                        c.pc = resume_pc;
                        break;
                    }
                    FieldStepResult::Halt { final_pc } => {
                        c.pc = final_pc;
                        break;
                    }
                    FieldStepResult::Pending { pc, .. } | FieldStepResult::Unknown { pc, .. } => {
                        c.pc = pc;
                        break;
                    }
                }
            }
        }
    }
}

/// The leg a compass-walk op at `pc` arms, as `(body0, body1, rate)`:
/// `[37|41 b0 b1]` on the running context, `[B7|C1 id b0 b1]` on another.
/// The rate is `0x80` for `0x37` and `0x40` for `0x41`
/// (`0x80037904` in the walk kernel).
// REF: FUN_801DE840 (the 0x37 / 0x41 arm), FUN_8003774C (0x8003789C..0x800379F8)
pub(crate) fn compass_walk_operands(bc: &[u8], pc: usize) -> Option<(u8, u8, i32)> {
    let op = *bc.get(pc)?;
    let rate = match op & 0x7F {
        0x37 => 0x80,
        0x41 => 0x40,
        _ => return None,
    };
    let at = pc + if op & 0x80 != 0 { 2 } else { 1 };
    Some((*bc.get(at)?, *bc.get(at + 1)?, rate))
}
