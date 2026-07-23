//! Core battle-action types: actor slots, action categories/states, poses, and the per-actor `BattleActor` / `BattleActionCtx` state structs.

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
    /// Item-target re-route (state `0x28` reseats `actor.active_target` to
    /// `ctx.item_target_b`). Not a true category - it's an intermediate
    /// signal that the item-arm of the magic flow uses.
    ItemRetargetB = 8,
    /// Item-target re-route (state `0x28` reseats `actor.active_target` to
    /// `ctx.item_target_a - 1`). Same caveat as `ItemRetargetB`.
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
    /// Battle complete - terminal.
    BattleComplete = 0xFF,
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
            0xFF => Self::BattleComplete,

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
    /// `+0x172` / `+0x174` - HP / max-HP (or current / max).
    pub hp: u16,
    pub max_hp: u16,
    /// HP-bar **display** cursor. Retail keeps the authoritative current
    /// HP at `+0x14C` and drains the on-screen bar value at `+0x172`
    /// toward it over several frames; the fade-down settle check
    /// (`FUN_801E7250`) compares the pair. The engine models the pair as
    /// `hp` (authoritative) vs this display cursor: `Some(shown)` while a
    /// drain animation runs, `None` when the host doesn't animate HP bars
    /// (always settled).
    pub hp_display: Option<u16>,
    /// `+0x178` - last-action MP cost (used to display `-N MP` on screen).
    pub last_mp_cost: u16,
    /// `+0x1A` - party-action queue counter. Incremented by `Begin`,
    /// counter-attack swap, run advance, end-of-action.
    pub action_queue_counter: u8,
    /// `+0x21D` - impact-step magnitude - multiplied into the per-frame X/Z
    /// drift during attacks.
    pub impact_step: u8,
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
    /// `+0x1D9` - current anim ID (read-only here; written by the animation
    /// system).
    pub current_anim: u8,
    /// `+0x1DA` - queued next anim ID. The state machine writes this; the
    /// animation system reads `current_anim` toward `queued_anim`.
    pub queued_anim: u8,
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
    /// Chosen Tactical Art for this turn. When `Some`, the strike-band
    /// states call `BattleActionHost::art_record(character, action)` to
    /// fetch power bytes / hit timings / status effect. `None` falls
    /// back to generic-attack defaults. Set by the engine when the
    /// command queue resolves to an art (via `resolve_action_queue`).
    pub chosen_art: Option<legaia_art::ActionConstant>,
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
    /// `[+0x274]` - queued action (copied to `actor.field_1A` at `Begin`).
    pub queued_action: u8,
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
    /// `[+0x269]` - multi-cast queue gate read at `DoneFadeDown`. Non-zero
    /// routes to `DoneMultiCast`; zero routes to `EndOfAction`.
    pub multi_cast_gate: u8,
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
/// Battle-end and terminal states surface via [`StepOutcome::BattleComplete`].
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
