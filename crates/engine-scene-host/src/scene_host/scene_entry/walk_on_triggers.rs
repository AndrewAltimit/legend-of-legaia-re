//! Walk-on trigger dispatch: arrival trigger records, portal classification, kind-1 walk-ons
//! and intra-scene teleports.

use super::*;

impl SceneHost {
    /// Spawn the partition-2 record referenced by a gate-1 kind-1 tile
    /// trigger at the arrival tile - the retail walk-on dispatch
    /// (`FUN_801D1EC4` -> `FUN_801D5630` -> `FUN_8003BDE0`), which fires on
    /// the first post-arrival frame because the last-tile globals are stale.
    ///
    /// Scoped to the opening chain (this immediate same-tick spawn is the
    /// chain's fly-in / arrival mechanism; door records resolve through the
    /// dedicated warp path, and an ordinary scene's arrival tile is covered by
    /// the per-frame [`Self::dispatch_walk_on_trigger`] stale-tile compare on
    /// the first post-entry tick). Live-probe-pinned: retail's `map01` and
    /// `town01` opening records spawn exactly this way - the entry seat
    /// lands on the trigger tile and the stale-tile compare fires the same
    /// tick; `opstati` / `opurud` spawn via op-`0x44` in their entry scripts
    /// instead and never reach this path.
    // REF: FUN_801D1EC4, FUN_801D5630, FUN_8003BDE0
    pub(super) fn spawn_arrival_trigger_record(&mut self, tile_x: u8, tile_z: u8) {
        if !self.world.cutscene.opening_chain_active || self.world.cutscene_timeline_active() {
            return;
        }
        let Some(scene) = self.scene.as_ref() else {
            return;
        };
        let Ok((primary, fallback)) = scene.field_tile_triggers(&self.index) else {
            return;
        };
        let Some(Ok(Some(man_bytes))) = self
            .scene
            .as_ref()
            .map(|s| s.field_man_payload(&self.index))
        else {
            return;
        };
        let Ok(man_file) = legaia_scene_formats::man_section::parse(&man_bytes) else {
            return;
        };
        let Some(trigger) = legaia_engine_field::field_regions::lookup_tile_trigger(
            &primary, &fallback, tile_x, tile_z,
        )
        .filter(|t| t.gate == 1) else {
            return;
        };
        if self
            .world
            .install_gated_p2_record(&man_file, &man_bytes, trigger.record as usize)
        {
            log::info!(
                "opening: walk-on trigger at ({tile_x:#x},{tile_z:#x}) spawned P2[{}]",
                trigger.record,
            );
        }
    }

