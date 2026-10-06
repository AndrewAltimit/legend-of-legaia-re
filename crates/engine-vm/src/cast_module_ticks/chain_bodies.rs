//! The band's **phase-chain** tick bodies: fourteen choreographies whose
//! simulation footprint is their arm map, their staging and their exits,
//! carried as data and run by one interpreter.
//!
//! Every tick body in PROT 0903..0966 has the same skeleton. A head reads
//! the module phase `ctx[+0x279]` and dispatches through a `beq`/`slti`
//! chain or an `sltiu` word table; each arm runs packet and camera work,
//! stages clips through `+0x1DA` / `+0x1DC`, sets animation rates at
//! `+0x21D`, and leaves by advancing the phase, storing a literal phase, or -
//! the terminal arm - clearing the saved register the routine returns, which
//! is what lets the drive loop proceed. For the bodies here that skeleton,
//! plus one damage site whose outcome the band's fold owns, is the whole
//! simulation, so the port is one runner ([`run_chain_body`]) over one
//! descriptor per body ([`chain_bodies`]). Each descriptor carries its own
//! `PORT:` tag naming its owning image.
//!
//! **What a descriptor records** is read off the owning image's own bytes at
//! slot-B base `0x801F69D8`: every arm the head names with its phase literal
//! and landing VA, the arm's exit on the path that progresses, every
//! caster / victim / summon-seat `+0x1DA` store with its clip source and the
//! `+0x1DC` store beside it, the `+0x21D` stores, and the `ctx[+0x278]` /
//! `ctx[+0x0D]` stores.
//!
//! **Damage is the fold's.** Where a body calls `FUN_801DD0AC` /
//! `FUN_801DD4B0` / `FUN_801DD6B4`, its baked power is already the fold's
//! magnitude ([`super::CAPTURE_SITE_POWERS`]) and the band's generic fold is
//! the hit's one owner (`docs/subsystems/cast-module.md`, "One owner per
//! hit"). The runner writes no HP. A stage retail makes only beside the hit -
//! the victim's or each swept seat's own `+0x1F1` reaction after the clamp -
//! is recorded with [`ChainStage::on_hit`] and skipped, since the fold stages
//! the reaction of the hit it lands.
//!
//! **When an arm leaves is the band's.** The runner leaves an arm on the tick
//! it is called; the caller calls it only on the tick the arm's module
//! countdown lets through, from the per-body tables in
//! `crate::cast_module_camera::capture_countdown` (every body here but PROT
//! 0919's). The clip-confirm and settle waits are not carried.
//!
//! **Not ported**, and disclosed per body: the GPU-packet and camera arms
//! (PROT 0948's beam excepted - `legaia_engine_ui::cast_beam`), and the
//! party-row presentation stores (`+0x04` tint, `+0x21C` hide/show,
//! per-seat `+0x21D` on a sweep).
//!
//! **The caster's literal stages belong to the band's pre-pass** where one
//! exists. A body with a row in [`CAPTURE_CASTER_STAGES`] has its caster
//! clips replayed at the head of phase `0x70`, each to its clip's end
//! (`World::capture_stager_tick`); the runner walks one arm per tick, so
//! re-staging the same literals here would restart the wind-up and cut it
//! off a few ticks later. For those bodies the runner leaves every caster
//! stage to the pre-pass ([`ChainBody::caster_stages_replayed`]); the
//! descriptor still records them, and the disc test still checks them.
//!
//! **One rate rule.** A `+0x21D` store is carried only where the body itself
//! puts the actor back to the normal rate `8` before it ends. A body that
//! leaves its caster slowed or frozen relies on a reset outside the module
//! that the port does not have, so carrying the store would leave the
//! actor frozen for the rest of the battle; those stores are named in the
//! body's docs instead. `every_chain_body_leaves_its_actors_at_the_normal_rate`
//! holds the rule.

use super::*;

/// Which actor an arm writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChainActor {
    /// `actor_table[ctx[+0x13]]`.
    Caster,
    /// `actor_table[caster[+0x1DD]]`.
    Victim,
    /// `actor_table[7]`, the summon seat ([`SUMMON_SEAT`]).
    Seat,
}

/// Where a staged clip id comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChainClip {
    /// An immediate (`addiu vX, zero, K; sb vX, 0x1da(...)`).
    Literal(u8),
    /// The actor's own knockdown reaction, `lbu +0x1F1`.
    Knockdown,
    /// The stepper: `lbu +0x1DA; addiu 1; sb +0x1DA`.
    Step,
    /// No `+0x1DA` store at all - only the `+0x1DC` restage beside it.
    Keep,
}

/// What happens to `+0x1DC` beside a stage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChainRestage {
    /// No `+0x1DC` store near the stage.
    Untouched,
    /// `lbu; addiu 1; sb` - the paired restage bump.
    Bump,
    /// A literal `+0x1DC = 1`.
    One,
}

/// One `+0x1DA` / `+0x1DC` store.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChainStage {
    /// Whose `+0x1DA`.
    pub actor: ChainActor,
    /// The clip id stored.
    pub clip: ChainClip,
    /// The `+0x1DC` store beside it.
    pub restage: ChainRestage,
    /// Retail writes it only beside a damage-wrapper hit, so the runner
    /// skips it while the fold owns that hit.
    pub on_hit: bool,
    /// Retail VA of the store.
    pub site: u32,
}

const fn stage(actor: ChainActor, clip: ChainClip, restage: ChainRestage, site: u32) -> ChainStage {
    ChainStage {
        actor,
        clip,
        restage,
        on_hit: false,
        site,
    }
}

/// A hit-side reaction stage: the swept seat's own `+0x1F1`, recorded and
/// skipped (the fold owns the hit it belongs to).
const fn reaction(restage: ChainRestage, site: u32) -> ChainStage {
    ChainStage {
        actor: ChainActor::Victim,
        clip: ChainClip::Knockdown,
        restage,
        on_hit: true,
        site,
    }
}

/// One `+0x21D` store.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChainRate {
    /// Whose `+0x21D`.
    pub actor: ChainActor,
    /// The value stored.
    pub rate: u8,
    /// Retail VA of the `sb ..., 0x21d(...)`.
    pub site: u32,
}

const fn rate(actor: ChainActor, rate: u8, site: u32) -> ChainRate {
    ChainRate { actor, rate, site }
}

/// How an arm leaves on the path that progresses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChainExit {
    /// `lbu; addiu 1; sb` into `ctx[+0x279]`.
    Advance,
    /// A literal phase store into `ctx[+0x279]`.
    Set(u8),
    /// A phase picked by the run's selector (PROT 0964's variant fork):
    /// selector `i` stores `table[i]`, any other selector advances.
    Select(&'static [u8]),
    /// The terminal arm: it clears the returned register and stores no
    /// phase.
    Finish,
}

/// A simulation write an arm makes beyond staging and rates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChainEffect {
    /// PROT 0956's `0x75` arm 2 on a live victim: `+0x16E |= 0x400`, then
    /// the band's turn-steal idiom ([`steal_turn`]).
    MarkAndStealTurn {
        /// The `+0x16E` bit it sets.
        flag: u16,
        /// Retail VA of the flag store.
        site: u32,
    },
}

