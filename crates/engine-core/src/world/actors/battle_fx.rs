//! Battle actor effects: the per-actor effect-script step, the animation cue
//! walk and its SFX routing, ghost / burn / weapon-trail draws, impact tint and
//! impact FX, the stage ambient, the defeat sink and the tween target.
//! Split out of `actors.rs`; no logic change.

use super::*;

impl World {
    /// Walk actor `i`'s committed effect script for one frame and queue the
    /// resulting spawn requests (drained via
    /// [`World::drain_battle_effect_spawns`]). The engine seat of the retail
    /// per-frame call `FUN_80047430` -> `FUN_801DEA50`: the block is the
    /// committed clip's disc entry head, the cursor persists at
    /// [`Actor::battle_effect_cursor`], the facing comes from the SM's
    /// bearing writes (`BattleActor::facing_angle`), and the move-power map
    /// is the installed [`crate::move_power::MovePowerCatalog`]'s id-index
    /// map when present.
    ///
    /// The terminator's context writes land in [`crate::world::CastFxState::move_fx_streak`] -
    /// the `ctx[+0x1014]` / `+0x6C6` / `+0x1144` block the afterimage streak
    /// projects from ([`crate::action_effect_script::MoveFxStreak`]).
    // REF: FUN_80047430 (the retail caller this substitutes for)
    pub(super) fn step_actor_effect_script(&mut self, i: usize, frame: i16) {
        use crate::action_effect_script as fx;
        let Some(actor) = self.actors.get(i) else {
            return;
        };
        let Some(script) = actor.battle_effect_script.as_ref() else {
            return;
        };
        if actor.battle_effect_cursor >= fx::MAX_CURSOR {
            return;
        }
        let frame = u8::try_from(frame.max(0)).unwrap_or(u8::MAX);
        let script_actor = fx::EffectScriptActor {
            cursor: actor.battle_effect_cursor,
            facing: actor.battle.facing_angle,
            world: (
                i32::from(actor.move_state.world_x),
                i32::from(actor.move_state.world_y),
                i32::from(actor.move_state.world_z),
            ),
            // Retail scales offsets by the render node's mesh-header scale
            // (`actor[+0x22C][+0x72]`); the engine actor carries no render
            // node, so the q12 unit stands in (see `fx::scale_offset`).
            scale: 1 << 12,
            scope: actor.battle.active_target,
            action: actor.battle.params.first().copied().unwrap_or(0),
            suppressed: actor
                .battle
                .flag_bits
                .has(vm::battle_action::ActorFlags::FX_SUPPRESSED),
            // `FUN_801DF570`'s inputs: this actor's live pair and its
            // target's seat pair. A scope byte naming no seated actor (the
            // `8` / `9` whole-side codes) leaves the clamp without a
            // separation to measure.
            approach: self
                .actors
                .get(usize::from(actor.battle.active_target))
                .map(|_| {
                    let (ref_x, ref_z) =
                        self.battle_seat_of(usize::from(actor.battle.active_target));
                    vm::battle_approach::ApproachPose {
                        x: actor.move_state.world_x,
                        z: actor.move_state.world_z,
                        ref_x,
                        ref_z,
                    }
                }),
        };
        // The catalog's map is based at 0x801F4E63 (`map[move_id]`); the
        // stepper's terminator reads the 0x801F4E64-based view (`map[action
        // - 1]`), so skip the first byte - same bytes, reconciled bases.
        let map = self
            .tables
            .move_power
            .as_ref()
            .and_then(|cat| cat.id_index_map_bytes().get(1..))
            .unwrap_or(&[]);
        let step = fx::step_effect_script(
            crate::action_effect_script::retail_rotation_lut(),
            script,
            script_actor,
            frame,
            map,
        );
        let cursor = step.cursor;
        // The table arm's code substitution (`0x801DF054..0x801DF094`): code
        // `0` reads as `9` while the context's **active** actor (`ctx[+0x13]`,
        // not the stepped one) has the dynamic art slot `0x11` committed in
        // `+0x1D9`. Every later read of the code - the `0x801F6418` CLUT map,
        // the `4` / `6` scale arms, the prototype table `0x801F6324` - takes
        // the substituted one, so an art's ray burst (code `9`, the purple
        // pool `9` mesh) replaces the plain swing's (code `0`, the red pool
        // `8` twin) - `battle_melee_hit_spark`'s Somersault.
        // PORT: FUN_801DEA50 (`0x801DF054..0x801DF094`)
        let art_slot_active = self
            .actors
            .get(usize::from(self.battle_ctx.active_actor))
            .is_some_and(|a| a.battle.current_anim == ACTION_FX_ART_SLOT);
        for s in &step.spawns {
            let mut effect = s.effect & !fx::EFFECT_DIRECT_BIT;
            if !s.direct && effect == 0 && art_slot_active {
                effect = ACTION_FX_ART_CODE;
            }
            self.battle
                .effect_spawns
                .push(crate::battle_events::BattleEffectSpawn {
                    actor_slot: i as u8,
                    effect,
                    direct: s.direct,
                    at: s.at,
                    facing: script_actor.facing,
                });
        }
        // Terminator sink: install the staged move-power record's `+0x04`
        // word and the launch position into the move-FX streak block. The
        // record id the terminator resolves indexes the same table
        // `MovePowerCatalog` holds, so the `+0x6C6` word is that record's
        // `counter_init()`.
        if let Some(band) = step.homing_band {
            // `ctx[+0x1014]` is the table base plus `index * 26`: the
            // record by table index, not by move id.
            let record = step
                .move_power_offset
                .map(|off| off / fx::MOVE_POWER_STRIDE);
            let counter = record
                .and_then(|idx| self.tables.move_power.as_ref()?.record_at_index(idx))
                .map(|rec| rec.counter_init());
            self.casting.move_fx_streak.install(&step, counter);
            if let Some(launch) = step.launch {
                self.seed_homing_slots(i, band, record, launch);
            }
        }
        if let Some(actor) = self.actors.get_mut(i) {
            actor.battle_effect_cursor = cursor;
        }
    }

