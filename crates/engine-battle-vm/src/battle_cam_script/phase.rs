//! Framing-phase classification off the live battle state.

/// Battle-camera framing phase, derived from the live battle state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BattleCamPhase {
    /// An in-battle dialogue box is up (the tutorial text).
    Dialogue,
    /// No menu owns the pad: the far framing with the idle orbit. Also the
    /// **top-level command chooser** - see [`phase_for_state`].
    #[default]
    Menu,
    /// An arts / spell / item **input** picker owns the pad
    /// (`FUN_801D5854` case `0`).
    Submenu,
    /// An action is executing (`FUN_801D5854` case `6`).
    Action,
    /// The post-strike recovery / return / done band: `FUN_801D5854` case
    /// `7`, the **two-shot** on the attacker-target midpoint. See
    /// [`recover_framing`](super::recover_framing).
    Recover,
    /// The end-of-action band: `FUN_801D5854` case `8`, framed on the
    /// **target**. See [`action_end_framing`](super::action_end_framing).
    ActionEnd,
    /// The target cursor on one enemy: `FUN_801D5854` case `1`, the shot
    /// that turns the member toward the cursor's target. See
    /// [`target_enemy_pose`](super::target_enemy_pose).
    TargetEnemy,
    /// The target cursor on one party member: `FUN_801D5854` case `3`. See
    /// [`target_ally_pose`](super::target_ally_pose).
    TargetAlly,
}

