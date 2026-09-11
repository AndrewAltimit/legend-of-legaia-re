//! The engine half of the action SM's **cast band**: who owes a cast's
//! outcome, when it lands, and the player-summon stager that stands in for
//! the per-summon overlay.
//!
//! ## Where retail folds a cast
//!
//! States `0x28..=0x2E` and `0x32..=0x38` of `FUN_801E295C` apply no damage
//! (the only `jal func_0x800402F4` in the dispatcher is the attack band's).
//! The magic band faces the caster, raises the monster-only spell-name
//! label, debits MP (`0x28`), and after the `0x14`-frame wait (`0x29`) runs
//! the party trigger `FUN_801DBF9C` and stages the cast's first anim byte;
//! the *outcome* is the streamed module's (`FUN_8003EC70` pages it in) and
//! lands while the clips play - for a Seru cast, inside the per-summon
//! stager the summon band ticks through `FUN_801F1ED4`.
//!
//! The engine carries the same shape with one owner, [`PendingCast`]: the
//! menu confirm / monster pick arm the SM and park the resolved targets
//! here; the fold runs once, at retail's seam - the stager's strike for a
//! summon, the `0x29` exit for everything else - through
//! [`World::cast_spell_on_slots_prepaid`], because the band's own `0x28` has
//! already charged the MP. A cast that leaves the band by any other door
//! (a capture branch, a dead caster) is still folded once, at the band's end,
//! so no turn spends MP for nothing.
//!
//! ## The stager
//!
//! Retail's summon band calls `FUN_801F1ED4` at `0x34` entry, every frame
//! of `0x35`, and at `0x36`, where it **holds while the call returns
//! non-zero** (`bne v0,zero` at `0x801E4CB0`). That routine dispatches into
//! the streamed per-summon stager (extraction PROT `903..=934`, slot B
//! `0x801F69D8`): overlay MIPS code the engine cannot run. [`SummonStager`]
//! is the engine's own choreography behind the same seam, and it reports
//! "busy" exactly where retail reads the stager's return.
//!
//! What it stages is capture-pinned on the player Gimard cast
//! (`gimard_summon_start` / `_visible` / `_burning_attack`, read out of the
//! PCSX-Redux states' RAM - `ctx[+7]`, `ctx[+0x279]`, the actor table):
//!
//! * `0x33` - the caster on clip `9`, the summon seat (slot 7) empty.
//! * `0x36`, stager phase 6 - every party seat and living monster hidden
//!   (`+0x21C = 0xFF`, prim word `0`), the creature seated at slot 7 about
//!   [`SUMMON_SPAWN_BEHIND`] units **behind** the caster on the party side
//!   (`x=185, z=-2272` against the caster's `x=82, z=-542`), wearing the
//!   caster's facing (`0xFD9`), on its idle clip.
//! * `0x36`, stager phase 11 - the creature closer in (`z=-1606`,
//!   [`SUMMON_STRIKE_BEHIND`] behind the caster) on clip `1`, the walk, with
//!   the flame part-actors live and the damage numeral up.
//!
//! So the retail creature walks **in from behind the party** toward the
//! target while its effect parts play, and the damage lands mid-walk. The
//! stager here does the same with the pieces the engine has: it requests the
//! namesake creature spawn ([`World::pending_summon_spawn`]) at the spawn
//! point, idles it, stages the walk clip and glides it to the strike point,
//! folds the outcome there, lingers, and despawns it.
//!
//! The per-summon effect parts are staged with it. They used to be the open
//! half here ("the `0x180C` move-VM records are not staged"); the module's
//! spawn records now come out of the **cast-effect pool**
//! ([`legaia_asset::cast_effect_pool`], installed by the scene host) and stage
//! through [`World::spawn_cast_module_fx`] at the same first tick, because the
//! records are the shape the summon path already runs. What stays open is the
//! module's *code* half - lift, camera, phase machine, damage shape - and that
//! function's `NOT WIRED:` names the worklist rows it covers.
//!
//! Timing is the engine's: the retail durations are the stager's own phase
//! machine and are not dumped, so the frame counts below are chosen to land
//! the strike inside the band's `0x78`-frame sustain, the way the capture
//! shows it.

use super::*;

use vm::battle_action::{ActionCategory, ActionState, StepOutcome};

/// How far behind the caster (toward negative Z, the party side) the
/// creature is seated - the capture's `-2272 - (-542)`.
pub const SUMMON_SPAWN_BEHIND: i16 = 1730;
/// Where the walk ends and the outcome lands - the capture's
/// `-1606 - (-542)`.
pub const SUMMON_STRIKE_BEHIND: i16 = 1064;
/// Frames the creature idles at its spawn point before the walk.
const SUMMON_IDLE_FRAMES: u16 = 30;
/// Walk speed, world units per frame.
const SUMMON_WALK_STEP: i16 = 12;
/// Frames the creature stands at the strike point after the outcome.
const SUMMON_LINGER_FRAMES: u16 = 40;
/// A host with no creature to seat (a headless driver) still owes the
/// outcome: fold it after this many frames without a seat.
const SUMMON_UNSEATED_GRACE: u16 = 60;

/// The arm of PROT 0927's and PROT 0966's nine that carries the sweep - the
/// `0x801F6A60` / `0x801F6A50` table's working entry. The other eight are
/// spawn arms the pool already stages.
pub const AOE_STAGER_WORKING_ARM: u8 = 4;

/// Combat seats in retail's actor table `DAT_801C9370` - three party rows and
/// five monster rows. The slot-B AoE sweeps' `ctx[+0]` / `ctx[+1]` bounds are
/// indices into that span, so the engine's own extra seats (the summon
/// creature at 7 and up, host debug actors) are outside both.
pub const BATTLE_TABLE_SLOTS: usize = 8;

/// The actor's own halfword, or the world's per-slot mirror when the actor
/// has not carried one yet. Used to seed the stat block the slot-B kernels
/// debuff: `seed_party_battle_stats` fills the mirrors, not the actor, so a
/// zero here would have Melt Spray compute on nothing.
fn nonzero_or(own: u16, mirror: Option<u16>) -> u16 {
    if own != 0 { own } else { mirror.unwrap_or(0) }
}

/// A cast the action SM is carrying whose outcome is still owed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingCast {
    /// Casting actor slot (party or monster).
    pub caster: u8,
    /// The spell id (`actor[+0x1DF]`).
    pub spell_id: u8,
    /// Absolute actor slots the outcome folds onto.
    pub targets: Vec<u8>,
}

/// Where the stager is in its choreography.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SummonPhase {
    /// Armed by the cast trigger (`0x29`); the first tick (`0x34` entry)
    /// requests the creature spawn.
    Armed,
    /// Creature requested / seated: idle, then walk in to the strike point.
    Approach,
    /// Outcome folded; the creature holds at the strike point.
    Linger,
    /// Creature despawned; the next tick reports not busy.
    Done,
}

/// One player summon in flight (see the module docs).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SummonStager {
    /// Casting party slot.
    pub caster: u8,
    /// The spell id (`actor[+0x1DF]`).
    pub spell_id: u8,
    pub phase: SummonPhase,
    frames: u16,
    /// Where the creature is seated (behind the caster).
    pub spawn: [i16; 3],
    /// Where the walk ends and the outcome lands.
    pub goal: [i16; 3],
}

impl World {
    /// Absolute actor slots a player cast lands on, from the submenu's
    /// resolution: a single-target shape takes the picked slot (enemy rows
    /// sit behind the party), a group shape takes the whole band.
    pub(in crate::world) fn spell_targets_for(
        &self,
        def: &crate::spells::SpellDef,
        target_row: crate::target_picker::CursorRow,
        target_slot: u8,
    ) -> Vec<u8> {
        use crate::spells::SpellTarget;
        use crate::target_picker::CursorRow;
        let party_count = self.party_count.clamp(1, 3);
        match def.target {
            SpellTarget::OneEnemy | SpellTarget::OneAlly | SpellTarget::SelfOnly => {
                let abs = match target_row {
                    CursorRow::Enemy => party_count + target_slot,
                    CursorRow::Ally => target_slot,
                };
                vec![abs]
            }
            SpellTarget::AllEnemies => (party_count..self.actors.len() as u8).collect(),
            SpellTarget::AllAllies => (0..party_count).collect(),
        }
    }

    /// The `+0x1DD` target byte a cast of `def` onto `targets` carries: a
    /// group shape is the group code the cast-begin facing store and the
    /// cue expander decode (`8` = the party, `9` = the enemy row), a single
    /// shape is the slot itself.
    fn cast_target_code(&self, def: &crate::spells::SpellDef, targets: &[u8], caster: u8) -> u8 {
        use crate::spells::SpellTarget;
        use vm::battle_target_group::{TARGET_GROUP_ENEMIES, TARGET_GROUP_PARTY};
        let caster_is_party = caster < self.party_count;
        match def.target {
            // Codes are in retail's absolute numbering: 8 = party, 9 = the
            // enemy row - whichever side the caster stands on.
            SpellTarget::AllEnemies => {
                if caster_is_party {
                    TARGET_GROUP_ENEMIES
                } else {
                    TARGET_GROUP_PARTY
                }
            }
            SpellTarget::AllAllies => {
                if caster_is_party {
                    TARGET_GROUP_PARTY
                } else {
                    TARGET_GROUP_ENEMIES
                }
            }
            _ => targets.first().copied().unwrap_or(caster),
        }
    }