/// One arm of a [`ChainBody`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChainArm {
    /// The phase literal the head dispatches to this arm.
    pub phase: u8,
    /// Retail VA the head lands on.
    pub entry: u32,
    /// The arm's exit.
    pub exit: ChainExit,
    /// Its stage stores, in image order.
    pub stages: &'static [ChainStage],
    /// Its `+0x21D` stores, in image order.
    pub rates: &'static [ChainRate],
    /// A literal `ctx[+0x278]` store, if the arm makes one.
    pub ctx_278: Option<u8>,
    /// The arm zeroes `ctx[+0x0D]`.
    pub clears_0d: bool,
    /// The VA of the damage-wrapper `jal` in this arm, if it has one (the
    /// fold's hit - recorded, not applied).
    pub wrapper_site: Option<u32>,
    /// Any other simulation write.
    pub effect: Option<ChainEffect>,
}

/// An arm with nothing but its exit.
const fn arm(phase: u8, entry: u32, exit: ChainExit) -> ChainArm {
    ChainArm {
        phase,
        entry,
        exit,
        stages: &[],
        rates: &[],
        ctx_278: None,
        clears_0d: false,
        wrapper_site: None,
        effect: None,
    }
}

impl ChainArm {
    const fn stages(mut self, s: &'static [ChainStage]) -> Self {
        self.stages = s;
        self
    }
    const fn rates(mut self, r: &'static [ChainRate]) -> Self {
        self.rates = r;
        self
    }
    const fn ctx_278(mut self, v: u8) -> Self {
        self.ctx_278 = Some(v);
        self
    }
    const fn clears_0d(mut self) -> Self {
        self.clears_0d = true;
        self
    }
    const fn hit(mut self, site: u32) -> Self {
        self.wrapper_site = Some(site);
        self
    }
    const fn effect(mut self, e: ChainEffect) -> Self {
        self.effect = Some(e);
        self
    }
}

/// One phase-chain tick body.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChainBody {
    /// Extraction PROT entry of the owning image.
    pub prot_entry: u32,
    /// Retail VA of the body. Several images put a body at the same VA, so
    /// only `(prot_entry, body)` is a key.
    pub body: u32,
    /// The action ids whose casts reach it (the trampoline's arms, or the
    /// spell-table rows paging a module with no trampoline).
    pub action_ids: &'static [u8],
    /// Its arms, in phase order.
    pub arms: &'static [ChainArm],
}

impl ChainBody {
    /// The band replays this body's caster clips ahead of its arms
    /// ([`CAPTURE_CASTER_STAGES`] has a row for it), so the runner writes no
    /// caster stage of its own.
    pub fn caster_stages_replayed(&self) -> bool {
        CAPTURE_CASTER_STAGES.iter().any(|r| {
            r.prot_entry == self.prot_entry
                && (r.action_ids.is_empty()
                    || self.action_ids.iter().any(|id| r.action_ids.contains(id)))
        })
    }

    /// The arm the head dispatches `phase` to.
    pub fn arm(&self, phase: u8) -> Option<&ChainArm> {
        self.arms.iter().find(|a| a.phase == phase)
    }
}

/// What one tick of a chain body produced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChainTick {
    /// Busy / done.
    pub step: CastTickStep,
    /// An item a turn steal refunded (retail's `FUN_800421D4`).
    pub refund: Option<u8>,
}

/// Run one tick of a [`ChainBody`]: the arm the phase names performs its
/// stages, rates, context stores and effect, then leaves by its
/// [`ChainExit`]. `selector` feeds [`ChainExit::Select`].
///
/// A phase the head names no arm for reports [`CastTickStep::Done`]. Retail
/// would fall out of the head with the busy register still `1` and the band
/// would park there; the port's phase is the engine's, so the answer that
/// does not softlock is the one [`run_tick`] gives too.
pub fn run_chain_body(
    body: &ChainBody,
    ctx: &mut CastModuleCtx,
    caster: &mut CastActorState,
    victim: &mut CastActorState,
    seat: &mut CastActorState,
    selector: u8,
) -> ChainTick {
    let mut out = ChainTick {
        step: CastTickStep::Busy,
        refund: None,
    };
    let Some(arm) = body.arm(ctx.phase) else {
        out.step = CastTickStep::Done;
        return out;
    };
    fn pick<'a>(
        who: ChainActor,
        c: &'a mut CastActorState,
        v: &'a mut CastActorState,
        s: &'a mut CastActorState,
    ) -> &'a mut CastActorState {
        match who {
            ChainActor::Caster => c,
            ChainActor::Victim => v,
            ChainActor::Seat => s,
        }
    }
    let replayed = body.caster_stages_replayed();
    for st in arm
        .stages
        .iter()
        .filter(|s| !s.on_hit && !(replayed && s.actor == ChainActor::Caster))
    {
        let a = pick(st.actor, caster, victim, seat);
        match st.clip {
            ChainClip::Literal(k) => a.staged_anim = k,
            ChainClip::Knockdown => a.staged_anim = a.knockdown_anim,
            ChainClip::Step => a.staged_anim = a.staged_anim.wrapping_add(1),
            ChainClip::Keep => {}
        }
        match st.restage {
            ChainRestage::Untouched => {}
            ChainRestage::Bump => a.restage = a.restage.wrapping_add(1),
            ChainRestage::One => a.restage = 1,
        }
    }
    for r in arm.rates {
        pick(r.actor, caster, victim, seat).anim_rate = r.rate;
    }
    if let Some(v) = arm.ctx_278 {
        ctx.ctx_278 = v;
    }
    if arm.clears_0d {
        ctx.ctx_0d = 0;
    }
    match arm.effect {
        Some(ChainEffect::MarkAndStealTurn { flag, .. }) if victim.hp != 0 => {
            victim.flags |= flag;
            out.refund = steal_turn(ctx, victim);
        }
        _ => {}
    }
    match arm.exit {
        ChainExit::Advance => advance_phase(ctx),
        ChainExit::Set(k) => ctx.phase = k,
        ChainExit::Select(table) => match table.get(selector as usize) {
            Some(&k) => ctx.phase = k,
            None => advance_phase(ctx),
        },
        ChainExit::Finish => out.step = CastTickStep::Done,
    }
    out
}

/// The chain body PROT `prot_entry` runs for a cast reaching tick `body`.
pub fn chain_body_for(prot_entry: u32, body: u32) -> Option<&'static ChainBody> {
    chain_bodies()
        .iter()
        .find(|b| b.prot_entry == prot_entry && b.body == body)
}

/// The chain body of a module with **no** trampoline - its tick arm calls
/// the body directly, so the entry alone names it.
pub fn direct_chain_body(prot_entry: u32) -> Option<&'static ChainBody> {
    chain_bodies()
        .iter()
        .find(|b| b.prot_entry == prot_entry && capture_trampoline_for(prot_entry).is_none())
}

use ChainActor::{Caster, Seat, Victim};
use ChainClip::{Keep, Literal, Step};
use ChainExit::{Advance, Finish, Set};
use ChainRestage::{Bump, One, Untouched};

const DONE: u8 = CHOREOGRAPHY_DONE_PHASE;