    /// Walk actor `i`'s committed clip's **animation cue track** for one
    /// frame - the engine seat of retail's `FUN_80047430` -> `FUN_800508DC`
    /// call (`0x800478E4` / `0x80047C34`, the same `(slot, entry, frame)`
    /// arguments the effect-script stepper takes). The track is the entry's
    /// `+0x54` `(frame, cue)` run, read off the committed entry head
    /// ([`Actor::battle_effect_script`]); the cursor is
    /// [`Actor::battle_anim_cue_cursor`].
    ///
    /// Every fired cue goes through the battle sound funnel `FUN_8004FE5C`
    /// ([`crate::sfx_cue::route_sfx_cue`]) with the actor's **retail**
    /// actor-table index as the category: the `>= 0x100` party arm starts a
    /// CD-XA clip ([`AudioState::battle_xa_cues`]), everything else lands in
    /// the SFX ring both hosts drain ([`World::take_sfx_ring_ops`]) - a
    /// runtime-bank id (`>= 0x200`) after its `bse.dat` row's `+4` category
    /// has been written. That category is the actor's render-node `+0x80`
    /// byte ([`Self::battle_sound_category`]), the literal `2` for a party
    /// cue `>= 0xA7`.
    ///
    /// This is what an ordinary swing, a footstep, a knockdown and a
    /// monster's attack sound like: none of those sounds is emitted by the
    /// battle-action SM or the melee kernel.
    // REF: FUN_80047430 (the retail caller this substitutes for)
    pub(super) fn step_actor_anim_cues(&mut self, i: usize, frame: i16) {
        use crate::anim_cue::{AnimCueActor, AnimCueEmit, AnimCueSlot, walk_anim_cues};
        let Some(actor) = self.actors.get(i) else {
            return;
        };
        let Some(head) = actor.battle_effect_script.as_ref() else {
            return;
        };
        let track: Vec<AnimCueSlot> = (0..usize::from(crate::anim_cue::ANIM_CUE_TRACK_LEN))
            .map_while(|k| {
                let o = crate::anim_cue::ANIM_CUE_TRACK_OFFSET + k * 4;
                let b = head.get(o..o + 4)?;
                Some(AnimCueSlot {
                    frame: u16::from_le_bytes([b[0], b[1]]),
                    cue: u16::from_le_bytes([b[2], b[3]]),
                })
            })
            .collect();
        if track.first().is_none_or(|s| s.cue == 0) {
            return;
        }
        let cursor = actor.battle_anim_cue_cursor;
        let anim_id = head.get(0x77).copied().unwrap_or(0);
        let category = self.retail_actor_category(i as u8);
        let party = category < 3;
        let char_id = if party {
            self.party_roster_slot(i) as u8 + 1
        } else {
            0
        };
        // Record `+0xF8 & 0x2000` - bit `32 + 13` of the ability bitfield
        // at `+0xF4`.
        let voice_muted = party
            && self
                .party
                .roster
                .members
                .get(self.party_roster_slot(i))
                .is_some_and(|r| r.ability_bits()[5] & 0x20 != 0);
        let cue_actor = AnimCueActor {
            slot: category,
            char_id,
            anim_id,
            voice_muted,
            cd_busy: self.audio.battle_xa_busy_frames > 0,
        };
        let key = u8::try_from(frame.max(0)).unwrap_or(u8::MAX);
        let walk = walk_anim_cues(&cue_actor, &track, cursor, key, &mut || {
            self.next_rand() as i32
        });
        if walk.cursor_committed
            && let Some(a) = self.actors.get_mut(i)
        {
            a.battle_anim_cue_cursor = walk.cursor;
        }
        for emit in walk.emits {
            match emit {
                AnimCueEmit::Route { id, slot } => self.route_battle_cue(id, slot),
                AnimCueEmit::Dispatch { id } => {
                    if let Some(rid) = crate::anim_cue::dispatch_ring_id(id) {
                        self.audio.sfx_ring_ops.push(SfxRingOp::Push(rid as i16));
                    }
                }
                AnimCueEmit::Suppressed { .. } => {}
            }
        }
    }

    /// One call into the battle sound funnel `FUN_8004FE5C(id, category)`
    /// ([`crate::sfx_cue::route_sfx_cue`]), with its two outputs placed where
    /// both hosts read them: the CD-XA leg on
    /// [`AudioState::battle_xa_cues`], the ring id on the SFX ring ops - after
    /// the runtime row's `+4` category write the funnel makes first.
    // REF: FUN_8004FE5C
    pub(in crate::world) fn route_battle_cue(&mut self, id: u16, category: u8) {
        let cats: Vec<u8> = (0..8u8).map(|c| self.battle_sound_category(c)).collect();
        let element_of = |c: u8| cats.get(usize::from(c)).copied().unwrap_or(7);
        let durations = self.audio.xa_cue_durations.as_deref();
        let xa_duration_raw = |n: u32| {
            durations
                .and_then(|t| t.get(n as usize).copied())
                .unwrap_or(0)
        };
        let src = crate::sfx_cue::SfxCueSources {
            element_of: &element_of,
            xa_duration_raw: &xa_duration_raw,
            side_band_streaming: false,
            cd_read_busy: self.audio.battle_xa_busy_frames > 0,
        };
        let mut ring = crate::sfx_cue::SfxCueRing::default();
        let out = crate::sfx_cue::route_sfx_cue(&mut ring, u32::from(id), category, &src);
        if let Some(xa) = out.xa
            && xa.duration_sectors > 0
        {
            self.push_battle_xa_cue(xa);
        }
        if let Some(rid) = out.enqueued {
            if let Some(cat) = out.element_write {
                self.write_battle_sfx_category(rid, cat);
            }
            self.audio.sfx_ring_ops.push(SfxRingOp::Push(rid as i16));
        }
    }