    /// Arm the action SM's Magic band for `caster`'s `def` cast onto
    /// `targets`: the committed category-2 action retail's menu confirm
    /// leaves for `FUN_801E295C` to seed (`actor[+0x1DE] = 2`, `+0x1DF` =
    /// the spell id, `+0x1DD` = the target byte), plus the [`PendingCast`]
    /// that owes the outcome.
    ///
    /// The band then does what retail's does: faces the target and debits
    /// the ability-bit-scaled MP at `0x28`, waits `0x14` frames at `0x29`,
    /// runs the trigger (the summon route for a Seru id), and carries the
    /// clips; the outcome folds at the seam the module docs name.
    pub(in crate::world) fn arm_player_cast(
        &mut self,
        caster: u8,
        def: &crate::spells::SpellDef,
        targets: Vec<u8>,
    ) {
        let code = self.cast_target_code(def, &targets, caster);
        self.clear_action_stream(caster);
        if let Some(a) = self.actors.get_mut(caster as usize) {
            a.battle.active_target = code;
            a.battle.action_category = ActionCategory::Magic.as_byte();
            a.battle.params[0] = def.id;
            a.battle.sub_route = 0;
        }
        self.pending_cast = Some(PendingCast {
            caster,
            spell_id: def.id,
            targets,
        });
        self.battle_ctx.active_actor = caster;
        self.battle_ctx.queued_action = ActionCategory::Magic.as_byte();
        self.battle_ctx.action_state = ActionState::Begin.as_byte();
    }

    /// The monster twin of [`Self::arm_player_cast`]. A monster's stream
    /// carries its cast clip behind the spell id (`params[1]`, the entry of
    /// its archive action table whose tag is the spell id - the `+0x1DA`
    /// stage the `0x29` arm makes), terminated at `params[2]`; a monster
    /// with no such clip installed stages the terminator at once and the
    /// band ends the action after the wait.
    pub(in crate::world) fn arm_monster_cast(
        &mut self,
        slot: u8,
        def: &crate::spells::SpellDef,
        targets: Vec<u8>,
    ) {
        let code = self.cast_target_code(def, &targets, slot);
        let clip = self
            .actors
            .get(slot as usize)
            .and_then(|a| a.battle_action_clips.as_ref())
            .and_then(|clips| {
                clips
                    .iter()
                    .position(|c| c.as_ref().is_some_and(|c| c.action_id == def.id))
            })
            .and_then(|i| u8::try_from(i).ok())
            .unwrap_or(0xFF);
        self.clear_action_stream(slot);
        if let Some(a) = self.actors.get_mut(slot as usize) {
            a.battle.active_target = code;
            a.battle.action_category = ActionCategory::Magic.as_byte();
            a.battle.params[0] = def.id;
            a.battle.params[1] = clip;
            a.battle.params[2] = 0xFF;
            a.battle.sub_route = 0;
        }
        self.pending_cast = Some(PendingCast {
            caster: slot,
            spell_id: def.id,
            targets,
        });
        self.battle_ctx.active_actor = slot;
        self.battle_ctx.queued_action = ActionCategory::Magic.as_byte();
        self.battle_ctx.action_state = ActionState::Begin.as_byte();
    }

    /// Fold the owed cast's outcome - exactly once. The MP was the band's
    /// (`0x28`), so the fold is the prepaid one; a monster cast also rolls
    /// its move's impact-status and AGL-status procs onto what it reached.
    pub(in crate::world) fn fold_pending_cast(&mut self) {
        let Some(pc) = self.pending_cast.take() else {
            return;
        };
        // Two casts in the band do not fold through the catalog at all:
        // PROT 0927 and PROT 0966 apply their damage inside the module's own
        // `0x801F6734` stager, over a seat range the spell record cannot
        // express and with a clamp that cannot kill. Where the module owns
        // the outcome, run the module - and do not also run the generic fold,
        // which would apply the hit twice.
        if self
            .run_cast_module_aoe_for(pc.caster, pc.spell_id)
            .is_some()
        {
            return;
        }
        // ...and neither do the three whole-row **tick** sweeps, once they
        // have run. PROT 0938's two bodies and PROT 0965's each roll their
        // own baked power per hittable seat and store the clamped net into
        // `+0x14C` themselves, over `actor_table[0 .. ctx[+0]]` - a seat
        // range the spell record cannot express, exactly like the two
        // stagers above. The one difference is the clamp: these kill, the
        // stagers cannot.
        //
        // The `phase > sweep_arm` test is what keeps this from *losing* an
        // outcome: a host that never drove the band past the sweep arm has
        // had no module damage applied, and skipping the generic fold there
        // would leave the cast owed forever instead of double-applied.
        if let Some(entry) = self.cast_module_for(pc.spell_id)
            && let Some(body) = vm::cast_module_ticks::capture_tick_body(entry, pc.spell_id)
            && vm::cast_module_ticks::tick_body_owns_the_fold(body)
            && let Some(arm) = vm::cast_module_ticks::sweep_arm_for(body)
            && self.cast_module_phase > arm
        {
            return;
        }
        let Some(def) = self.spell_catalog.get(pc.spell_id).cloned() else {
            return;
        };
        let hit_fx_start = self.battle_hit_fx.len();
        self.cast_spell_on_slots_prepaid(pc.caster, &def, &pc.targets);
        if pc.caster >= self.party_count {
            self.apply_enemy_move_status(pc.caster, def.id, hit_fx_start);
            self.apply_enemy_agl_status(pc.caster, def.id, &pc.targets);
        }
    }

    /// The live loop's post-step glue for the cast band: fold the owed cast
    /// at retail's seam.
    ///
    /// * A non-summon cast folds the frame the SM leaves `0x29` for the anim
    ///   chain (the module has been paged in and the first clip staged); the
    ///   party trigger already folded the `< 0x25` arm's cast, so this is
    ///   the monster's fold.
    /// * The summon route (`0x29 -> 0x32`) leaves the fold to the stager's
    ///   strike.
    /// * Any door out of the bands (`0x50` and above: done, the capture
    ///   branch) folds whatever is still owed, so a cast never spends its MP
    ///   for nothing.
    pub(in crate::world) fn settle_cast_band(&mut self, outcome: &StepOutcome) {
        let Some(pc) = &self.pending_cast else {
            return;
        };
        if pc.caster != self.battle_ctx.active_actor {
            return;
        }
        let StepOutcome::Transition { from, to } = *outcome else {
            return;
        };
        let left_precast = from == ActionState::MagicPreCastWait.as_byte()
            && to != ActionState::MagicPreCastWait.as_byte()
            && to != ActionState::SummonInvoke.as_byte();
        let band_over = to >= ActionState::DoneCleanup.as_byte();
        if left_precast || band_over {
            self.fold_pending_cast();
        }
    }

    /// Arm the stager for `caster`'s `spell_id` cast. Called from the cast
    /// trigger (`FUN_801DBF9C`'s `>= 0x25` arm); the band's first stager
    /// tick does the spawn.
    ///
    /// Public because it is the band's arming *seam*, not an internal step:
    /// the trigger runs it for a host's cast, and a parity oracle that drives
    /// the band a frame at a time has to reach the same door rather than a
    /// second one of its own.
    pub fn arm_summon_stager(&mut self, caster: u8, spell_id: u8) {
        let (cx, cy, cz) = self
            .actors
            .get(caster as usize)
            .map(|a| {
                (
                    a.move_state.world_x,
                    a.move_state.world_y,
                    a.move_state.world_z,
                )
            })
            .unwrap_or((0, 0, -542));
        // Retail's cast-start site `0x801E4B1C` zeroes `ctx+0x278` and the
        // module phase `ctx+0x279` before the first tick.
        self.cast_module_phase = 0;
        self.cast_module_ctx_278 = 0;
        self.summon_stager = Some(SummonStager {
            caster,
            spell_id,
            phase: SummonPhase::Armed,
            frames: 0,
            spawn: [cx, cy, cz.saturating_sub(SUMMON_SPAWN_BEHIND)],
            goal: [cx, cy, cz.saturating_sub(SUMMON_STRIKE_BEHIND)],
        });
    }