/// PROT 0919 (Spoon) tick body, `0x801F69D8..0x801F8578`, the
/// `0x801CF4EC` arm for id `0x91`.
///
/// `beq`/`slti` chain: `0..=8` and `0xFF`. Arms 1..7 each wait on the
/// countdown `0x801F8E5C` and advance; arm 8 stores `0xFF` (`0x801F853C`);
/// `0xFF` zeroes the busy word at `sp+0x38`. The creature is the summon seat
/// (`s6 = actor_table[7]`): arm 4 steps its clip with a restage bump and
/// sets its rate `1`, arms 6 and 7 set `1` and `4`.
///
/// Arm 7 is a **heal**, not a hit: no damage wrapper anywhere in the body.
/// It walks party seats with a non-zero `0x8007BD10[seat]`, adds
/// `level * 0x80 + 0x380` capped at `+0x14E - HP` straight into `+0x14C`,
/// credits the spell-EXP word, and at level 3 or above applies the cure tier
/// of `0x801F6960`. That outcome is the fold's (the cast's heal), so it is
/// recorded here and not applied.
///
/// Not ported: the packet and camera arms, arm 2's load poll
/// (`FUN_8003F2B8(1)`) and creature seating (`FUN_801F19EC`, the summon
/// band's own seat step), and the countdown gates.
///
/// PORT: FUN_801F69D8 (PROT 0919; phase chain + the summon-seat staging; packet arms unported)
pub const SPOON_CHAIN: ChainBody = ChainBody {
    prot_entry: 919,
    body: 0x801F_69D8,
    action_ids: &[0x91],
    arms: &[
        arm(0, 0x801F_6AF4, Advance),
        arm(1, 0x801F_6C90, Advance),
        arm(2, 0x801F_6D18, Advance),
        arm(3, 0x801F_6E6C, Advance),
        arm(4, 0x801F_6F8C, Advance)
            .stages(&[stage(Seat, Step, Bump, 0x801F_70A8)])
            .rates(&[rate(Seat, 1, 0x801F_70A4)]),
        arm(5, 0x801F_7334, Advance),
        arm(6, 0x801F_75BC, Advance).rates(&[rate(Seat, 1, 0x801F_77FC)]),
        arm(7, 0x801F_78A8, Advance).rates(&[rate(Seat, 4, 0x801F_7C40)]),
        arm(8, 0x801F_848C, Set(DONE)),
        arm(DONE, 0x801F_8540, Finish),
    ],
};

/// PROT 0935 (Earthquake, `0x4A`) tick body, `0x801F69D8..0x801F7FF0`.
///
/// `beq`/`slt` chain: `0..=6` and `0xFF`. Arm 0 faces the victim and stages
/// caster clip `9` with a bump; arm 1 freezes the caster (`+0x21D = 0`),
/// arm 2 restores it (`8`). Arm 5 is the hit: it clears the caster's stage
/// (`+0x1DA = 0`, no bump, `0x801F7A8C`), then sweeps the party row
/// (`FUN_801DD4B0(0x1AE)` at `0x801F7AFC`) when the caster's `+0x1DD` is
/// `8` or below `3`, else the enemy row minus the caster (`0x801F7C50`) -
/// shape A both ways, each hit seat staged with its own reaction. Arm 6 waits
/// for the row to settle and stores `0xFF`; `0xFF` restores every live
/// seat's rate to `8` and clears the busy register (`0x801F7FBC`).
///
/// Not ported: the packet and camera arms, the party-row presentation
/// stores, the holds of arms 1 and 3 (caster `+0x21B`, model `+0x68`) and
/// arm 6's row settle. Countdowns: `capture_countdown::EARTHQUAKE`.
///
/// PORT: FUN_801F69D8 (PROT 0935; phase chain + caster staging; packet arms unported)
pub const EARTHQUAKE_CHAIN: ChainBody = ChainBody {
    prot_entry: 935,
    body: 0x801F_69D8,
    action_ids: &[0x4A],
    arms: &[
        arm(0, 0x801F_6AE8, Advance).stages(&[stage(Caster, Literal(9), Bump, 0x801F_6B50)]),
        arm(1, 0x801F_6C20, Advance).rates(&[rate(Caster, 0, 0x801F_6C88)]),
        arm(2, 0x801F_6F78, Advance).rates(&[rate(Caster, 8, 0x801F_7000)]),
        arm(3, 0x801F_70BC, Advance),
        arm(4, 0x801F_737C, Advance),
        arm(5, 0x801F_7738, Advance)
            .stages(&[
                stage(Caster, Literal(0), Untouched, 0x801F_7A8C),
                reaction(One, 0x801F_7B70),
            ])
            .hit(0x801F_7AFC),
        arm(6, 0x801F_7D88, Set(DONE)),
        arm(DONE, 0x801F_7F60, Finish).rates(&[rate(Caster, 8, 0x801F_7FA8)]),
    ],
};

/// PROT 0936 (Hyper Crush, `0x4B`) tick body, `0x801F69D8..0x801F7BD0`.
///
/// `beq`/`slti` chain: `0..=8` and `0xFF`. Arm 0 stages caster clip `0x0D`,
/// arm 2 clip `0x0B` (both bumped). Arm 4 holds until its own counter
/// `0x801F817C` reaches `6`. Arm 6 is the hit: caster `+0x1DA = 0` (no bump,
/// `0x801F78B0`), then an unconditional party-row sweep
/// (`FUN_801DD4B0(0x1E4)` at `0x801F797C`, shape A), each hit seat staged
/// with its own reaction. Arm 8 waits for the row to settle and stores
/// `0xFF`; `0xFF` only clears the busy register.
///
/// Not ported: the packet and camera arms, arm 4's tick-counter hold and
/// arm 8's row settle. Countdowns: `capture_countdown::HYPER_CRUSH`.
///
/// PORT: FUN_801F69D8 (PROT 0936; phase chain + caster staging; packet arms unported)
pub const HYPER_CRUSH_CHAIN: ChainBody = ChainBody {
    prot_entry: 936,
    body: 0x801F_69D8,
    action_ids: &[0x4B],
    arms: &[
        arm(0, 0x801F_6B14, Advance).stages(&[stage(Caster, Literal(0x0D), Bump, 0x801F_6BA0)]),
        arm(1, 0x801F_6C20, Advance),
        arm(2, 0x801F_6E70, Advance).stages(&[stage(Caster, Literal(0x0B), Bump, 0x801F_6EB0)]),
        arm(3, 0x801F_6F30, Advance),
        arm(4, 0x801F_7228, Advance),
        arm(5, 0x801F_72F8, Advance),
        arm(6, 0x801F_7614, Advance)
            .stages(&[
                stage(Caster, Literal(0), Untouched, 0x801F_78B0),
                reaction(One, 0x801F_79F0),
            ])
            .hit(0x801F_797C),
        arm(7, 0x801F_7A54, Advance),
        arm(8, 0x801F_7AE4, Set(DONE)),
        arm(DONE, 0x801F_7BA0, Finish),
    ],
};