    /// The byte the cue funnel writes into a runtime row's `+4` category for
    /// a cue fired by **retail** actor-table index `category` - the actor's
    /// render node `+0x80` byte (`*(0x801C9370[cat] + 0x22C) + 0x80`). It is
    /// a VAB slot, not an element: battle init `FUN_800513F0` seeds every
    /// node with `7` (`0x80051548`), and the battle scene loader
    /// `FUN_800520F0` re-seeds the monsters - `7` for every present monster
    /// (`0x8005225C`, slot 7 then takes `monster.snd` bank `min id - 1`),
    /// then `8` for the monsters carrying the largest id when the formation
    /// mixes ids (`0x8005234C`, slot 8 takes bank `max id - 1`). Slots 7 / 8
    /// are the battle's two `monster.snd` banks (`docs/formats/sfx-table.md`).
    // REF: FUN_800513F0, FUN_800520F0
    /// The `monster.snd` banks this battle opens, as `(VAB slot, bank
    /// index)`: slot `7` takes bank `min id - 1`, and - when the formation
    /// mixes monster ids - slot `8` takes bank `max id - 1`. Empty outside
    /// battle or with no monster seated. Both hosts stage these while the
    /// battle is on screen, which is what makes the monster-side runtime cues
    /// ([`Self::battle_sound_category`]) audible.
    ///
    /// Read off the battle scene loader `FUN_800520F0`: the first pass
    /// (`0x80052218..0x80052288`) keeps the smallest non-zero id of the four
    /// `DAT_8007BD0C` bytes and calls `FUN_8003E104(min - 1, 7, ..)`
    /// (`0x80052288..0x80052294`); the second (`0x800522B8..0x80052308`)
    /// keeps the largest and counts ids that differ from it on the way, and
    /// only a non-zero count reaches `FUN_8003E104(max - 1, 8, ..)`
    /// (`0x80052360..0x8005236C`). `FUN_8003E104` indexes the archive's
    /// sector table with that bank number directly.
    // REF: FUN_800520F0, FUN_8003E104
    pub fn battle_monster_sound_banks(&self) -> Vec<(u8, u16)> {
        let ids: Vec<u16> = self
            .battle_monster_slots()
            .into_iter()
            .map(|(_, id, _)| id)
            .filter(|&id| id != 0)
            .collect();
        let (Some(&min), Some(&max)) = (ids.iter().min(), ids.iter().max()) else {
            return Vec::new();
        };
        let mut out = vec![(7u8, min - 1)];
        if min != max {
            out.push((8, max - 1));
        }
        out
    }

    pub(in crate::world) fn battle_sound_category(&self, category: u8) -> u8 {
        if category < 3 {
            return 7;
        }
        let ids: Vec<u16> = self
            .battle_monster_slots()
            .into_iter()
            .map(|(_, id, _)| id)
            .collect();
        let slot = usize::from(self.engine_slot_of_retail_category(category));
        let Some(id) = self.actors.get(slot).and_then(|a| a.battle_monster_id) else {
            return 7;
        };
        let max = ids.iter().copied().max().unwrap_or(id);
        let mixed = ids.iter().any(|&x| x != ids[0]);
        if mixed && id == max { 8 } else { 7 }
    }

    /// The move-FX streak block the effect script's terminator installs -
    /// retail's `ctx[+0x1014]` / `+0x6C6` / `+0x24E` / `+0x1144` quartet.
    /// The render layer projects the afterimage streak from it; `is_armed()`
    /// is `false` until a terminator has run.
    pub fn move_fx_streak(&self) -> crate::action_effect_script::MoveFxStreak {
        self.casting.move_fx_streak
    }

    /// Plan this frame's arts after-image ghosts - the engine seat of the
    /// retail per-actor walk `FUN_80049348` (see
    /// [`crate::battle_afterimage`]). For every battle actor with a pose
    /// history, sample the two rate-scheduled ring depths, keep the
    /// ghost-eligible ones, and resolve each ghost's flat additive colour
    /// (per-character base from the SCUS `0x80076908` table via the
    /// present-party ordinal; monsters share the `0x80076914` word; each
    /// drawn ghost decays by `0x101010`). Hosts draw each returned pose as
    /// a flat-coloured additive copy of the actor's mesh, behind the live
    /// body (retail pushes the ghost `0x50` OT buckets deeper).
    // REF: FUN_80049348 (the walk; kernel in `crate::battle_afterimage`)
    pub fn battle_ghost_draws(&self) -> Vec<BattleGhostDraw> {
        use crate::battle_afterimage as ai;
        let mut out = Vec::new();
        for (i, actor) in self.actors.iter().enumerate() {
            if actor.battle_pose_history.is_empty() {
                continue;
            }
            let monster = actor.battle_monster_id.is_some();
            let base = if monster {
                ai::GHOST_COLOR_MONSTER
            } else {
                *ai::GHOST_COLOR_PARTY
                    .get(self.party_roster_slot(i))
                    .unwrap_or(&ai::GHOST_COLOR_MONSTER)
            };
            let hist = &actor.battle_pose_history;
            let plans = ai::plan_ghosts(actor.battle.anim_rate.get(), monster, base, |depth| {
                hist.get(depth.saturating_sub(1))
                    .map(|f| f.ghost_eligible)
                    .unwrap_or(false)
            });
            for p in plans {
                let Some(f) = hist.get(p.depth.saturating_sub(1)) else {
                    continue;
                };
                out.push(BattleGhostDraw {
                    actor_slot: i as u8,
                    pos: f.pos,
                    pose: f.pose.clone(),
                    color: p.color,
                });
            }
        }
        out
    }