    /// A host seated the summon creature at actor `slot`: adopt the seat,
    /// place it at the stager's spawn point wearing the caster's facing (the
    /// capture's slot-7 record), and mark it active. Hosts call this right
    /// after binding the creature's mesh, idle player and clip set.
    pub fn seat_summon_actor(&mut self, slot: usize) {
        let staged = self.summon_stager.as_ref().map(|st| {
            (
                st.spawn,
                self.actors
                    .get(st.caster as usize)
                    .map(|a| a.battle.facing_angle)
                    .unwrap_or(0),
            )
        });
        self.summon_actor_slot = Some(slot as u8);
        let Some(a) = self.actors.get_mut(slot) else {
            return;
        };
        a.active = true;
        // A debug spawn outside a cast keeps the host's own placement.
        let Some((spawn, facing)) = staged else {
            return;
        };
        a.move_state.world_x = spawn[0];
        a.move_state.world_y = spawn[1];
        a.move_state.world_z = spawn[2];
        a.battle.facing_angle = facing;
        a.battle.queued_anim = 0;
        a.battle.current_anim = 0;
    }

    /// Install the cast-effect pool - the DATA half of the slot-B cast-module
    /// band (PROT 0903..0966), parsed off the disc by the scene host (which
    /// holds the PROT index; `World` is index-agnostic, the same split
    /// [`Self::pending_summon_spawn`](crate::world::World::pending_summon_spawn)
    /// uses). Idempotent; a host that never calls it leaves every cast staging
    /// no module records, which is the disc-free behaviour.
    pub fn install_cast_effect_pool(
        &mut self,
        pool: std::sync::Arc<legaia_asset::cast_effect_pool::CastEffectPool>,
    ) {
        self.cast_effect_pool = Some(pool);
    }

    /// The band entry a cast of `spell_id` pages - the **pool handle** PROT
    /// 0898's two tick dispatchers resolve to.
    ///
    /// Which dispatcher applies is the record's `+0` class byte, exactly as it
    /// is for the damage-kernel pick ([`World::spell_table_class`]):
    ///
    /// * class `'c'` - the capture band. `FUN_801F2160` reads the record's
    ///   `+0x01` byte and jumps through `0x801CF56C`, so the module is
    ///   PROT `935 + sub_id`. Same byte the pager
    ///   `FUN_8003EC70(record[+1] + 0x28)` resolves.
    /// * anything else - `FUN_801F1ED4` keys on the queued action id itself
    ///   and jumps through `0x801CF4EC`, so the module is
    ///   PROT `903 + (id - 0x81)`.
    ///
    /// Without a disc spell table the class byte is unknown and every id is
    /// treated as the action-id band, which is what a disc-free battle's
    /// placeholder catalog means anyway.
    ///
    /// REF: FUN_801F1ED4, FUN_801F2160
    pub fn cast_module_for(&self, spell_id: u8) -> Option<u32> {
        use vm::battle_cast_dispatch::{seru_spell_emitter, spell_class_emitter};
        if self.spell_table_class(spell_id) == Some(legaia_asset::spell_names::CAPTURE_CLASS) {
            let sub = self.spell_effect_class(spell_id)?;
            return spell_class_emitter(sub, 0).module;
        }
        seru_spell_emitter(spell_id, 0).module
    }

    /// The record's `+0x01` **effect class**: the disc table's byte when one is
    /// installed, else the catalog record's
    /// ([`crate::spells::SpellDef::effect_class`], which
    /// [`crate::retail_magic`] fills from `SCUS_942.54`).
    pub fn spell_effect_class(&self, spell_id: u8) -> Option<u8> {
        self.spell_table_sub_class(spell_id)
            .or_else(|| self.spell_catalog.get(spell_id).map(|d| d.effect_class))
    }

    /// Stage the cast module's **spawn records** at `origin` - the engine's
    /// answer to the dispatchers, and the half of a cast that is data.
    ///
    /// Each record is `[i16 model_sel][u16 flags][move-VM bytecode]`, the shape
    /// the whole spawn stack shares, so the records run through the same
    /// [`crate::summon::SummonScene`] the summon and move-FX paths already use
    /// and both hosts draw and tick them with no host change
    /// (`active_summon_part_draws` / `tick_summon`).
    ///
    /// Returns `false` - staging nothing - when the spell names no band entry,
    /// no pool is installed (disc-free), or the module carries no record (the
    /// band's null stub PROT 0926, and PROT 0952 whose two spawn sites load
    /// their record pointer out of a saved register).
    ///
    /// This is the **DATA** half of the band's worklist. The **PORT** half -
    /// the six tick bodies and the seven state-touching stagers - is
    /// [`legaia_engine_vm::cast_module_ticks`], driven from
    /// [`Self::run_cast_module_code`] at this same seam. The **SCOPE-IGNORE**
    /// rows are the six null stagers and the one unreferenced routine; there
    /// is nothing to stage for them either.
    ///
    /// REF: FUN_801F1ED4, FUN_801F2160 (the dispatchers that name the module;
    /// their emitter arms are the unported half above)
    /// REF: FUN_80050ED4, FUN_80021B04 (the spawn calls whose `a2` records
    /// these are)
    pub fn spawn_cast_module_fx(&mut self, spell_id: u8, origin: [i16; 3]) -> bool {
        let Some(entry) = self.cast_module_for(spell_id) else {
            return false;
        };
        let Some(pool) = self.cast_effect_pool.clone() else {
            return false;
        };
        let Some(module) = pool.module(entry) else {
            return false;
        };
        if module.parts.is_empty() {
            return false;
        }
        self.active_summon = Some(crate::summon::SummonScene::spawn_parts(
            &module.parts,
            &module.bytes,
            crate::scene::EFFECT_MODEL_LIBRARY_BASE,
            origin,
        ));
        true
    }

    /// One stager tick - the engine body behind
    /// `BattleActionHost::summon_stager_tick`. Returns `true` while the
    /// choreography is still running (retail: the stager's non-zero return
    /// the `0x36` arm holds on).
    ///
    /// PORT: FUN_801F1ED4 (the dispatch seam; the choreography is the
    /// engine's - see the module docs)
    pub fn summon_stager_tick(&mut self) -> bool {
        let Some(mut st) = self.summon_stager.take() else {
            return false;
        };
        // Retail re-enters the paged module every frame from this seam; the
        // band's PORT rows are the code that runs there.
        let module_arm = self.cast_module_phase;
        let _ = self.run_cast_module_code(st.spell_id, module_arm);
        let busy = match st.phase {
            SummonPhase::Armed => {
                // Phase 0: seat the creature (retail: the stager's
                // `FUN_801F19EC` installs the streamed record as slot 7).
                self.pending_summon_spawn = Some((st.spell_id, st.spawn));
                // ...and stage the module's own effect parts. This is the
                // `0x801E4B1C` site's other half: `FUN_801F1ED4` dispatches
                // into the paged module, whose spawn records are the cast's
                // particle layer. Seated where the creature is, since the
                // records carry summon-local offsets.
                self.spawn_cast_module_fx(st.spell_id, st.spawn);
                st.phase = SummonPhase::Approach;
                st.frames = 0;
                true
            }
            SummonPhase::Approach => {
                st.frames = st.frames.saturating_add(1);
                let seat = self
                    .summon_actor_slot
                    .filter(|&s| self.actors.get(s as usize).is_some_and(|a| a.active));
                let strike = match seat {
                    Some(slot) => {
                        st.frames > SUMMON_IDLE_FRAMES
                            && self.summon_walk_step(slot as usize, st.goal)
                    }
                    None => st.frames >= SUMMON_UNSEATED_GRACE,
                };
                if strike {
                    self.fold_pending_cast();
                    st.phase = SummonPhase::Linger;
                    st.frames = 0;
                }
                true
            }
            SummonPhase::Linger => {
                st.frames = st.frames.saturating_add(1);
                if st.frames == 1
                    && let Some(slot) = self.summon_actor_slot
                    && let Some(a) = self.actors.get_mut(slot as usize)
                {
                    // Back to the idle loop for the hold.
                    a.battle.queued_anim = 0;
                }
                if st.frames >= SUMMON_LINGER_FRAMES {
                    self.despawn_summon_actor();
                    st.phase = SummonPhase::Done;
                }
                true
            }
            SummonPhase::Done => false,
        };
        if busy {
            self.summon_stager = Some(st);
        }
        busy
    }

    /// One tick of the **capture band's** resident module - the engine body
    /// behind `BattleActionHost::capture_stager_tick`, and the twin of
    /// [`Self::summon_stager_tick`] on the other dispatcher.
    ///
    /// Retail's battle phase `0x70` re-enters `FUN_801F2160` every frame
    /// (`jal` at `0x801E50C8`) and holds while it returns non-zero
    /// (`bne v0,zero` at `0x801E50D0`); the return is the selected slot-B
    /// module tick's own return. So this runs the resident module's code half
    /// ([`Self::run_cast_module_code`]) and reports what it reported.
    ///
    /// The one deviation, and it is deliberate: a module whose tick body is
    /// **not** ported reports "not busy" rather than
    /// [`CastModuleCodeRun::busy`]'s seeded `true`. Holding on an unported
    /// row would park the band forever, which is a softlock and not a
    /// fidelity gain; [`CastModuleCodeRun::tick_ported`] is what distinguishes
    /// the two.
    ///
    /// PORT: FUN_801F2160 (the dispatch seam; the per-module tick bodies are
    /// [`legaia_engine_vm::cast_module_ticks`])
    pub fn capture_stager_tick(&mut self) -> bool {
        let Some(spell_id) = self.capture_cast_spell else {
            return false;
        };
        let arm = self.cast_module_phase;
        let Some(run) = self.run_cast_module_code(spell_id, arm) else {
            self.capture_cast_spell = None;
            return false;
        };
        if run.tick_ported && run.busy {
            return true;
        }
        // The band is leaving `0x70`; the module stops being re-entered.
        self.capture_cast_spell = None;
        false
    }

