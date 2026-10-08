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
//! * `0x36`, stager phase 11 - the creature closer in (`z=-1606`) on clip
//!   `1`, the walk, with the flame part-actors live and the victim still at
//!   full HP: the walk arm holds on the range poll `FUN_8004E2F0(7, victim)`
//!   and lands the hit only once the creature reaches the victim.
//!
//! So the retail creature walks **in from behind the party** onto the target
//! while its effect parts play, and the damage lands when it arrives. The
//! stager here does the same with the pieces the engine has: it requests the
//! namesake creature spawn ([`crate::world::CastFxState::pending_summon_spawn`]) at the spawn
//! point, idles it, stages the walk clip and walks it onto the victim until
//! the range metric reads in range (a module with no directed walk arm glides
//! it to the fixed strike point [`SUMMON_STRIKE_BEHIND`] instead), folds the
//! outcome there, lingers, and despawns it.
//!
//! The per-summon effect parts are staged with it. They used to be the open
//! half here ("the `0x180C` move-VM records are not staged"); the module's
//! spawn records now come out of the **cast-effect pool**
//! ([`legaia_asset::cast_effect_pool`], installed by the scene host) and stage
//! through [`World::spawn_cast_module_fx`] at the same first tick, because the
//! records are the shape the summon path already runs. What stays open is the
//! module's *code* half - lift, camera, phase machine, damage shape - where a
//! module's tick bodies are not ported
//! (`docs/subsystems/cast-module.md`).
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
/// Where an undirected module's walk ends and the outcome lands. A directed
/// walk arm walks onto the victim instead (`summon_walk_to_victim`); this is
/// the `gimard_burning_attack` capture's mid-walk `-1606 - (-542)`.
pub const SUMMON_STRIKE_BEHIND: i16 = 1064;
/// Frames the creature idles at its spawn point before the walk.
const SUMMON_IDLE_FRAMES: u16 = 30;
/// Walk speed, world units per frame.
const SUMMON_WALK_STEP: i16 = 12;
/// A directed walk arm's creature speed, world units per display frame,
/// along its heading onto the victim: Gimard's walk clip (archive creature
/// `10`, clip 1, tag `1`) carries root speed `28`, which the anim tick's
/// root-motion term integrates for a creature seated with its clips. A
/// per-vsync log from `shiny_refactor_gimard_plus35` measures the same rate:
/// each game frame at step `4` moves the creature `111` units (`(-16, 110)`)
/// and swings the yaw base `6 * step * scalar + drift = 197`, and the arm's
/// first pass moves the creature before the swing has run - which is what an
/// earlier reading off `gimard_burning_attack`'s yaw (`985`, read as 20.5
/// frames and `32.7` a frame) left out. This constant only walks a creature
/// whose playing clip carries no speed (a headless seat).
const SUMMON_DIRECTED_WALK_STEP: i32 = 28;
/// Frames the creature stands at the strike point after the outcome.
const SUMMON_LINGER_FRAMES: u16 = 40;
/// A host with no creature to seat (a headless driver) still owes the
/// outcome: fold it after this many frames without a seat.
const SUMMON_UNSEATED_GRACE: u16 = 60;

/// The arm of PROT 0927's and PROT 0966's nine that carries the sweep - the
/// `0x801F6A60` / `0x801F6A50` table's working entry. The other eight are
/// spawn arms the pool already stages.
pub const AOE_STAGER_WORKING_ARM: u8 = 4;

/// The screen fade PROT 0966's arm `phase` spawns on its pass, each a
/// `FUN_80024E80(0x801C9070, 1)` over the template the arm writes:
///
/// | arm | kind | frames | from | to | delay / hold |
/// |---|---|---|---|---|---|
/// | 2 (`0x801F6EE8`) | 1 | `0x10` | `(0xFF, 0x40, 0x40)` | black | - |
/// | 4 (`0x801F6FC0`) | 1 | `0x40` | black | white | held |
/// | 10 (`0x801F70A0`) | 1 | `0x80` | white | black | - |
/// | 26 (`0x801F8574`) | 1 | `0x40` | black | white | `0xC0` delay, held |
/// | 27 (`0x801F879C`) | 1 | `0x80` | white | black | - |
///
/// Arm 26 spawns three fades at once (a white flash, a held blue grade, and
/// the delayed white-in); the engine has one fade seat, so it carries the
/// last, which is the one still on screen when arm 27 fades out of it.
fn evil_seru_magic_fade(phase: u8) -> Option<crate::fade::FadeTemplate> {
    let (duration, start_rgb, end_rgb, mode) = match phase {
        2 => (0x10, [0xFF, 0x40, 0x40], [0; 3], [0, 0, 0]),
        4 => (0x40, [0; 3], [0xFF; 3], [0, -1, 0]),
        10 | 27 => (0x80, [0xFF; 3], [0; 3], [0, 0, 0]),
        26 => (0x40, [0; 3], [0xFF; 3], [0xC0, -1, 0]),
        _ => return None,
    };
    Some(crate::fade::FadeTemplate {
        kind: 1,
        duration,
        start_rgb,
        end_rgb,
        mode,
    })
}

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

/// The walk entry a melee body stages while its caster closes in (the
/// literal `1` of every `li v1,0x1; sb v1,0x1da(<caster>)` walk site).
pub const CAPTURE_WALK_ENTRY: u8 = 1;

/// Longest the band waits on the caster's stages before it gives up and lets
/// the module run - a guard against a clip that never commits, not a timing.
pub const CASTER_STAGE_TICK_LIMIT: u16 = 900;

/// The caster's clip stages a capture-class body owes and its port does not
/// write ([`legaia_engine_vm::cast_module_ticks::CAPTURE_CASTER_STAGES`]),
/// in flight at the head of battle phase `0x70`.
///
/// Each stage is written into the caster's `+0x1DA` and the next one is
/// written behind it as soon as it commits, so the commit's boundary rule
/// plays every clip to its natural end; the last is followed by `0`, retail's
/// closing `sb zero,0x1DA(<caster>)`, and the run ends when the caster is
/// back on its idle entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CasterStageRun {
    /// The casting seat.
    pub slot: u8,
    /// The clips to stage, in order.
    pub clips: Vec<u8>,
    /// Index of the clip being staged / waited on.
    pub cursor: usize,
    /// The seat the caster walks into reach of before the first stage
    /// ([`vm::cast_module_ticks::capture_body_approaches`]); `None` for a
    /// body that strikes from where it stands, or a group target.
    pub approach: Option<u8>,
    /// Where the run is.
    pub phase: CasterStagePhase,
    /// Ticks spent holding the band so far ([`CASTER_STAGE_TICK_LIMIT`]).
    pub ticks: u16,
}

/// Whether the clip actor `a` has committed has run as far as it will on its
/// own: played out, or reached its authored loop window (a park, or the start
/// of a looping tail) with cycles still owed.
fn caster_clip_settled(a: &crate::world::Actor) -> bool {
    let Some(p) = a.battle_animation.as_ref() else {
        return true;
    };
    if p.finished() {
        return true;
    }
    let window_start = a
        .battle_action_clips
        .as_ref()
        .and_then(|c| c.get(usize::from(a.battle.current_anim)))
        .and_then(|c| c.as_ref())
        .and_then(|c| c.entry_loop_window())
        .map(|(_, start, _)| i16::from(start));
    matches!(window_start, Some(w) if p.loop_cycles_remaining() > 0 && p.current_frame() >= w)
}

/// The three stretches of a [`CasterStageRun`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CasterStagePhase {
    /// Walking into reach: the walk entry `1` staged and the caster turned
    /// onto its victim every tick (its root motion carries it), until the
    /// range poll `FUN_8004E2F0` reads zero.
    Approach,
    /// Staging the clips: each is held until it has committed and either
    /// played out or reached its authored loop-window park.
    Staging,
    /// The caster sits on the last clip (parked, when it has a window) while
    /// the module's own arms run.
    Module,
    /// The module is done: the park is released (`sh zero,0x176(<caster>)`)
    /// and idle staged behind it (`sb zero,0x1DA(<caster>)`); the band holds
    /// until the caster is back on its idle entry.
    Closing,
}