/// Retail's own test for "an action owns the framing", from the action state
/// machine's prologue rather than from the phase script.
///
/// `FUN_801E295C` runs the idle yaw orbit itself, before its state switch,
/// and gates it on exactly two states (`0x801E2A3C..0x801E2A6C`):
///
/// ```text
/// 801e2a3c  beq  v1,zero,0x801e2a4c   ; ctx[7] == 0x00 (Begin)
/// 801e2a44  bne  v1,0xb,0x801e2a70    ; ctx[7] == 0x0B (QueuedFromMenu)
/// 801e2a58  lbu  v1,0x393(v1)         ; DAT_1F800393, the frame step
/// 801e2a5c  lhu  v0,0x2(a0)           ; _DAT_8007B792, the camera yaw
/// 801e2a60  sll  v1,v1,0x1            ; step * 2
/// 801e2a64  subu v0,v0,v1             ; yaw -= step * 2
/// 801e2a68  andi v0,v0,0xfff
/// ```
///
/// That is [`ORBIT_STEP`](super::ORBIT_STEP) falling out of the disassembly: two display frames
/// per camera step at `2` units per frame is the `-4` the trace measured, and
/// it pins *when* the orbit runs - only while the SM is idling between
/// actions.
///
/// The battle tick `FUN_801D0748` carries the same store in its own
/// prologue (`0x801D07AC..0x801D07CC`), gated on the command-flow byte
/// `ctx[+6]` being `0x1E` / `0x32` / `0x6E` / `0xFE` - the Begin/Run prompt
/// among them. The two never add up: while the flow byte owns the frame the
/// action SM is not run at all. A 240-vsync PCSX-Redux trace parked at the
/// `battle_gaza2_prompt` state (`scripts/pcsx-redux/autorun_battle_cam_orbit.lua`)
/// counts the dispatcher's store once per battle tick and the SM's store
/// never, the yaw stepping `-2 * DAT_1F800393` each time - `-2` per display
/// frame, i.e. [`ORBIT_STEP`](super::ORBIT_STEP) per camera step, from either writer alone.
///
/// ## The test is a **band**
///
/// The `ctx[7]` space is banded, and `FUN_801E295C`'s own arms arm the camera
/// per band rather than per byte:
///
/// | Band | `ctx[7]` | What `FUN_801E295C` arms |
/// |---|---|---|
/// | Setup | `0x00`, `0x0A`, `0x0B` | nothing; the last framing stands and the yaw orbits ([`FRAMING_HOLD_STATES`]) |
/// | Seed | `0x0C` | `FUN_801D5854(ctx[0x13], 6)` (`0x801E2D50`) |
/// | Action | `0x14..=0x48` | case `6` per state (`0x14`: `jal 0x801d5854` at `0x801E32E4`) |
/// | Done | `0x50..=0x52` | case `6` / `8` **per category** ([`done_band_phase`]), under the `ctx[+0x6D8]` tail timer |
/// | End of action | `0x5A` | the category fork once more ([`end_of_action_phase`], `0x801E6428..0x801E6470`) |
/// | Run | `0x64..=0x67` | case `9` + the orbit (`jal 0x801d5854`, `li a1,0x9` at `0x801E5BDC`) |
///
/// **The Done band is a per-action framing, not an idle.** The `0x50` arm
/// (`0x801E5E90..0x801E5EF4`) and the `0x51` arm (`0x801E5FC0..0x801E6018`)
/// both fork on `actor[+0x1DE]`: category `5` (Run) skips the framing call
/// and runs the yaw orbit instead, category `3` (Attack) takes `li a1,0x8`, a
/// target that is itself a party seat (`actor[+0x1DD] < 3`) and whose live HP
/// `+0x14C` reads zero takes `0x8` too, and everything else `li a1,0x6` - re-armed every pass for the
/// `ctx[+0x6D8] = 0x3C` display frames the tail lasts. A retail save parked
/// in `0x51` after a monster's spell (`zora_glare_petrify_post`) reads case
/// 6's in-fight pose - pitch `0`, `TR (0, 0x500, prescale(ctx[+0x6D0]))`
/// with the tween one step short, focus the caster's own seat, yaw
/// `ctx[+0x6DA] - actor[+0x46]` - not the far framing.
///
/// An earlier port reading kept the whole Done band on the far framing
/// because "the per-action close-up" would otherwise own about half of a
/// fight's frames. That close-up was the battle-over arm applied to a
/// running fight (see the module doc); the in-fight arm frames both
/// combatants, so holding it through the tail is the retail look, and the
/// tail is bounded by the same `0x3C`-frame timer the port ticks.
///
/// **Between two actors of a round retail stays on the last framing.** No
/// state from `0x5A` to the next seed `0x0C` arms case `9`: `0x5A` arms case
/// `8` or `6` on the actor that just finished, and the setup states arm
/// nothing. Every pre-cast capture parked in `0x0A` behind an earlier actor
/// reads that actor's shot with its tween landed
/// (`kor_ghost_knight_pre_v_windhash`: pitch `128`, `TR (0, 1024, 3276)`, focus
/// the Spirit member's seat). Only a round's first actor stands on the far
/// framing there, the one the prompt's case `9` left
/// (`evil_medallion_rage_battle`, `taiku_topaz_pre_hyper_heal`).
/// [`action_state_frames_the_action`] stays the band test for the
/// three-boolean [`phase_for`]; [`phase_for_state`] and the camera's
/// [`BattleCamera::holds_framing`](super::BattleCamera) carry the rest.
///
/// The Run band is idle on retail's own authority, not as a deviation: both
/// the category-`5` seed arm and the `0x50`/`0x51` arms skip the framing call
/// for `actor[+0x1DE] == 5` and run the yaw orbit instead.
///
/// The retail orbit pair stays as [`RETAIL_ORBIT_STATES`] so the difference
/// between retail's gate and this band model is visible rather than folded
/// away.
pub const fn action_state_frames_the_action(action_state: u8) -> bool {
    !matches!(
        action_state,
        // Setup band: nothing committed yet.
        0x00 | 0x0A | 0x0B
        // End of action: its framing is [`end_of_action_phase`]'s.
        | 0x5A
        // Run band: retail arms case 9 + the orbit here itself.
        | 0x64
            ..=0x67
        // Terminal / between-round holds.
        | 0xFD | 0xFE | 0xFF
    )
}

/// The two `ctx[7]` values retail's own orbit gate accepts
/// (`0x801E2A3C..0x801E2A6C`). [`action_state_frames_the_action`] is the
/// band model built on it - see its note.
pub const RETAIL_ORBIT_STATES: [u8; 2] = [0x00, 0x0B];