/// PROT 0937 (Hyper Lightning, `0x4C`) tick body, `0x801F69D8..0x801F7850`.
///
/// `beq`/`slti` chain: `0..=7` and `0xFF`. Arm 0 stages caster clip `0x0D`,
/// arm 2 clip `0x0C` (both bumped). Arm 7 is four hits on the one victim,
/// each `FUN_801DD4B0(0x26A)` at `0x801F76AC` with the roll **quartered**
/// before it applies; hits 1..3 clamp to `HP - 1` and hit 4 to HP (shape A),
/// and the victim's reaction is staged per hit. Once the counter reaches 4
/// and the victim settles the arm stores `0xFF`; `0xFF` clears the busy
/// register and the caster's stage (`+0x1DA = 0`, no bump, `0x801F7814`).
///
/// Not ported: the packet and camera arms, the victim settle, and the
/// four-hit split (the fold lands the cast's outcome as one). Countdowns:
/// `capture_countdown::HYPER_LIGHTNING`, arm 7's hit cadence included.
///
/// PORT: FUN_801F69D8 (PROT 0937; phase chain + caster staging; packet arms unported)
pub const HYPER_LIGHTNING_CHAIN: ChainBody = ChainBody {
    prot_entry: 937,
    body: 0x801F_69D8,
    action_ids: &[0x4C],
    arms: &[
        arm(0, 0x801F_6AE0, Advance).stages(&[stage(Caster, Literal(0x0D), Bump, 0x801F_6B38)]),
        arm(1, 0x801F_6BB8, Advance),
        arm(2, 0x801F_6E08, Advance).stages(&[stage(Caster, Literal(0x0C), Bump, 0x801F_6E80)]),
        arm(3, 0x801F_7040, Advance),
        arm(4, 0x801F_7090, Advance),
        arm(5, 0x801F_7314, Advance),
        arm(6, 0x801F_7500, Advance),
        arm(7, 0x801F_757C, Set(DONE))
            .stages(&[reaction(One, 0x801F_775C)])
            .hit(0x801F_76AC),
        arm(DONE, 0x801F_77F8, Finish).stages(&[stage(Caster, Literal(0), Untouched, 0x801F_7814)]),
    ],
};

/// PROT 0939 (Spore Gas, `0x4F`) tick body, `0x801F69D8..0x801F74B4`.
///
/// `beq`/`slt` chain: `0..=5` and `0xFF`. Arm 1 stages caster clip `0x0A`
/// with a bump; arm 2 clears it (`+0x1DA = 0`, no bump, every tick). Arm 4 is
/// four hits on the victim, each `FUN_801DD4B0(0x60)` at `0x801F70E0` with
/// the roll cut to a third, shape A; hits 1..3 stage `+0x1EF` / `+0x1EF` /
/// `+0x1F0` and roll the graded status bits `0x08` / `0x10` / `0x20`
/// (guarded by the character record's `+0x6BC` immunity bits), hit 4 stages
/// `+0x1F1`. Arm 5 waits for the victim to settle and stores `0xFF`; `0xFF`
/// clears the busy register (`fp`).
///
/// Not ported: the packet arms, the victim settle, and the per-hit status
/// rolls (the fold lands the cast's outcome as one hit). Countdowns:
/// `capture_countdown::SPORE_GAS`.
///
/// PORT: FUN_801F69D8 (PROT 0939; phase chain + caster staging; packet arms unported)
pub const SPORE_GAS_CHAIN: ChainBody = ChainBody {
    prot_entry: 939,
    body: 0x801F_69D8,
    action_ids: &[0x4F],
    arms: &[
        arm(0, 0x801F_6AC4, Advance),
        arm(1, 0x801F_6BC8, Advance).stages(&[stage(Caster, Literal(0x0A), Bump, 0x801F_6C54)]),
        arm(2, 0x801F_6E34, Advance).stages(&[stage(Caster, Literal(0), Untouched, 0x801F_6E3C)]),
        arm(3, 0x801F_6ED4, Advance),
        arm(4, 0x801F_6FD0, Advance)
            .stages(&[reaction(One, 0x801F_73C8)])
            .hit(0x801F_70E0),
        arm(5, 0x801F_7428, Set(DONE)),
        arm(DONE, 0x801F_746C, Finish),
    ],
};

/// PROT 0942's `0xAA` tick body, `0x801F69F4..0x801F7D34`.
///
/// `sltiu 7` through the table at `0x801F69D8`: arms `0..=6`, every one but
/// the last advancing. Arm 0 writes `ctx[+0x278] = 3`, stages caster clip
/// `9` with a bump; arm 2 clears the stage (`+0x1DA = 0`, no bump); arm 3
/// bumps `+0x1DC` alone. Arm 4 is a whole-party sweep,
/// `FUN_801DD4B0(0x280)` at `0x801F774C`, shape A, each hit seat staged with
/// its own reaction. Arm 6 is terminal: `ctx[+0x0D] = 0`, busy cleared at
/// `0x801F7CFC`.
///
/// Rates left out under the module's rate rule: the caster ends at `2`
/// (`+0x21D = 4` at `0x801F6B50`, `2` at `0x801F6DEC`) with no restore in the
/// body. Not ported: the packet arms, the party-row moves and presentation
/// stores and the monster-AI block bytes `+0x84..+0x86`. Countdowns:
/// `capture_countdown::POWER_UP_AA`.
///
/// PORT: FUN_801F69F4 (PROT 0942; the `0xAA` arm's phase chain + caster staging; packet arms unported)
pub const POWER_UP_AA_CHAIN: ChainBody = ChainBody {
    prot_entry: 942,
    body: 0x801F_69F4,
    action_ids: &[0xAA],
    arms: &[
        arm(0, 0x801F_6A9C, Advance)
            .stages(&[stage(Caster, Literal(9), Bump, 0x801F_6B4C)])
            .ctx_278(3),
        arm(1, 0x801F_6C9C, Advance),
        arm(2, 0x801F_6E38, Advance).stages(&[stage(Caster, Literal(0), Untouched, 0x801F_7070)]),
        arm(3, 0x801F_715C, Advance).stages(&[stage(Caster, Keep, Bump, 0x801F_7348)]),
        arm(4, 0x801F_73F8, Advance)
            .stages(&[reaction(Bump, 0x801F_77EC)])
            .hit(0x801F_774C),
        arm(5, 0x801F_7A2C, Advance),
        arm(6, 0x801F_7BAC, Finish).clears_0d(),
    ],
};

/// PROT 0947 (V-Windhash `0x57`, Neo Windhash `0xA7`) tick body,
/// `0x801F69F0..0x801F78F8`. One body for both spells; it branches on the
/// caster's `+0x1DF` for the cue, one extra spawn, and the hit.
///
/// `sltiu 6` through the table at `0x801F69D8`. Arm 0 stages caster clip
/// `0x0A` with a bump; arm 1 clears it (no bump). Arm 3 is the hit:
/// `FUN_801DD4B0` at `0x801F7414` with power `0x7B` (set in a delay slot at
/// `0x801F73DC`), or `0xEA` plus a 1-in-4 `+0x16E |= 0x1000` for `0xA7`;
/// shape A, the victim staged with its reaction. Arm 5 is terminal: it waits
/// for the victim to settle, zeroes `ctx[+0x0D]` and clears the busy word.
///
/// Not ported: the packet arms, the hide/show stores, and the countdown and
/// settle gates.
///
/// PORT: FUN_801F69F0 (PROT 0947; phase chain + caster staging; packet arms unported)
pub const V_WINDHASH_CHAIN: ChainBody = ChainBody {
    prot_entry: 947,
    body: 0x801F_69F0,
    action_ids: &[0x57, 0xA7],
    arms: &[
        arm(0, 0x801F_6AB4, Advance).stages(&[stage(Caster, Literal(0x0A), Bump, 0x801F_6B20)]),
        arm(1, 0x801F_6CFC, Advance).stages(&[stage(Caster, Literal(0), Untouched, 0x801F_6D04)]),
        arm(2, 0x801F_6DEC, Advance),
        arm(3, 0x801F_70BC, Advance)
            .stages(&[reaction(One, 0x801F_74B8)])
            .hit(0x801F_7414),
        arm(4, 0x801F_74EC, Advance),
        arm(5, 0x801F_773C, Finish).clears_0d(),
    ],
};