    /// Arm the retail **impact tint triple** on `target`: `+0x04 = impact
    /// table[selector - 1]`, `+0x21F = selector`, `+0x0C = 0x1000` - the
    /// writes both impact arms perform when a hit lands. The melee / arts
    /// routine `FUN_801EC3E4` (`0x801EE3D4..0x801EE43C`) reads the selector
    /// off the acting actor's action record `+0x7A`
    /// ([`crate::battle_anim::MonsterAnimPlayer::impact_class`]) and gates it
    /// `0 < sel < 6` (`sltiu v0,v0,0x6` at `0x801EE3E0` - a class past the
    /// table skips all three writes; callers apply that gate, see
    /// [`crate::move_power::IMPACT_CLASS_LIMIT`]). The monster special-attack arm
    /// `FUN_801E09F8` (`0x801E15AC..0x801E15EC`, at each arm's impact phase)
    /// reads the move-power record's `+0x0A` and stamps unguarded (its
    /// selector ladder ends at `5`, so that byte never carries a `6`); a
    /// zero selector arms nothing in either (`beq v0,zero` past the writes).
    /// The `+0x7A` byte is the hit routine's whole **status / impact
    /// selector**, and the disc does carry `6` on it (the melee-path Curse
    /// arm at `0x801EE690`: a 1-in-4 `+0x16E |= 0x1000` roll and no tint) -
    /// that is what the `sltiu` gate is for. A selector past the table
    /// with no gate leaves the colour word alone here.
    ///
    /// The tint then decays through [`Self::tick_battle_impact_fx`]'s
    /// presentation SM (`FUN_80050120` arm 0): colour eases to neutral, the
    /// blend drains, the selector retires. Retail captures of the armed
    /// state: `battle_gimard_tail_fire_a` / `_b` (Vahn at `+0x21F = 1`,
    /// `+0x0C = 0x1000`, the red word eight lane-units apart between the two
    /// frames).
    // PORT: FUN_801EC3E4 (the `+0x7A` impact-tint arm only; the damage /
    // status body is the battle loop's)
    // REF: FUN_801E09F8 (the move-power sibling arm - same three writes)
    pub fn arm_impact_tint(&mut self, target: usize, selector: u8) {
        if selector == 0 {
            return;
        }
        let word = self
            .tables
            .move_power
            .as_ref()
            .and_then(|t| t.impact_table())
            .and_then(|t| t.get(usize::from(selector - 1)).copied());
        let Some(a) = self.actors.get_mut(target) else {
            return;
        };
        if let Some(word) = word {
            a.battle.render_color = word;
        }
        a.battle.impact_state = selector;
        a.battle.render_blend = vm::battle_formulas::TINT_BLEND_FULL;
    }

    /// Per-frame **battle ambient ramp**: the trailing block of
    /// `FUN_80050120` (`0x800505B0..0x8005083C`). The packed base
    /// `ctx[+0x890]` steps down while the summon close-up holds
    /// `ctx[+0x243]` and back up while it is clear
    /// (`legaia_engine_vm::battle_ground_grid::ambient_base_step`), and the
    /// pass stores the base the grid's near and far colours derive from
    /// unless it skips on the floor. One call per vsync, so `dt = 1`.
    // PORT: FUN_80050120 (the trailing ambient / far-colour block; the
    // backdrop pair's `+0x78` / `+0x56` ramp is not modelled)
    pub(super) fn tick_battle_ambient(&mut self) {
        use vm::battle_ground_grid as grid;
        if self.mode != SceneMode::Battle {
            return;
        }
        let ctx = &mut self.battle_ctx;
        ctx.ambient_base = grid::ambient_base_step(ctx.ambient_base, ctx.gauge_rearm_latch != 0, 1);
        let rgb = grid::ambient_base_rgb(ctx.ambient_base);
        // `ctx[+0x278]` is one retail byte the port carries in two places:
        // the summon band's own `summon_staging_a` (set at `0x32 -> 0x33`,
        // `0x801E49F8`; cleared at the `0x34` exit) and the slot-B module's
        // scratch copy from `0x35` on. The band's `1` is what freezes the
        // ambient once the base reaches the floor through `0x33` / `0x34`.
        let ctx_278 = ctx.summon_staging_a | self.casting.module_ctx_278;
        // The backdrop pair's own ramp runs ahead of the ambient one in the
        // same pass (`0x80050600..0x80050714`), on the same two bytes.
        self.battle.backdrop_cue = grid::backdrop_cue_step(
            self.battle.backdrop_cue,
            ctx.gauge_rearm_latch,
            ctx_278,
            self.battle.stage_outdoor,
            1,
        );
        if !grid::ambient_store_skipped(rgb, ctx.gauge_rearm_latch, ctx_278) {
            self.battle.ambient_stored = rgb;
        }
    }

    /// The backdrop pair's depth-cue weight this frame, `1.0 = 0x1000`, or
    /// `None` when `FUN_80050120` has taken the pair off the draw (a weight
    /// of exactly `0x1000` switches `+0x56` to `0`, `0x80050850..0x80050880`).
    /// The pull is toward black: the records' colour word `+0x74` is `0`.
    /// Both battle hosts cue their backdrop draw with it.
    pub fn battle_backdrop_cue(&self) -> Option<f32> {
        let w = self.battle.backdrop_cue;
        (w != vm::battle_ground_grid::BACKDROP_CUE_FULL).then(|| f32::from(w) / 4096.0)
    }