    /// Arm [`Self::capture_stager_tick`] for `spell_id`'s module - the pager
    /// seam retail runs at its `0x6F` exit, where `sb zero,0x279(v0)`
    /// (`0x801E5048`) also zeroes the module phase before phase `0x70` starts
    /// ticking it.
    pub(in crate::world) fn arm_capture_cast_module(&mut self, spell_id: u8) {
        if self.cast_module_for(spell_id).is_none() {
            return;
        }
        self.cast_module_phase = 0;
        self.cast_module_ctx_278 = 0;
        self.capture_cast_spell = Some(spell_id);
    }

    /// Stage the walk clip (id `1`, the looping approach - the capture's
    /// `+0x1D9 == 1`) and glide the creature one step toward `goal`.
    /// Returns `true` on arrival.
    fn summon_walk_step(&mut self, slot: usize, goal: [i16; 3]) -> bool {
        let Some(a) = self.actors.get_mut(slot) else {
            return true;
        };
        if a.battle.queued_anim != 1 {
            a.battle.queued_anim = 1;
        }
        let ms = &mut a.move_state;
        let dx = i32::from(goal[0]) - i32::from(ms.world_x);
        let dz = i32::from(goal[2]) - i32::from(ms.world_z);
        let step = i32::from(SUMMON_WALK_STEP);
        if dx.abs() <= step && dz.abs() <= step {
            ms.world_x = goal[0];
            ms.world_z = goal[2];
            return true;
        }
        ms.world_x = (i32::from(ms.world_x) + dx.clamp(-step, step)) as i16;
        ms.world_z = (i32::from(ms.world_z) + dz.clamp(-step, step)) as i16;
        false
    }

    /// Retire the summon creature: the seat goes inactive so both hosts
    /// stop drawing it (native: the `active` gate of the battle draw loop;
    /// browser: the transform row's `active` float).
    pub(in crate::world) fn despawn_summon_actor(&mut self) {
        if let Some(slot) = self.summon_actor_slot.take()
            && let Some(a) = self.actors.get_mut(slot as usize)
        {
            a.active = false;
            a.battle_animation = None;
            a.pose_frame = None;
            a.battle.queued_anim = 0;
            a.battle.current_anim = 0;
        }
    }

    /// The full-screen fade quad to composite this frame, as
    /// `(rgb 0xRRGGBB, abr_mode, ot_index)` for
    /// `legaia_engine_ui::screen_prim::fade_prim` - `None` while no fade is
    /// live or its start delay is still running (retail's tick returns `-1`
    /// and draws nothing). The ABR mode is the template's kind word and the
    /// OT index the id `FUN_80024E80` stamped (`AddPrim(ot + id*4, ..)` in
    /// `FUN_80024EE4`).
    pub fn screen_fade_draw(&self) -> Option<(u32, u8, u32)> {
        let f = self.screen_fade.as_ref()?;
        if !f.visible() {
            return None;
        }
        let [r, g, b] = f.rgb();
        Some((
            (u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b),
            f.kind.clamp(0, 3) as u8,
            f.mode[2].max(0) as u32,
        ))
    }
}

// ---------------------------------------------------------------------------
// The band's PORT half: the slot-B module code kernels
// ---------------------------------------------------------------------------

/// The equipment-slot index the first accessory occupies: record `+0x196` is
/// slot 0 and `+0x19B` - the byte PROT 0955's Void Accessories arm forms as
/// `0x80084140 + (char - 1) * 0x414 + 0x75E + 5 + slot` - is slot 5.
const ACCESSORY_EQUIP_SLOT_0: usize = 5;

/// Where a module's code half wrote, so a caller can see the kernel ran
/// without re-reading every actor.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CastModuleCodeRun {
    /// The band entry that was resident (`903 + row` / `935 + sub_id`).
    pub prot_entry: u32,
    /// The module phase after the tick (`ctx+0x279`).
    pub phase: u8,
    /// `ctx+0x278` after the tick.
    pub ctx_278: u8,
    /// Seats an AoE stager swept, with the amount applied to each.
    pub aoe_hits: Vec<vm::cast_module_ticks::AoeHit>,
    /// `true` while the module's phase machine still reports busy.
    ///
    /// Seeded `true` for a resident module and only *lowered* by a ported
    /// tick body, so it is meaningless on its own - read it together with
    /// [`Self::tick_ported`].
    pub busy: bool,
    /// Whether a ported tick body actually ran this frame. `false` means the
    /// entry is resident but its code half is one of the band's unported
    /// rows, and then [`Self::busy`] carries no information: a caller that
    /// held on it would hold forever.
    pub tick_ported: bool,
    /// The item PROT 0955's turn-steal arms handed back to the bag
    /// (`FUN_800421D4(victim[+0x1DF], 1)`), when the victim had an Item
    /// action queued.
    pub item_refund: Option<u8>,
    /// What PROT 0955's Void Accessories arm decided this frame.
    pub voided_accessory: Option<vm::cast_module_ticks::VoidAccessoriesOutcome>,
    /// `(element, group)` PROT 0964's Element Change committed onto the first
    /// monster seat's record this frame (`+0x1D` / `+0x1C`).
    pub element_change: Option<(u8, u8)>,
}

impl World {
    /// Lift one actor slot into the state view the slot-B kernels take.
    ///
    /// The engine carries every field these routines touch except two: the
    /// `+0x0C` root-speed word and the `+0x1DC` restage counter, which live
    /// in the run's own scratch because the engine's `flag_bits` byte at that
    /// offset is a flag set, not a counter (`docs/subsystems/battle-action.md`
    /// and `docs/subsystems/cast-module.md` read `+0x1DC` differently, and the
    /// bytes here only ever increment it).
    fn cast_actor_state(&self, slot: u8) -> vm::cast_module_ticks::CastActorState {
        use vm::cast_module_ticks::{ANIM_RATE_NORMAL, CastActorState};
        let Some(a) = self.actors.get(slot as usize) else {
            return CastActorState {
                anim_rate: ANIM_RATE_NORMAL,
                ..Default::default()
            };
        };
        CastActorState {
            root_speed: 0,
            hp_bar_delta: a.battle.hp_bar_pending,
            hp: a.battle.hp,
            flags: a.battle.field_flags,
            playing_anim: a.battle.current_anim,
            staged_anim: a.battle.queued_anim,
            restage: 0,
            target_code: a.battle.active_target,
            // `+0x1F1` sits inside the per-action parameter stream that starts
            // at `+0x1DF`.
            knockdown_anim: a.battle.params.get(0x1F1 - 0x1DF).copied().unwrap_or(0),
            render_flag: a.battle.render_flag,
            anim_rate: a.battle.anim_rate.get(),
            // The whole `+0x158..+0x16A` stat block, five `(working, base)`
            // pairs, now has a home on the battle actor - so every one of
            // Melt Spray's ten halfword stores and both of Power Charge's
            // `+0x15A` stores land instead of stopping at the view. The
            // defence pair is still kept in the world's per-slot split,
            // which is where the physical-defence facet reads it.
            agl: a.battle.agl,
            agl_base: a.battle.agl_base,
            atk: a.battle.atk_working,
            atk_base: a.battle.atk_base,
            udf: self.cast_defence_split(slot).0,
            udf_base: self.cast_defence_split(slot).0,
            ldf: self.cast_defence_split(slot).1,
            ldf_base: self.cast_defence_split(slot).1,
            // SPD and INT live in two places: the actor's own halfwords (new,
            // so the band's writers have somewhere to land) and the world's
            // per-slot mirrors, which are what turn order / the escape roll
            // (`battle_speed`) and the accuracy seed (`battle_accuracy`)
            // actually read. Seed from the mirror whenever the actor's own
            // halfword is still zero - `seed_party_battle_stats` fills the
            // mirrors, not the actor - so a debuff computes on a real number
            // instead of underflowing zero.
            spd: nonzero_or(a.battle.spd, self.battle_speed.get(slot as usize).copied()),
            spd_base: nonzero_or(
                a.battle.spd_base,
                self.battle_speed.get(slot as usize).copied(),
            ),
            intel: nonzero_or(
                a.battle.intel,
                self.battle_accuracy.get(slot as usize).copied(),
            ),
            intel_base: nonzero_or(
                a.battle.intel_base,
                self.battle_accuracy.get(slot as usize).copied(),
            ),
            init_key: a.battle.init_key,
            action_category: a.battle.action_category,
            queued_action: a.battle.params.first().copied().unwrap_or(0),
            reaction_alt: a.battle.params.get(0x1EF - 0x1DF).copied().unwrap_or(0),
            reaction_alt2: a.battle.params.get(0x1F0 - 0x1DF).copied().unwrap_or(0),
            reaction_gate: a.battle.params.get(0x1F2 - 0x1DF).copied().unwrap_or(0),
        }
    }