/// Extraction PROT entry of the Cross Beam module.
pub const CROSS_BEAM_ENTRY: u32 = 948;

/// What the beam builder `FUN_801F726C` adds to its counter per call: `step
/// * 2` (`0x801F72B8..0x801F72D4`), at the engine's one-vsync frame step.
pub const CROSS_BEAM_COUNTER_PER_TICK: i32 = 2;

/// PROT 0948 (Cross Beam, `0x58`) tick body, `0x801F69F0..0x801F726C`.
///
/// `sltiu 6` through the table at `0x801F69D8`. Arm 0 stages caster clip `8`
/// with a bump. Arm 3 calls `0x801F726C` every tick - the beam's packet
/// builder, which writes no actor or context state - then the hit,
/// `FUN_801DD4B0(0x78)` at `0x801F6FB0`, shape A, the victim staged with its
/// reaction. Arm 5 is terminal: it waits for the victim to settle, zeroes
/// `ctx[+0x0D]`, clears the caster's stage with a bump (`0x801F7238`) and the
/// busy register.
///
/// The beam is drawn: `World::run_cast_module_code` keeps the builder's
/// counter and both hosts build its packets through
/// `legaia_engine_ui::cast_beam`. The countdown gates are
/// `cast_module_camera::capture_countdown::CROSS_BEAM`. Not ported: the
/// camera shots, the hide/show stores, and arm 5's settle wait.
///
/// PORT: FUN_801F69F0 (PROT 0948; phase chain + caster staging + countdown gates; beam drawn)
pub const CROSS_BEAM_CHAIN: ChainBody = ChainBody {
    prot_entry: 948,
    body: 0x801F_69F0,
    action_ids: &[0x58],
    arms: &[
        arm(0, 0x801F_6AA4, Advance).stages(&[stage(Caster, Literal(8), Bump, 0x801F_6AD0)]),
        arm(1, 0x801F_6CC0, Advance),
        arm(2, 0x801F_6E40, Advance),
        arm(3, 0x801F_6EF4, Advance)
            .stages(&[reaction(One, 0x801F_7008)])
            .hit(0x801F_6FB0),
        arm(4, 0x801F_7048, Advance),
        arm(5, 0x801F_70A8, Finish)
            .stages(&[stage(Caster, Literal(0), Bump, 0x801F_7238)])
            .clears_0d(),
    ],
};

/// PROT 0956's `0x75` tick body, `0x801F69D8..0x801F7298`.
///
/// `beq` chain: `0..=3` and `0xFF`. Arm 2 is the hit,
/// `FUN_801DD4B0(0x100)` at `0x801F6EF8`, shape A. A dead victim, or one
/// whose character record `+0x6BC` carries `0x08000000` / `0x10000000`, is
/// staged with its reaction and a bump (`0x801F6FAC`); any other victim gets
/// `+0x16E |= 0x400` (`0x801F6FC4`) and loses its turn - the band's
/// turn-steal idiom, with the queued item refunded (`0x801F7054..0x801F70A8`).
/// Arm 3 ramps `0x801F86A8` and stores `0xFF`; `0xFF` zeroes `ctx[+0x0D]`,
/// restores every live seat and returns zero.
///
/// The port runs the mark and the steal on a live victim. It cannot see the
/// record's two immunity bits, and the victim's liveness is read before the
/// fold lands the hit rather than after it as retail does.
///
/// Not ported: the packet arms, the per-character effect pointer
/// (`0x800774AC`) and `FUN_801D8DE8(0x5B, 0)` banner, and the victim
/// settle. Countdowns: `capture_countdown::PARALYZING_WAVE`.
///
/// PORT: FUN_801F69D8 (PROT 0956; the `0x75` arm's phase chain + the mark and turn steal; packet arms unported)
pub const WATER_HAZARD_75_CHAIN: ChainBody = ChainBody {
    prot_entry: 956,
    body: 0x801F_69D8,
    action_ids: &[0x75],
    arms: &[
        arm(0, 0x801F_6AB4, Advance),
        arm(1, 0x801F_6CB4, Advance),
        arm(2, 0x801F_6E74, Advance)
            .stages(&[reaction(Bump, 0x801F_6FAC)])
            .hit(0x801F_6EF8)
            .effect(ChainEffect::MarkAndStealTurn {
                flag: 0x0400,
                site: 0x801F_6FC4,
            }),
        arm(3, 0x801F_70E4, Set(DONE)),
        arm(DONE, 0x801F_7200, Finish).clears_0d(),
    ],
};

/// PROT 0959 (Megaton Press, `0x7A`) tick body, `0x801F69F0..0x801F8250`.
///
/// `beq`/`slti` chain: `0..=0x11` and `0xFF`. Arm 4 stages caster clip `0x0A`
/// with a bump and writes `ctx[+0x278] = 3`; arm 5 slows the caster to `6`
/// and bumps `+0x1DC` alone; arm 6 restores `8` with a bump; arm `0x0A`
/// steps the caster's clip. Three hits, all `FUN_801DD6B4` aimed at
/// `actor_table[0]` (`a2 = 0`, not the derived victim): `0x80` at
/// `0x801F71E8` (arm 7), `0x80` at `0x801F7A90` (arm `0x0D`, which also
/// writes `ctx[+0x278] = 0`), `0x30` at `0x801F7EBC` (arm `0x0F`, which
/// clears the caster's stage with a bump on exit). Arm `0x11` on a live
/// victim stages it `0` with a bump, puts both actors back at rate `8` and
/// stores `0xFF`; `0xFF` zeroes `ctx[+0x0D]`.
///
/// Countdowns: `capture_countdown::MEGATON_PRESS`. Not ported: the packet
/// and camera arms, arm `0x11`'s
/// dead-victim branch (which forces battle state 5), and the victim's
/// presentation rates between the hits.
///
/// PORT: FUN_801F69F0 (PROT 0959; phase chain + caster staging; packet arms unported)
pub const MEGATON_PRESS_CHAIN: ChainBody = ChainBody {
    prot_entry: 959,
    body: 0x801F_69F0,
    action_ids: &[0x7A],
    arms: &[
        arm(0, 0x801F_6B80, Advance),
        arm(1, 0x801F_6C78, Advance),
        arm(2, 0x801F_6D60, Advance),
        arm(3, 0x801F_6DD4, Advance),
        arm(4, 0x801F_6E70, Advance)
            .stages(&[stage(Caster, Literal(0x0A), Bump, 0x801F_6F40)])
            .ctx_278(3),
        arm(5, 0x801F_6F68, Advance)
            .stages(&[stage(Caster, Keep, Bump, 0x801F_7038)])
            .rates(&[rate(Caster, 6, 0x801F_6F6C)]),
        arm(6, 0x801F_703C, Advance)
            .stages(&[stage(Caster, Keep, Bump, 0x801F_7120)])
            .rates(&[rate(Caster, 8, 0x801F_7118)]),
        arm(7, 0x801F_7144, Advance)
            .stages(&[reaction(Bump, 0x801F_7278)])
            .rates(&[rate(Caster, 8, 0x801F_7150)])
            .hit(0x801F_71E8),
        arm(8, 0x801F_7284, Advance),
        arm(9, 0x801F_7440, Advance).rates(&[rate(Caster, 0, 0x801F_74B8)]),
        arm(0x0A, 0x801F_753C, Advance).stages(&[stage(Caster, Step, Bump, 0x801F_768C)]),
        arm(0x0B, 0x801F_7698, Advance).rates(&[rate(Caster, 0, 0x801F_7858)]),
        arm(0x0C, 0x801F_7878, Advance),
        arm(0x0D, 0x801F_78D0, Advance)
            .rates(&[rate(Caster, 0, 0x801F_79F0)])
            .ctx_278(0)
            .hit(0x801F_7A90),
        arm(0x0E, 0x801F_7B04, Advance),
        arm(0x0F, 0x801F_7E28, Advance)
            .stages(&[stage(Caster, Literal(0), Bump, 0x801F_7FDC)])
            .hit(0x801F_7EBC),
        arm(0x10, 0x801F_804C, Advance),
        arm(0x11, 0x801F_8118, Set(DONE))
            .stages(&[stage(Victim, Literal(0), Bump, 0x801F_81AC)])
            .rates(&[rate(Caster, 8, 0x801F_81BC), rate(Victim, 8, 0x801F_81C0)]),
        arm(DONE, 0x801F_8210, Finish).clears_0d(),
    ],
};