/// The retail phase for one frame of battle state. Both hosts feed the same
/// three booleans: `dialogue_up` = an in-battle dialogue / inline-dialogue box
/// owns the screen, `submenu_open` = a per-character command / arts / spell /
/// item session owns the pad, `action_executing` =
/// [`action_state_frames_the_action`] over the live `ctx[7]`.
///
/// The precedence is retail's own: the tutorial dialogue draws over an open
/// menu, and a menu can only be open while no action runs, so `Action` sits
/// below both. An action that is *not* running leaves the far menu framing,
/// which is what retail's case `9` re-arms at end of action.
pub fn phase_for(dialogue_up: bool, submenu_open: bool, action_executing: bool) -> BattleCamPhase {
    if dialogue_up {
        BattleCamPhase::Dialogue
    } else if submenu_open {
        BattleCamPhase::Submenu
    } else if action_executing {
        BattleCamPhase::Action
    } else {
        BattleCamPhase::Menu
    }
}

/// `ctx[7]` values whose arm hands `FUN_801D5854` mode **`7`** - the
/// attacker-target two-shot ([`recover_framing`](super::recover_framing)) -
/// or, on a fork, mode `8` ([`post_strike_phase`]).
///
/// The strike loop `0x1E` (jump-table entry `0x801E35F0`) arms mode `7` on
/// every pass with no fork: the block that commits the next strike falls
/// through to `li a1,0x7` / `jal 0x801d5854` at `0x801E36E4..0x801E36EC`.
/// The `player_steal_skeleton_pre` capture (`ctx[7] == 0x1E`, style `2`)
/// reads that pose's tween targets - pitch `0x80`, `TR (0, 0x400,
/// prescale(0xC00))`, yaw `(ctx[+0x6DA] - actor[+0x46] - 0x700) & 0xFFF` -
/// not case 6's.
///
/// `0x1F` (recovery wait, `0x801E3A88`) and `0x20` (return, `0x801E54EC`)
/// take mode `7` by **default** and mode `8` when the target's current anim
/// `+0x1D9` is its knockdown `+0x1F1` or its non-zero get-up `+0x1F2`
/// (`0x801E3A88..0x801E3AC4` / `0x801E5660..0x801E5684`); `0x20` alone also
/// takes `8` when the target is a party seat in a death clip (anim `7` /
/// `8`, `0x801E568C..0x801E56A4`). A retail `ctx[7] == 0x1F` save state
/// corroborates the default pose: with `ctx[+0xD] == 2` it reads pitch `0x80`
/// and `TR.y = 0x400` - case 7's style-2 tweak - over `TR.z =
/// prescale(ctx[+0x6D0])`; `battle_melee_hit_spark` and
/// `player_steal_skeleton_banner` (`0x20`, target on its knockdown) read
/// case 8.
///
/// The Done band's `0x50` / `0x51` are not here: their arm forks per
/// category ([`done_band_phase`]) and never reaches case `7`.
pub const RECOVER_STATES: [u8; 3] = [0x1E, 0x1F, 0x20];

/// The phase the post-strike band arms ([`RECOVER_STATES`]) for one frame:
/// case `7`, or case `8` on the target-reaction fork the `0x1F` / `0x20`
/// arms run. `None` outside the band.
pub const fn post_strike_phase(action_state: u8, done: DoneBandInputs) -> Option<BattleCamPhase> {
    let case8 = match action_state {
        0x1E => false,
        0x1F => done.target_knocked,
        0x20 => done.target_knocked || (done.target_party_seat && done.target_death_clip),
        _ => return None,
    };
    Some(if case8 {
        BattleCamPhase::ActionEnd
    } else {
        BattleCamPhase::Recover
    })
}

/// `ctx[7]` values whose arm hands `FUN_801D5854` mode **`8`**
/// unconditionally ([`action_end_framing`](super::action_end_framing)): `0x52`
/// (Seru-absorb banner hold) and `0xFD` (idle hold), both `li a1,0x8` at `0x801E5F74`.
/// The Done-cleanup pair forks per category instead - [`DONE_STATES`] and
/// [`done_band_phase`].
pub const ACTION_END_STATES: [u8; 2] = [0x52, 0xFD];

