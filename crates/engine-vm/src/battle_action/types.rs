//! Core battle-action types: actor slots, action categories/states, poses, and the per-actor `BattleActor` / `BattleActionCtx` state structs.

pub use crate::battle_anim_rate::AnimRate;

/// Number of battle actor pointer-table slots (`0x801C9370` in retail).
/// Slots `0..3` are party members, `3..8` are monsters.
pub const ACTOR_SLOTS: usize = 8;

/// Number of bytes in the per-action parameter stream
/// (`actor[+0x1DF..+0x1F2]`).
pub const ACTION_PARAM_BYTES: usize = 0x14;

/// Action category - the actor's `+0x1DE` byte.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ActionCategory {
    /// Martial Arts (Tactical Arts). The directional input chain is staged
    /// before this driver runs; by the time `action_state` hits `0x0C`, the
    /// chain is recorded and the action is "done" for this state machine.
    TacticalArts = 0,
    /// Item.
    Item = 1,
    /// Magic.
    Magic = 2,
    /// Standard physical attack.
    Attack = 3,
    /// Spirit (Originals).
    Spirit = 4,
    /// Run / Defend.
    Run = 5,
    /// Item-target re-route. Not a true category - it's an intermediate
    /// signal that the item-arm of the magic flow uses.
    ///
    /// NB the re-route itself is **not** keyed on this byte: state `0x28`
    /// reads the *target* byte `+0x1DD` (see
    /// `battle_action::magic`'s `retarget_item_codes`). These two variants
    /// name the same values in the category space, and what the category
    /// dispatch does with them is open.
    ItemRetargetB = 8,
    /// Item-target re-route. Same caveat as [`ActionCategory::ItemRetargetB`].
    ItemRetargetA = 9,
}

impl ActionCategory {
    /// Decode from the raw byte stored at `actor[+0x1DE]`. Reserved values
    /// (`>= 6` except `8` and `9`) decode as [`ActionCategory::TacticalArts`]
    /// to match the retail "category-zero" fallback.
    pub fn from_byte(b: u8) -> Self {
        match b {
            0 => Self::TacticalArts,
            1 => Self::Item,
            2 => Self::Magic,
            3 => Self::Attack,
            4 => Self::Spirit,
            5 => Self::Run,
            8 => Self::ItemRetargetB,
            9 => Self::ItemRetargetA,
            _ => Self::TacticalArts,
        }
    }

    /// Encode back to the byte at `actor[+0x1DE]`.
    pub const fn as_byte(self) -> u8 {
        self as u8
    }
}

/// Symbolic names for the `ctx.action_state` cursor. The retail dispatch is a
/// 256-entry jump table at `0x801E29A8 + (action_state << 2)`; values not
/// listed here fall through to the function epilogue (no-op for that frame).
///
/// Names mirror the band classifications in `docs/subsystems/battle-action.md`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ActionState {
    /// Action begin - resets ctx counters, copies queued action.
    Begin = 0x00,
    /// Pre-action wait (FUN_8003F2B8 gate).
    PreActionWait = 0x0A,
    /// Action queued from menu (holds while `ctx.menu_open != 0`).
    QueuedFromMenu = 0x0B,
    /// Action seed - reads action category, dispatches into appropriate band.
    ActionSeed = 0x0C,

    /// Attack - face target.
    AttackFace = 0x14,
    /// Attack - windup.
    AttackWindup = 0x15,
    /// Attack - advance toward target.
    AttackAdvance = 0x16,
    /// Attack - close-range.
    AttackCloseRange = 0x17,
    /// Attack - strike.
    AttackStrike = 0x18,
    /// Attack - short-step (party slot < 3 only).
    AttackShortStep = 0x19,
    /// Attack chain - strike loop.
    AttackChain = 0x1E,
    /// Attack - recovery wait.
    AttackRecovery = 0x1F,
    /// Attack - return.
    AttackReturn = 0x20,

    /// Magic / Item - cast begin.
    MagicCastBegin = 0x28,
    /// Magic - pre-cast wait.
    MagicPreCastWait = 0x29,
    /// Magic - animation chain.
    MagicAnimChain = 0x2A,
    /// Magic - sustained anim.
    MagicSustain = 0x2B,
    /// Magic - hit-frame loop.
    MagicHitLoop = 0x2C,
    /// Magic - recovery.
    MagicRecovery = 0x2D,
    /// Magic - exit.
    MagicExit = 0x2E,

    /// Summon - invoke.
    SummonInvoke = 0x32,
    /// Summon - fade in.
    SummonFadeIn = 0x33,
    /// Summon - actor freeze.
    SummonActorFreeze = 0x34,
    /// Summon - sustain.
    SummonSustain = 0x35,
    /// Summon - return-from-fade.
    SummonReturn = 0x36,
    /// Summon - verify all alive.
    SummonVerifyAlive = 0x37,
    /// Summon - done.
    SummonDone = 0x38,

    /// Spirit / Item - pre-arm.
    SpiritPreArm = 0x3C,
    /// Spirit - wait.
    SpiritWait = 0x3D,
    /// Spirit - fire.
    SpiritFire = 0x3E,
    /// Spirit - wait & fire damage.
    SpiritFireDamage = 0x3F,
    /// Spirit - post-damage.
    SpiritPostDamage = 0x40,

    /// Spirit super-arts - entry variant.
    SpiritArtsEntry = 0x46,
    /// Spirit-arts - sustain.
    SpiritArtsSustain = 0x47,
    /// Spirit-arts - flush.
    SpiritArtsFlush = 0x48,

    /// Done - cleanup phase. Universal "action concluded, clean up" arm.
    DoneCleanup = 0x50,
    /// Done - fade-down.
    DoneFadeDown = 0x51,
    /// Done - multi-cast continuation.
    DoneMultiCast = 0x52,
    /// End-of-action gate.
    EndOfAction = 0x5A,

    /// Run - flee anim begin.
    RunBegin = 0x64,
    /// Run - wait. On timer expiry the retail 0x65 case branches on the
    /// run outcome: a FAILED run routes back to `0x50` (Done band - the
    /// action is consumed, the battle continues), a SUCCESSFUL escape
    /// routes to `0x66`.
    RunWait = 0x65,
    /// Run - successful-escape teardown. The retail 0x66 case stages a
    /// 0x40-frame `(0xFF,0xFF,0xFF) → (0,0,0)` screen fade through the
    /// fade-primitive spawner (`FUN_80024E80`, template at `DAT_801C9070`),
    /// sets the battle-end signal `DAT_8007BD71 = 0xFE` (the same byte the
    /// `0x5A` wipe gate sets), and parks in the `0x67` terminal hold - the
    /// party leaves the battle. (An earlier reading labelled this state
    /// "run failed, battle continues"; the battle-end signal byte falsifies
    /// that - the failed-run path is the `0x65 → 0x50` branch above.)
    RunEscape = 0x66,
    /// Capture - start.
    CaptureStart = 0x68,
    /// Capture - wait.
    CaptureWait = 0x69,
    /// Capture - sustain.
    CaptureSustain = 0x6A,
    /// Capture - end.
    CaptureEnd = 0x6B,

    /// Magic-capture branch.
    MagicCaptureBranch = 0x6E,
    /// Magic-capture - fade.
    MagicCaptureFade = 0x6F,
    /// Magic-capture - phase 2.
    MagicCapturePhase2 = 0x70,
    /// Magic-capture - finalize.
    MagicCaptureFinalize = 0x71,

    /// Idle hold (battle paused?).
    IdleHold = 0xFD,
    /// End of round. Retail's only writer is the **non-wipe** arm of the
    /// `0x5A` end-of-action gate, reached when every living actor has acted
    /// and BOTH sides still have someone standing. The wipe arms never write
    /// a state byte - they raise the battle-end signal `DAT_8007BD71 = 0xFE`
    /// instead (as does the escape teardown `0x66`), so battle end is
    /// signalled through the signal byte, never through this state. See
    /// `docs/subsystems/battle-action.md`
    /// § "`0xFF` is the round boundary, not the battle's end".
    RoundEnd = 0xFF,
}