    /// Per-frame walk-on tile-trigger dispatch - the retail field loop's
    /// tile compare (`FUN_801D1EC4`): when the player crosses into a new
    /// 128-unit collision tile during free-roam field play, the `.MAP`
    /// kind-1 trigger table is consulted (`FUN_801D5630`) and a gate-1 hit
    /// spawns MAN partition-2 record `record` as a field-VM context
    /// (`FUN_8003BDE0`, C1/C2 story-flag gates checked inside
    /// [`legaia_engine_core::world::World::install_gated_p2_record`]).
    ///
    /// This is how town exits work: e.g. Rim Elm's south-gate tiles carry a
    /// gate-1 trigger for the partition-2 record whose script runs the
    /// `0x3F` named scene-change to `map01` - and how walk-on story beats
    /// (the post-naming Vahn's-house scenes) launch. A `None` last tile
    /// (scene entry / warp arrival) fires the trigger at the *current* tile,
    /// mirroring retail's stale last-tile globals on the first post-arrival
    /// frame.
    ///
    /// Skipped while a modal cutscene timeline owns the frame (a walk-on beat
    /// record is cutscene-class: it seizes the camera and locks locomotion,
    /// one at a time) and while any dialog or name entry is up. Concurrent
    /// helper contexts ([`legaia_engine_core::world::FieldVmState::helper_contexts`]) do NOT
    /// block the dispatch.
    ///
    /// Runs in both plain field mode **and** the overworld (world-map) mode.
    /// On the overworld a gate-1 trigger whose record is a **portal** (carries
    /// a `0x3F` named scene-change) is left to the world-map entity SM (the
    /// `OverworldPortal` walk-onto path); only gate-1 **beat** records - the
    /// Drake mist-wall force-walk bands (`map01` partition-2 records gated on
    /// `C1=[0x482]`) and their kin - are spawned here as a cutscene timeline,
    /// exactly as a town walk-on beat is. This is what makes those bands honor
    /// their C1 one-shot latch on the overworld instead of never firing.
    /// `true` when partition-2 record `record` carries a `0x3F` named
    /// scene-change op (a portal / door record) - the same test
    /// [`legaia_engine_field::man_field_scripts::overworld_portal_sites`] uses to select
    /// entrance records. Used by the overworld walk-on dispatch to leave portal
    /// records to the entity SM and spawn only beat records. A clean
    /// fall-through walk from the record's true `pc0`; `false` on a bad span.
    // REF: FUN_8003BDE0
    fn p2_record_is_portal(
        man_file: &legaia_scene_formats::man_section::ManFile,
        man: &[u8],
        record: usize,
    ) -> bool {
        let Some((start, pc0, len)) =
            legaia_engine_field::man_field_scripts::partition_record_span(man_file, man, 2, record)
        else {
            return false;
        };
        let body = &man[start..start + len];
        let mut pc = pc0;
        while pc < body.len() {
            let Ok(insn) = legaia_scene_formats::field_disasm::decode(body, pc) else {
                return false;
            };
            if insn.size == 0 {
                return false;
            }
            if matches!(
                insn.info,
                legaia_scene_formats::field_disasm::InsnInfo::SceneChange { .. }
            ) {
                return true;
            }
            pc += insn.size;
        }
        false
    }