/// The Done-cleanup pair: `0x50` (cleanup; seeds the `ctx[+0x6D8] = 0x3C`
/// tail timer) and `0x51` (fade-down; ticks it). Their framing call forks on
/// the action category - [`done_band_phase`].
pub const DONE_STATES: [u8; 2] = [0x50, 0x51];

/// The end-of-action gate, which arms one more framing before the next
/// actor's setup ([`end_of_action_phase`]).
pub const END_OF_ACTION_STATE: u8 = 0x5A;

/// The states ahead of the action seed `0x0C`. `FUN_801E295C` has no framing
/// call in any of them: the tween the last pass armed walks on and the yaw
/// orbits, by the prologue's store in `0x00` / `0x0B`
/// (`0x801E2A3C..0x801E2A6C`) and by `0x0A`'s own (`0x801E5BE4..0x801E5C04`).
pub const FRAMING_HOLD_STATES: [u8; 3] = [0x00, 0x0A, 0x0B];

/// What the Done-band arms read before choosing a framing case
/// (`0x801E5E90..0x801E5EF4` in `0x50`, `0x801E5FC0..0x801E6018` in `0x51`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DoneBandInputs {
    /// `actor[+0x1DE]` - the committed action category (`1` item, `3`
    /// attack, `4` spirit, `5` run).
    pub category: u8,
    /// `actor[+0x1DD] < 3` - the acting actor's **target** is a party seat.
    ///
    /// The arms read the spill `sp+0x20`, which the prologue fills from
    /// `lbu t2,0x1dd(s3)` (`0x801E29B0..0x801E29C0`, `s3` the acting actor)
    /// beside `s8`, the target actor itself - the target's index, not the
    /// acting seat `ctx[+0x13]`. A party caster whose spell killed a monster
    /// therefore keeps case 6 on the caster through the Done band
    /// (`shiny_refactor_gimard_levelup`: target slot 3 at `0` HP, retail's
    /// tween endpoints case 6's), and case 8 over a corpse is a monster's
    /// blow that felled a party member.
    pub target_party_seat: bool,
    /// The acting actor's target reads zero live HP (`s8[+0x14C] == 0`).
    pub target_dead: bool,
    /// The target's current anim `+0x1D9` is its knockdown `+0x1F1`, or its
    /// get-up `+0x1F2` when that is non-zero - the post-strike arms' case-`8`
    /// test ([`post_strike_phase`]).
    pub target_knocked: bool,
    /// The target's current anim `+0x1D9` is `7` or `8`, a downed party
    /// member's death chain ([`post_strike_phase`], `0x20` only).
    pub target_death_clip: bool,
    /// The results sequencer has begun (`ctx[+0x6CE] != 0`), which takes an
    /// Attack off case `8` in [`end_of_action_phase`].
    pub results_begun: bool,
}

/// `actor[+0x1DE]` value the Done band runs the orbit for instead of a framing.
pub const DONE_CATEGORY_RUN: u8 = 5;
/// `actor[+0x1DE]` value the Done band hands case `8` for.
pub const DONE_CATEGORY_ATTACK: u8 = 3;

/// The framing case the Done band's per-category fork arms, as a phase.
/// Read off the `0x50` arm (the `0x51` arm at `0x801E5FC0` is the same
/// ladder):
///
/// ```text
/// 801e5ea0  li   v0,0x5
/// 801e5ea4  beq  v1,v0,0x801e5f00     ; category 5 (Run): no framing, orbit
/// 801e5ea8  _li  v0,0x3
/// 801e5eac  beq  v1,v0,0x801e5ed8     ; category 3 (Attack): case 8
/// 801e5eb4  lw   t2,0x20(sp)          ; actor[+0x1DD], the target index
/// 801e5ebc  sltu v0,t2,v0             ; < 3 ?
/// 801e5ec0  beq  v0,zero,0x801e5eec   ; monster target: case 6
/// 801e5ec8  lhu  v0,0x14c(s8)         ; the target's live HP
/// 801e5ed0  bne  v0,zero,0x801e5eec   ; alive: case 6
/// 801e5edc  jal  0x801d5854           ; a1 = 8: dead party target
/// 801e5ef0  jal  0x801d5854           ; a1 = 6
/// ```
///
/// The Run category's `Menu` is the far framing with the idle orbit: the
/// `0x801E5F00` arm runs the same `yaw -= step * 2` store the prologue does.
pub const fn done_band_phase(done: DoneBandInputs) -> BattleCamPhase {
    if done.category == DONE_CATEGORY_RUN {
        BattleCamPhase::Menu
    } else if done.category == DONE_CATEGORY_ATTACK || (done.target_party_seat && done.target_dead)
    {
        BattleCamPhase::ActionEnd
    } else {
        BattleCamPhase::Action
    }
}