    /// The live UDF / LDF pair for one battle slot, out of the world's own
    /// per-slot defence split (the same store the physical-defence facet
    /// reads).
    fn cast_defence_split(&self, slot: u8) -> (u16, u16) {
        self.battle_defense_split
            .get(slot as usize)
            .copied()
            .flatten()
            .unwrap_or((0, 0))
    }

    /// The monster **record**'s AGL (`+0x0E`) for one battle seat - what
    /// PROT 0942's Power Up arm reads through `0x801C9348[seat - 3]` before
    /// it writes `caster[+0x156]`.
    ///
    /// Retail's table only covers the monster seats; a party caster indexes
    /// out of it, so the engine falls back to the actor's own AGL base rather
    /// than reading whatever sits below the table.
    fn cast_record_agl(&self, slot: u8) -> u16 {
        self.actors
            .get(slot as usize)
            .and_then(|a| a.battle_monster_id)
            .and_then(|id| self.monster_catalog.get(id))
            .map(|d| d.agl)
            .unwrap_or_else(|| {
                self.actors
                    .get(slot as usize)
                    .map(|a| a.battle.agl_base)
                    .unwrap_or(0)
            })
    }

    /// The stand-in for retail's `0x801C8FE4`: which of PROT 0964's three
    /// elements the record currently carries, so the re-roll cannot land on
    /// it again. `0xFF` when the record's element is none of the three, which
    /// makes the first draw acceptable - the same thing a never-written
    /// `0x801C8FE4` does.
    fn cast_element_change_last_roll(&self) -> u8 {
        use vm::cast_module_ticks::{ELEMENT_CHANGE_ELEMENTS, FIRST_MONSTER_SEAT};
        let Some(elem) = self
            .actors
            .get(FIRST_MONSTER_SEAT as usize)
            .and_then(|a| a.battle_monster_id)
            .and_then(|id| self.monster_catalog.get(id))
            .map(|d| d.element)
        else {
            return 0xFF;
        };
        ELEMENT_CHANGE_ELEMENTS
            .iter()
            .position(|e| *e == elem)
            .map(|i| i as u8)
            .unwrap_or(0xFF)
    }

    /// Commit PROT 0964's new element onto the first monster seat.
    ///
    /// Retail writes the loaded record at `0x801C9348[0]`; the engine's
    /// equivalent store is the catalog entry that seat's `battle_monster_id`
    /// names, which is what `World::battle_slot_element` reads back. The
    /// difference from retail is scope: retail's write is per **seat**, so a
    /// battle fielding two copies of the same monster id would see only one
    /// of them change; here both do.
    fn apply_cast_element_change(&mut self, element: u8) {
        use vm::cast_module_ticks::FIRST_MONSTER_SEAT;
        let Some(id) = self
            .actors
            .get(FIRST_MONSTER_SEAT as usize)
            .and_then(|a| a.battle_monster_id)
        else {
            return;
        };
        if let Some(def) = self.monster_catalog.by_id.get_mut(&id) {
            def.element = element;
        }
    }

    /// Write a kernel's state view back onto an actor slot.
    fn write_cast_actor_state(&mut self, slot: u8, st: &vm::cast_module_ticks::CastActorState) {
        use vm::battle_anim_rate::AnimRate;
        let Some(a) = self.actors.get_mut(slot as usize) else {
            return;
        };
        a.battle.hp_bar_pending = st.hp_bar_delta;
        a.battle.hp = st.hp;
        a.battle.field_flags = st.flags;
        a.battle.queued_anim = st.staged_anim;
        a.battle.active_target = st.target_code;
        a.battle.render_flag = st.render_flag;
        a.battle.anim_rate = AnimRate(st.anim_rate);
        a.battle.agl = st.agl;
        a.battle.agl_base = st.agl_base;
        a.battle.atk_working = st.atk;
        a.battle.atk_base = st.atk_base;
        a.battle.spd = st.spd;
        a.battle.spd_base = st.spd_base;
        a.battle.intel = st.intel;
        a.battle.intel_base = st.intel_base;
        a.battle.init_key = st.init_key;
        a.battle.action_category = st.action_category;
        // ...and back into the mirrors the rest of the engine reads, so a
        // five-stat debuff is visible to turn order and the accuracy seed
        // rather than only to the next module tick.
        if let Some(s) = self.battle_speed.get_mut(slot as usize) {
            *s = st.spd;
        }
        if let Some(s) = self.battle_accuracy.get_mut(slot as usize) {
            *s = st.intel;
        }
        if let Some(s) = self.battle_defense_split.get_mut(slot as usize)
            && s.is_some()
        {
            *s = Some((st.udf, st.ldf));
        }
    }

    /// The context bytes the kernels read (`ctx+0`, `+1`, `+0x13`, `+0x278`,
    /// `+0x279`).
    ///
    /// Both counts are bounded by [`BATTLE_TABLE_SLOTS`]: retail's `ctx[+0]`
    /// and `ctx[+1]` index `DAT_801C9370`, whose battle span is the eight
    /// combat seats - the summon seat and any host-side extras above them are
    /// not part of either sweep's range. The monster count starts at
    /// `cast_module_ticks::FIRST_MONSTER_SEAT`, the fixed base the Juggernaut
    /// sweep's `addiu s4, zero, 0xc` encodes.
    fn cast_module_ctx(&self) -> vm::cast_module_ticks::CastModuleCtx {
        use vm::cast_module_ticks::FIRST_MONSTER_SEAT;
        let table = self.actors.len().min(BATTLE_TABLE_SLOTS);
        vm::cast_module_ticks::CastModuleCtx {
            actor_count: table as u8,
            monster_count: (FIRST_MONSTER_SEAT as usize..table)
                .filter(|&s| self.actors[s].active)
                .count() as u8,
            caster_seat: self.battle_ctx.active_actor,
            ctx_278: self.cast_module_ctx_278,
            phase: self.cast_module_phase,
            ctx_0d: 0,
            turn_cursor: self.battle_ctx.turn_cursor,
            ctx_27a: 0,
        }
    }