impl ActionState {
    /// Decode from the raw byte. Returns `None` for unmapped values; callers
    /// treat those as "default no-op arm" (the retail dispatcher's default
    /// epilogue).
    pub fn from_byte(b: u8) -> Option<Self> {
        Some(match b {
            0x00 => Self::Begin,
            0x0A => Self::PreActionWait,
            0x0B => Self::QueuedFromMenu,
            0x0C => Self::ActionSeed,

            0x14 => Self::AttackFace,
            0x15 => Self::AttackWindup,
            0x16 => Self::AttackAdvance,
            0x17 => Self::AttackCloseRange,
            0x18 => Self::AttackStrike,
            0x19 => Self::AttackShortStep,
            0x1E => Self::AttackChain,
            0x1F => Self::AttackRecovery,
            0x20 => Self::AttackReturn,

            0x28 => Self::MagicCastBegin,
            0x29 => Self::MagicPreCastWait,
            0x2A => Self::MagicAnimChain,
            0x2B => Self::MagicSustain,
            0x2C => Self::MagicHitLoop,
            0x2D => Self::MagicRecovery,
            0x2E => Self::MagicExit,

            0x32 => Self::SummonInvoke,
            0x33 => Self::SummonFadeIn,
            0x34 => Self::SummonActorFreeze,
            0x35 => Self::SummonSustain,
            0x36 => Self::SummonReturn,
            0x37 => Self::SummonVerifyAlive,
            0x38 => Self::SummonDone,

            0x3C => Self::SpiritPreArm,
            0x3D => Self::SpiritWait,
            0x3E => Self::SpiritFire,
            0x3F => Self::SpiritFireDamage,
            0x40 => Self::SpiritPostDamage,

            0x46 => Self::SpiritArtsEntry,
            0x47 => Self::SpiritArtsSustain,
            0x48 => Self::SpiritArtsFlush,

            0x50 => Self::DoneCleanup,
            0x51 => Self::DoneFadeDown,
            0x52 => Self::DoneMultiCast,
            0x5A => Self::EndOfAction,

            0x64 => Self::RunBegin,
            0x65 => Self::RunWait,
            0x66 => Self::RunEscape,
            0x68 => Self::CaptureStart,
            0x69 => Self::CaptureWait,
            0x6A => Self::CaptureSustain,
            0x6B => Self::CaptureEnd,

            0x6E => Self::MagicCaptureBranch,
            0x6F => Self::MagicCaptureFade,
            0x70 => Self::MagicCapturePhase2,
            0x71 => Self::MagicCaptureFinalize,

            0xFD => Self::IdleHold,
            0xFF => Self::RoundEnd,

            _ => return None,
        })
    }

    /// Encode back to the byte at `ctx.action_state`.
    pub const fn as_byte(self) -> u8 {
        self as u8
    }
}

/// Pose IDs used by `FUN_801D5854(actor_id, pose_id)`. Surfaced from the
/// docs:
///
/// - `6` = idle / breathing
/// - `7` = ready / pre-action
/// - `8` = action-end / hit-recovery
/// - `9` = defeat / down
///
/// Retail-side these select **camera/presentation programs** (the driver
/// never writes the anim fields; anim ids are entry indices with idle = 0,
/// aligned with this space at 7/8/9 by design - see
/// `docs/subsystems/battle-action.md`). The engine's pose host hook also
/// drives the same-numbered action clips, which matches the frames retail
/// shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Pose {
    Idle = 6,
    Ready = 7,
    Recover = 8,
    Defeat = 9,
}

/// Per-actor flag bits at `actor[+0x1DC]`. Set by the strike / spell loops.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ActorFlags(pub u8);

