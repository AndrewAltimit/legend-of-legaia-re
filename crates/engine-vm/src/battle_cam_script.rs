//! Phase-scripted retail battle camera (game mode `0x15`) - the ONE model
//! both hosts run: the native play-window (`engine-shell`'s `window/battle_cam`
//! adapter) and the browser play page (`web-viewer::play_battle_render`) drive
//! this same state machine and project through [`battle_vp`], so the browser
//! battle frames exactly like the native window instead of approximating it.
//!
//! Retail's battle camera is NOT a fixed orbit: it glides between four
//! scripted framings keyed on the battle phase, holding static in the
//! close-ups and idling in a slow orbit only in the far "menu" framing.
//! Pinned per-frame from the PCSX-Redux camera trace on the
//! `s5_tetsu_battle` anchor (rotation trio `0x8007B790`, translation trio
//! `0x800840B8`, GTE `H = 256`), cross-checked against the four catalogued
//! mednafen Tetsu battle states:
//!
//! - **Dialogue** (tutorial / stage-overlay text up): held close-up,
//!   pitch `0`, yaw `0`, TR `(0, 1280, 1638)`, focus the speaking monster's
//!   seat (`(0, 800)` in both Tetsu captures) - static, no orbit.
//! - **Menu** (top Begin/Run framing, and any time no menu owns the pad):
//!   pitch `32`, TR `(0, 1280, z)` with `z` sized to the live formation (the
//!   traced solo fight lands on `7680`), idle orbit `-4` yaw units per
//!   camera step.
//! - **Submenu** (per-character command menu open): glide to the
//!   active-character close-up - yaw `2288`, TR `(-512, 1152, 2457)` -
//!   then held static while the submenu is open.
//! - **Action** (an action is executing): `FUN_801D5854` case `6`, the
//!   framing the action SM re-arms at nearly every action state. See
//!   [`action_framing`].
//!
//! One camera step spans **2 vsyncs** (every trace entry lands on an even
//! frame delta); the glide laws are the measured per-step increments:
//!
//! - Dialogue dismiss: pitch `+6`/step clamped at `32`, TR.z `+864`/step
//!   clamped at `7680`, while the idle orbit resumes immediately (yaw runs
//!   `-4`/step from `0` during the glide).
//! - Submenu open: all components arrive together over **6** steps
//!   (linear per-component increments, shortest-arc yaw).
//! - Submenu exit: a scripted swing back out - 6 steps up to the
//!   over-the-shoulder pose (pitch `256`, yaw eased to `0` mod 4096,
//!   TR `(0, 1536, 3276)`), then 7 steps back down to the menu framing
//!   with the idle orbit already running. (Retail holds the swing pose
//!   while the strike animation plays; the engine chains the two segments
//!   back-to-back.)
//!
//! ## Submenu framing is a formula, not a per-seat table
//!
//! The submenu close-up comes from `FUN_801D5854` case `0` (mode `0`,
//! called with the active battle-actor slot). Every component is either a
//! constant or a function of the acting actor - there is no seat table and
//! no `base + seat * delta` angle law:
//!
//! ```text
//! pitch = 0x20                                  // constant
//! yaw   = 0x8F0 - actor[+0x46]                  // facing-relative
//! TR    = (-0x200, HEIGHT[char_id], 0x600)      // x, z constant
//! focus = -actor[+0x34/+0x36/+0x38]             // negated world position
//! ```
//!
//! Two things follow. First, the measured `yaw 2288` is not a seat magic
//! number - it is `0x8F0` with Vahn's battle facing of `0` subtracted, so
//! the framing is a fixed over-the-shoulder offset that generalizes to any
//! seat once the actor's facing is tracked. Second, the per-seat variation
//! lives entirely in the **focus** trio (`0x80089118/1C/20`), which is the
//! negated position of whichever actor is acting: the camera orbits about
//! the active character. A solo-Vahn trace cannot distinguish that from a
//! constant, which is why the original measurement read as one fixed pose.
//!
//! `TR.z` is the one prescaled slot. `FUN_801D829C` rewrites its argument
//! as `(z << 8) / 0xA0` - a world distance into GTE projection units
//! (`0xA0` = 160 = screen half-width, `<< 8` = `H = 256`). The measured
//! `2457` is `floor(0x600 * 256 / 160)`; the truncation is why the traced
//! values are not exact divides.
//!
//! `TR.y` is the only genuine table: `0x801F4D2C + (char_id - 1) * 2`, keyed
//! on **character identity** (`DAT_8007BD10[slot]`, the 1-based party-record
//! selector), not on seat - a per-model height offset. It is disc data, read
//! off the battle-action overlay by `legaia_asset::battle_camera_table` and
//! handed to [`BattleCamActor::height`] by the host rather than transcribed
//! here; [`SUBMENU_HEIGHT_FALLBACK`] covers a disc-free host.
//!
//! ## The far "menu" framing is also computed, not a constant
//!
//! `FUN_801D5854` case `9` builds the Begin/Run framing from the **live
//! formation**, which is why its depth is not a magic number either:
//!
//! ```text
//! pitch = 0x20                       // constant
//! yaw   = _DAT_8007B792              // unchanged - the idle orbit owns it
//! TR    = (0, 0x500, span * 3)       // span clamped up to 0x800
//! focus = -(bbox centre of the framed actors)
//! ```
//!
//! The bbox spans the actor slots selected by the framing argument (whole
//! field / enemies only / party only), over actors whose `+0x14c` presence
//! halfword is non-zero, taking `min`/`max` of `actor[+0x34]` (X) and
//! `actor[+0x38]` (Z). `span = max(dx, dz)`, and `TR.z = max(span * 3,
//! 0x800)`. The traced `7680` is `prescale(0x12C0)`, i.e. a span of `1600` in
//! that particular fight - a measurement of one formation, not a constant.
//! See [`menu_framing`].
//!
//! ## The action framing is case `6`, and a live fight takes ONE arm
//!
//! `FUN_801D5854` is called with mode `6` from almost every arm of the
//! action state machine `FUN_801E295C` (`0x801E2D50`, `0x801E3060`,
//! `0x801E32E4`, `0x801E3364`, `0x801E34BC`, `0x801E3510`, `0x801E3560`,
//! `0x801E3B44`, `0x801E3DDC`, …), always as `FUN_801D5854(ctx[+0x13], 6)`.
//! It is therefore *the* per-action framing, and it forks immediately:
//!
//! ```text
//! 801d5ce8  lbu  v1,-0x428f(v0)     ; DAT_8007BD71, the battle-END signal
//! 801d5cf4  bne  v1,0xFE,0x801d64c4 ; fight still running -> in-fight arm
//! 801d5cfc  beq  (slot < 3)==0, 0x801d64c4  ; monster slot -> in-fight arm
//! ```
//!
//! `DAT_8007BD71` is **not** an "in-battle" state: `0xFE` is the battle-end
//! signal. Its writers are the action SM's successful-escape teardown
//! (`0x801E5A94`, right after `ctx[7] = 0x67`), its `0x5A` party-wipe and
//! monster-wipe scans (`0x801E65D8` / `0x801E6674`, beside the wipe cause in
//! `_DAT_8007BD2C`) and the capture-effect module (`0x801F7318`); SCUS
//! `0x80056014` zeroes it at battle init. Twelve battle save states - five
//! Begin/Run prompts, two mid-strike frames, three mid-approach parks, the
//! arts-input close-up and the tutorial open - all read `0xFF`. So while a
//! fight runs, **every** action, party or monster, frames through the
//! `0x801D64C4` arm; the `0x801D5CFC` arm is the end-of-battle framing (its
//! per-character script keys on the win-pose anim band `0x11..=0x18`).
//!
//! **In-fight arm** (`0x801D64C4`): pitch `0`, yaw `ctx[+0x6DA] - facing`,
//! TR `(0, 0x500, ctx[+0x6D0])`, focus the actor's live position
//! `actor[+0x34/+0x38]` with the height left at the stage floor (`sp+0x22`
//! is zeroed in the prologue and never written here), then a style byte
//! `ctx[+0xD]` selects one of three tweaks and a character id of `4`
//! overrides the whole translation. This is the arm that reads `ctx[+0x6D0]` -
//! the depth `FUN_801F0348` computes at action seed from the framed
//! monster's size class ([`crate::battle_formulas::camera_height_for_frame`],
//! mirrored on the engine side as `World::battle.camera_frame_height`).
//! Pinned byte-exact by three PCSX-Redux `ctx[7] == 0x19` captures (Gaza
//! acting): `TR (0, 0x500, prescale(ctx[+0x6D0]))` with `0x6D0 = 0xD00`
//! landing on `5324`, yaw `(ctx[+0x6DA] - actor[+0x46]) & 0xFFF` eight units
//! behind the live counter (the tween chases it), focus the negated
//! `+0x34/+0x38` pair, and the `ctx[+0xD] == 2` capture reading pitch `0x80`
//! over `TR.y = 0x400`. See [`ActionFraming`].
//!
//! **Battle-over arm** (`0x801D5CFC`): pitch `0`, yaw `0x800 - actor[+0x46]`
//! (over the actor's shoulder from behind), TR `(0, -5 * actor[+0x3E],
//! 0x500)`, focus the actor's display position `actor[+0x3C/+0x3E/+0x40]`,
//! then a per-character (`DAT_8007BD10[slot]`) / per-anim (`actor[+0x1DB]`)
//! script and a **height floor with a pitch compensation** (`0x801D6494`): a
//! TR.y below `0x280` is raised to `0x280` and a quarter of the shortfall is
//! added to the pitch. Character `2` in anim `0x16` skips the floor. The
//! port carries the arm behind [`ActionFraming::battle_over`], which no host
//! raises yet - the victory-pose sequence is not modelled.
//!
//! ## The yaw counter `ctx[+0x6DA]` is re-seeded per action
//!
//! The in-fight arm's yaw base is a counter the action SM both advances and
//! re-seeds, so the angle a strike is filmed from is a ladder of stores, not
//! a drift from battle entry:
//!
//! | Site | When | `ctx[+0x6DA]` |
//! |---|---|---|
//! | `0x801E2B40` | the `0x00` round-begin arm | `0` |
//! | `0x801E2CF8` | the `0x0C` seed arm, every category | `0x800` |
//! | `0x801E2F20` | the seed's Attack branch, as it stores `ctx[7] = 0x14` | `0x200` |
//! | `FUN_8004E13C` `0x8004E2B0` | a **party** attacker's first swing-clip commit | `(rand() % 2) * 0x800 + 0x280`, and `ctx[+0xD] = 0` |
//! | `0x801E2A24` | every SM pass | `+= max(1, 4 * frame_step / 3)` |
//!
//! `FUN_8004E13C` runs from the anim commit `FUN_8004AD80` (`0x8004BE28`)
//! with the committed clip's header byte `+0x87` as its argument, and seeds
//! only when that byte is `2`, the previous commit's was not
//! (`ctx[+0x243]`), and `ctx[+0x13] < 3`. A monster's attack therefore keeps
//! the `0x200` base; a party attack lands on `0x280` or `0xA80`. The
//! `battle_melee_hit_spark` capture reads `0x298` mid-art - `0x280` plus 24
//! frames of drift. The port keys the same ladder on the action-state edges
//! it observes ([`BattleCamera::observe_action_state`]); the swing-clip
//! commit is stood in for by the edge into the strike loop `0x1E`.
//!
//! ## Every glide duration is retail's own `a3`
//!
//! The framing cases pass `FUN_801D829C` a duration in **display frames**,
//! and a camera step is 2 frames, so the step counts here are `a3 / 2`:
//! cases `0`, `1`, `2`, `3` and `6` all pass `0xC` (6 steps) and case `9`
//! passes `0xE` (`0x801D712C`, 7 steps). That is an independent check on the
//! step counts the trace produced - [`SUBMENU_ENTER_STEPS`],
//! [`SUBMENU_SWING_STEPS`] and [`SWING_RETURN_STEPS`] were measured, and the
//! `a3` operands agree with all three.
//!
//! ## Screen shake
//!
//! `FUN_801D9D30` ([`crate::battle_camera::apply_shake`]) jitters the same
//! translation pair this pose carries (`0x800840B8/BC`) by an LCG offset
//! whose amplitude is the global `_DAT_8007B630`. The engine holds the
//! offset beside the pose rather than inside it - see
//! [`BattleCamera::set_shake_amplitude`] for why, and for where retail's own
//! callers live.
//!
//! ## Focus trio
//!
//! Every case passes a focus trio alongside the rotation and translation, and
//! `FUN_801D829C` tweens all nine components together over one duration. The
//! focus is the negated world point the camera orbits: the acting actor for
//! the close-ups, the formation centre for the menu framing. It is the only
//! place per-seat variation lives, so a host that drops it frames every seat
//! on the formation centre - see [`BattleCamPose::focus`].
//!
//! ## The glides step on retail's own increments
//!
//! `FUN_801D829C` is the tween builder retail's framing cases arm, and its
//! port is [`crate::battle_camera::build_camera_angle_tween`]. `Glide::linear`
//! calls it: the arrive-together glides take their per-component rates, their
//! 12-bit shortest-arc yaw and their TR.z projection prescale straight out of
//! it, so there is one implementation of the stepping arithmetic rather than
//! two, and the increments are retail's `ceil(|delta| / duration)` integers
//! rather than an exact float divide.
//!
//! The dialogue-dismiss glide keeps its own per-component rates: the trace has
//! pitch settling on step 6 and TR.z on step 7, so that transition is not one
//! arrive-together tween and no single duration reproduces it.
//!
//! REF: FUN_801D5854 (the framing cases), FUN_801D829C (the angle-tween
//! builder).

mod action;
mod camera;
mod camera_step;
mod module_shot;
mod phase;
mod pose;
mod post_action;
mod projection;

pub use action::*;
pub use camera::*;
pub use module_shot::*;
pub use phase::*;
pub use pose::*;
pub use post_action::*;
pub use projection::*;

#[cfg(test)]
mod tests;