    /// The battle ambient base the ground grid is coloured from this frame,
    /// 8 bits a channel: near colour `battle_ground_grid::battle_ambient_colour`
    /// of it, far colour `battle_ground_grid::grid_far_colour` of it. Settles
    /// on `0x80` and dims toward `0x20` through a summon close-up.
    pub fn battle_ambient_base(&self) -> [u8; 3] {
        self.battle.ambient_stored
    }

    /// Per-frame **presentation tint + impact freeze** maintenance: the
    /// per-actor arms of `FUN_80050120` (kernel
    /// `legaia_engine_vm::battle_formulas::tint_sm_step`) followed by
    /// `FUN_8004CE2C` pass 2's per-clip impact arms (kernel
    /// `legaia_engine_vm::battle_impact_fx`).
    ///
    /// The SM runs on every seated battle actor and dispatches on the
    /// render flag `+0x21C` exactly as retail's jump table does: `0` eases
    /// the colour word to neutral, then drains the `+0x0C` blend, then
    /// retires the `+0x21F` selector (one phase per frame, in that order);
    /// `1`/`3`/`4`/`6..=10` ease toward their fixed colours with the full
    /// blend stamped; `2` runs the defeat / capture fade; `5` and every
    /// out-of-table value (the cursor's `200`, the summon-hide `0xFF`)
    /// leave the words alone. The ease is unconditional on flag `0` -
    /// retail has no "was it armed" test, so a colour word any writer left
    /// off-neutral (the retired target cursor's dim word included) eases
    /// back at the same rate.
    ///
    /// Runs before the per-clip arms so an in-window arm's rewrite owns the
    /// frame - the retail order (`FUN_80046A20` calls `FUN_8004CE2C`, then
    /// the presentation tick, then the next frame's arms re-stamp).
    // PORT: FUN_8004CE2C (pass 2 - per-clip impact arms; pass 4 is
    // `crate::battle_status_clut`)
    // REF: FUN_80050120 (the per-actor arms, ported as `tint_sm_step`; this is
    // the per-frame walk over the actor table that drives them)
    /// The defeat fade's **sink** (`FUN_80050120` arm 2,
    /// `0x80050360..0x800504A8`): a monster seat (`3..=6`) fading out on
    /// render flag `2` sinks into the stage floor, `+0x36 += (size_class *
    /// dt) >> 2` a battle frame - `size_class` the record's `+0x1F` - while
    /// its node colour (`node[+0x74]`; the port reads its fading colour
    /// word) is non-zero. Gated off by a Seru absorb staged for the action
    /// (`ctx[+0x269]`, which raises the body for the absorb instead), a
    /// captured actor (`+0x225`) and a scripted fight (`ctx[+0x287]`) whose
    /// formation carries no second monster (`gp+0x9F5` = `0x8007BD0D` zero;
    /// the port reads "more than one monster seated").
    ///
    /// That last gate is also the one that raises the **lone-monster defeat
    /// latch** `ctx[+0x288]` (`sb s4,0x288(v1)`, `s4 = 1`, at `0x800504E8`):
    /// the scripted lone monster dies in place instead of sinking, and the
    /// latch is what lets the action SM's state-`0x20` reaction hold out
    /// without waiting for its fade (`BattleActionCtx::lone_defeat_latch`).
    ///
    /// The post-strike death re-frame forks on the height this moves
    /// (`target[+0x36] != 0` takes the ramped shot):
    /// `player_steal_skeleton_banner` reads its killed skeleton `183` down.
    ///
    /// PORT: FUN_80050120 (arm 2's monster sink and its `ctx[+0x288]` latch)
    pub(super) fn tick_battle_defeat_sink(&mut self) {
        if self.battle_ctx.multi_cast_gate != 0 {
            return;
        }
        let lone_scripted =
            self.battle_ctx.scripted_fight != 0 && self.battle_monster_slots().len() <= 1;
        let first = self.party.party_count as usize;
        for slot in first..(first + 4).min(self.actors.len()) {
            let a = &self.actors[slot];
            if !a.active
                || a.battle_monster_id.is_none()
                || a.battle.render_flag != vm::battle_formulas::STATE_DEFEAT_FADE
                || a.battle.capture_state != 0
                || a.battle.render_color & 0x00FF_FFFF == 0
            {
                continue;
            }
            if lone_scripted {
                // `0x80050454..0x8005045C` skips the sink; `0x800504BC..
                // 0x800504E8` raises the latch.
                self.battle_ctx.lone_defeat_latch = 1;
                continue;
            }
            // `(size * dt) >> 2` a battle frame of `dt` vsyncs; the engine
            // ticks once a vsync.
            let sink = i16::from(self.battle_size_class_of(slot as u8)) >> 2;
            let ms = &mut self.actors[slot].move_state;
            ms.world_y = ms.world_y.wrapping_add(sink);
        }
    }