impl ActorFlags {
    pub const WINDUP_DONE: u8 = 0x01;
    pub const ADVANCE_DONE: u8 = 0x02;
    pub const EXIT: u8 = 0x04;
    /// "This actor's effects are suppressed": the per-frame effect-script
    /// walk (`FUN_801DEA50`, `0x801decd0..dc`) is a no-op while set.
    pub const FX_SUPPRESSED: u8 = 0x08;

    pub const fn empty() -> Self {
        Self(0)
    }
    pub const fn has(self, mask: u8) -> bool {
        (self.0 & mask) != 0
    }
    pub fn set(&mut self, mask: u8) {
        self.0 |= mask;
    }
    pub fn clear(&mut self, mask: u8) {
        self.0 &= !mask;
    }
}

/// Per-actor state read or written by `FUN_801E295C`.
///
/// Field naming uses the byte-offset convention from `docs/subsystems/battle-action.md`
/// to keep the link to the decompilation explicit. Engines free to back this
/// with whatever data structure makes sense - the state machine mutates this
/// struct directly and dispatches side effects through [`BattleActionHost`].
#[derive(Debug, Clone, Default)]
pub struct BattleActor {
    /// `+0x14C` - liveness flag (non-zero = alive). Read by every state's
    /// "is target valid" check.
    pub liveness: u16,
    /// `+0x150` - current MP (subtracted by Magic / Spirit cast costs).
    pub mp: u16,
    /// `+0x16E` - per-actor flag bank. Bit `0x4` = "non-targetable", bits
    /// `0x380` = AI-controlled, `0x404` = AI + non-targetable. Read at state
    /// `ActionSeed` to decide between party-setup and monster-setup hooks.
    pub field_flags: u16,
    /// `+0x16c` - per-turn **initiative key**. The next-actor selector
    /// (`recompute_battle_order` / `FUN_801daba4`) picks the living actor with
    /// the highest key each turn (random tiebreak), then the key is consumed.
    /// Seeded each round from the actor's SPD (`+0x164`):
    /// `init_key = speed + rand()%(speed/2 + 1) + 1` (`overlay_0897_801e23ec`).
    /// `0` = "has acted this round / dead" (the selector zeroes dead actors'
    /// keys). See `docs/subsystems/battle-formulas.md`.
    pub init_key: u16,
    /// `+0x154` - **live action gauge (AGL)**, the pool a turn's actions are
    /// paid out of. Restored at every round boundary by `FUN_801D88CC` loop A
    /// ([`crate::battle_formulas::round_reset_agility`]) from [`Self::agl_base`],
    /// and spent per swing by the enemy budget loop
    /// ([`crate::battle_action::enemy_action_budget`]).
    ///
    /// Distinct from [`Self::init_key`]: the key decides *when* an actor acts,
    /// this decides *how much* it can do once it does.
    pub agl: u16,
    /// `+0x156` - **base action gauge**, the value [`Self::agl`] is restored to
    /// each round. Read-only during a battle.
    pub agl_base: u16,
    /// `+0x170` - **spirit-art gauge** (0..=100). The shared damage finisher
    /// `FUN_801ddb30` accrues this on the *defender* from each hit's
    /// post-mitigation damage (`pct = max(1, over*100/maxhp)`, plus the two
    /// equipment "spirit gain up" bits for a party defender), clamped to 100;
    /// the engine fills it via [`crate::battle_formulas::spirit_gauge_fill`].
    /// A party member's Spirit-Art (`ActionState::SpiritArtsEntry`) becomes
    /// available once this reaches its ceiling. Distinct from the per-turn AP
    /// budget the **Spirit command** charges (`ApGauge::charge_spirit`).
    ///
    /// REF: FUN_801ddb30 (the finisher's spirit stage; ported as
    /// [`crate::battle_formulas::spirit_gauge_fill`])
    pub spirit_gauge: u16,
    /// `+0x14C` / `+0x14E` - live (authoritative) HP and max HP. Every
    /// liveness test in the SM reads `+0x14C`; [`Self::liveness`] is the same
    /// halfword under the name the flag-shaped reads use.
    pub hp: u16,
    pub max_hp: u16,
    /// `+0x172` - HP-bar **display** cursor. Retail keeps the authoritative
    /// current HP at `+0x14C` and drains the on-screen bar value at `+0x172`
    /// toward it over several frames; the fade-down settle check
    /// (`FUN_801E7250`) compares the pair. `Some(shown)` while the host
    /// animates HP bars, `None` when it does not (always settled).
    pub hp_display: Option<u16>,
    /// `+0x10` - signed **pending HP-bar delta accumulator**: how much
    /// [`Self::hp_display`] still owes. Positive means the bar has to fall.
    ///
    /// The bar and live HP converge only through this field
    /// ([`crate::battle_hp_bar`]): `FUN_80047430` moves a quarter of it into
    /// the bar per frame on a party slot, and does nothing at all while it is
    /// zero (the guard at `0x800474E8`). A `hp != hp_display` pair with a zero
    /// accumulator is therefore absorbing, and parks the action SM in state
    /// `0x51` forever on any party-targeted action.
    pub hp_bar_pending: i32,
    /// `+0x178` - last-action MP cost (used to display `-N MP` on screen).
    pub last_mp_cost: u16,
    /// `+0x21D` - the per-actor **animation-rate scalar** (normal `8`). Three
    /// consumers read it in the dumped corpus, which is what settles the
    /// name:
    ///
    /// * the SCUS anim tick `FUN_80047430` advances the render node's 12.4
    ///   anim cursor by `(frame_dt * rate * clip_rate) >> 1` (`>> 2` on the
    ///   idle branch), so `4` is half speed, `2` quarter speed, `0` a freeze;
    /// * the arts after-image walk `FUN_80049348` spaces its two mesh ghosts
    ///   `8 / rate` frames apart, so the trail stretches as time slows;
    /// * the attack band multiplies it into the per-frame X/Z impact drift,
    ///   so knockback slows with the clock (the reading this field was
    ///   previously named for - "impact-step magnitude" - is that one
    ///   consumer, not the field).
    ///
    /// Writers: the anim commit `FUN_8004AD80` drives the arts slow-motion
    /// (see [`crate::battle_anim_rate`]), and `FUN_801E93C8` restores every
    /// slot to `8` once the art clip has ended
    /// ([`crate::battle_gauge_rearm::rearm_gauge`]).
    pub anim_rate: AnimRate,
    /// `+0x21F` - the 1-based **impact-effect selector**: which entry of
    /// the 5-entry impact-config table (`0x801F53D4`) currently owns this
    /// actor's tint word [`Self::render_color`]. Written by the move-power
    /// impact arm (`FUN_801E09F8` stores the record's `+0x0A` selector) and
    /// by the per-clip impact arms (`FUN_8004CE2C` -
    /// [`crate::battle_impact_fx`]); cleared by the presentation tick
    /// `FUN_80050120` once the tint has decayed to neutral. `0` = no
    /// impact tint armed.
    pub impact_state: u8,
    /// `+0x224` - action recoil magnitude - written by `DoneCleanup`.
    pub action_recoil: u8,
    /// `+0x225` - capture state byte - `2` while captured.
    pub capture_state: u8,
    /// `+0x21B` - hit-count bound (script-defined; loop exits at
    /// `ctx.hit_counter >= hit_count_bound`).
    pub hit_count_bound: u8,
    /// `+0x21C` - per-actor render flag - `0xFF` while hidden by summon
    /// fade, `0x02` while captured, `0` otherwise. The target-select cursor
    /// ([`crate::battle_action::target_cursor_highlight`]) also drives it as a
    /// brightness level: `5` on the pointed-at monster, `200` on the others.
    pub render_flag: u8,
    /// `+0x4` - per-actor mesh colour/tint word fed to the battle actor
    /// renderer. The target-select cursor writes `0x20080200` (bright) or
    /// `0x00401004` (dimmed); the summon fade clears it to `0`.
    pub render_color: u32,
    /// `+0xC` - per-actor mesh brightness/scale word (`0x1000` = the neutral
    /// q12 unit). The target-select cursor sets it to `0x1000` when the cursor
    /// is up and `0` when it retires.
    pub render_scale: u32,
    /// `+0x46` - facing angle (i12 in `0xFFF` range; written from bearing
    /// checks).
    pub facing_angle: u16,
    /// `+0x3C` / `+0x40` - the actor's **seat** (anchor) position pair. The
    /// battle setup writes the authored formation seat here and copies it
    /// into the live pair `+0x34`/`+0x38` (`FUN_800513F0`; see
    /// `engine-core::battle_seats`). The range law measures the *target*
    /// side against this pair, the separation pass measures overlap on it,
    /// and state `0x16`'s arrival shove moves it together with the live
    /// pair. `None` = not seated yet - hosts fall back to the live position.
    pub seat: Option<(i16, i16)>,
    /// `+0x1D9` - current anim ID (read-only here; written by the animation
    /// system).
    pub current_anim: u8,
    /// `+0x1DA` - queued next anim ID. The state machine writes this; the
    /// animation system reads `current_anim` toward `queued_anim`.
    pub queued_anim: u8,
    /// `+0x1DB` - the **latched** staged anim id: `FUN_8004AD80` copies
    /// `+0x1DA` here once per animation tick, *before* an art-bank id is
    /// rewritten to its dynamic slot (`0x8004AEB0..0x8004AEB8`), so this byte
    /// keeps the raw id of the clip that is playing.
    ///
    /// It is the byte the battle camera dispatches on - twice, over two
    /// disjoint bands of the same id space: `FUN_801D5854` case 6's own
    /// per-character arms take `0x11..=0x18` and the per-art attack camera
    /// `FUN_801D71B8` takes `0x1A..=0x2D`
    /// ([`crate::battle_attack_camera::ART_JUMP_TABLES`]). Nothing else in
    /// the ported state machine reads it.
    pub latched_anim: u8,
    /// `+0x1DC` - per-actor flag bits. See [`ActorFlags`].
    pub flag_bits: ActorFlags,
    /// `+0x1DD` - active-target slot index (used by Magic / Item to retarget
    /// mid-chain).
    pub active_target: u8,
    /// `+0x1DE` - action category. See [`ActionCategory`].
    pub action_category: u8,
    /// `+0x1DF..+0x1F2` - per-action parameter byte stream (item ID / spell
    /// ID / strike-anim list). The attack band terminates on `0x00`, the
    /// magic band on `0xFF` (`-1`) - retail uses different sentinels per
    /// band. Read sequentially via `params[strike_index]`. Pre-sized to
    /// [`ACTION_PARAM_BYTES`].
    pub params: [u8; ACTION_PARAM_BYTES],
    /// `+0x15` - per-strike index used to walk `params` during attack-chain
    /// and magic-anim-chain. Each strike bumps it.
    pub strike_index: u8,
    /// `+0x16` - combo bit (cleared by `AttackShortStep` when in range).
    pub combo_bit: u8,
    /// `+0x1F4` - arms input cursor. `FUN_801EC3E4` uses it both to index the
    /// caller's command record and as a head guard (`< 4`).
    ///
    /// It is retail's **per-art hit index**, and it is not the same counter as
    /// [`Self::strike_index`]. `FUN_801EC3E4` reads it at `0x801EC45C`, bounds
    /// it with `sltiu v0,v1,0x4` at `0x801EC480`, fetches exactly one power
    /// byte at that offset, and advances it once in the epilogue
    /// (`0x801EECDC..0x801EECE8`). Its caller is the **animation** tick
    /// `FUN_80047430` (`0x800478A0`, `0x80047BF0`) - one call per hit event in
    /// the staged clip - so a single staged art constant walks its whole power
    /// list without the stream cursor moving. See
    /// `docs/subsystems/battle-action.md` § A Tactical Art is an ordinary
    /// attack-band action for what the port does instead.
    pub input_cursor: u8,
    /// `+0x158` - ATK **working** (the attacker's offense the damage routine
    /// reads; `+0x15A` is the base a buff restores to). The Arms execution
    /// resolver folds the equipped weapon's attack bonus into this per
    /// committed command - see
    /// [`crate::battle_formulas::arms_weapon_atk_fold`].
    pub atk_working: u16,
    /// `+0x1F5` - anim-cue flag (read at state `SummonFadeIn` for fade-in
    /// trigger).
    pub anim_cue: u8,
    /// `+0x1F9` - "spirit shield" flag - gates spirit-arts variant path.
    pub spirit_shield: u8,
    /// `+0x1FA` - spell-cast iteration counter.
    pub spell_iter: u8,
    /// `+0x18` - UI element id (transient - written by `ActionSeed`).
    pub ui_element_id: u8,
    /// `+0x1E0` - sub-routing byte. `9` routes Magic to summon path.
    pub sub_route: u8,
    /// `+0x1E7` - queued anim staged for spirit / item paths.
    pub queued_anim_b: u8,
    /// `+0x1E8` - the committed action's **effect class**, seeded once at
    /// [`ActionState::SpiritPreArm`] and read by everything downstream that
    /// has to know *what kind* of thing this action does.
    ///
    /// Retail seeds it from one of two disc tables, picked on the category
    /// byte `+0x1DE` (`overlay_battle_action_801e295c.txt`
    /// `0x801E3B70..0x801E3CB0`):
    ///
    /// * category `1` (Item): the item's property record `+1`
    ///   (`0x80074368 + id*0xC`) indexes the **item-effect descriptor table**
    ///   `0x800752C0` (4-byte stride) and this takes the record's `+0` byte -
    ///   `legaia_asset::item_effect::ItemEffect::class`.
    /// * any other category (Magic / Spirit): the **spell table** record
    ///   `0x800754C8 + id*0xC`, byte `+0` -
    ///   `legaia_asset::spell_names::SpellEntry::class`.
    ///
    /// Both legs land in one class space: `0..=8` are the applier's effect
    /// classes (heal / cure / revive / shield / buff), and the larger values
    /// (`0x14` plain cast, `0x32` summon, `0x63` capture) are the spell-band
    /// routing bytes. Consumers: the damage primitive's `a0`
    /// ([`BattleActionHost::apply_damage`], retail `0x801E4124`), the
    /// cue-group site selection ([`crate::battle_cue_group::cue_group_for`])
    /// and the cast-audio dispatcher
    /// ([`crate::battle_cast_cue::cast_audio_cue`]).
    pub cast_class: u8,
    /// `+0x1E9` - the class's **tier / sub-index**, seeded beside
    /// [`Self::cast_class`] from byte `+1` of the same record.
    ///
    /// For an item this is `legaia_asset::item_effect::ItemEffect::tier` (the
    /// heal-amount row, the buff stat, ...); for a spell it is the record's
    /// `+1` sub-index (`docs/formats/spell-table.md`). It is the `param_2`
    /// the applier's cue-group sites turn into a group id for classes `0`,
    /// `1`, `2` and `7`, and the byte the cast-cue dispatcher's class-`7` arm
    /// gates on.
    pub cast_sub_class: u8,
    /// Chosen Tactical Art for this turn. When `Some`, the strike-band
    /// states call `BattleActionHost::art_record(character, action)` to
    /// fetch power bytes / hit timings / status effect. `None` falls
    /// back to generic-attack defaults. Set by the engine when the
    /// command queue resolves to an art (via `resolve_action_queue`).
    pub chosen_art: Option<legaia_art::ActionConstant>,
    /// **Port-side carrier, no retail offset.** The per-strike power profile
    /// the acting party member's Tactical-Arts entry resolved to, staged
    /// beside [`Self::params`] and read by the same [`Self::strike_index`]
    /// cursor.
    ///
    /// Retail needs no such array: the strike loop stages an art constant and
    /// the damage resolver reads that art's record. The port's entry resolver
    /// (`engine-core`'s `resolve_arts_input_entry`) has already folded three
    /// things the record alone cannot answer - a Miracle / Super finisher's
    /// replacement queue, an unmatched direction's synthetic plain swing, and
    /// the tier-0 degradation for an art whose record is not loaded - so the
    /// profile it produced is the authority for the turn and this is where it
    /// travels with the action. A slot with `None` here falls back to the art
    /// record's own `power[strike_index]`, which is the pre-carrier behaviour.
    pub art_power: [Option<legaia_art::PowerByte>; ACTION_PARAM_BYTES],
    /// Sibling of [`Self::art_power`]: the status effect the staged entry
    /// applies on a landing hit. [`legaia_art::EnemyEffect::None`] defers to
    /// the art record's own `enemy_effect`.
    pub art_enemy_effect: legaia_art::EnemyEffect,
    /// Which playable character occupies this slot. Used as the lookup
    /// key into the per-character art tables. Defaults to Vahn - engines
    /// must set this for the correct slot before the strike runs.
    pub character: legaia_art::Character,
}