    // REF: FUN_801D1EC4, FUN_801D5630, FUN_8003BDE0
    pub(super) fn dispatch_walk_on_trigger(&mut self) {
        let on_world_map = match self.world.mode {
            legaia_engine_mode::mode::SceneMode::Field => false,
            legaia_engine_mode::mode::SceneMode::WorldMap => true,
            _ => return,
        };
        // A kind-0 warp in flight owns the dispatcher: retail's timer half
        // returns before the tile compare, and the landing frame seats the
        // player, restamps the crossing tile and runs the landing tile's
        // kind-1 record inline (`FUN_801D1EC4` at `0x801D1EEC..0x801D2064`).
        if !on_world_map {
            match self.world.tick_field_warp() {
                legaia_engine_core::world::FieldWarpTick::Idle => {}
                legaia_engine_core::world::FieldWarpTick::Running => return,
                legaia_engine_core::world::FieldWarpTick::Landed {
                    query_tile,
                    landing_tile,
                } => {
                    self.last_trigger_tile = Some(landing_tile);
                    // The landing calls `FUN_8003BDE0` straight (no clip
                    // reset - that store is the changed-tile arm's).
                    self.dispatch_kind1_walk_on(query_tile, false, false);
                    return;
                }
            }
        }
        // A committed battle holds the whole field frame: retail's intro
        // overlay (PROT 0979) is loaded over the field overlay's head, frame
        // pump and this dispatcher included, so no tile compare runs between
        // a `3E FF` and the fight (`World::field_scripts_held_for_battle`).
        if self.world.name_entry_active()
            || self.world.board.grid.is_some()
            || self.world.active_fmv().is_some()
            || self.world.field_scripts_held_for_battle()
        {
            return;
        }
        let Some(slot) = self.world.player_actor_slot else {
            return;
        };
        let Some(actor) = self.world.actors.get(slot as usize) else {
            return;
        };
        // A script holding the player (retail: the `+0x10 & 0x80000` lock the
        // context runner raises every frame it steps one) **consumes** a
        // crossing rather than deferring it: on a new tile under the lock the
        // dispatcher stores the tile and returns (`0x801D214C..0x801D2158` ->
        // `0x801D2270`). So a record that teleports the player onto another
        // trigger tile (`rugi`'s warp pads: P2[0] seats the player on P2[1]'s
        // pad, which seats it back on P2[0]'s) does not fire the landing
        // pad when it lets go. Deferring the compare instead fired it on the
        // first free frame and bounced the player between the two pads for
        // good. A stale compare (scene entry) is left stale, so an arrival
        // tile still fires once the entry's script releases.
        let script_owns = self.world.cutscene_timeline_active() || self.world.dialogue_owns_input();
        if script_owns {
            if self.last_trigger_tile.is_some() {
                let quant = |w: i16| -> i32 { i32::from(w) >> 7 };
                let (tx, tz) = (
                    quant(actor.move_state.world_x),
                    quant(actor.move_state.world_z),
                );
                if (0..=0x7F).contains(&tx) && (0..=0x7F).contains(&tz) {
                    self.last_trigger_tile = Some((tx as u8, tz as u8));
                }
            }
            return;
        }
        // Retail's tile quantisation **in this dispatcher** is the raw
        // `world >> 7` (`FUN_801D1EC4` at `0x801d2068`: `sll 0x10; sra 0x17`
        // on each of `player+0x14` / `+0x18`), not the `(world - 0x40) >> 7`
        // form the region refresh uses. The two agree at tile centres and
        // differ by a half-tile band elsewhere; the kind-0 door tiles are one
        // tile deep, so the band matters.
        let quant = |w: i16| -> i32 { i32::from(w) >> 7 };
        let (tx, tz) = (
            quant(actor.move_state.world_x),
            quant(actor.move_state.world_z),
        );
        if !(0..=0x7F).contains(&tx) || !(0..=0x7F).contains(&tz) {
            return;
        }
        let tile = (tx as u8, tz as u8);
        let locked =
            actor.move_state.flags & legaia_engine_field::field_warp_tile::MOVEMENT_LOCK != 0;
        let repoll = legaia_engine_core::world::WALK_ON_REPOLL_FLAG;
        if self.last_trigger_tile == Some(tile) {
            // Same tile as last tick: triggers fire on crossings, except
            // that a record which raised the re-poll bit (op `2E 13`) is
            // re-run every tick the player stays - kind-1 only, and not
            // under the player's movement lock (`0x801D2090..0x801D20F8`).
            // Rim Elm's "stand here and press" beats (`town01` P2[12..14]
            // and their `town0b..0e` twins) poll the pad this way.
            if self.world.flags.story_flags & repoll != 0 && !locked {
                self.dispatch_kind1_walk_on(tile, on_world_map, false);
            }
            return;
        }
        self.last_trigger_tile = Some(tile);
        // A crossing drops the re-poll bit before anything else
        // (`0x801D2110..0x801D2120`), so a poll never outlives its tile.
        self.world.flags.story_flags &= !repoll;
        // A crossing made while a script holds the player is spent: retail
        // tests the player's movement-disabled bit `+0x10 & 0x80000`
        // (`0x801D214C..0x801D2158`), and its failure branch (`0x801D226C`)
        // stores the new tile as "last" with no lookup - neither the kind-1
        // record nor the kind-0 teleport runs.
        // The field VM's context runner raises that bit on every frame it
        // steps a spawned record (`FUN_80039B7C`), so a concurrent helper
        // that moves the player (`A3 F8 ..`) never trips the triggers on the
        // tiles it moves the player across. `taiku` P2[16], Zora Castle's
        // post-boss cutscene, seats the player on P2[15]'s walk-on tile;
        // dispatching there re-raised `0x393`, and P1[0] re-spawned P2[16]
        // forever. (A modal timeline or conversation is consumed the same
        // way, earlier in this dispatcher.)
        if locked || self.world.script_context_engages_player() {
            return;
        }
        self.dispatch_kind1_walk_on(tile, on_world_map, true);
        // Retail runs the kind-0 arm on the SAME crossing, after the kind-1
        // spawn (`FUN_801D1EC4` falls through to `0x801d21c0`), so a tile can
        // both spawn its record and teleport.
        if !on_world_map {
            self.dispatch_intra_scene_teleport(tile);
        }
    }