/// PROT 0960's `0xA6` (Neo Star Slash) tick body, `0x801F69D8..0x801F74E4`.
/// It derives no victim: its one sweep walks the party row.
///
/// `beq`/`slti` chain: `0..=6` and `0xFF`. Arm 1 stages caster clip `0x0A`
/// with a bump and rate `4`; arm 2 keeps `4`; arm 3 restores `8`. Arm 5 is
/// the sweep: `FUN_801DD4B0(0x2C6)` at `0x801F7280` per live party seat,
/// shape A, each staged with its own reaction (`+0x1DC = 1`) - no Stone
/// guard. Arm 6 waits for the row to settle and stores `0xFF`; `0xFF` zeroes
/// `ctx[+0x0D]`.
///
/// Rate left out under the module's rate rule: arm 4's caster
/// `+0x21D = 0` (`0x801F6F8C`) is never restored inside the body. Not
/// ported: the packet arms, the party-row presentation stores, and the
/// countdown and settle gates.
///
/// PORT: FUN_801F69D8 (PROT 0960; the `0xA6` arm's phase chain + caster staging; packet arms unported)
pub const NEO_STAR_SLASH_CHAIN: ChainBody = ChainBody {
    prot_entry: 960,
    body: 0x801F_69D8,
    action_ids: &[0xA6],
    arms: &[
        arm(0, 0x801F_6AC4, Advance),
        arm(1, 0x801F_6B48, Advance)
            .stages(&[stage(Caster, Literal(0x0A), Bump, 0x801F_6B98)])
            .rates(&[rate(Caster, 4, 0x801F_6BA4)]),
        arm(2, 0x801F_6BCC, Advance).rates(&[rate(Caster, 4, 0x801F_6BD4)]),
        arm(3, 0x801F_6D5C, Advance).rates(&[rate(Caster, 8, 0x801F_6DC0)]),
        arm(4, 0x801F_6EB4, Advance),
        arm(5, 0x801F_7048, Advance)
            .stages(&[reaction(One, 0x801F_72F4)])
            .hit(0x801F_7280),
        arm(6, 0x801F_73A8, Set(DONE)),
        arm(DONE, 0x801F_7490, Finish).clears_0d(),
    ],
};

/// Extraction PROT entry of the Dead End Crisis / Final Crisis module.
pub const DEAD_END_CRISIS_ENTRY: u32 = 961;

/// The first formation monster (`0x8007BD0C`) for which PROT 0961's party
/// sweep rolls its wrapper: the evolved Cort (`li v0,0xb5` /
/// `bne v1,v0` at `0x801F7394..0x801F7398`).
pub const DEAD_END_CRISIS_ROLL_FORMATION: u8 = 0xB5;

/// What PROT 0961's sweep writes per party seat in any other fight
/// (`li s0,0x270f` at `0x801F739C`), clamped to the seat's HP.
pub const DEAD_END_CRISIS_WIPE_DAMAGE: u16 = 9999;

/// PROT 0961 (`0xA1` / `0xB4`) tick body, `0x801F69D8..0x801F78A4`. The
/// body never reads the caster's `+0x1DF`, so both ids run it identically;
/// it forks on the formation id `0x8007BD0C == 0xB5` instead.
///
/// `beq`/`slti` chain: `0..=6` and `0xFF`. Arm 0 writes `ctx[+0x278] = 2`.
/// Arm 1 stages caster clip `6` with a bump in formation `0xB5`
/// (`0x801F6D58`; clip `8` at `0x801F6DE8` otherwise) and writes
/// `ctx[+0x278] = 3`. Arm 3 is the party sweep: `FUN_801DD4B0(0x880)` at
/// `0x801F73AC` per seat in formation `0xB5`, shape A, each staged with its
/// own reaction. Arm 5 clears the caster's stage with a bump when a party
/// seat is still standing. Arm 6 stores `0xFF`; `0xFF` zeroes `ctx[+0x0D]`.
///
/// Both outcomes are the fold's. In formation `0xB5` it rolls the wrapper
/// power; anywhere else - Koru's round-4 finisher - arm 3 keeps the `9999`
/// it loaded in the `bne` delay slot (`0x801F739C`) and calls no wrapper, so
/// the fold lands [`DEAD_END_CRISIS_WIPE_DAMAGE`] per party seat, clamped to
/// its HP (`World::dead_end_crisis_wipes`). Arm 5's no-one-standing branch
/// (`0x801F76E4..0x801F774C`: seat rates `0`, the HUD actors retired, the
/// battle-end signal `0xFE` with cause `5`) is the same party-wipe end the
/// action SM's `0x5A` gate raises, which the engine reaches on its own once
/// the fold has emptied the party. Rate left out under the module's rate
/// rule: arm 1's caster `+0x21D = 3` (`0x801F6D5C`). Not ported: the packet
/// arms, the seat line-up and its restore.
///
/// PORT: FUN_801F69D8 (PROT 0961; phase chain + caster staging, both formation paths; packet arms unported)
pub const DEAD_END_CRISIS_CHAIN: ChainBody = ChainBody {
    prot_entry: 961,
    body: 0x801F_69D8,
    action_ids: &[0xA1, 0xB4],
    arms: &[
        arm(0, 0x801F_6AC0, Advance).ctx_278(2),
        arm(1, 0x801F_6C8C, Advance)
            .stages(&[stage(Caster, Literal(6), Bump, 0x801F_6D58)])
            .ctx_278(3),
        arm(2, 0x801F_6E78, Advance),
        arm(3, 0x801F_7098, Advance)
            .stages(&[reaction(Bump, 0x801F_7370)])
            .hit(0x801F_73AC),
        arm(4, 0x801F_7440, Advance),
        arm(5, 0x801F_7520, Advance).stages(&[stage(Caster, Literal(0), Bump, 0x801F_7690)]),
        arm(6, 0x801F_7780, Set(DONE)),
        arm(DONE, 0x801F_77D4, Finish).clears_0d(),
    ],
};