impl BattleActor {
    pub fn new() -> Self {
        Self::default()
    }

    /// Read a parameter byte at `strike_index + offset`.
    /// Out-of-range reads return `0xFF` (the sentinel terminator).
    pub fn read_param(&self, offset: usize) -> u8 {
        let idx = self.strike_index as usize + offset;
        self.params.get(idx).copied().unwrap_or(0xFF)
    }

    /// Drop the staged Tactical-Arts profile ([`Self::art_power`] /
    /// [`Self::art_enemy_effect`] / [`Self::chosen_art`]).
    ///
    /// Called wherever the action-parameter stream itself is cleared: a
    /// profile that outlived its stream would re-key the *next* action's
    /// strikes to the previous turn's art, which is the same carried-over-byte
    /// class of defect the stream clear exists to prevent.
    pub fn clear_art_profile(&mut self) {
        self.art_power = [None; ACTION_PARAM_BYTES];
        self.art_enemy_effect = legaia_art::EnemyEffect::None;
        self.chosen_art = None;
    }

    /// Stage a Tactical-Arts turn's per-strike profile: `power[i]` is the
    /// power byte the `i`-th staged stream byte resolves damage from. Longer
    /// lists are truncated to [`ACTION_PARAM_BYTES`].
    pub fn stage_art_profile(
        &mut self,
        art: Option<legaia_art::ActionConstant>,
        power: &[legaia_art::PowerByte],
        enemy_effect: legaia_art::EnemyEffect,
    ) {
        self.art_power = [None; ACTION_PARAM_BYTES];
        for (slot, pb) in self.art_power.iter_mut().zip(power.iter()) {
            *slot = Some(*pb);
        }
        self.art_enemy_effect = enemy_effect;
        self.chosen_art = art;
    }