    /// The burning-body emitter at the tail of the anim decode
    /// `FUN_8004998C` (`0x8004A5FC..0x8004A8D8`): every body with a non-zero
    /// `+0x21F` selector spawns one effect-pool sprite per `0x10` of the
    /// frame's accumulator `ctx[+0x328]`, at a random object of its current
    /// pose jittered by its size - the fire a red selector-`1` body sheds
    /// ([`vm::battle_impact_fx::burn_effect`]): PROT 0903's arm-9 Gimard
    /// (`gimard_burning_attack` holds two of its effect-`0x0B` puffs at the
    /// creature's mouth) and a Tail-Fire-struck party member.
    ///
    /// The accumulator is the frame driver's: low nibble kept, `+8` a frame
    /// (`FUN_80046A20`, `0x8004713C..0x80047160`, `DAT_1F800393 << 3` at one
    /// step a tick).
    ///
    /// Not ported: selector `2`'s screen-shake globals (`_DAT_8007B92C` /
    /// `_DAT_8007B930`, `gp+0xA30..0xA34`, `0x8004A838..0x8004A8BC`).
    ///
    /// PORT: FUN_8004998C (`0x8004A5FC..0x8004A8D8`, the selector emit loop)
    pub(in crate::world) fn emit_battle_burn_sprites(&mut self) {
        use crate::action_effect_script::RotationLut;
        use vm::battle_impact_fx as ifx;
        let acc = (self.battle.burn_emit_accum & 0xF) + 8;
        self.battle.burn_emit_accum = acc;
        let emits = acc / ifx::BURN_EMIT_QUANTUM;
        if emits == 0 {
            return;
        }
        for i in 0..self.actors.len() {
            let a = &self.actors[i];
            let selector = a.battle.impact_state;
            if !a.active || selector == 0 {
                continue;
            }
            let Some(objects) = a
                .pose_frame
                .as_ref()
                .map(|p| p.bone_outputs.iter().map(|(t, _)| *t).collect::<Vec<_>>())
                .filter(|o| !o.is_empty())
            else {
                continue;
            };
            let Some(plan) = self.battle_actor_draw_plan(i, None, 4.0, false) else {
                continue;
            };
            let a = &self.actors[i];
            let base = [
                a.move_state.world_x,
                a.move_state.world_y,
                a.move_state.world_z,
            ];
            let facing = a.battle.facing_angle;
            let red = plan.tint.colour as u8;
            let lut = crate::action_effect_script::retail_rotation_lut();
            for _ in 0..emits {
                let idx = self.next_rand() as usize % objects.len();
                let rands = [
                    self.next_rand() as i32,
                    self.next_rand() as i32,
                    self.next_rand() as i32,
                ];
                let p = ifx::burn_emit_point(
                    base,
                    facing,
                    objects[idx],
                    plan.radius,
                    rands,
                    |a| lut.b(i32::from(a)),
                    |a| lut.a(i32::from(a)),
                );
                if p[1] <= 0
                    && let Some(fx) = ifx::burn_effect(selector, red)
                {
                    self.try_spawn_effect(fx, p, facing & 0xFFF);
                }
            }
        }
    }