    /// Run the resident module's **code** half for one frame - the band's
    /// **PORT** worklist rows, ported in
    /// [`legaia_engine_vm::cast_module_ticks`].
    ///
    /// Retail re-enters the paged module every frame through
    /// `FUN_801F1ED4` / `FUN_801F2160` and holds battle phase `0x70` while
    /// the tick reports busy; the engine calls this from
    /// [`Self::summon_stager_tick`], which the action SM already drives at
    /// states `0x34` / `0x35` / `0x36` (`crate::world::vm_hosts` ->
    /// `legaia_engine_vm::battle_action::summon`). So the chain from a host
    /// root is: `play-window` / the browser play page -> `World::tick` ->
    /// the action SM's summon band -> here.
    ///
    /// What runs is the module's **presentation and phase** half: the
    /// `ctx+0x279` walk, `ctx+0x278`, the staged-clip and animation-rate
    /// writes, the summon-seat pose and the `+0x1DD` retarget. The damage
    /// half is deliberately *not* re-applied here - the engine folds a cast's
    /// HP outcome once, at [`Self::cast_spell_on_slots_prepaid`], and the
    /// module's own numbers reach that fold as the baked per-hit power
    /// ([`vm::cast_module_ticks::baked_power_for`], read by
    /// `capture_bypass_predamage` / `capture_respect_predamage`) rather than
    /// as a second application.
    ///
    /// Returns `None` when no band entry is resident (a disc-free host, or a
    /// spell that names no module).
    pub fn run_cast_module_code(&mut self, spell_id: u8, arm: u8) -> Option<CastModuleCodeRun> {
        use vm::cast_module_ticks as ticks;

        let entry = self.cast_module_for(spell_id)?;
        let mut ctx = self.cast_module_ctx();
        let caster_slot = self.battle_ctx.active_actor;
        let victim_slot = self
            .actors
            .get(caster_slot as usize)
            .map(|a| a.battle.active_target)
            .unwrap_or(0);
        let seat_slot = self.summon_actor_slot.unwrap_or(ticks::SUMMON_SEAT);

        let mut caster = self.cast_actor_state(caster_slot);
        let mut victim = self.cast_actor_state(victim_slot);
        let mut seat = self.cast_actor_state(seat_slot);
        let mut run = CastModuleCodeRun {
            prot_entry: entry,
            busy: true,
            ..Default::default()
        };

        // The seven state-touching spawn stagers, by owning entry.
        match entry {
            906 => ticks::gizam_stager(&mut ctx, &mut seat, arm),
            909 => {
                ticks::viguro_stager(&mut ctx, &mut seat, arm);
            }
            922 => ticks::puera_stager(&mut ctx, arm),
            923 => ticks::gilium_stager(&mut ctx, arm),
            949 => ticks::water_crystals_stager(&mut victim, arm),
            _ => {}
        }

        // The tick bodies, by owning entry. `hit` is `None` because the fold
        // is the band seam's, not the tick's (see the note above).
        //
        // A capture-class module reaches its body through a **trampoline**
        // (`ticks::capture_tick_body`), which switches on the caster's queued
        // action id, so a multi-spell cell picks a different choreography for
        // each of its ids. Where the module has one, the trampoline decides
        // whether anything ticks at all: an id it does not name returns zero
        // and the drive loop proceeds.
        //
        // The arm has to be keyed on `(entry, body)`, never on the body VA
        // alone: six modules in the band put a tick body at `0x801F69D8` -
        // the load base itself - so PROT 0960's Neo Star Slash and PROT
        // 0965's Doomsday wear the same address in different images.
        let body = ticks::capture_tick_body(entry, spell_id);
        let has_trampoline = ticks::capture_trampoline_for(entry).is_some();
        let step = if has_trampoline {
            match (entry, body) {
                (958, Some(ticks::BLAZING_SLASH_TICK)) => {
                    Some(ticks::blazing_slash_tick(&mut ctx, &mut victim, None))
                }
                (952, Some(ticks::ASTRAL_SLASH_TICK)) => {
                    Some(ticks::astral_slash_tick(&mut ctx, &mut caster, &mut victim))
                }
                (945, Some(ticks::WATER_COLUMN_TICK)) => {
                    Some(ticks::water_column_tick(&mut ctx, &mut victim, None, 0))
                }
                // PROT 0945's other choreography: the band's widest stat
                // write, on the caster.
                (945, Some(ticks::ALL_STATS_SURGE_TICK)) => {
                    let agl_record = self.cast_record_agl(caster_slot);
                    Some(ticks::all_stats_surge_tick(
                        &mut ctx,
                        &mut caster,
                        agl_record,
                    ))
                }
                (960, Some(ticks::PLASMA_STRIKE_TICK)) => Some(ticks::plasma_strike_tick(
                    &mut ctx,
                    &mut caster,
                    &mut victim,
                    None,
                )),
                (957, Some(ticks::SUMMON_EFFECT_TICK_B)) => {
                    Some(ticks::summon_effect_tick_b(&mut ctx, &mut victim))
                }
                (957, Some(ticks::SUMMON_EFFECT_TICK_A)) => {
                    Some(ticks::summon_effect_tick_a(&mut ctx, &mut victim, None))
                }
                // PROT 0942's Power Up reads the caster's own monster record
                // `+0x0E` and writes the AGL **base** half only.
                (942, Some(ticks::POWER_UP_TICK)) => {
                    let agl_record = self.cast_record_agl(caster_slot);
                    Some(ticks::power_up_tick(&mut ctx, &mut caster, agl_record))
                }
                // The three whole-row sweeps. Each rolls the module's own
                // baked power per hittable seat off this world's RNG cursor,
                // in seat order, so the draw order stays retail's; the two
                // 1-in-8 status draws Chaos Breath makes per seat come off
                // the same cursor.
                (938, Some(ticks::CHAOS_BREATH_TICK))
                | (938, Some(ticks::MYSTIC_CIRCLE_TICK))
                | (965, Some(ticks::DOOMSDAY_TICK)) => {
                    let body = body.unwrap_or_default();
                    let mut seats: Vec<ticks::CastActorState> = (0..self.actors.len() as u8)
                        .map(|s| self.cast_actor_state(s))
                        .collect();
                    let sweep_arm = ctx.phase;
                    let rolls = self.sweep_status_rolls(&ctx, &seats, body, caster_slot);
                    // The module's own baked power, rolled per seat: these
                    // three write `+0x14C` themselves, so they own the
                    // outcome and `fold_pending_cast` skips the generic fold
                    // for them (see `sweep_status_rolls`).
                    let take = |seat: u8| {
                        rolls
                            .iter()
                            .find(|(s, _, _)| *s == seat)
                            .map(|(_, d, _)| *d)
                            .unwrap_or(0)
                    };
                    let status = |seat: u8| {
                        rolls
                            .iter()
                            .find(|(s, _, _)| *s == seat)
                            .map(|(_, _, st)| *st)
                            .unwrap_or((1, 1))
                    };
                    let (step, hits) = match body {
                        ticks::CHAOS_BREATH_TICK => ticks::chaos_breath_tick(
                            &mut ctx,
                            &mut caster,
                            &mut seats,
                            take,
                            status,
                        ),
                        ticks::MYSTIC_CIRCLE_TICK => ticks::mystic_circle_tick(
                            &mut ctx,
                            &mut seats,
                            sweep_arm == ticks::MYSTIC_CIRCLE_SWEEP_ARM,
                            take,
                        ),
                        _ => ticks::doomsday_tick(
                            &mut ctx,
                            &mut seats,
                            sweep_arm == ticks::DOOMSDAY_SWEEP_ARM,
                            take,
                        ),
                    };
                    for (slot, st) in seats.iter().enumerate() {
                        self.write_cast_actor_state(slot as u8, st);
                    }
                    run.aoe_hits = hits
                        .iter()
                        .map(|h| ticks::AoeHit {
                            seat: h.seat,
                            applied: h.applied as i32,
                        })
                        .collect();
                    Some(step)
                }
                (951, Some(ticks::CHAOS_FLARE_TICK)) => {
                    Some(ticks::chaos_flare_tick(&mut ctx, &mut victim, None))
                }
                (951, Some(ticks::SCYTHE_WIND_TICK)) => {
                    Some(ticks::scythe_wind_tick(&mut ctx, &mut victim, None))
                }
                (952, Some(ticks::BLOODY_HORNS_TICK)) => Some(ticks::bloody_horns_tick(
                    &mut ctx,
                    &mut caster,
                    &mut victim,
                    None,
                )),
                // PROT 0955's six-spell cell. Its four status / buff bodies
                // write simulation state no damage fold can express, so they
                // run here and their outcome is reported back on the run.
                (955, Some(ticks::WHITE_SHIELD_TICK)) => {
                    // Retail reads the caster's own monster **record** at
                    // `0x801C9348[seat - 3]` rather than the live actor, so
                    // the buff is idempotent; the engine's nearest equivalent
                    // is the un-buffed defence split it seeded at battle load.
                    let record = (caster.udf_base, caster.ldf_base);
                    Some(ticks::white_shield_tick(&mut ctx, &mut caster, record))
                }
                (955, Some(ticks::KISS_OF_DEATH_TICK)) => {
                    let roll = Some(self.next_rng());
                    let (step, refund) = ticks::kiss_of_death_tick(&mut ctx, &mut victim, roll);
                    run.item_refund = refund;
                    Some(step)
                }
                (955, Some(ticks::MELT_SPRAY_TICK)) => {
                    let debuff = ctx.phase == ticks::MELT_SPRAY_DEBUFF_ARM;
                    Some(ticks::melt_spray_tick(&mut ctx, &mut victim, debuff))
                }
                (955, Some(ticks::TERROR_SCREAM_TICK)) => {
                    let (step, refund) = ticks::terror_scream_tick(&mut ctx, &mut victim);
                    run.item_refund = refund;
                    Some(step)
                }
                (955, Some(ticks::POWER_CHARGE_TICK)) => {
                    Some(ticks::power_charge_tick(&mut ctx, &mut caster))
                }
                (955, Some(ticks::VOID_ACCESSORIES_TICK)) => {
                    let rolls = Some((self.next_rng(), self.next_rng()));
                    let accessories = self.cast_victim_accessories(victim_slot);
                    let (step, outcome) =
                        ticks::void_accessories_tick(&mut ctx, &mut victim, accessories, rolls);
                    run.voided_accessory = outcome;
                    Some(step)
                }
                // PROT 0964's Element Change re-rolls the first monster
                // seat's element. `last_roll` is derived from what the record
                // holds now, which is what the previous commit wrote.
                (964, Some(ticks::ELEMENT_CHANGE_TICK)) => {
                    let mut seats: Vec<ticks::CastActorState> = (0..self.actors.len() as u8)
                        .map(|s| self.cast_actor_state(s))
                        .collect();
                    let last_roll = self.cast_element_change_last_roll();
                    let mut rolls: Vec<u32> = Vec::new();
                    if ctx.phase == 0 {
                        for _ in 0..=ticks::ELEMENT_CHANGE_MAX_REROLLS {
                            rolls.push(self.next_rng());
                        }
                    }
                    let mut cursor = rolls.into_iter();
                    let (step, outcome) =
                        ticks::element_change_tick(&mut ctx, &mut seats, last_roll, || {
                            cursor.next().unwrap_or(0)
                        });
                    for (slot, st) in seats.iter().enumerate() {
                        self.write_cast_actor_state(slot as u8, st);
                    }
                    if let Some(out) = outcome {
                        self.apply_cast_element_change(out.element);
                        run.element_change = Some((out.element, out.group));
                    }
                    Some(step)
                }
                // Every ported trampoline arm the band names. An arm whose
                // body has no port ticks nothing, which is exactly what
                // retail's fall-through does for an id the trampoline does
                // not name.
                _ => None,
            }
        } else {
            match entry {
                // The summon band's own tick bodies - no trampoline, the
                // `0x801CF4EC` arm calls them directly.
                918 => ticks::kemaro_tick(&mut ctx, &mut victim, None),
                922 => Some(ticks::puera_tick(&mut ctx, &mut victim, None)),
                924 => Some(ticks::ultimate_rave_tick(
                    &mut ctx,
                    &mut caster,
                    &mut victim,
                )),
                925 => Some(ticks::spikefish_tick(&mut ctx, &mut caster)),
                927 => Some(ticks::juggernaut_tick(&mut ctx, &mut victim, None)),
                949 => Some(ticks::water_crystals_tick(&mut ctx, &mut victim, None)),
                _ => None,
            }
        };
        if let Some(step) = step {
            run.busy = step == ticks::CastTickStep::Busy;
            run.tick_ported = true;
        }

        self.write_cast_actor_state(caster_slot, &caster);
        self.write_cast_actor_state(victim_slot, &victim);
        self.write_cast_actor_state(seat_slot, &seat);
        self.cast_module_ctx_278 = ctx.ctx_278;
        self.cast_module_phase = ctx.phase;
        // The turn-steal arms bump `ctx[+0x1A]`; it is a context byte, so it
        // has to travel back out of the view.
        self.battle_ctx.turn_cursor = ctx.turn_cursor;
        run.phase = ctx.phase;
        run.ctx_278 = ctx.ctx_278;
        // PROT 0955's two arms that reach outside the battle actor: the
        // refunded item goes back in the bag (retail's `FUN_800421D4`), and
        // the voided accessory is cleared out of the character record and
        // handed back (retail's record write plus `FUN_80042558`).
        if let Some(item) = run.item_refund {
            *self.inventory.entry(item).or_insert(0) += 1;
        }
        if let Some(out) = run.voided_accessory
            && let Some(id) = out.voided
        {
            let rslot = self.party_roster_slot(victim_slot as usize);
            if let Some(rec) = self.roster.members.get_mut(rslot) {
                let mut eq = rec.equipment();
                if let Some(slot) = eq.slots.get_mut(ACCESSORY_EQUIP_SLOT_0 + out.slot as usize) {
                    *slot = 0;
                }
                rec.set_equipment(eq);
            }
            *self.inventory.entry(id).or_insert(0) += 1;
            self.refresh_party_ability_bits();
        }
        Some(run)
    }