    /// Seed the HP-bar accumulator the way a landed hit does
    /// ([`crate::battle_hp_bar::accumulate_pending`], retail `FUN_801EC3E4`):
    /// **add** `delta` to any remainder still in flight, then clamp it to the
    /// value currently drawn on the bar so the ramp cannot overshoot zero.
    ///
    /// `delta` is positive for damage (the bar has to fall). No-op when the
    /// host does not animate bars (`hp_display == None`), which keeps a
    /// non-animating engine exactly where it was.
    ///
    /// REF: FUN_801EC3E4 (kernel + `// PORT:` tag in `battle_hp_bar`)
    pub fn accumulate_hp_bar(&mut self, delta: i32) {
        let Some(display) = self.hp_display else {
            return;
        };
        self.hp_bar_pending =
            crate::battle_hp_bar::accumulate_pending(self.hp_bar_pending, display, delta);
    }

    /// Seed the HP-bar accumulator the way the **item / restore applier**
    /// does ([`crate::battle_hp_bar::assign_pending`], retail `FUN_800402F4`):
    /// a bare *assignment* of `-delta`, discarding any remainder still in
    /// flight.
    ///
    /// `delta` is the signed change applied to live HP - a heal is positive, a
    /// hit negative - i.e. the same `s4` the routine folds into the stat
    /// halfword at `0x800408A8` before the three identical seed stores at
    /// `0x800408FC` / `0x80040D28` / `0x800410BC`. Retail leaves the displayed
    /// value `+0x172` alone here; the ramp is what carries it, which is why a
    /// caller that writes live HP and **skips this seed** leaves the pair
    /// `hp != hp_display` with a zero accumulator - the absorbing state that
    /// parks the action SM at `0x51` forever (see [`crate::battle_hp_bar`]).
    ///
    /// No-op when the host does not animate bars (`hp_display == None`),
    /// matching [`Self::accumulate_hp_bar`].
    ///
    /// REF: FUN_800402F4 (kernel + `// PORT:` tag in `battle_hp_bar`)
    pub fn assign_hp_bar(&mut self, delta: i16) {
        if self.hp_display.is_none() {
            return;
        }
        self.hp_bar_pending = crate::battle_hp_bar::assign_pending(delta);
    }