/// What [`World::capture_stager_tick`] does with the module this tick.
enum CasterStageGate {
    /// The caster's stages hold the band; the module does not run.
    Hold,
    /// Run the module.
    RunModule,
    /// The run closed out: leave `0x70`.
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
    /// The creature's walk has reached [`Self::goal`] (the answer a directed
    /// module's walk arm polls).
    pub walked: bool,
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
        let party_count = self.party.party_count.clamp(1, 3);
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
        let caster_is_party = caster < self.party.party_count;
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
        self.casting.pending_cast = Some(PendingCast {
            caster,
            spell_id: def.id,
            targets,
        });
        self.battle_ctx.active_actor = caster;
        self.battle_ctx.queued_action = ActionCategory::Magic.as_byte();
        self.battle_ctx.action_state = ActionState::Begin.as_byte();
    }

    /// The monster twin of [`Self::arm_player_cast`]. A monster's stream
    /// carries its cast clip behind the spell id (`params[1]`, the `+0x1E0`
    /// byte the `0x29` arm stages into `+0x1DA` - see
    /// [`Self::monster_cast_clip`]), then its opening camera shot
    /// (`params[2]`, `+0x1E1`, a case of the cast-effect driver - see
    /// `SpellAnimPairs::opening_shot`), terminated at `params[3]`
    /// (`FUN_801E9FD4`, `0x801EA53C..0x801EA588`); a monster with no such
    /// clip installed stages the terminator at once and the band ends the
    /// action after the wait.
    pub(in crate::world) fn arm_monster_cast(
        &mut self,
        slot: u8,
        def: &crate::spells::SpellDef,
        targets: Vec<u8>,
    ) {
        let code = self.cast_target_code(def, &targets, slot);
        let clip = self.monster_cast_clip(slot, def.id).unwrap_or(0xFF);
        // A table that was not read stages the terminator in the shot's
        // place, the stream the port carried before the shot existed.
        let shot = self
            .battle
            .spell_anim_pairs
            .opening_shot(def.id)
            .unwrap_or(0xFF);
        self.clear_action_stream(slot);
        if let Some(a) = self.actors.get_mut(slot as usize) {
            a.battle.active_target = code;
            a.battle.action_category = ActionCategory::Magic.as_byte();
            a.battle.params[0] = def.id;
            a.battle.params[1] = clip;
            a.battle.params[2] = shot;
            a.battle.params[3] = 0xFF;
            a.battle.sub_route = 0;
            // The picker stores the same entry into `+0x1E7` one instruction
            // ahead of `+0x1E0` (`sb s2,0x1e7(s4)` at `0x801EA53C`, and the
            // `OneAlly` re-pick's `sb s5,0x1e7(s4)` at `0x801EA6C4`). That is
            // the byte the action seed's **Spirit band** stages instead of
            // `+0x1E0` (`lbu v0,0x1e7(s3); sb v0,0x1da(s3)` at
            // `0x801E3B4C..0x801E3B54`), which every monster cast of a
            // class-`< 0x14` record below id `0x65` - the heals and buffs -
            // runs through. A walk that found no entry writes neither byte.
            if clip != 0xFF {
                a.battle.queued_anim_b = clip;
            }
        }
        self.casting.pending_cast = Some(PendingCast {
            caster: slot,
            spell_id: def.id,
            targets,
        });
        self.battle_ctx.active_actor = slot;
        self.battle_ctx.queued_action = ActionCategory::Magic.as_byte();
        self.battle_ctx.action_state = ActionState::Begin.as_byte();
    }

    /// The archive entry a monster's cast plays - the `+0x1E0` byte the
    /// picker `FUN_801E9FD4` stores beside the spell id.
    ///
    /// Retail's generic magic pick does not look the clip up by the spell id.
    /// It counts the record's live magic slots (`+0x21..+0x23` at `>= 2`,
    /// `0x801EA3E0..0x801EA408`), rolls `k = rand() % count`, then walks the
    /// entry table from index 2 counting the entries tagged `0x23`
    /// (`0x801EA4C8..0x801EA4D0`): the `k`-th one is the clip, and the spell
    /// is magic slot `count - 1 - k` (`+0x21 + (s1 - 1)` with `s1` counted
    /// down beside `k`, `0x801EA4E0..0x801EA544`). So the clip is keyed on the
    /// spell's **slot**, reversed, and on no tag the spell id names - Gimard's
    /// Tail Fire `0x27` plays his lone tag-`0x23` entry 8, which a search for
    /// tag `0x27` never finds (`battle_gimard_tail_fire_a` reads `+0x1E0 = 8`).
    ///
    /// A spell outside the record's magic slots (a per-monster scripted
    /// cast) falls back to the entry whose tag is the spell id.
    ///
    /// PORT: FUN_801E9FD4 (the tag-`0x23` clip walk, `0x801EA4A4..0x801EA548`)
    pub(in crate::world) fn monster_cast_clip(&self, slot: u8, spell_id: u8) -> Option<u8> {
        /// The tag the picker walks for a castable's clip.
        const CAST_CLIP_TAG: u8 = 0x23;
        /// The walk starts past the idle and walk entries (`li s2,0x2`).
        const FIRST_CAST_ENTRY: usize = 2;
        let actor = self.actors.get(slot as usize)?;
        let clips = actor.battle_action_clips.as_ref()?;
        let magic = actor
            .battle_monster_id
            .and_then(|id| self.tables.monster_catalog.get(id))
            .map(|d| d.magic_attacks.as_slice())
            .unwrap_or(&[]);
        if let Some(m) = magic.iter().position(|&id| id == spell_id) {
            let k = magic.len() - 1 - m;
            return clips
                .iter()
                .enumerate()
                .skip(FIRST_CAST_ENTRY)
                .filter(|(_, c)| c.as_ref().is_some_and(|c| c.action_id == CAST_CLIP_TAG))
                .nth(k)
                .and_then(|(i, _)| u8::try_from(i).ok());
        }
        clips
            .iter()
            .position(|c| c.as_ref().is_some_and(|c| c.action_id == spell_id))
            .and_then(|i| u8::try_from(i).ok())
    }

    /// Fold the owed cast's outcome - exactly once. The MP was the band's
    /// (`0x28`), so the fold is the prepaid one; a monster cast also rolls
    /// its move's impact-status and AGL-status procs onto what it reached.
    pub(in crate::world) fn fold_pending_cast(&mut self) {
        let Some(pc) = self.casting.pending_cast.take() else {
            return;
        };
        // A capture body that took a branch with no damage site (PROT 0953's
        // charge, `0x801F6CEC`) owes nothing: retail never reaches its
        // `FUN_801DD6B4` on that path.
        if std::mem::take(&mut self.casting.module_skips_fold) {
            return;
        }
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
            && vm::cast_module_ticks::tick_body_owns_the_fold(entry, body)
            && let Some(arm) = vm::cast_module_ticks::sweep_arm_for(entry, body)
            && self.casting.module_phase > arm
        {
            return;
        }
        // --- W1-D: the fourteen trampoline arms ---
        // The same property for the three whole-row sweeps among them (PROT
        // 0941's `0xB9`, 0950's `0xAB`, 0956's `0x71`): each rolls its own
        // baked power per hittable seat and stores the clamped net into
        // `+0x14C` itself, over a seat range the spell record cannot express.
        // The `phase > arm` test is the same guard - a host that never drove
        // the band past the sweep arm is still owed the generic fold.
        if let Some(entry) = self.cast_module_for(pc.spell_id)
            && let Some(body) = vm::cast_module_ticks::capture_tick_body(entry, pc.spell_id)
            && let Some(arm) = vm::cast_arm_ticks::arm_sweep_arm(entry, body)
            && self.casting.module_phase > arm
        {
            return;
        }
        // --- end W1-D ---
        // A monster's capture-class special is not in the catalog (its record
        // is the module's), so the fold resolves it the way the arm did -
        // and only when the module has a damage site to seed it with; a
        // status-only body (Glare) folds nothing here.
        let def = self
            .tables
            .spell_catalog
            .get(pc.spell_id)
            .cloned()
            .or_else(|| {
                self.monster_capture_power(pc.caster, pc.spell_id)
                    .and_then(|_| self.monster_cast_def(pc.spell_id))
            });
        let Some(def) = def else {
            return;
        };
        let hit_fx_start = self.battle.hit_fx.len();
        self.cast_spell_on_slots_prepaid(pc.caster, &def, &pc.targets);
        if pc.caster >= self.party.party_count {
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
        let Some(pc) = &self.casting.pending_cast else {
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
        // The capture band (`0x6E..=0x71`) sits above `0x50` in the state
        // space but is not a door out: its module has not run yet, so its
        // fold waits for the band's own exit into `0x50`, after the caster's
        // stages and the module's arms - where retail's module lands its hit.
        let capture_band = (ActionState::MagicCaptureBranch.as_byte()
            ..=ActionState::MagicCaptureFinalize.as_byte())
            .contains(&to);
        let band_over = to >= ActionState::DoneCleanup.as_byte() && !capture_band;
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
        self.casting.summon_seat_owed = None;
        // The cast's side-band stream: its `summon.dat` texture slots land in
        // the battle VRAM before the module's parts sample them.
        if let Some(summon_dat) = self.tables.summon_dat.clone() {
            crate::battle_sideband_textures::record_cast_sideband_textures(
                &summon_dat,
                spell_id,
                &mut self.battle.vram_loads,
            );
        }
        // Retail's cast-start site `0x801E4B1C` zeroes `ctx+0x278` and the
        // module phase `ctx+0x279` before the first tick.
        self.casting.module_phase = 0;
        self.casting.module_ctx_278 = 0;
        // A new cast re-arms the Nighto verdict; the roll happens on the first
        // tick of the module, the way retail's arm 0 draws it. The ring sweep
        // starts from zero for the same reason.
        self.casting.module_nighto_outcome = None;
        self.casting.module_ring_angle = 0;
        self.casting.module_theeder = Default::default();
        self.casting.module_swordie = Default::default();
        self.casting.module_settle_countdown = 0;
        self.casting.module_cam = Default::default();
        self.casting.summon_stager = Some(SummonStager {
            caster,
            spell_id,
            phase: SummonPhase::Armed,
            frames: 0,
            spawn: [cx, cy, cz.saturating_sub(SUMMON_SPAWN_BEHIND)],
            goal: [cx, cy, cz.saturating_sub(SUMMON_STRIKE_BEHIND)],
            walked: false,
        });
        self.emit_cast_module_voice(spell_id);
    }

    /// Raise the paged module's **own** CD-XA voice - the head cue every
    /// slot-B image calls the dispatcher `FUN_8004FCC8` with near its head
    /// (`docs/subsystems/cast-module.md`, "The cast's own CD-XA voice"),
    /// scanned off the module's bytes at cast time
    /// ([`legaia_engine_vm::battle_cast_cue::module_head_cue`]) and run
    /// through the dispatcher's CD-XA arm
    /// ([`legaia_engine_vm::battle_cast_cue::admit_voice_cue`]).
    ///
    /// Retail raises it from inside the module's first tick, after the
    /// module's own poll of the side-band stage byte; the port raises it at
    /// the arming seam, the frame the module becomes resident - which is
    /// the same frame, since the port's side-band is resident rather than
    /// streamed (`VoiceCueGates::side_band_stage` is `0` here for that
    /// reason). The other gate is live: a cast inside the previous clip's
    /// read span (`battle_xa_busy_frames`, retail's `gp+0x91C`) plays no
    /// voice, exactly as `FUN_8003DE7C(1)` declines it.
    ///
    /// The request lands on [`crate::world::AudioState::battle_xa_cues`],
    /// the `(clip_slot, channel, dur)` channel both hosts already play
    /// through their XA lane, and arms the busy span the way the melee
    /// kernel's clip start does. Nothing here touches gameplay state; a
    /// disc-free build (no pool, no span table) emits nothing.
    ///
    /// REF: FUN_8004FCC8 (the CD-XA arm), FUN_8003D53C (the starter the
    /// request stands for)
    fn emit_cast_module_voice(&mut self, spell_id: u8) {
        let Some(entry) = self.cast_module_for(spell_id) else {
            return;
        };
        let Some(pool) = self.casting.effect_pool.clone() else {
            return;
        };
        let Some(module) = pool.module(entry) else {
            return;
        };
        let Some(head) = vm::battle_cast_cue::module_head_cue(&module.bytes) else {
            return;
        };
        let id = head.resolve(|| self.next_rand());
        self.emit_battle_xa_cue(id);
    }

    /// Run one CD-XA cue id through the dispatcher's CD-XA arm
    /// ([`legaia_engine_vm::battle_cast_cue::admit_voice_cue`]) onto
    /// [`crate::world::AudioState::battle_xa_cues`] - the half of
    /// [`Self::emit_cast_module_voice`] after the module's head cue is
    /// resolved, and what a module arm's own `FUN_8004FCC8(id >= 0x100)` call
    /// raises.
    ///
    /// REF: FUN_8004FCC8 (the CD-XA arm)
    pub(in crate::world) fn emit_battle_xa_cue(&mut self, id: u16) {
        let raw = self
            .audio
            .xa_cue_durations
            .as_deref()
            .and_then(|t| t.get(usize::from(id).wrapping_sub(0x100)).copied());
        let gates = vm::battle_cast_cue::VoiceCueGates {
            side_band_stage: 0,
            clip_span_left: self.audio.battle_xa_busy_frames,
        };
        if let vm::battle_cast_cue::VoiceCueVerdict::Play(req) =
            vm::battle_cast_cue::admit_voice_cue(id, gates, raw)
        {
            // The starter holds the drive for the clip's read span
            // (`FUN_8003D53C`; the same latch `push_battle_xa_cue` arms for
            // the melee kernel's clips).
            self.audio.battle_xa_busy_frames = req.duration_sectors.min(u16::MAX as u32) as u16;
            self.audio.battle_xa_cues.push(crate::sfx_cue::XaVoiceClip {
                clip: req.clip_slot,
                channel: req.channel,
                duration_sectors: req.duration_sectors,
            });
        }
    }

    /// Every cast-voice clip `spell_id`'s module can raise - the
    /// [`Self::emit_cast_module_voice`] resolution without its gates or its
    /// coin flip: a literal head cue gives one request, a random one
    /// (`base + rand() % span`, PROT 0936 / 0937) one per candidate. Empty
    /// when the module, its head cue or the span table is missing.
    pub(in crate::world) fn cast_module_voice_candidates(
        &self,
        spell_id: u8,
    ) -> Vec<crate::sfx_cue::XaVoiceClip> {
        let Some(entry) = self.cast_module_for(spell_id) else {
            return Vec::new();
        };
        let Some(module) = self
            .casting
            .effect_pool
            .as_ref()
            .and_then(|pool| pool.module(entry))
        else {
            return Vec::new();
        };
        let ids: Vec<u16> = match vm::battle_cast_cue::module_head_cue(&module.bytes) {
            Some(vm::battle_cast_cue::ModuleHeadCue::Literal(id)) => vec![id],
            Some(vm::battle_cast_cue::ModuleHeadCue::Random { base, span }) => {
                (0..u16::from(span.max(1)))
                    .map(|k| base.wrapping_add(k))
                    .collect()
            }
            None => Vec::new(),
        };
        let open = vm::battle_cast_cue::VoiceCueGates {
            side_band_stage: 0,
            clip_span_left: 0,
        };
        ids.into_iter()
            .filter_map(|id| {
                let raw = self
                    .audio
                    .xa_cue_durations
                    .as_deref()
                    .and_then(|t| t.get(usize::from(id).wrapping_sub(0x100)).copied());
                match vm::battle_cast_cue::admit_voice_cue(id, open, raw) {
                    vm::battle_cast_cue::VoiceCueVerdict::Play(req) => {
                        Some(crate::sfx_cue::XaVoiceClip {
                            clip: req.clip_slot,
                            channel: req.channel,
                            duration_sectors: req.duration_sectors,
                        })
                    }
                    _ => None,
                }
            })
            .collect()
    }

    /// List the cast voices the round's committed party spells may raise onto
    /// [`crate::world::AudioState::battle_xa_prestage`] - called at the
    /// round's start, before the first action dispatches.
    pub fn list_round_cast_voices(&mut self) {
        let spells: Vec<u8> = self
            .battle
            .round_flow
            .pending
            .iter()
            .filter_map(|p| match p {
                Some(crate::battle_round::PendingPartyAction::Spell { spell_id, .. }) => {
                    Some(*spell_id)
                }
                _ => None,
            })
            .collect();
        for spell in spells {
            for clip in self.cast_module_voice_candidates(spell) {
                if !self.audio.battle_xa_prestage.contains(&clip) {
                    self.audio.battle_xa_prestage.push(clip);
                }
            }
        }
    }

    /// A host seated the summon creature at actor `slot`: adopt the seat,
    /// place it at the stager's spawn point wearing the caster's facing (the
    /// capture's slot-7 record), and mark it active. Hosts call this right
    /// after binding the creature's mesh, idle player and clip set.
    pub fn seat_summon_actor(&mut self, slot: usize) {
        let staged = self.casting.summon_stager.as_ref().map(|st| {
            (
                st.spawn,
                self.actors
                    .get(st.caster as usize)
                    .map(|a| a.battle.facing_angle)
                    .unwrap_or(0),
            )
        });
        self.casting.summon_actor_slot = Some(slot as u8);
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

    /// The seat half of a summon spawn request for a host that draws
    /// nothing: take [`crate::world::CastFxState::pending_summon_spawn`] and
    /// seat an unrendered creature at the hosts' slot (`8 + party_count`),
    /// through the same [`Self::seat_summon_actor`] the play hosts call
    /// after binding the mesh.
    ///
    /// The seat is not presentation-only. A directed module's walk arm
    /// (PROT 0903's arm 11) walks the seated creature and lands the hit when
    /// it reaches the victim; with no creature seated the arm passes on its
    /// first tick, so a headless run folded the outcome hundreds of frames
    /// before either play host would. Returns `true` when a creature was
    /// seated.
    pub fn seat_summon_creature_unrendered(&mut self) -> bool {
        if self.mode != SceneMode::Battle {
            return false;
        }
        if self.take_pending_summon_spawn().is_none() {
            return false;
        }
        let slot = self
            .casting
            .summon_actor_slot
            .map_or(8 + usize::from(self.party.party_count), usize::from);
        let Some(a) = self.actors.get_mut(slot) else {
            return false;
        };
        a.active = true;
        self.seat_summon_actor(slot);
        true
    }

    /// Install the cast-effect pool - the DATA half of the slot-B cast-module
    /// band (PROT 0903..0966), parsed off the disc by the scene host (which
    /// holds the PROT index; `World` is index-agnostic, the same split
    /// [`crate::world::CastFxState::pending_summon_spawn`](crate::world::CastFxState::pending_summon_spawn)
    /// uses). Idempotent; a host that never calls it leaves every cast staging
    /// no module records, which is the disc-free behaviour.
    pub fn install_cast_effect_pool(
        &mut self,
        pool: std::sync::Arc<legaia_asset::cast_effect_pool::CastEffectPool>,
    ) {
        self.casting.effect_pool = Some(pool);
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
        self.spell_table_sub_class(spell_id).or_else(|| {
            self.tables
                .spell_catalog
                .get(spell_id)
                .map(|d| d.effect_class)
        })
    }

    /// Stage the cast module's **spawn records** at `origin` - the engine's
    /// answer to the dispatchers, and the half of a cast that is data.
    ///
    /// Each record is `[i16 model_sel][u16 reserved][move-VM bytecode]`, the shape
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
        let Some(pool) = self.casting.effect_pool.clone() else {
            return false;
        };
        let Some(module) = pool.module(entry) else {
            return false;
        };
        if module.parts.is_empty() {
            return false;
        }
        // A module whose body is ported whole seats its own records, each on
        // the pass its arm spawns it.
        if entry == vm::cast_fatal_decision::FATAL_DECISION_ENTRY {
            return false;
        }
        self.casting.active_summon = Some(crate::summon::SummonScene::spawn_parts(
            &module.parts,
            &module.bytes,
            crate::scene::EFFECT_MODEL_LIBRARY_BASE,
            origin,
        ));
        true
    }

    /// Seat the spawn calls one module arm made this pass, each on its own
    /// anchor, into the cast's running scene - the per-arm form of
    /// [`Self::spawn_cast_module_fx`] for a module whose director reports its
    /// spawns ([`vm::cast_module_camera::ModuleProfile::stages_spawns`]).
    ///
    /// A record's program is bounded by the next record start, so each one
    /// is parsed against the whole record set of its image: the module's own
    /// recovered records for a [`SpawnRecord::Module`], the effect-prototype
    /// table's for a [`SpawnRecord::BattleProto`].
    ///
    /// REF: FUN_80021B04 (the spawn calls), FUN_80058490 (the `MoveImage`)
    ///
    /// [`SpawnRecord::Module`]: vm::cast_module_camera::SpawnRecord::Module
    /// [`SpawnRecord::BattleProto`]: vm::cast_module_camera::SpawnRecord::BattleProto
    fn stage_module_arm_spawns(
        &mut self,
        prot_entry: u32,
        spawns: &[vm::cast_module_camera::ModuleSpawn],
        shot: Option<vm::cast_module_camera::ModuleShot>,
    ) {
        use legaia_asset::move_power::{self, BATTLE_OVERLAY_BASE};
        use vm::cast_module_camera::SpawnRecord;
        const LINK_BASE: u32 = legaia_asset::summon_overlay::SUMMON_OVERLAY_LINK_BASE;
        let creature = self
            .casting
            .module_cam
            .creature
            .or(self.casting.module_cam.creature_live);
        let victim = self
            .casting
            .module_cam
            .victim_slot
            .and_then(|s| self.actors.get(usize::from(s)))
            .map(|a| vm::cast_module_camera::ModuleSeat {
                x: a.move_state.world_x,
                y: a.move_state.world_y,
                z: a.move_state.world_z,
                facing: a.battle.facing_angle & 0xFFF,
            });
        for spawn in spawns {
            let Some((pos, rot)) =
                vm::cast_module_camera::spawn_anchor_point(spawn.anchor, creature, victim, shot)
            else {
                continue;
            };
            let (bytes, parts, off): (std::sync::Arc<[u8]>, Vec<_>, usize) = match spawn.record {
                SpawnRecord::Module(va) => {
                    let Some(module) = self
                        .casting
                        .effect_pool
                        .as_ref()
                        .and_then(|p| p.module(prot_entry))
                    else {
                        continue;
                    };
                    let Some(off) = va.checked_sub(LINK_BASE).map(|o| o as usize) else {
                        continue;
                    };
                    let mut offs: Vec<usize> = module.parts.iter().map(|p| p.record_off).collect();
                    offs.push(off);
                    let parts =
                        legaia_asset::summon_overlay::parse_records_at(&module.bytes, &offs);
                    (module.bytes.clone(), parts, off)
                }
                SpawnRecord::BattleProto(word_va) => {
                    let Some(overlay) = self.tables.move_power_overlay.clone() else {
                        continue;
                    };
                    let Some(ptr) = word_va
                        .checked_sub(BATTLE_OVERLAY_BASE)
                        .and_then(|o| overlay.get(o as usize..o as usize + 4))
                        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
                    else {
                        continue;
                    };
                    let Some(off) = ptr.checked_sub(BATTLE_OVERLAY_BASE).map(|o| o as usize) else {
                        continue;
                    };
                    let mut offs: Vec<usize> = move_power::parse_effect_proto_records(&overlay)
                        .unwrap_or_default()
                        .iter()
                        .map(|p| p.record_off)
                        .collect();
                    offs.push(off);
                    let parts = legaia_asset::summon_overlay::parse_records_at(&overlay, &offs);
                    (overlay, parts, off)
                }
            };
            let picked: Vec<_> = parts.into_iter().filter(|p| p.record_off == off).collect();
            if picked.is_empty() {
                continue;
            }
            match self.casting.active_summon.as_mut() {
                Some(scene) => scene.push_parts(&picked, &bytes, pos, rot),
                None => {
                    let mut scene = crate::summon::SummonScene::spawn_parts(
                        &[],
                        &bytes,
                        crate::scene::EFFECT_MODEL_LIBRARY_BASE,
                        pos,
                    );
                    scene.push_parts(&picked, &bytes, pos, rot);
                    self.casting.active_summon = Some(scene);
                }
            }
        }
    }

    /// One stager tick - the engine body behind
    /// `BattleActionHost::summon_stager_tick`. Returns `true` while the
    /// choreography is still running (retail: the stager's non-zero return
    /// the `0x36` arm holds on).
    ///
    /// PORT: FUN_801F1ED4 (the dispatch seam; the choreography is the
    /// engine's - see the module docs)
    pub fn summon_stager_tick(&mut self) -> bool {
        let Some(mut st) = self.casting.summon_stager.take() else {
            return false;
        };
        // Retail re-enters the paged module every frame from this seam; the
        // band's PORT rows are the code that runs there.
        let module_arm = self.casting.module_phase;
        // The creature's live seat and walk state, as a directed module's
        // walk arm reads them.
        let seat_live = self
            .casting
            .summon_actor_slot
            .filter(|&s| self.actors.get(s as usize).is_some_and(|a| a.active))
            .and_then(|s| self.actors.get(s as usize))
            .map(|a| vm::cast_module_camera::ModuleSeat {
                x: a.move_state.world_x,
                y: a.move_state.world_y,
                z: a.move_state.world_z,
                facing: a.battle.facing_angle & 0xFFF,
            });
        self.casting.module_cam.creature_live = seat_live;
        self.casting.module_cam.creature_arrived = st.walked;
        let placed_before = self.casting.module_cam.creature.is_some();
        let run = self.run_cast_module_code(st.spell_id, module_arm);
        // The module seats its creature itself (Gimard's arm 3); put the
        // engine's creature where it did.
        if !placed_before
            && let Some(c) = self.casting.module_cam.creature
            && let Some(slot) = self.casting.summon_actor_slot
            && let Some(a) = self.actors.get_mut(slot as usize)
        {
            a.move_state.world_x = c.x;
            a.move_state.world_z = c.z;
            a.battle.facing_angle = c.facing;
        }
        // A module whose arms are paced by their own countdown
        // (`vm::cast_module_camera`) owns the band's length the way retail's
        // does: `0x36` holds on the module's return, and the outcome lands on
        // the module's hit arm rather than on the engine's walk-in.
        let profile = run
            .as_ref()
            .and_then(|r| vm::cast_module_camera::module_profile(r.prot_entry))
            .filter(|p| p.paces_band());
        // A module whose director reports its spawn calls seats each record
        // on the pass its arm makes the call, after the arm's own creature
        // placement above (Gimard's arm 3 spawns on the seat it just set).
        let stages_spawns = run
            .as_ref()
            .and_then(|r| vm::cast_module_camera::module_profile(r.prot_entry))
            .is_some_and(|p| p.stages_spawns);
        if let Some(r) = run.as_ref().filter(|_| stages_spawns) {
            if let Some(m) = r.vram_move {
                let i = |v: u16| v as i16;
                self.battle.vram_moves.push(crate::world::ScriptVramMove {
                    src: (i(m.src.0), i(m.src.1)),
                    size: (i(m.size.0), i(m.size.1)),
                    dst: (i(m.dst.0), i(m.dst.1)),
                });
            }
            if !r.spawns.is_empty() {
                self.stage_module_arm_spawns(r.prot_entry, r.spawns, r.camera_shot);
            }
            if let Some(c) = r.caption {
                self.casting.module_caption = Some(c);
            }
            // The arm's own fades run beside the band's flash: retail's
            // spawner takes a fresh pool actor per call.
            if r.kills_fades {
                self.presentation.module_fades.clear();
            }
            for (t, id) in r.fades {
                let mut seat = None;
                crate::fade::spawn_fade(&mut seat, &crate::fade::summon_template(t), *id);
                self.presentation.module_fades.extend(seat);
            }
        }
        let directed_hit = profile.and_then(|p| p.hit_arm);
        let module_busy =
            profile.is_some() && run.as_ref().is_some_and(|r| r.tick_ported && r.busy);
        let module_phase = run.as_ref().map_or(0, |r| r.phase);
        if let Some(shot) = run.as_ref().and_then(|r| r.camera_shot)
            && let Some(cam) = self.battle.camera.as_mut()
        {
            let (pose, raw_z) = shot.pose();
            cam.arm_module_shot(pose, raw_z, u32::from(shot.frames));
        }
        if let Some(n) = run.as_ref().and_then(|r| r.camera_nudge)
            && let Some(cam) = self.battle.camera.as_mut()
        {
            cam.nudge_module(n.pitch, n.tr_y, n.tr_z);
        }
        if let Some(f) = run.as_ref().and_then(|r| r.camera_follow)
            && let Some(cam) = self.battle.camera.as_mut()
        {
            let actor = legaia_engine_vm::battle_cam_script::BattleCamActor {
                facing: i32::from(f.seat.facing),
                world: [
                    f32::from(f.seat.x),
                    f32::from(f.seat.y),
                    f32::from(f.seat.z),
                ],
                height: None,
            };
            cam.arm_module_follow(actor, f.yaw_base, f.depth_raw);
        }
        if run.as_ref().is_some_and(|r| r.camera_end_frame)
            && let Some(cam) = self.battle.camera.as_mut()
        {
            cam.arm_module_end_frame();
        }
        // The module's seat arm (`FUN_801F19EC`): a seat owed since the
        // stager armed is requested as the module reaches it.
        let seat_arm = run
            .as_ref()
            .and_then(|r| vm::cast_module_camera::module_profile(r.prot_entry))
            .and_then(|p| p.seat_arm);
        if let Some((arm, spell, at)) = self.casting.summon_seat_owed
            && (module_phase >= arm || run.is_none())
        {
            self.casting.summon_seat_owed = None;
            self.casting.pending_summon_spawn = Some((spell, at));
        }
        let busy = match st.phase {
            SummonPhase::Armed => {
                // Phase 0: seat the creature (retail: the stager's
                // `FUN_801F19EC` installs the streamed record as slot 7) -
                // here, unless the module seats it in a later arm.
                match seat_arm.filter(|&a| module_phase < a) {
                    Some(arm) => {
                        self.casting.summon_seat_owed = Some((arm, st.spell_id, st.spawn));
                    }
                    None => {
                        self.casting.pending_summon_spawn = Some((st.spell_id, st.spawn));
                    }
                }
                // ...and stage the module's own effect parts. This is the
                // `0x801E4B1C` site's other half: `FUN_801F1ED4` dispatches
                // into the paged module, whose spawn records are the cast's
                // particle layer. Seated where the creature is, since the
                // records carry summon-local offsets.
                if !stages_spawns {
                    self.spawn_cast_module_fx(st.spell_id, st.spawn);
                }
                st.phase = SummonPhase::Approach;
                st.frames = 0;
                true
            }
            SummonPhase::Approach => {
                st.frames = st.frames.saturating_add(1);
                let seat = self
                    .casting
                    .summon_actor_slot
                    .filter(|&s| self.actors.get(s as usize).is_some_and(|a| a.active));
                // A directed module walks its creature in its own walk arm,
                // not after the engine's idle count.
                let may_walk = match profile {
                    Some(p) => p.walk_arm.is_some_and(|w| module_phase >= w),
                    None => st.frames > SUMMON_IDLE_FRAMES,
                };
                // A directed walk arm walks the creature onto the **victim**
                // and holds on the range poll `FUN_8004E2F0(7, victim)`
                // (PROT 0903's arm 11, `bne v0,zero` at `0x801F7418`); the
                // engine's fixed goal stands in only where no module walks.
                let victim = profile
                    .filter(|p| p.walk_arm.is_some())
                    .map(|_| self.actors.get(st.caster as usize))
                    .and_then(|c| c.map(|c| c.battle.active_target))
                    .filter(|&v| self.actors.get(usize::from(v)).is_some_and(|a| a.active));
                let walked = match (seat, victim) {
                    (Some(slot), Some(v)) => may_walk && self.summon_walk_to_victim(slot, v),
                    (Some(slot), None) => may_walk && self.summon_walk_step(slot as usize, st.goal),
                    (None, _) => {
                        self.casting.summon_seat_owed.is_none()
                            && st.frames >= SUMMON_UNSEATED_GRACE
                    }
                };
                st.walked = walked;
                let strike = match directed_hit {
                    Some(hit) => module_phase > hit || !module_busy,
                    None => walked,
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
                    && let Some(slot) = self.casting.summon_actor_slot
                    && let Some(a) = self.actors.get_mut(slot as usize)
                {
                    // Back to the idle loop for the hold.
                    a.battle.queued_anim = 0;
                }
                if st.frames >= SUMMON_LINGER_FRAMES && !module_busy {
                    self.despawn_summon_actor();
                    st.phase = SummonPhase::Done;
                }
                true
            }
            SummonPhase::Done => false,
        };
        if busy {
            self.casting.summon_stager = Some(st);
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
        let Some(spell_id) = self.casting.capture_spell else {
            return false;
        };
        // The caster's own stages the body's port does not write: the
        // wind-up plays first, the module's arms run with the caster on its
        // park, and the close releases the park once they are done.
        match self.step_caster_stages() {
            CasterStageGate::Hold => return true,
            CasterStageGate::Done => {
                self.casting.capture_spell = None;
                return false;
            }
            CasterStageGate::RunModule => {}
        }
        let arm = self.casting.module_phase;
        let Some(run) = self.run_cast_module_code(spell_id, arm) else {
            if self.close_caster_stages() {
                return true;
            }
            self.casting.capture_spell = None;
            return false;
        };
        // `0x70` frames nothing of its own: the camera is the module's.
        if let Some(cam) = self.battle.camera.as_mut() {
            if let Some(shot) = run.camera_shot {
                let (pose, raw_z) = shot.pose();
                cam.arm_module_shot(pose, raw_z, u32::from(shot.frames));
            }
            if let Some(d) = run.capture_drift {
                cam.drift_module(d.pitch, d.yaw, d.tr_y, d.tr_z);
                if d.tr_x != 0 {
                    cam.drift_module_tr_x(d.tr_x);
                }
            }
        }
        if run.tick_ported && run.busy {
            return true;
        }
        if self.close_caster_stages() {
            return true;
        }
        // The band is leaving `0x70`; the module stops being re-entered.
        self.casting.capture_spell = None;
        self.casting.fatal_banner = None;
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
        self.casting.module_phase = 0;
        self.casting.module_ctx_278 = 0;
        self.casting.module_skips_fold = false;
        self.casting.fatal_decision = None;
        self.casting.fatal_banner = None;
        self.casting.module_swordie = Default::default();
        self.casting.module_settle_countdown = 0;
        self.casting.module_cam = Default::default();
        self.casting.module_beam_counter = 0;
        self.casting.module_beam_live = false;
        self.casting.module_hit_arm = None;
        self.casting.module_settle_ticks = 0;
        self.casting.capture_spell = Some(spell_id);
        self.casting.caster_stages = self.caster_stage_run_for(spell_id);
        self.emit_cast_module_voice(spell_id);
    }

    /// The [`CasterStageRun`] a capture-class cast of `spell_id` by the
    /// acting seat owes, when its body's port stages nothing on the caster
    /// ([`vm::cast_module_ticks::capture_caster_stages`]). Only a monster seat
    /// with installed clips plays one, and a stage naming an entry the
    /// caster's record does not carry is dropped - retail would index past
    /// the record's offset array there (the Gobu Gobu Curse fault on
    /// `docs/subsystems/cast-module.md`).
    fn caster_stage_run_for(&self, spell_id: u8) -> Option<CasterStageRun> {
        let entry = self.cast_module_for(spell_id)?;
        let slot = self.battle_ctx.active_actor;
        let actor = self.actors.get(usize::from(slot))?;
        let seat_monster = (actor.battle_monster_id? & 0xFF) as u8;
        let installed = actor.battle_action_clips.as_ref()?;
        let approach = vm::cast_module_ticks::capture_body_approaches(entry, spell_id)
            .then_some(actor.battle.active_target)
            .filter(|&t| t < 8 && usize::from(t) < self.actors.len() && t != slot);
        let clips: Vec<u8> = vm::cast_module_ticks::capture_caster_stages(
            entry,
            spell_id,
            seat_monster,
            self.battle_first_monster_byte(),
        )
        .unwrap_or_default()
        .into_iter()
        .filter(|&c| {
            installed
                .get(usize::from(c))
                .is_some_and(|c| c.as_ref().is_some_and(|c| c.frame_count > 0))
        })
        .collect();
        (!clips.is_empty() || approach.is_some()).then_some(CasterStageRun {
            slot,
            clips,
            cursor: 0,
            approach,
            phase: if approach.is_some() {
                CasterStagePhase::Approach
            } else {
                CasterStagePhase::Staging
            },
            ticks: 0,
        })
    }

    /// Advance the [`CasterStageRun`] by one band tick.
    fn step_caster_stages(&mut self) -> CasterStageGate {
        let Some(mut run) = self.casting.caster_stages.take() else {
            return CasterStageGate::RunModule;
        };
        let Some(a) = self.actors.get_mut(usize::from(run.slot)) else {
            return CasterStageGate::RunModule;
        };
        if run.phase != CasterStagePhase::Module {
            run.ticks = run.ticks.saturating_add(1);
            if run.ticks > CASTER_STAGE_TICK_LIMIT {
                // A clip that never committed: hand the band back rather
                // than hold it.
                if let Some(p) = a.battle_animation.as_mut() {
                    p.release_loop_window();
                }
                a.battle.queued_anim = 0;
                return if run.phase == CasterStagePhase::Closing {
                    CasterStageGate::Done
                } else {
                    CasterStageGate::RunModule
                };
            }
        }
        let gate = match run.phase {
            CasterStagePhase::Approach => {
                let victim = run.approach.unwrap_or(run.slot);
                if self.battle_range_metric(run.slot, victim) == 0 {
                    // Arrived. A body whose port stages its own strike takes
                    // the band from here; the rest go on to their stages.
                    run.ticks = 0;
                    let a = &mut self.actors[usize::from(run.slot)];
                    if a.battle.queued_anim == CAPTURE_WALK_ENTRY {
                        a.battle.queued_anim = 0;
                    }
                    if run.clips.is_empty() {
                        run.phase = CasterStagePhase::Module;
                        self.casting.caster_stages = Some(run);
                        return CasterStageGate::RunModule;
                    }
                    run.phase = CasterStagePhase::Staging;
                    self.casting.caster_stages = Some(run);
                    return CasterStageGate::Hold;
                }
                // `FUN_80019B28(victim, caster) + 0x800` onto the caster's
                // facing each tick, then the walk stage (`li v1,0x1;
                // sb v1,0x1da(<caster>)`, e.g. `0x801F72C8` in PROT 0952).
                let (vx, vz) = self.battle_seat_of(usize::from(victim));
                let a = &mut self.actors[usize::from(run.slot)];
                let bearing = vm::battle_action::bearing_12bit_approx(
                    vz,
                    vx,
                    a.move_state.world_z,
                    a.move_state.world_x,
                );
                a.battle.facing_angle = bearing.wrapping_add(0x800) & 0xFFF;
                a.battle.queued_anim = CAPTURE_WALK_ENTRY;
                CasterStageGate::Hold
            }
            CasterStagePhase::Staging => {
                let want = run.clips[run.cursor];
                if run.ticks == 1 {
                    a.battle.queued_anim = want;
                    CasterStageGate::Hold
                } else if a.battle.current_anim == want && caster_clip_settled(a) {
                    if run.cursor + 1 < run.clips.len() {
                        // A wind-up's park is released for the stage that
                        // follows it.
                        if let Some(p) = a.battle_animation.as_mut() {
                            p.release_loop_window();
                        }
                        run.cursor += 1;
                        a.battle.queued_anim = run.clips[run.cursor];
                        CasterStageGate::Hold
                    } else {
                        run.phase = CasterStagePhase::Module;
                        CasterStageGate::RunModule
                    }
                } else {
                    CasterStageGate::Hold
                }
            }
            CasterStagePhase::Module => CasterStageGate::RunModule,
            CasterStagePhase::Closing if run.clips.is_empty() => CasterStageGate::Done,
            CasterStagePhase::Closing => {
                if a.battle.current_anim == 0 {
                    return CasterStageGate::Done;
                }
                CasterStageGate::Hold
            }
        };
        self.casting.caster_stages = Some(run);
        gate
    }

    /// The module reported done: release the caster's park and stage idle
    /// behind it. `true` when a [`CasterStageRun`] is now closing (the band
    /// holds until the caster is back on idle).
    fn close_caster_stages(&mut self) -> bool {
        let Some(run) = self.casting.caster_stages.as_mut() else {
            return false;
        };
        if run.phase != CasterStagePhase::Module || run.clips.is_empty() {
            return false;
        }
        run.phase = CasterStagePhase::Closing;
        if let Some(a) = self.actors.get_mut(usize::from(run.slot)) {
            if let Some(p) = a.battle_animation.as_mut() {
                p.release_loop_window();
            }
            a.battle.queued_anim = 0;
            if a.battle.current_anim == 0 {
                self.casting.caster_stages = None;
                return false;
            }
        }
        true
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

    /// A directed walk arm's step: stage the walk clip, glide the creature
    /// one step toward `victim`'s seat, and report arrival on the range poll
    /// retail's arm holds on - `FUN_8004E2F0(7, victim)` reading `0`. Retail
    /// turns the creature onto the victim every pass (`sh v0,0x46(s3)` at
    /// `0x801F73C4`); the creature's own `+0x1F` reads `0` in the
    /// `gimard_burning_attack` capture, so the size class is the victim's.
    fn summon_walk_to_victim(&mut self, slot: u8, victim: u8) -> bool {
        let Some(a) = self.actors.get(usize::from(slot)) else {
            return true;
        };
        let pos = (a.move_state.world_x, a.move_state.world_z);
        if self.creature_range_metric(pos, 0, victim) == 0 {
            return true;
        }
        let (vx, vz) = self.battle_seat_of(usize::from(victim));
        // `FUN_80019B28(victim, creature) + 0x800` (`0x801F73A4..0x801F73C4`).
        let bearing = vm::battle_action::bearing_12bit_approx(vz, vx, pos.1, pos.0);
        let facing = bearing.wrapping_add(0x800) & 0xFFF;
        // The walk is the creature clip's own root motion: the anim tick's
        // positive-speed term steps it along this facing while the range
        // poll against its target `+0x1DD` (the victim -
        // `gimard_burning_attack` reads `3`) still fails
        // (`World::drive_playing_root_motion`). Stepping it here as well
        // walked it twice - 58 units a tick against retail's ~30 (the
        // capture's creature `+0x21D = 4` halves the clip's speed). The
        // measured constant stays for a creature with no root speed to
        // play (a headless seat with no clip).
        let root_driven = self
            .battle_playing_root_motion(usize::from(slot))
            .is_some_and(|(speed, _)| speed > 0);
        if let Some(a) = self.actors.get_mut(usize::from(slot)) {
            a.battle.facing_angle = facing;
            a.battle.active_target = victim;
            if a.battle.queued_anim != 1 {
                a.battle.queued_anim = 1;
            }
            if root_driven {
                return false;
            }
            let ms = &mut a.move_state;
            let (dx, dz) = (
                i32::from(vx) - i32::from(ms.world_x),
                i32::from(vz) - i32::from(ms.world_z),
            );
            if dx.abs() + dz.abs() <= SUMMON_DIRECTED_WALK_STEP {
                ms.world_x = vx;
                ms.world_z = vz;
            } else {
                let (sin, cos) = vm::battle_action::motion::trig12(facing);
                ms.world_x = (i32::from(ms.world_x)
                    + ((i32::from(sin) * SUMMON_DIRECTED_WALK_STEP) >> 12))
                    as i16;
                ms.world_z = (i32::from(ms.world_z)
                    + ((i32::from(cos) * SUMMON_DIRECTED_WALK_STEP) >> 12))
                    as i16;
            }
        }
        let Some(a) = self.actors.get(usize::from(slot)) else {
            return true;
        };
        let pos = (a.move_state.world_x, a.move_state.world_z);
        self.creature_range_metric(pos, 0, victim) == 0 || pos == (vx, vz)
    }

    /// Retire the summon creature: the seat goes inactive so both hosts
    /// stop drawing it (native: the `active` gate of the battle draw loop;
    /// browser: the transform row's `active` float).
    pub(in crate::world) fn despawn_summon_actor(&mut self) {
        if let Some(slot) = self.casting.summon_actor_slot.take()
            && let Some(a) = self.actors.get_mut(slot as usize)
        {
            a.active = false;
            a.battle_animation = None;
            a.pose_frame = None;
            a.battle.queued_anim = 0;
            a.battle.current_anim = 0;
        }
    }

    /// PROT 0948's beam counter while its builder ran on this module tick -
    /// the one input `legaia_engine_ui::cast_beam::cross_beam_prims` builds
    /// the frame's beam packets from, on both hosts. `None` outside arm 3.
    pub fn cross_beam_draw(&self) -> Option<i32> {
        (self.mode == SceneMode::Battle && self.casting.module_beam_live)
            .then_some(self.casting.module_beam_counter)
    }

    /// The full-screen fade quad to composite this frame, as
    /// `(rgb 0xRRGGBB, abr_mode, ot_index)` for
    /// `legaia_engine_ui::screen_prim::fade_prim` - `None` while no fade is
    /// live or its start delay is still running (retail's tick returns `-1`
    /// and draws nothing). The ABR mode is the template's kind word and the
    /// OT index the id `FUN_80024E80` stamped (`AddPrim(ot + id*4, ..)` in
    /// `FUN_80024EE4`).
    pub fn screen_fade_draw(&self) -> Option<(u32, u8, u32)> {
        fade_draw(self.presentation.fade.as_ref()?)
    }

    /// Every live full-screen fade quad this frame, the world's fade first
    /// and then the module fades running beside it
    /// ([`crate::world::ScreenFxState::module_fades`]), each as
    /// [`Self::screen_fade_draw`] gives it. Both hosts composite the list.
    pub fn screen_fade_draws(&self) -> Vec<(u32, u8, u32)> {
        self.presentation
            .fade
            .iter()
            .chain(self.presentation.module_fades.iter())
            .filter_map(fade_draw)
            .collect()
    }
}

/// One fade's quad, `None` while its start delay runs.
fn fade_draw(f: &crate::fade::FadeState) -> Option<(u32, u8, u32)> {
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
    /// The camera shot the module armed this frame
    /// ([`vm::cast_module_camera::ModuleShot`]).
    pub camera_shot: Option<vm::cast_module_camera::ModuleShot>,
    /// The case-6 follow the module re-armed this frame
    /// ([`vm::cast_module_camera::ModuleFollow`]).
    pub camera_follow: Option<vm::cast_module_camera::ModuleFollow>,
    /// The module framed the acting actor through case 8 this frame
    /// ([`vm::cast_module_camera::ArmDirection::end_frame`]).
    pub camera_end_frame: bool,
    /// The drift the module wrote into the camera globals this frame.
    pub camera_nudge: Option<vm::cast_module_camera::ModuleNudge>,
    /// A capture-class body's drift this frame
    /// ([`vm::cast_module_camera::capture_camera_director`]).
    pub capture_drift: Option<vm::cast_module_camera::CaptureDrift>,
    /// The spawn calls the module's arm made this frame, for a director that
    /// reports them ([`vm::cast_module_camera::ModuleProfile::stages_spawns`]).
    pub spawns: &'static [vm::cast_module_camera::ModuleSpawn],
    /// The `MoveImage` the module's arm issued this frame.
    pub vram_move: Option<vm::cast_module_camera::ModuleVramMove>,
    /// The text the module's arm put up this frame.
    pub caption: Option<vm::cast_module_camera::ModuleCaption>,
    /// The fades the module's arm spawned this frame, each `(template, id)`.
    pub fades: &'static [(vm::battle_action::SummonFadeTemplate, i16)],
    /// The arm killed the module's earlier fades first.
    pub kills_fades: bool,
}

// --- W1-D: the fourteen trampoline arms ---
/// The clip PROT 0941's Steal stages on its caster in arm `0`.
///
/// Retail picks it with `FUN_80050E2C(record + 0x4C, 1, record[+0x4A])` - a
/// draw over the monster record's own clip list, which the engine has no
/// equivalent for. `1` is the approach clip every other body in the band
/// stages, and the arm's own contribution - the **OR** restage `+0x1DC |= 1`
/// rather than the band's usual bump - is what the port carries exactly.
const CAST_STEAL_RUN_CLIP: u8 = 1;
// --- end W1-D ---

mod code_run;
mod seat_map;

#[cfg(test)]
mod capture_hold_tests {
    use super::*;

    fn band_world() -> World {
        let mut world = World {
            party: crate::world::PartyState {
                party_count: 3,
                ..Default::default()
            },
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

    /// A module wrapper hit runs the shared finisher (`FUN_801DD4B0` calls
    /// `0x801DDB30` at `0x801DD678`): Mystic Circle's sweep is halved on a
    /// guarding seat, and the hit fills the defender's Spirit gauge.
    #[test]
    fn a_module_hit_is_finished_guard_halve_and_spirit() {
        use vm::cast_module_ticks::{MYSTIC_CIRCLE_TICK, sweep_damage_shape_for};
        let shape = sweep_damage_shape_for(MYSTIC_CIRCLE_TICK).expect("Mystic Circle shape");
        let mut world = band_world();
        world.actors[0].battle.max_hp = 2000;
        world.actors[0].battle.spirit_gauge = 0;
        assert_eq!(world.finish_module_hit(shape, 3, 0, 1200), 1200);
        assert_eq!(world.actors[0].battle.spirit_gauge, 60, "1200 of 2000 HP");

        world.actors[0].battle.spirit_gauge = 0;
        world.battle.guarding[0] = true;
        assert_eq!(world.finish_module_hit(shape, 3, 0, 1200), 600);
        assert_eq!(world.actors[0].battle.spirit_gauge, 30);
    }

    /// Nothing paged in: the seam is inert, so a host that never reaches the
    /// capture band behaves exactly as it did before the hold existed.
    #[test]
    fn an_unarmed_band_is_never_busy() {
        let mut world = band_world();
        assert!(world.casting.capture_spell.is_none());
        assert!(!world.capture_stager_tick());
    }

    /// Arming resets the module phase pair - retail's `sb zero,0x279(v0)` at
    /// the `0x6F` exit (`0x801E5048`), just before `0x70` starts ticking.
    #[test]
    fn arming_resets_the_module_phase_pair() {
        let mut world = band_world();
        world.casting.module_phase = 9;
        world.casting.module_ctx_278 = 7;
        world.arm_capture_cast_module(0x87);
        assert_eq!(world.casting.capture_spell, Some(0x87));
        assert_eq!(world.casting.module_phase, 0);
        assert_eq!(world.casting.module_ctx_278, 0);
    }

    /// A spell that names no band entry arms nothing, so no host can be
    /// parked on a module that is not there.
    #[test]
    fn a_spell_with_no_module_arms_nothing() {
        let mut world = band_world();
        world.arm_capture_cast_module(0x00);
        assert!(world.casting.capture_spell.is_none());
        assert!(!world.capture_stager_tick());
    }

    /// The no-softlock rule. `CastModuleCodeRun::busy` seeds `true` for any
    /// resident entry, so a module whose tick body is unported would hold
    /// phase `0x70` forever; the hold reads `tick_ported` first and lets the
    /// band through instead, disarming as it goes.
    #[test]
    fn an_unported_tick_body_never_holds_the_phase() {
        let mut world = band_world();
        // --- W1-C ---
        // PROT 0914's tick body is one of the band's unported rows. (This
        // test used to key on PROT 0909, whose tick body is now ported -
        // `legaia_engine_vm::cast_seru_ticks_b::viguro_tick` - so keying on
        // it would have made the assertion vacuous.)
        assert_eq!(world.cast_module_for(0x8C), Some(914));
        let run = world.run_cast_module_code(0x8C, 0).unwrap();
        assert!(run.busy, "the seeded value on its own says 'busy'");
        assert!(!run.tick_ported, "but no tick body ran");

        world.arm_capture_cast_module(0x8C);
        // --- end W1-C ---
        assert!(
            !world.capture_stager_tick(),
            "so the band is not held on it"
        );
        assert!(
            world.casting.capture_spell.is_none(),
            "and the module stops being re-entered"
        );
    }
}

/// The live command flow's sweep encoding: a group-shaped cast reaches the SM
/// as retail's group code (`8` party / `9` enemy row, absolute numbering),
/// never a sentinel, and a self-target stays the caster's own slot - the
/// value `FUN_801E295C`'s cast-begin split (`sltiu v0,t2,0x8` at
/// `0x801E433C`) and its self-skip (`beq v0,t2` at `0x801E4350`) expect.
#[cfg(test)]
mod sweep_code_tests {
    use super::*;
    use crate::spells::{SpellDef, SpellTarget};
    use vm::battle_target_group::{TARGET_GROUP_ENEMIES, TARGET_GROUP_PARTY};

    fn staged_target(caster: u8, target: SpellTarget, targets: Vec<u8>) -> u8 {
        let mut world = World::default();
        world.enter_battle(3, 2);
        let def = SpellDef {
            id: 0x81,
            target,
            ..SpellDef::default()
        };
        world.arm_player_cast(caster, &def, targets);
        world.actors[caster as usize].battle.active_target
    }

    #[test]
    fn a_party_casters_sweeps_carry_retail_group_codes() {
        assert_eq!(
            staged_target(0, SpellTarget::AllAllies, vec![0, 1, 2]),
            TARGET_GROUP_PARTY
        );
        assert_eq!(
            staged_target(0, SpellTarget::AllEnemies, vec![3, 4]),
            TARGET_GROUP_ENEMIES
        );
    }

    #[test]
    fn a_monster_casters_sweeps_are_mirrored() {
        assert_eq!(
            staged_target(3, SpellTarget::AllAllies, vec![3, 4]),
            TARGET_GROUP_ENEMIES
        );
        assert_eq!(
            staged_target(3, SpellTarget::AllEnemies, vec![0, 1, 2]),
            TARGET_GROUP_PARTY
        );
    }

    #[test]
    fn a_self_target_is_the_casters_own_slot() {
        assert_eq!(staged_target(1, SpellTarget::SelfOnly, vec![1]), 1);
    }
}

#[cfg(test)]
mod divide_split_tests {
    use super::*;

    fn split() -> vm::cast_arm_ticks::GlareDivideSplit {
        vm::cast_arm_ticks::GlareDivideSplit {
            // Retail pool space: the second monster of any party.
            clone_seat: vm::cast_module_ticks::FIRST_MONSTER_SEAT + 1,
            clone_hp: 69,
            saved_caster_target: 0,
            weakened: None,
        }
    }

    /// Retail's pool slot `3 + k` is the engine's `party_count + k`; an empty
    /// small-party seat has no engine slot. Element Change lands on the first
    /// monster whatever the party size.
    #[test]
    fn retail_pool_seats_map_onto_the_compacted_row() {
        let mut world = World::default();
        world.enter_battle(1, 2);
        assert_eq!(world.engine_slot_for_retail_pool(0), Some(0));
        assert_eq!(world.engine_slot_for_retail_pool(1), None);
        assert_eq!(world.engine_slot_for_retail_pool(3), Some(1));
        assert_eq!(world.engine_slot_for_retail_pool(4), Some(2));
        world.actors[1].battle_monster_id = Some(7);
        world.apply_cast_element_change(5);
        assert_eq!(world.actors[1].battle_element, Some(5));
        let mut world = World::default();
        world.enter_battle(3, 2);
        for r in 0..8 {
            assert_eq!(
                world.engine_slot_for_retail_pool(r),
                Some(r),
                "identity at 3"
            );
        }
    }

    /// A lone party member's monster row starts at engine slot 1, so a
    /// Divide clone takes slot 2 - inside the five-seat row the target picker
    /// walks - and each later clone the next seat, up to retail's five.
    #[test]
    fn a_small_partys_divide_clone_lands_in_the_engine_row() {
        let mut world = World::default();
        world.enter_battle(1, 1);
        world.actors[1].battle_monster_id = Some(7);
        world.actors[1].battle.max_hp = 69;
        world.actors[1].battle.hp = 69;
        assert!(world.apply_glare_divide_split(1, &split()));
        assert_eq!(world.actors[2].battle_monster_id, Some(7));
        assert_eq!(world.actors[2].battle.hp, 69);
        let (_, monsters) = world.battle_target_rows();
        assert!(monsters[1].alive, "the clone is a targetable enemy seat");
        for seat in 3..6 {
            assert!(world.apply_glare_divide_split(1, &split()));
            assert_eq!(world.actors[seat].battle_monster_id, Some(7));
        }
        assert!(
            !world.apply_glare_divide_split(1, &split()),
            "a sixth monster has no seat"
        );
    }
}