    /// Run the band's two whole-row AoE stagers - PROT 0927 (Juggernaut,
    /// `FUN_801F85A8`) and PROT 0966 (Evil Seru Magic, `FUN_801F8D64`) -
    /// which is where those two casts' damage actually lands in retail.
    ///
    /// Both sweep a seat range with the module's own guards (skip dead, skip
    /// `+0x16E & 4`) and both clamp to `HP - 1`, so neither **sweep** can
    /// kill. That is a property of these two stagers only - PROT 0927's own
    /// tick (`0x801F6A84`) uses the unsigned `sltu` clamp instead and is
    /// kill-capable (`legaia_engine_vm::cast_module_ticks`,
    /// `docs/subsystems/cast-module.md`). ESM also
    /// stages each victim's own `+0x1F1` reaction and drops its animation rate
    /// to `2`. The roll per seat is the module's **baked** power through the
    /// module's own wrapper, drawn off this world's RNG cursor so the draw
    /// order stays retail's.
    ///
    /// Returns `None` when the spell names neither module, so the ordinary
    /// [`Self::fold_pending_cast`] path is unaffected.
    pub fn run_cast_module_aoe(&mut self, spell_id: u8, arm: u8) -> Option<CastModuleCodeRun> {
        self.run_cast_module_aoe_as(self.battle_ctx.active_actor, spell_id, arm)
    }

    /// [`Self::run_cast_module_aoe`] at the cast band's fold seam: the caster
    /// is the [`PendingCast`]'s, not whoever the context happens to point at,
    /// and the arm is the working one of the module's nine.
    fn run_cast_module_aoe_for(&mut self, caster: u8, spell_id: u8) -> Option<CastModuleCodeRun> {
        self.run_cast_module_aoe_as(caster, spell_id, AOE_STAGER_WORKING_ARM)
    }

    fn run_cast_module_aoe_as(
        &mut self,
        caster: u8,
        spell_id: u8,
        arm: u8,
    ) -> Option<CastModuleCodeRun> {
        use vm::cast_module_ticks as ticks;

        let entry = self.cast_module_for(spell_id)?;
        let shape = ticks::damage_shape_for(entry).filter(|s| s.never_kills)?;
        let ctx = self.cast_module_ctx();

        let mut seats: Vec<ticks::CastActorState> = (0..self.actors.len() as u8)
            .map(|s| self.cast_actor_state(s))
            .collect();

        // Pre-roll so the sweep borrows nothing from `self`, but roll only
        // for the seats the sweep will actually hit and in the order it visits
        // them: retail's loop calls the wrapper *after* its two skip guards,
        // so a dead or non-targetable seat draws nothing and the shared RNG
        // cursor must not advance for it either.
        let order: Vec<u8> = if entry == 966 {
            (0..ctx.actor_count).collect()
        } else {
            (0..ctx.monster_count)
                .map(|i| ticks::FIRST_MONSTER_SEAT.saturating_add(i))
                .collect()
        };
        let mut rolls: Vec<(u8, i32)> = Vec::with_capacity(order.len());
        for seat in order {
            let hittable = seats
                .get(seat as usize)
                .is_some_and(ticks::aoe_seat_is_hittable);
            if !hittable {
                continue;
            }
            let roll = self
                .capture_module_roll(shape, caster, seat)
                .unwrap_or_default();
            rolls.push((seat, roll));
        }
        let take = |seat: u8| {
            rolls
                .iter()
                .find(|(s, _)| *s == seat)
                .map(|(_, r)| *r)
                .unwrap_or_default()
        };

        let hits = if entry == 966 {
            ticks::evil_seru_magic_stager(&ctx, &mut seats, arm, take)
        } else {
            ticks::juggernaut_stager(&ctx, &mut seats, arm, take)
        };
        for (slot, st) in seats.iter().enumerate() {
            self.write_cast_actor_state(slot as u8, st);
        }
        Some(CastModuleCodeRun {
            prot_entry: entry,
            phase: ctx.phase,
            ctx_278: ctx.ctx_278,
            busy: false,
            tick_ported: true,
            aoe_hits: hits,
            item_refund: None,
            voided_accessory: None,
            element_change: None,
        })
    }

    /// One hit off a module's own damage shape: its baked power, its wrapper,
    /// this world's RNG cursor. The raw signed wrapper return - the caller
    /// applies the module's clamp.
    fn capture_module_roll(
        &mut self,
        shape: &vm::cast_module_ticks::CastDamageShape,
        attacker: u8,
        target: u8,
    ) -> Option<i32> {
        use legaia_engine_vm::battle_damage_wrappers::{WrapperAttacker, WrapperDefender};
        use vm::cast_module_ticks::roll_module_hit;

        let element_affinity_pct = self.enemy_affinity_pct(attacker, target);
        let attacker_hp = self.actors.get(attacker as usize)?.battle.hp;
        let defender = self.summon_roll_defender(target)?;
        let a = WrapperAttacker {
            hp: attacker_hp,
            agl: self
                .battle_accuracy
                .get(attacker as usize)
                .copied()
                .unwrap_or(0),
            spell_power: self
                .battle_attack
                .get(attacker as usize)
                .copied()
                .unwrap_or(0),
            status: 0,
        };
        let d = WrapperDefender {
            hp: defender.hp,
            agl: defender.agl,
            stat_a: defender.stat_a,
            stat_b: defender.stat_b,
            status: 0,
            guard: 0,
        };
        let rng = [
            (self.next_rng() & 0x7fff) as u16,
            (self.next_rng() & 0x7fff) as u16,
            (self.next_rng() & 0x7fff) as u16,
        ];
        Some(roll_module_hit(
            shape,
            0,
            &a,
            &d,
            element_affinity_pct,
            rng,
            || (self.next_rng() & 0x7fff) as u16,
        ))
    }