    /// Begin animating this actor's HP bar from its current live HP, if the
    /// host is not already animating it. Idempotent.
    ///
    /// Retail has no equivalent - `+0x172` is seeded at battle load and never
    /// unset. The port's `Option` models "this host draws bars at all", so a
    /// host opts in once and the pair behaves like retail from then on.
    pub fn arm_hp_bar(&mut self) {
        if self.hp_display.is_none() {
            self.hp_display = Some(self.hp);
            self.hp_bar_pending = 0;
        }
    }

    /// One frame of HP-bar ramp for this actor in `slot`
    /// ([`crate::battle_hp_bar::bar_step_for_slot`], retail `FUN_80047430`).
    ///
    /// REF: FUN_80047430 (kernel + `// PORT:` tags in `battle_hp_bar`)
    pub fn tick_hp_bar(&mut self, slot: u8) {
        let Some(display) = self.hp_display else {
            return;
        };
        let st = crate::battle_hp_bar::bar_step_for_slot(slot, display, self.hp_bar_pending);
        self.hp_display = Some(st.display);
        self.hp_bar_pending = st.pending;
    }

    /// Force the displayed HP back onto live HP, dropping any outstanding
    /// accumulator - the per-round status ticker's re-sync
    /// ([`crate::battle_hp_bar::resync_display`], retail `FUN_801E752C`).
    ///
    /// REF: FUN_801E752C (kernel + `// PORT:` tag in `battle_hp_bar`)
    pub fn resync_hp_bar(&mut self) {
        if self.hp_display.is_none() {
            return;
        }
        let st = crate::battle_hp_bar::resync_display(self.hp);
        self.hp_display = Some(st.display);
        self.hp_bar_pending = st.pending;
    }
}