    pub(super) fn tick_battle_impact_fx(&mut self) {
        use vm::battle_formulas::{FadeInputs, TintWords, tint_sm_step};
        use vm::battle_impact_fx as ifx;
        // The tag-`0x67` ribbon is a per-frame call in retail: re-derived
        // below every tick, so it drops the frame the window closes.
        self.battle.clip_ribbon = None;
        if self.mode != SceneMode::Battle {
            return;
        }
        // Whether each actor's reaction channel holds its knockdown entry
        // `+0x1F1` (which is the flinch entry on an actor without one).
        let on_knockdown: Vec<bool> = (0..self.actors.len())
            .map(|i| {
                let entry = self.actors[i].battle_reaction_entry;
                entry.is_some() && entry == self.battle_reaction_map(i).map(|m| m[2])
            })
            .collect();
        for (a, on_knockdown) in self.actors.iter_mut().zip(on_knockdown) {
            // `+0x22C == 0` (no battle record) skips the slot.
            if !a.active {
                continue;
            }
            let party = a.battle_monster_id.is_none();
            let b = &mut a.battle;
            let words = TintWords {
                color: b.render_color,
                blend: b.render_blend,
                selector: b.impact_state,
            };
            // The arm-2 gate compares the committed anim against the cached
            // knockdown entry `+0x1F1`; the engine plays a knockdown through
            // the reaction channel without re-pointing `current_anim`, so
            // the reaction channel's committed entry stands in for that
            // equality.
            let fade = FadeInputs {
                party,
                committed_anim: b.current_anim,
                knockdown_entry: if on_knockdown { b.current_anim } else { 0xFF },
                captured: b.capture_state != 0,
            };
            let (next, fx) = tint_sm_step(b.render_flag, words, fade, 1);
            b.render_color = next.color;
            b.render_blend = next.blend;
            b.impact_state = next.selector;
            if fx.mode_semi_transparent {
                // Arm 2 ORs `0x81000000` into the mode word `+0x08`
                // (`0x80050230..0x80050244`): the fading body draws additive.
                b.flag_word |= 0x8100_0000;
            }
            if fx.party_fade_done {
                // `0x80050344..0x80050354`: state 0, staged anim 0, the
                // `+0x1DC` fade-done bit, blend `0x800` (already in `next`).
                b.render_flag = 0;
                b.queued_anim = 0;
                b.flag_bits =
                    vm::battle_action::ActorFlags(vm::battle_action::ActorFlags::WINDUP_DONE);
            }
        }
        self.tick_battle_defeat_sink();
        self.emit_battle_burn_sprites();
        // The per-clip arms: the acting actor's committed record key + cursor
        // window select the writes onto it and its target.
        let acting = self.battle_ctx.active_actor as usize;
        let Some(actor) = self.actors.get(acting) else {
            return;
        };
        let Some(player) = &actor.battle_animation else {
            return;
        };
        let key = player.attach_key();
        let cursor = player.cursor_sixteenths();
        let target = actor.battle.active_target as usize;
        if actor.battle_monster_id.is_some() {
            // The monster arm (tag `0x3B`, `0x8004D2DC..0x8004D32C`).
            if let Some((own, tgt)) = ifx::monster_render_arm(key, actor.battle.hit_count_bound) {
                if let Some(t) = self.actors.get_mut(target) {
                    t.battle.render_flag = tgt;
                }
                if let Some(a) = self.actors.get_mut(acting) {
                    a.battle.render_flag = own;
                }
            }
            return;
        }
        let char_id = self.party_roster_slot(acting) as u8 + 1;
        let hit_index = actor.battle.input_cursor;
        if let Some(sel) = ifx::clip_impact_acting_selector(char_id, key)
            && let Some(a) = self.actors.get_mut(acting)
        {
            a.battle.impact_state = sel;
        }
        // Vahn's tag-`0x2B` arm writes the acting actor's `+0x21C`.
        if let Some(flag) = ifx::vahn_render_arm(char_id, key, cursor)
            && let Some(a) = self.actors.get_mut(acting)
        {
            a.battle.render_flag = flag;
        }
        // Noa's tag-`0x29` / `0x2D` status arm.
        let first_monster = self
            .battle_monster_slots()
            .into_iter()
            .find(|&(_, _, slot)| slot == 0)
            .map_or(0, |(_, id, _)| id as u8);
        let scripted = self.battle.scripted_fight;
        if ifx::noa_status_arm(char_id, key, hit_index, scripted, first_monster, || {
            self.next_rand()
        }) && let Some(t) = self.actors.get_mut(target)
        {
            t.battle.field_flags |= ifx::NOA_STATUS_BITS;
        }
        let Some(w) = ifx::clip_impact(char_id, key, cursor) else {
            return;
        };
        // `w.effect_at_target` is tag `0x67`'s per-frame
        // `FUN_801E1D98(&target[+0x3C], 0xC)` (`addiu a0,s1,0x3c` in the
        // delay slot at `0x8004D220`, `li a1,0xc` at `0x8004D224`): the
        // chained streak ribbon anchored on the target's seat vector.
        // `+0x3C..+0x43` is the spawn node's seat copied verbatim by the
        // battle setup (`0x8005158C..0x80051598`); the engine keeps its
        // `x`/`z` as `BattleActor::seat` and every retail seat row has
        // `y = 0` (`crate::battle_seats`), so the seat's Y is `0`.
        if w.effect_at_target
            && let Some(t) = self.actors.get(target)
        {
            let (sx, sz) = t
                .battle
                .seat
                .unwrap_or((t.move_state.world_x, t.move_state.world_z));
            self.battle.clip_ribbon = Some(super::ClipRibbon {
                seat: [sx, 0, sz],
                trail_id: ifx::GALA_EFFECT_ARG,
            });
        }
        let tint = self
            .tables
            .move_power
            .as_ref()
            .and_then(|t| t.impact_table())
            .map(|t| t[usize::from(w.impact_selector - 1)]);
        let Some(t) = self.actors.get_mut(target) else {
            return;
        };
        if w.freeze_target {
            t.battle.anim_rate = vm::battle_anim_rate::AnimRate(vm::battle_anim_rate::RATE_FROZEN);
        }
        if let Some(word) = tint {
            t.battle.render_color = word;
        }
        // The selector + full blend arm even without disc data (the freeze
        // is data-free; the tint word just has nothing to carry). Every tint
        // arm stamps `+0x0C = 0x1000` (`sw v0,0xc(s1)` at `0x8004D180` /
        // `0x8004D1DC` / `0x8004D234` / `0x8004D294`).
        t.battle.impact_state = w.impact_selector;
        t.battle.render_blend = vm::battle_formulas::TINT_BLEND_FULL;
    }

    /// The acting actor's impact-effect class for a landing hit - the
    /// `+0x7A` byte of its committed action record, read off the playing
    /// clip; `0` when nothing is playing (a synthetic battle) or the clip
    /// carries no class.
    pub(in crate::world) fn attacker_impact_class(&self, attacker: usize) -> u8 {
        self.actors
            .get(attacker)
            .and_then(|a| a.battle_animation.as_ref())
            .map(|p| p.impact_class())
            .unwrap_or(0)
    }