    /// The kind-1 arm of the tile-crossing dispatch: a gate-1 trigger spawns
    /// its partition-2 record.
    // REF: FUN_801D1EC4, FUN_8003BDE0
    ///
    /// `clip_reset` is the changed-tile arm's store under `_DAT_8007B6A8`;
    /// the same-tile re-run and the warp landing call `FUN_8003BDE0`
    /// without it.
    fn dispatch_kind1_walk_on(&mut self, tile: (u8, u8), on_world_map: bool, clip_reset: bool) {
        let (primary, fallback) = &self.field_triggers;
        let Some(trigger) = legaia_engine_field::field_regions::lookup_tile_trigger(
            primary, fallback, tile.0, tile.1,
        )
        .filter(|t| t.gate == 1) else {
            return;
        };
        // Under `_DAT_8007B6A8` a kind-1 hit also resets the clip base to
        // idle (`0x801D218C..0x801D21BC`).
        if clip_reset {
            self.world.field_walk_on_clip_reset();
        }
        let Some(man_bytes) = self.field_man_cache.clone() else {
            return;
        };
        let Ok(man_file) = legaia_scene_formats::man_section::parse(&man_bytes) else {
            return;
        };
        // On the overworld, a gate-1 trigger whose record is a **portal**
        // (carries a `0x3F` named scene-change) belongs to the world-map entity
        // SM (`OverworldPortal`), which already gates + engages it on walk-onto.
        // Dispatching it here too would double-handle the hop (and would let a
        // story-gated-out entrance fall through to a raw timeline scene-change).
        // Only spawn gate-1 **beat** records - the mist-wall force-walk bands -
        // as a cutscene timeline on the overworld.
        if on_world_map && Self::p2_record_is_portal(&man_file, &man_bytes, trigger.record as usize)
        {
            return;
        }
        if self
            .world
            .install_gated_p2_record(&man_file, &man_bytes, trigger.record as usize)
        {
            log::info!(
                "{}: walk-on trigger at ({},{}) spawned P2[{}]",
                if on_world_map { "world-map" } else { "field" },
                tile.0,
                tile.1,
                trigger.record,
            );
        }
    }

    /// The **kind-0** arm: the `.MAP`'s intra-scene-teleport table - the door
    /// class whose destination is map data, not a MAN script.
    ///
    /// This is how most house **exits** work. The interior is a sub-area of the
    /// same collision grid; the exit is a plain tile just inside the doorway
    /// carrying a kind-0 record, and crossing onto it repositions the player.
    /// No object, no script, no ＩＮ/ＯＵＴ record name - so a MAN-only door
    /// census cannot see it, and an engine that dispatches only the kind-1
    /// table lets the player walk in and never back out.
    ///
    /// The crossing does not move the player. It arms the warp
    /// ([`legaia_engine_core::world::World::arm_field_warp`]): a `0x26`-frame timer and the
    /// fade to black, with the pad held off. The landing - seat, floor
    /// re-sample, camera re-pin, the arrival tile's kind-1 record - runs from
    /// [`Self::dispatch_walk_on_trigger`] the frame the timer runs out.
    ///
    /// PORT: FUN_801D1EC4 (kind-0 arm, `0x801d21c0..0x801d2268`)
    fn dispatch_intra_scene_teleport(&mut self, tile: (u8, u8)) {
        let (primary, fallback) = &self.field_intra_teleports;
        let Some(tp) = legaia_engine_field::field_regions::lookup_intra_scene_teleport(
            primary, fallback, tile.0, tile.1,
        ) else {
            return;
        };
        let Some(slot) = self.world.player_actor_slot else {
            return;
        };
        let Some(actor) = self.world.actors.get(slot as usize) else {
            return;
        };
        // Retail skips the teleport while the player's movement-disabled flag
        // (`+0x10 & 0x80000`) is set - an encounter / cutscene owns the body.
        if actor.move_state.flags & 0x0008_0000 != 0 {
            return;
        }
        self.world.arm_field_warp((tp.dest_x, tp.dest_z));
        log::info!(
            "field: intra-scene warp armed at ({},{}) -> world {:?} tile {:?}",
            tile.0,
            tile.1,
            tp.dest_world(),
            tp.dest_tile(),
        );
    }
}