/// Battle context fields read or written by `FUN_801E295C`. The retail layout
/// is the live struct at `0x800EB654` pointed-to by `_DAT_8007BD24`. Field
/// names mirror the `+0xNNN` offsets from `docs/subsystems/battle-action.md`.
///
/// We model only the fields the action state machine touches; the full ctx
/// struct is much larger and managed by the rest of the battle overlay.
#[derive(Debug, Clone, Default)]
pub struct BattleActionCtx {
    /// `[7]` - execution phase / action-state cursor. The outer `switch
    /// (ctx[7])`. Stored as raw byte so unmapped values round-trip.
    pub action_state: u8,
    /// `[+0x13]` - active actor slot index (drives the
    /// `(&DAT_801C9370)[ctx[0x13]]` lookup). Range `0..=7`.
    pub active_actor: u8,
    /// `[+0x274]` - the action category the battle UI staged for this turn.
    /// `recompute_battle_order` (`FUN_801DABA4`) is its retail writer
    /// (`lbu v0,0x11(v1); sb v0,0x274`).
    pub queued_action: u8,
    /// `[+0x1A]` - the **turn cursor**: how many entries of this round's
    /// battle order have been consumed.
    ///
    /// It is a context field, not an actor one. Every access in
    /// `FUN_801E295C` goes through `s5 = ctx + 0x11` as `0x9(s5)`, and there
    /// are exactly four across the dispatcher's 4099 instructions:
    ///
    /// * `Begin` seeds it from the formation-advantage byte `+0x290` -
    ///   `0` when the byte is `0`, `ctx[+0x00]` (the party count) when it is
    ///   `1`, `ctx[+0x01]` (the monster count) when it is `2`, and left alone
    ///   for anything else (`0x801E2AC0..0x801E2B24`).
    /// * the counter-attack swap bumps it (`0x801E36D0`),
    /// * the run arm bumps it once per combatant it removes from the round
    ///   (`0x801E5870`),
    /// * the end-of-action gate bumps it and compares the result against
    ///   `ctx[+0x00] + ctx[+0x01] - ctx[+0x25]` to decide "next actor" versus
    ///   "round over" (`0x801E679C..0x801E67C8`).
    ///
    /// That last compare is what pins the reading: the bound is the seated
    /// combatant count less the skipped tail, so the thing being compared is a
    /// position in the order, not anything per-actor.
    ///
    /// None of the three bytes the bound is built from is modelled here.
    /// `ctx[+0x00]` / `ctx[+0x01]` are the **seated** party / monster counts
    /// (`FUN_801E7250`'s all-target arm scans `0 .. ctx[+0x00]`, which is what
    /// makes it a party-side scan). `ctx[+0x25]` is the **round-skip** count -
    /// cleared once per round at `0x801DAB84` and bumped at `0x801DAC2C` for
    /// each slot that is dead *and* still holds an unspent initiative key,
    /// i.e. a combatant that died before its turn came up. The port compares
    /// against the living count instead; see
    /// `end_of_action`'s comment in `battle_action::done` for the one direction
    /// in which that differs.
    ///
    /// REF: FUN_801E295C (`ctx[+0x1A]`; the `PORT:` anchor for the seeding
    /// arm is `battle_action::dispatch`'s `seed_turn_cursor`)
    pub turn_cursor: u8,
    /// `[+0x276]` - menu-open flag (gates the `QueuedFromMenu`/`PreActionWait`
    /// transition). Non-zero while a menu is still drawing.
    pub menu_open: u8,
    /// `[+0x277]` - summon-frame index written at `SummonInvoke`.
    pub summon_frame_idx: u8,
    /// `[+0x278]` / `[+0x279]` - summon staging counters.
    pub summon_staging_a: u8,
    pub summon_staging_b: u8,
    /// `[+0x287]` / `[+0x288]` - counter-attack trigger flags read at
    /// `AttackReturn`.
    pub counter_attack_a: u8,
    pub counter_attack_b: u8,
    /// `[+0x290]` - the formation advantage the battle-setup roll
    /// (`FUN_80051D84`) wrote: `1` back attack, `2` pre-emptive strike. `Begin`
    /// **latches** it into [`Self::formation_latched`] and then clears it, so
    /// this field is only live for the first pass through state `0x00`.
    ///
    /// REF: FUN_80051D84
    pub formation_advantage: u8,
    /// `[+0x291]` - the latched copy of [`Self::formation_advantage`], written
    /// by `Begin`. This is the copy that survives the battle, and it is what
    /// the escape roll reads: `== 2` (pre-emptive strike) means escape is
    /// assured. Clearing `+0x290` without latching it here silently disables
    /// pre-emptive-strike escapes.
    ///
    /// REF: FUN_801E791C
    pub formation_latched: u8,
    /// Port-only: has the state-`0x00` formation arm
    /// ([`crate::battle_action::begin_formation_arm`]) already run for this
    /// battle?
    ///
    /// Retail needs no such flag because it enters state `0x00` **once per
    /// battle**: `ctx[7]` has exactly one zero-writer in the corpus, the
    /// battle flow SM's `0xFE` arm (`FUN_801D0748`, `0x801D3224`), and the
    /// end-of-action gate hands the next actor `0x0A`, never `0x00`. The port
    /// re-arms [`ActionState::Begin`] per action instead, and an unguarded
    /// re-entry would rewind the turn cursor mid-round and - worse - copy the
    /// already-cleared `+0x290` over `+0x291`, silently disabling
    /// pre-emptive-strike escapes (`+0x291` has one writer, `0x801E2B38`, and
    /// one reader, the escape roll at `0x801E7AD8`).
    ///
    /// Reset with the rest of the context at battle entry.
    pub formation_armed: bool,
    /// `[+0x269]` - multi-cast queue gate read at `DoneFadeDown`. Non-zero
    /// routes to `DoneMultiCast`; zero routes to `EndOfAction`.
    pub multi_cast_gate: u8,
    /// `[+0x243]` - the byte the gauge re-arm clears once it has run
    /// (`FUN_801E93C8`'s tail store at `0x801E94F8`, reached only on the arm
    /// whose gate passed). Cleared by [`crate::battle_action::done_cleanup`]
    /// via [`crate::battle_gauge_rearm::rearm_gauge`].
    pub gauge_rearm_latch: u8,
    /// `[+0x249]` - exit gate read at `MagicExit`.
    pub magic_exit_gate: u8,
    /// `[+0x24A]` - item-target byte A (read at `MagicCastBegin` for
    /// `ItemRetargetA`).
    pub item_target_a: u8,
    /// `[+0x24B]` - item-target byte B (read at `MagicCastBegin` for
    /// `ItemRetargetB`).
    pub item_target_b: u8,
    /// `[+0x24C]` - hit counter incremented by the spell hit-loop. The loop
    /// exits when `>= actor.hit_count_bound`.
    pub hit_counter: u8,
    /// `[+0x24D]` - recovery gate read at `MagicRecovery`.
    pub magic_recovery_gate: u8,
    /// `[+0x18]` - the battle **message id** the last printer call mirrored
    /// here. Retail's `FUN_801F3C34` writes `0x66` into it beside the
    /// `FUN_801D8DE8(0x66, 0)` call it makes
    /// ([`crate::move_no_effect_guard::queued_magic_message`]); the port
    /// writes it from the same place, through
    /// [`BattleActionHost::ui_element`].
    pub message_id: u8,
    /// The follow-up latch `0x801F6960` - non-zero while a queued-magic
    /// follow-up is already pending, which is what makes
    /// [`crate::move_no_effect_guard::queued_magic_message`] stay silent.
    ///
    /// It is a battle-overlay global rather than a `ctx` byte in retail; the
    /// port keeps it here because its only reader and its only writer are
    /// both inside this state machine's reach and it has the same lifetime as
    /// the rest of the action context.
    pub follow_up_pending: u8,
    /// `[+0x6D6]` - per-action ramp target (the state machine's "PC offset"
    /// cursor for the action body - separate from `action_state`).
    pub ramp_target: u16,
    /// `[+0x6D8]` - frame countdown timer (signed; decremented by frame dt
    /// every state that needs to wait).
    pub frame_timer: i16,
    /// `[+0x6DA]` - combo / sub-timer (separate from `frame_timer`).
    pub combo_timer: i16,
    /// `[+0x6DC]` - damage-target value used by spirit-arts ramps.
    pub damage_target: i16,
    /// `[+0x6DE]` - HP-bar target (paired with `damage_target`).
    pub hp_bar_target: i16,
    /// `[+0x6E6 + i*2]` - per-actor facing offsets (one per slot 0..7).
    pub per_actor_facing: [u16; ACTOR_SLOTS],
    /// Models the randomizer's enemy-ally ("charm") **victory widen**: the
    /// one-word overlay edit at `0x801E6638` that turns the monster-wipe
    /// scan's down-mask from `andi 0x4` into `andi 0x384`, so a living
    /// charmed monster (`+0x16E & 0x380`) counts as "down" and the player
    /// does not have to kill their own ally to win. `false` = retail mask
    /// `0x4`. See `docs/subsystems/battle.md` § enemy-ally charm at the
    /// end-of-action gate.
    pub charm_widen: bool,
}