/// The framing the end-of-action gate `0x5A` arms on the actor that just
/// finished (`0x801E6428..0x801E6470`):
///
/// ```text
/// 801e6428  lbu  v1,0x1de(s3)         ; the category
/// 801e6430  bne  v1,3,0x801e6460      ; not an Attack: the fork below
/// 801e6444  lh   v0,0x6ce(v0)         ; results half
/// 801e644c  bne  v0,zero,0x801e6460
/// 801e6458  j    0x801e646c           ; a1 = 8: an Attack's case 8
/// 801e6460  beq  v1,5,0x801e6474      ; Run: no framing call
/// 801e6464  _li  a1,0x6               ; everything else: case 6
/// ```
pub const fn end_of_action_phase(done: DoneBandInputs) -> BattleCamPhase {
    if done.category == DONE_CATEGORY_RUN {
        BattleCamPhase::Menu
    } else if done.category == DONE_CATEGORY_ATTACK && !done.results_begun {
        BattleCamPhase::ActionEnd
    } else {
        BattleCamPhase::Action
    }
}

/// The retail phase for one frame of battle state, over the live `ctx[7]`.
///
/// Two things this resolves that the three-boolean [`phase_for`] cannot.
///
/// **The round prompt is the FAR framing; a member's surfaces are the
/// close-up.** Retail's battle menu driver `FUN_801D388C` arms *both* case
/// `0` and case `9` (`0x801D475C` / `0x801D53B8` pass `a1 = 0`;
/// `0x801D4908` / `0x801D5688` pass `a1 = 9`), so "a menu is open" does not
/// by itself pick the close-up. The library's battle captures separate them
/// on the command-flow byte: every save on the **Begin / Run** prompt
/// (`0x1E`) reads `pitch 32, TR (0, 1280, 7680), focus origin` - case 9's
/// `max(span*3, 0x800)` over `+-800` seats, exactly - and frames both
/// fighters; every save on a member's command ring (`0x28`) or arts input
/// (`0x50`) reads `TR (-512, height[char], 2457)` and `yaw = 0x8F0 -
/// actor[+0x46]` - case 0 - framed on that member. `input_menu_open` is the
/// caller's "a member's surface is up" (the engine's
/// `battle_cam_inputs::member_surface_open`).
///
/// **The post-strike band is case 7 / case 8, not case 6.** See
/// [`RECOVER_STATES`] and [`ACTION_END_STATES`].
///
/// **The Done band frames per category.** `0x50` / `0x51` arm case `8` for
/// an Attack (or a dead party target), the orbit for a Run, and
/// case `6` for everything else - [`done_band_phase`] over `done`, which
/// both hosts fill from the acting actor. So does the gate after it
/// ([`end_of_action_phase`]); the caller hands `0x5A` here only while the
/// round is executing, since the engine's SM rests on that byte through the
/// next round's prompt.
pub fn phase_for_state(
    dialogue_up: bool,
    input_menu_open: bool,
    action_state: u8,
    done: DoneBandInputs,
) -> BattleCamPhase {
    if dialogue_up {
        BattleCamPhase::Dialogue
    } else if input_menu_open {
        BattleCamPhase::Submenu
    } else if let Some(phase) = post_strike_phase(action_state, done) {
        phase
    } else if DONE_STATES.contains(&action_state) {
        done_band_phase(done)
    } else if action_state == END_OF_ACTION_STATE {
        end_of_action_phase(done)
    } else if ACTION_END_STATES.contains(&action_state) {
        BattleCamPhase::ActionEnd
    } else if action_state_frames_the_action(action_state) {
        BattleCamPhase::Action
    } else {
        BattleCamPhase::Menu
    }
}