    /// Plan this frame's weapon-trail sweeps - the engine seat of the
    /// retail trigger `FUN_8005112C` + sweep driver `FUN_80048310`
    /// (kernels in `legaia_engine_vm::battle_trail`; geometry emission in
    /// `legaia_engine_ui::battle_trail`).
    ///
    /// For each party battle actor whose committed clip's `+0x77`
    /// identity byte matches its character's trigger row, sample the pose
    /// ring at even depths (one sweep step per two frames - the retail
    /// `2 * rate` cursor rewind under the per-frame `rate` advance),
    /// stopping at the clip boundary (ring `clip_key` mismatch), the
    /// 16-step budget, or a pose without the trigger's control points.
    /// A sweep below two steps draws nothing (`FUN_80048310`'s
    /// `slti 0x2` gate).
    // PORT: FUN_8005112C (trigger; table in `engine-vm::battle_trail`)
    // REF: FUN_80048310 (sweep capture - the ring sampling here replaces
    // the rewound re-decode; see `engine-vm::battle_trail` module docs)
    pub fn battle_weapon_trail_draws(&self) -> Vec<BattleWeaponTrailDraw> {
        use vm::battle_trail as wt;
        let mut out = Vec::new();
        for (i, actor) in self.actors.iter().enumerate() {
            // Party-only: retail's trigger gates on seat < 3; the engine
            // keys party-ness on identity, not slot (monsters may sit
            // anywhere - the slot-gate trap).
            if actor.battle_monster_id.is_some() {
                continue;
            }
            let Some(player) = &actor.battle_animation else {
                continue;
            };
            let key = player.attach_key();
            // Retail char id space: `DAT_8007BD10[seat]` = roster ordinal
            // + 1 (1 = Vahn, 2 = Noa, 3 = Gala).
            let char_id = self.party_roster_slot(i) as u8 + 1;
            let Some(trig) = wt::trail_trigger(char_id, key) else {
                continue;
            };
            let hist = &actor.battle_pose_history;
            let mut steps = Vec::new();
            'sweep: for k in 0..wt::MAX_SWEEP_STEPS {
                let Some(f) = hist.get(k * wt::SWEEP_FRAMES_PER_STEP) else {
                    break;
                };
                if f.clip_key != key {
                    break;
                }
                let mut pts = [[0i16; 3]; wt::TRAIL_POINTS];
                for (p, pt) in pts.iter_mut().enumerate() {
                    match f.pose.bone_outputs.get(trig.base_part + p) {
                        Some((t, _rot)) => *pt = *t,
                        None => break 'sweep,
                    }
                }
                steps.push(pts);
            }
            if steps.len() >= 2 {
                out.push(BattleWeaponTrailDraw {
                    actor_slot: i as u8,
                    steps,
                    rgb: trig.rgb,
                });
            }
        }
        out
    }

    /// Commit every actor's staged battle anim id (`queued_anim` vs
    /// `current_anim`) through the retail anim-commit ladder. Engine port of
    /// the per-frame consumer that converges `+0x1D9` toward `+0x1DA`:
    ///
    /// - staged `0` converges and resumes the idle loop;
    /// - staged `q < 0x10` plays action-table entry `q` directly (the
    ///   equipment-spliced weapon swings live at `0xC..0xF`); `1` (the
    ///   walk/approach) loops, everything else plays one-shot;
    /// - staged `q >= 0x10` on an actor carrying an art bank materializes
    ///   bank record `q - 0x10` into dynamic slot `0x10`/`0x11` (ids `0x10`
    ///   and `0x1A` install at `0x11`) and **rewrites the staged id to the
    ///   slot number** - `legaia_engine_vm::anim_vm::resolve_staged_anim`;
    ///   without a bank (monsters) the id is a plain entry index;
    /// - an actor with no usable clip converges immediately and clears
    ///   `ADVANCE_DONE` (a zero-length swing), so clip-less hosts keep the
    ///   pre-animation pacing.
    ///
    /// Idempotent per frame (a converged pair is a no-op). Called by
    /// [`Self::step_battle`] (pre-step) and [`Self::tick_battle_animations`].
    // PORT: FUN_8004AD80 (staged-anim commit; the id -> slot/record ladder
    // lives in `legaia_engine_vm::anim_vm::resolve_staged_anim`).
    /// The clip the decoder tweens `slot`'s last frame into - the port of
    /// the next-entry rule's queued arm (`FUN_8004998C`
    /// `0x80049A7C..0x80049BD0`). Retail reads `+0x1DA`; the engine keeps
    /// that byte on two channels, so this resolves the entry the engine will
    /// actually install at the natural end: the reaction channel's staged
    /// entry (idle when `+0x1DC` bit 2 is up or nothing is staged), a byte
    /// staged behind a playing swing, the swing itself on a re-commit, and
    /// otherwise the idle a finished one-shot falls back to. A looping clip
    /// re-queues itself.
    ///
    /// The gate is retail's: HP `+0x14C` non-zero and the queued id below
    /// `0x10`, else the last frame blends toward itself with no Z term. A
    /// monster whose queued stream has a different part count also blends
    /// toward itself, but keeps the Z term (`0x80049B9C` branches into the
    /// `+0xE` arm with `a1 = t0`). The Z term is the **committed** entry's
    /// `+0x0E`; the `+0x228` byte that suppresses it has no store in the
    /// dump corpus and is taken as clear.
    // PORT: FUN_8004998C (`0x80049A7C..0x80049BD0`, the queued-clip arm of
    // the next-entry rule)
    pub(in crate::world) fn battle_tween_target(
        &self,
        slot: usize,
    ) -> Option<crate::battle_anim::TweenTarget> {
        use crate::battle_anim::TweenTarget;
        use vm::battle_action::ActorFlags;
        let a = self.actors.get(slot)?;
        let player = a.battle_animation.as_ref()?;
        let queued: u8 = if a.battle_reaction.is_some() {
            if a.battle.flag_bits.has(ActorFlags::EXIT) {
                0
            } else {
                a.battle_reaction_next.unwrap_or(0)
            }
        } else if let Some(id) = a.battle_staged_anim {
            if player.is_looping() || a.battle.queued_anim != id {
                a.battle.queued_anim
            } else if a.battle.flag_bits.has(ActorFlags::ADVANCE_DONE) {
                id
            } else {
                0
            }
        } else if player.is_looping() {
            a.battle.current_anim
        } else {
            0
        };
        if a.battle.hp == 0 || queued >= 0x10 {
            return Some(TweenTarget {
                frame0: None,
                z_bias: 0,
            });
        }
        let z_bias = player.end_root_step();
        let frame0 = if player.is_looping() && queued == a.battle.current_anim {
            Some(player.first_frame().to_vec())
        } else {
            a.battle_action_clips
                .as_ref()
                .and_then(|cl| cl.get(usize::from(queued)))
                .and_then(|c| c.as_ref())
                .and_then(|c| c.frames.first().cloned())
        };
        // Retail's monster arm checks the queued stream's part count on the
        // non-idle path only; the player also refuses a mismatched frame
        // for either seat, which a party table never produces.
        let frame0 = frame0.filter(|f| f.len() == player.part_count());
        Some(TweenTarget { frame0, z_bias })
    }
}