impl BattleActionCtx {
    pub fn new() -> Self {
        Self::default()
    }

    /// Read the [`ActionState`] cursor; returns the underlying byte if it
    /// doesn't decode to a known state.
    pub fn current_state(&self) -> Result<ActionState, u8> {
        ActionState::from_byte(self.action_state).ok_or(self.action_state)
    }

    /// Set the [`ActionState`] cursor. Convenience wrapper.
    pub fn set_state(&mut self, state: ActionState) {
        self.action_state = state.as_byte();
    }
}

/// Outcome of a single battle action `step`.
///
/// Most states return [`StepOutcome::Stay`] (waiting on an animation match or
/// timer expiration); transitions are signalled by [`StepOutcome::Transition`].
/// Battle end surfaces via [`StepOutcome::BattleComplete`], raised only by
/// the paths that raise retail's battle-end signal `DAT_8007BD71 = 0xFE`
/// (the wipe arms of the `0x5A` gate and the successful-escape teardown
/// `0x66`) - never by the `0xFF` round boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepOutcome {
    /// Stayed in the current state - condition not yet met.
    Stay,
    /// Transitioned from `from` to `to`.
    Transition { from: u8, to: u8 },
    /// Battle complete. The mode-state machine should unload the battle
    /// overlay.
    BattleComplete,
    /// Unknown / unmapped state byte. The retail dispatcher's default arm is
    /// a no-op (function epilogue); we surface this so engines can log.
    UnknownState { state: u8 },
}

/// Per-strike values resolved from the active actor's chosen Tactical Art.
///
/// Built by [`ActionState::AttackChain`] when the actor has `chosen_art`
/// set and [`BattleActionHost::art_record`] returns a record. Surfaces the
/// power byte, dmg_timing, status effect, and hit cue for the current
/// strike (1-indexed via `actor.strike_index`).
///
/// `power` is `None` when the strike index runs past the recorded power
/// bytes (e.g. an extra anim frame at the end of the chain) - engines
/// should treat that as "this anim plays but does no damage."
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArtStrikeInfo {
    /// 0-indexed strike position within the art's power list.
    pub strike_index: u8,
    /// Animation byte read from the actor's strike-script (`params[strike_index]`).
    pub anim_byte: u8,
    /// Source / target party slots. `actor_slot` is the party / monster
    /// slot that owns the strike; `target_slot` is the resolved
    /// `actor.active_target` value.
    pub actor_slot: u8,
    pub target_slot: u8,
    /// The character whose art table we looked up.
    pub character: legaia_art::Character,
    /// Action constant identifying the active art.
    pub art: legaia_art::ActionConstant,
    /// Decoded power byte for this hit, if the art's power vec includes
    /// the current strike index.
    pub power: Option<legaia_art::PowerByte>,
    /// Animation-frame timing for this hit, if `dmg_timing` covers the
    /// current strike index. Engines use this to schedule the HP-deduction
    /// at the correct frame within the anim.
    pub dmg_timing: Option<u8>,
    /// Enemy status effect the art applies on hit (if any).
    pub enemy_effect: legaia_art::EnemyEffect,
    /// Hit cue (sound / visual) for this strike, if the art's hit-cue list
    /// covers the current strike index.
    pub hit_cue: Option<legaia_art::HitCue>,
}

/// Cause classification for `BattleEnd`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BattleEndCause {
    /// Party wipe (all party `liveness == 0`). `_DAT_8007BD2C = 5`.
    PartyWipe,
    /// Monster wipe (all monsters `liveness == 0`). `_DAT_8007BD2C = 0`.
    MonsterWipe,
    /// Party escaped (the `0x66` run teardown). Sets the same battle-end
    /// signal byte (`DAT_8007BD71 = 0xFE`) as the wipe gate but neither
    /// wipe cause - the battle ends with no loot and no defeat.
    Escaped,
}