/// PROT 0963 (Genocidal Cannon, `0xB3`) tick body, `0x801F6A20..0x801F81A0`.
///
/// `sltiu 0x12` through the table at `0x801F69D8`. Words 4 and 5 point at
/// the epilogue and nothing reaches them: arm 3 stores phase `6`
/// (`0x801F710C`). Arm 0 writes `ctx[+0x278] = 3`; arm 2 stages caster clip
/// `8` with a bump; arm `0x0C` writes `ctx[+0x278] = 0`; arm `0x0E` is the
/// party-row sweep, `FUN_801DD4B0(0x540)` at `0x801F7C54`, shape A, each hit
/// seat staged with its own reaction, and `ctx[+0x278] = 1`; arm `0x10`
/// clears the caster's stage (no bump). Arm `0x11` is terminal: `ctx[+0x0D]
/// = 0` (`0x801F8104`), busy cleared in the delay slot at `0x801F8120`.
///
/// Rates left out under the module's rate rule: arms 6..0x0B alternate the
/// caster between `3` and `4` and the body leaves it at `4`. Not ported: the
/// packet arms, the lighting and camera-record writes, the party-row
/// presentation stores. Countdowns: `capture_countdown::GENOCIDAL_CANNON`.
///
/// PORT: FUN_801F6A20 (PROT 0963; phase table + caster staging; packet arms unported)
pub const GENOCIDAL_CANNON_CHAIN: ChainBody = ChainBody {
    prot_entry: 963,
    body: 0x801F_6A20,
    action_ids: &[0xB3],
    arms: &[
        arm(0, 0x801F_6AD4, Advance).ctx_278(3),
        arm(1, 0x801F_6BD8, Advance),
        arm(2, 0x801F_6D00, Advance).stages(&[stage(Caster, Literal(8), Bump, 0x801F_6EC8)]),
        arm(3, 0x801F_6F4C, Set(6)),
        arm(6, 0x801F_7110, Advance),
        arm(7, 0x801F_7204, Advance),
        arm(8, 0x801F_73C0, Advance),
        arm(9, 0x801F_7464, Advance),
        arm(0x0A, 0x801F_75A8, Advance),
        arm(0x0B, 0x801F_7658, Advance),
        arm(0x0C, 0x801F_7838, Advance).ctx_278(0),
        arm(0x0D, 0x801F_799C, Advance),
        arm(0x0E, 0x801F_7A08, Advance)
            .stages(&[reaction(Bump, 0x801F_7CF0)])
            .ctx_278(1)
            .hit(0x801F_7C54),
        arm(0x0F, 0x801F_7D80, Advance),
        arm(0x10, 0x801F_7ED0, Advance).stages(&[stage(
            Caster,
            Literal(0),
            Untouched,
            0x801F_8008,
        )]),
        arm(0x11, 0x801F_80AC, Finish).clears_0d(),
    ],
};

/// PROT 0964's `0xB0` / `0xB1` / `0xB2` tick body, `0x801F69D8..0x801F88EC`.
///
/// The three ids run it identically: nothing in the body reads `+0x1DF`. It
/// picks one of three six-arm variants from the word at `0x801C8FE4` - the
/// roll the module's own `0xAF` Element Change body last accepted
/// ([`element_change_tick`]) - in arm 1: selector `0` stores phase `0x32`,
/// `1` stores `0x64`, `2` advances to `2`. Each variant's arm `k` (phase
/// `2 + k`, `0x32 + k` or `0x64 + k`) does the same thing: `k = 2` sets the
/// creature seat's rate `2`, `k = 3` steps its clip with a bump and rate `4`,
/// `k = 4` clears its stage and sweeps the party row
/// (`FUN_801DD4B0(0x580)` at `0x801F853C` / `0x801F7D28` / `0x801F74CC`,
/// shape A, each hit seat staged with its own reaction), and `k = 5` despawns
/// the creature (its `+0x14C = 0`, not a hit). The closing arm (`8`, `0x38`,
/// `0x6A`, all at `0x801F8800`) stores `0xFF`; `0xFF` zeroes `ctx[+0x0D]`,
/// stages the seat `0` with a bump and clears the busy flag.
///
/// Not ported: the packet arms, the creature's seating (`FUN_801F19EC`),
/// its effect retire and HP zero, and the per-variant `+0x04` tints.
/// Countdowns: `capture_countdown::ELEMENT_STRIKE`.
///
/// PORT: FUN_801F69D8 (PROT 0964; the `0xB0`..`0xB2` arms' phase chain + the variant fork + seat staging; packet arms unported)
pub const ELEMENT_STRIKE_CHAIN: ChainBody = ChainBody {
    prot_entry: 964,
    body: 0x801F_69D8,
    action_ids: &[0xB0, 0xB1, 0xB2],
    arms: &[
        arm(0, 0x801F_6BCC, Advance),
        arm(1, 0x801F_6D8C, ChainExit::Select(&[0x32, 0x64])),
        // Selector 2.
        arm(2, 0x801F_7F88, Advance),
        arm(3, 0x801F_8020, Advance),
        arm(4, 0x801F_8104, Advance).rates(&[rate(Seat, 2, 0x801F_816C)]),
        arm(5, 0x801F_8170, Advance)
            .stages(&[stage(Seat, Step, Bump, 0x801F_81D0)])
            .rates(&[rate(Seat, 4, 0x801F_81F4)]),
        arm(6, 0x801F_82B4, Advance)
            .stages(&[
                stage(Seat, Literal(0), Untouched, 0x801F_8378),
                reaction(Bump, 0x801F_85DC),
            ])
            .hit(0x801F_853C),
        arm(7, 0x801F_8664, Advance),
        arm(8, 0x801F_8800, Set(DONE)),
        // Selector 0.
        arm(0x32, 0x801F_77D0, Advance),
        arm(0x33, 0x801F_785C, Advance),
        arm(0x34, 0x801F_7924, Advance).rates(&[rate(Seat, 2, 0x801F_79AC)]),
        arm(0x35, 0x801F_79B0, Advance)
            .stages(&[stage(Seat, Step, Bump, 0x801F_7A10)])
            .rates(&[rate(Seat, 4, 0x801F_7A34)]),
        arm(0x36, 0x801F_7AD4, Advance)
            .stages(&[
                stage(Seat, Literal(0), Untouched, 0x801F_7B78),
                reaction(Bump, 0x801F_7DC8),
            ])
            .hit(0x801F_7D28),
        arm(0x37, 0x801F_7E44, Advance),
        arm(0x38, 0x801F_8800, Set(DONE)),
        // Selector 1.
        arm(0x64, 0x801F_6F04, Advance),
        arm(0x65, 0x801F_6F98, Advance),
        arm(0x66, 0x801F_707C, Advance).rates(&[rate(Seat, 2, 0x801F_70EC)]),
        arm(0x67, 0x801F_70F0, Advance)
            .stages(&[stage(Seat, Step, Bump, 0x801F_714C)])
            .rates(&[rate(Seat, 4, 0x801F_7170)]),
        arm(0x68, 0x801F_721C, Advance)
            .stages(&[
                stage(Seat, Literal(0), Untouched, 0x801F_72E4),
                reaction(Bump, 0x801F_756C),
            ])
            .hit(0x801F_74CC),
        arm(0x69, 0x801F_75F8, Advance),
        arm(0x6A, 0x801F_8800, Set(DONE)),
        arm(DONE, 0x801F_8854, Finish)
            .stages(&[stage(Seat, Literal(0), Bump, 0x801F_8880)])
            .clears_0d(),
    ],
};