    /// Pre-roll the status draws one whole-row sweep makes, in the order
    /// retail visits its seats: the two `FUN_80056798` calls PROT 0938's
    /// `0x4E` body makes per hittable seat (`0x801F7888` / `0x801F78B0`),
    /// which decide Venom then Toxic.
    ///
    /// The **damage** roll comes first, per seat, because retail's loop calls
    /// the wrapper before the status draws: each of the three sweeps opens
    /// its seat body with `jal 0x801DD4B0` on its own baked power
    /// ([`vm::cast_module_ticks::sweep_damage_shape_for`]) and stores the
    /// clamped net into `+0x14C` itself. So these bodies **own** the cast's
    /// HP outcome the way the PROT 0927 / 0966 stagers do, and
    /// [`Self::fold_pending_cast`] must not also run the generic fold - which
    /// is the double-application trap. The clamp is the kill-capable one
    /// (`sltu` at `0x801F77EC` / `0x801F7118` / `0x801F77E0`), unlike the two
    /// stagers' `HP - 1`.
    ///
    /// Returns `(seat, damage, (status_a, status_b))` in visit order.
    fn sweep_status_rolls(
        &mut self,
        ctx: &vm::cast_module_ticks::CastModuleCtx,
        seats: &[vm::cast_module_ticks::CastActorState],
        body: u32,
        caster: u8,
    ) -> Vec<(u8, i32, (u32, u32))> {
        use vm::cast_module_ticks as ticks;
        // Only the body's own sweep arm draws anything: retail reaches the
        // wrapper and the two status calls inside that one arm, so rolling on
        // every re-entry would run the shared `rand()` cursor forward on
        // frames retail never draws.
        let sweep_arm = match body {
            ticks::CHAOS_BREATH_TICK => ticks::CHAOS_BREATH_SWEEP_ARM,
            ticks::MYSTIC_CIRCLE_TICK => ticks::MYSTIC_CIRCLE_SWEEP_ARM,
            _ => ticks::DOOMSDAY_SWEEP_ARM,
        };
        if ctx.phase != sweep_arm {
            return Vec::new();
        }
        let shape = ticks::sweep_damage_shape_for(body);
        let mut out = Vec::new();
        for seat in 0..ctx.actor_count {
            let Some(s) = seats.get(seat as usize) else {
                continue;
            };
            // PROT 0938's `0xB7` body and PROT 0965's skip only a dead seat;
            // the `0x4E` body also skips `+0x16E & 4`.
            let hittable = if body == ticks::CHAOS_BREATH_TICK {
                ticks::aoe_seat_is_hittable(s)
            } else {
                s.hp != 0
            };
            if !hittable {
                continue;
            }
            let damage = match shape {
                Some(sh) => self.capture_module_roll(sh, caster, seat).unwrap_or(0),
                None => 0,
            };
            out.push((seat, damage, (self.next_rng(), self.next_rng())));
        }
        out
    }

    /// The three accessory ids PROT 0955's Void Accessories rolls between -
    /// `record[+0x19B + slot]` for the character seated at `slot`.
    fn cast_victim_accessories(&self, slot: u8) -> [u8; 3] {
        let mut out = [0u8; 3];
        let Some(rec) = self
            .roster
            .members
            .get(self.party_roster_slot(slot as usize))
        else {
            return out;
        };
        // `+0x196` is equipment slot 0, so `+0x19B` - the module's
        // `+ 0x75E + 5` - is index 5, and the three accessory slots are
        // 5, 6, 7 (`legaia_save::character::EquipmentSlots`).
        let eq = rec.equipment();
        for (i, o) in out.iter_mut().enumerate() {
            *o = eq
                .slots
                .get(ACCESSORY_EQUIP_SLOT_0 + i)
                .copied()
                .unwrap_or(0);
        }
        out
    }

    /// Apply one enemy-cast hit through retail's **safe** applier - the hit
    /// arm of `FUN_801E09F8`'s per-slot effect-child driver
    /// (`legaia_engine_vm::battle_cast_census::effect_child_hit`).
    ///
    /// This is the path a monster's cast takes in retail, and it differs from
    /// the action band's accumulating seed in the one way that matters: the
    /// roll is clamped against live HP **once** and that single value reaches
    /// both the readout accumulator `+0x10` and live HP `+0x14C`, so the bar
    /// can never be asked to travel further than HP moved. The action band's
    /// seed can, which is the `0x51` settle park
    /// `legaia_engine_vm::battle_hp_bar` documents.
    ///
    /// Also carried: the reaction-clip pick (`+0x1F2` gates `+0x1F1` against
    /// `+0x1EF` / `+0x1F0`, and a dead victim always takes `+0x1F1`), the
    /// `+0x1DC` **bit** ORs - retail ORs here, it does not bump - and the
    /// readout cursor `ctx[+0x262]`.
    ///
    /// Returns the damage actually applied.
    pub(in crate::world) fn apply_effect_child_hit(&mut self, slot: usize, damage: i32) -> i32 {
        use vm::battle_cast_census::{EffectChildVictim, effect_child_hit};
        let mut cursor = self.battle_ctx.cast_readout_cursor;
        let Some(a) = self.actors.get_mut(slot) else {
            return 0;
        };
        a.battle.arm_hp_bar();
        let p = |i: usize| a.battle.params.get(i - 0x1DF).copied().unwrap_or(0);
        let mut victim = EffectChildVictim {
            hp: a.battle.hp,
            hp_bar_delta: a.battle.hp_bar_pending,
            flags: a.battle.field_flags,
            staged_anim: a.battle.queued_anim,
            restage: 0,
            reaction_alt: p(0x1EF),
            reaction_alt2: p(0x1F0),
            knockdown_anim: p(0x1F1),
            reaction_gate: p(0x1F2),
        };
        let hit = effect_child_hit(&mut victim, damage, &mut cursor);
        a.battle.hp = victim.hp;
        a.battle.hp_bar_pending = victim.hp_bar_delta;
        a.battle.queued_anim = victim.staged_anim;
        // `hp == 0 -> liveness = 0` holds for **present** actors only, the
        // same `max_hp > 0` guard `apply_battle_hp_delta` applies.
        if a.battle.max_hp > 0 && a.battle.hp == 0 {
            a.battle.liveness = 0;
        }
        self.battle_ctx.cast_readout_cursor = cursor;
        hit.applied
    }
}

#[cfg(test)]
mod capture_hold_tests {
    use super::*;

    fn band_world() -> World {
        let mut world = World {
            party_count: 3,
            ..World::default()
        };
        while world.actors.len() < 8 {
            world.actors.push(crate::world::Actor::default());
        }
        for a in world.actors.iter_mut() {
            a.active = true;
            a.battle.hp = 100;
            a.battle.max_hp = 100;
            a.battle.liveness = 1;
        }
        world.mode = SceneMode::Battle;
        world
    }

    /// Nothing paged in: the seam is inert, so a host that never reaches the
    /// capture band behaves exactly as it did before the hold existed.
    #[test]
    fn an_unarmed_band_is_never_busy() {
        let mut world = band_world();
        assert!(world.capture_cast_spell.is_none());
        assert!(!world.capture_stager_tick());
    }

    /// Arming resets the module phase pair - retail's `sb zero,0x279(v0)` at
    /// the `0x6F` exit (`0x801E5048`), just before `0x70` starts ticking.
    #[test]
    fn arming_resets_the_module_phase_pair() {
        let mut world = band_world();
        world.cast_module_phase = 9;
        world.cast_module_ctx_278 = 7;
        world.arm_capture_cast_module(0x87);
        assert_eq!(world.capture_cast_spell, Some(0x87));
        assert_eq!(world.cast_module_phase, 0);
        assert_eq!(world.cast_module_ctx_278, 0);
    }

    /// A spell that names no band entry arms nothing, so no host can be
    /// parked on a module that is not there.
    #[test]
    fn a_spell_with_no_module_arms_nothing() {
        let mut world = band_world();
        world.arm_capture_cast_module(0x00);
        assert!(world.capture_cast_spell.is_none());
        assert!(!world.capture_stager_tick());
    }

    /// The no-softlock rule. `CastModuleCodeRun::busy` seeds `true` for any
    /// resident entry, so a module whose tick body is unported would hold
    /// phase `0x70` forever; the hold reads `tick_ported` first and lets the
    /// band through instead, disarming as it goes.
    #[test]
    fn an_unported_tick_body_never_holds_the_phase() {
        let mut world = band_world();
        // PROT 0909's code half is a *stager*, not a tick body.
        assert_eq!(world.cast_module_for(0x87), Some(909));
        let run = world.run_cast_module_code(0x87, 0).unwrap();
        assert!(run.busy, "the seeded value on its own says 'busy'");
        assert!(!run.tick_ported, "but no tick body ran");

        world.arm_capture_cast_module(0x87);
        assert!(
            !world.capture_stager_tick(),
            "so the band is not held on it"
        );
        assert!(
            world.capture_cast_spell.is_none(),
            "and the module stops being re-entered"
        );
    }
}