/// Every phase-chain body, keyed `(prot_entry, body)`.
///
/// A function rather than a `const` table so each descriptor's `PORT:` tag
/// is referenced from a reachable function body, which is what the port
/// catalog's liveness reading keys on.
pub fn chain_bodies() -> &'static [ChainBody] {
    static ALL: [ChainBody; 14] = [
        SPOON_CHAIN,
        EARTHQUAKE_CHAIN,
        HYPER_CRUSH_CHAIN,
        HYPER_LIGHTNING_CHAIN,
        SPORE_GAS_CHAIN,
        POWER_UP_AA_CHAIN,
        V_WINDHASH_CHAIN,
        CROSS_BEAM_CHAIN,
        WATER_HAZARD_75_CHAIN,
        MEGATON_PRESS_CHAIN,
        NEO_STAR_SLASH_CHAIN,
        DEAD_END_CRISIS_CHAIN,
        GENOCIDAL_CANNON_CHAIN,
        ELEMENT_STRIKE_CHAIN,
    ];
    &ALL
}

#[cfg(test)]
mod chain_tests {
    use super::*;

    fn actor() -> CastActorState {
        CastActorState {
            hp: 500,
            anim_rate: ANIM_RATE_NORMAL,
            knockdown_anim: 0x0B,
            ..Default::default()
        }
    }

    /// Walk a body from phase 0 to its terminal arm; returns the ticks taken
    /// and the three actors as they end.
    fn walk(
        b: &ChainBody,
        selector: u8,
    ) -> (usize, CastActorState, CastActorState, CastActorState) {
        let mut ctx = CastModuleCtx {
            party_count: 3,
            ctx_0d: 1,
            ..Default::default()
        };
        let (mut c, mut v, mut s) = (actor(), actor(), actor());
        for n in 1..=0x200 {
            let t = run_chain_body(b, &mut ctx, &mut c, &mut v, &mut s, selector);
            if t.step == CastTickStep::Done {
                return (n, c, v, s);
            }
        }
        panic!("PROT {:04} never finished", b.prot_entry);
    }

    /// Every body reaches its own terminal arm from phase 0 - through named
    /// arms only - rather than falling off the head.
    #[test]
    fn every_chain_body_walks_to_its_terminal_arm() {
        for b in chain_bodies() {
            for sel in 0..3 {
                let mut ctx = CastModuleCtx::default();
                let (mut c, mut v, mut s) = (actor(), actor(), actor());
                loop {
                    let arm = b.arm(ctx.phase).unwrap_or_else(|| {
                        panic!(
                            "PROT {:04}: phase {:#x} names no arm",
                            b.prot_entry, ctx.phase
                        )
                    });
                    let finish = arm.exit == ChainExit::Finish;
                    let t = run_chain_body(b, &mut ctx, &mut c, &mut v, &mut s, sel);
                    assert_eq!(t.step == CastTickStep::Done, finish);
                    if finish {
                        break;
                    }
                }
            }
        }
    }

    /// The module's rate rule: no body leaves its caster or victim off the
    /// normal rate, since the port has no reset outside the module.
    #[test]
    fn every_chain_body_leaves_its_actors_at_the_normal_rate() {
        for b in chain_bodies() {
            for sel in 0..3 {
                let (_, c, v, _) = walk(b, sel);
                assert_eq!(
                    c.anim_rate, ANIM_RATE_NORMAL,
                    "PROT {:04} caster",
                    b.prot_entry
                );
                assert_eq!(
                    v.anim_rate, ANIM_RATE_NORMAL,
                    "PROT {:04} victim",
                    b.prot_entry
                );
            }
        }
    }

    /// The fold owns every hit: no body writes HP, and no hit-side reaction
    /// stage runs.
    #[test]
    fn no_chain_body_writes_hp() {
        for b in chain_bodies() {
            let (_, c, v, s) = walk(b, 0);
            for a in [c, v, s] {
                assert_eq!(a.hp, 500, "PROT {:04}", b.prot_entry);
                assert_eq!(a.hp_bar_delta, 0, "PROT {:04}", b.prot_entry);
            }
        }
    }

    /// A body whose caster clips the band replays stages nothing on the
    /// caster itself; one without such a row does.
    #[test]
    fn caster_stages_go_to_the_pre_pass_where_it_has_a_row() {
        for b in chain_bodies() {
            let (_, c, _, _) = walk(b, 0);
            let stages_caster = b
                .arms
                .iter()
                .flat_map(|a| a.stages)
                .any(|s| s.actor == ChainActor::Caster && !s.on_hit);
            if b.caster_stages_replayed() {
                assert_eq!(c.restage, 0, "PROT {:04}", b.prot_entry);
            } else if stages_caster {
                assert_ne!(c.restage, 0, "PROT {:04}", b.prot_entry);
            }
        }
        assert!(EARTHQUAKE_CHAIN.caster_stages_replayed());
        assert!(!POWER_UP_AA_CHAIN.caster_stages_replayed());
        assert!(!MEGATON_PRESS_CHAIN.caster_stages_replayed());
    }

    /// PROT 0964's arm 1 forks on the last Element Change roll.
    #[test]
    fn element_strike_forks_on_the_selector() {
        let b = &ELEMENT_STRIKE_CHAIN;
        for (sel, next) in [(0u8, 0x32u8), (1, 0x64), (2, 2)] {
            let mut ctx = CastModuleCtx {
                phase: 1,
                ..Default::default()
            };
            let (mut c, mut v, mut s) = (actor(), actor(), actor());
            run_chain_body(b, &mut ctx, &mut c, &mut v, &mut s, sel);
            assert_eq!(ctx.phase, next, "selector {sel}");
        }
    }

    /// PROT 0956's `0x75` steals a live victim's turn and refunds its item.
    #[test]
    fn water_hazard_75_marks_and_steals_the_turn() {
        let b = &WATER_HAZARD_75_CHAIN;
        let mut ctx = CastModuleCtx {
            phase: 2,
            ..Default::default()
        };
        let (mut c, mut s) = (actor(), actor());
        let mut v = CastActorState {
            init_key: 5,
            action_category: ACTION_CATEGORY_ITEM,
            queued_action: 0x21,
            ..actor()
        };
        let t = run_chain_body(b, &mut ctx, &mut c, &mut v, &mut s, 0);
        assert_eq!(t.refund, Some(0x21));
        assert_eq!(v.flags & 0x0400, 0x0400);
        assert_eq!(v.init_key, 0);
        assert_eq!(ctx.turn_cursor, 1);
        assert_eq!(ctx.phase, 3);
    }

    /// Bodies are keyed by `(entry, body)`: six images share `0x801F69D8`.
    #[test]
    fn keys_are_unique_and_disjoint_from_the_other_ports() {
        for (i, a) in chain_bodies().iter().enumerate() {
            for b in &chain_bodies()[i + 1..] {
                assert!((a.prot_entry, a.body) != (b.prot_entry, b.body));
            }
            if capture_trampoline_for(a.prot_entry).is_some() {
                for id in a.action_ids {
                    assert_eq!(capture_tick_body(a.prot_entry, *id), Some(a.body));
                }
            }
        }
        assert!(chain_body_for(960, PLASMA_STRIKE_TICK).is_none());
        assert!(chain_body_for(965, DOOMSDAY_TICK).is_none());
        assert_eq!(direct_chain_body(935).map(|b| b.body), Some(0x801F_69D8));
        assert!(direct_chain_body(960).is_none());
    }
}
