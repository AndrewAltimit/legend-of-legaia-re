//! The field overlay's ledge-hop arc **setup** - the routine that turns the
//! landing-point triple the locomotion controller builds on its stack into a
//! quadratic-Bezier hop clip on a freshly spawned helper actor.
//!
//! REF: FUN_801d1878, FUN_80020de0, FUN_801db510, FUN_801daa50
//!
//! Each ported entry carries its `PORT` tag on the Rust item that implements
//! it - [`build_hop_arc`] (`FUN_801d2404`), [`spawn_arc_helper`]
//! (`FUN_801d5780`), [`spawn_arc_with_emitter`] (`FUN_801d25ec`) and
//! [`advance_hop_session`] (`FUN_801d2298`) - never at module level, so the
//! liveness audit sees one anchor per port site rather than a coarse
//! file-wide one.
//!
//! # Three spawners, one arithmetic
//!
//! `FUN_801d2404` is not alone. The field overlay carries **three** entries
//! that build the same clip out of the same `0x801F227C` template, differing
//! only in where the start point comes from and what else they attach:
//!
//! | Entry | Start point | Second record |
//! |---|---|---|
//! | `FUN_801d5780` | the `a0` actor's `+0x14..+0x1B` | none |
//! | `FUN_801d2404` | the player context `_DAT_8007C364` | template `0x801F2294`, `+0x9E = frames` |
//! | `FUN_801d25ec` | the `a0` actor's `+0x14..+0x1B` | template `0x801F22AC`, an emitter |
//!
//! The Bezier arithmetic - midpoints, the `min(P0y, P2y) - apex` control
//! point, the `0x1000 / frames` cursor step and its `blez` guard - is
//! instruction-for-instruction identical in all three, which is why
//! [`build_hop_arc`] is the single implementation and the two siblings are
//! spawn wrappers around it.
//!
//! # Provenance
//!
//! `FUN_801d2404` lives in the field overlay (PROT entry `0897_xxx_dat`, file
//! offset `0x3BEC`, slot-A base `0x801CE818`) and is 122 instructions long.
//! The short standalone dump `ghidra/scripts/funcs/801d2404.txt` is an
//! 8-instruction wrong-base fragment; the base-correct body is
//! `ghidra/scripts/funcs/overlay_0897_door_801d2404.txt`, which agrees
//! instruction-for-instruction with the extracted `0897_xxx_dat.BIN` bytes at
//! that offset.
//!
//! The image holds exactly **one** `jal 0x801D2404` site, at `0x801D1B70`
//! inside the ledge-hop trigger `FUN_801d1878`. Reading the argument set-up in
//! its delay-slot window pins the signature:
//!
//! ```text
//! 801d1b20  addiu a0, sp, 0x18      ; a0 = &stack triple {x, y, z}
//! 801d1b2c  addiu a1, zero, 0x10    ; a1 = apex height (hop up)
//! 801d1b44  addiu a1, zero, 0x18    ;      or 0x18 (hop down)
//! 801d1b60  addiu a2, zero, 0x10    ; a2 = clip length in frames (always 16)
//! 801d1b70  jal   0x801d2404
//! ```
//!
//! so the call is `FUN_801d2404(&landing_xyz, apex, frames)`. The engine side
//! already builds that triple: `engine_core::world::FieldLedgeHop` is posted by
//! `World::try_field_ledge_hop` with exactly `target_x / target_y / target_z`
//! and `kind` = the `0x10` / `0x18` value that lands in `a1`.
//!
//! # What the routine does
//!
//! Reading the disassembly (not the decompiled C - the C renders the apex
//! store as `(min - apex) * 2 - mid`, which is the same value but hides that
//! retail computes it as `mid + 2 * (min - apex - mid)`):
//!
//! 1. Bail out when the scene control block's player slot
//!    (`0x8007C348 + 0x1C` = `_DAT_8007C364`, the player context pointer) is
//!    null.
//! 2. Allocate a helper actor from the pool (`func_0x80020DE0` with the
//!    template pointer `0x801F227C` and the pool handle at `0x8007C34C`);
//!    on failure nothing else happens.
//! 3. Back-link the player into the helper's `+0x90`, and copy the player's
//!    8-byte transform block `+0x14..+0x1B` into the helper - this is the
//!    Bezier start point `P0` (`+0x14` = X, `+0x16` = Y, `+0x18` = Z).
//! 4. Store the landing triple into `+0x24 / +0x26 / +0x28` - the end point
//!    `P2`.
//! 5. Store the three midpoints into `+0x3C / +0x3E / +0x40`, then **replace**
//!    the Y one with the arc's control point `C`.
//! 6. Seed the clip cursor `+0x9C = 0` and the per-frame step
//!    `+0x9E = 0x1000 / frames` (`0x1000` for `frames <= 0`, the retail
//!    divide-by-zero guard).
//! 7. Allocate a second helper from template `0x801F2294` with
//!    `+0x9E = frames`, `+0x9C = 0`, and set the player's movement-lock bit
//!    `0x80000` in `+0x10`. If that second allocation fails, the first helper
//!    gets the tear-down bit `8` in its `+0x10` instead and the hop is
//!    abandoned.
//!
//! # The arc
//!
//! The control point is the only interesting arithmetic, and it has a clean
//! closed form. With `mid = (p0 + p2) / 2` (arithmetic shift, so it rounds
//! toward negative infinity) and `hi = min(p0, p2)`:
//!
//! ```text
//! c = mid + 2 * (hi - apex - mid)
//! ```
//!
//! Evaluating the quadratic Bezier at `t = 0.5` gives `(p0 + 2c + p2) / 4`,
//! which collapses to exactly `hi - apex`. PSX world Y grows downward, so
//! subtracting `apex` raises the point: **the hop peaks `apex` units above
//! whichever endpoint is higher**, independent of how far apart they are.
//! That invariant is what [`hop_apex_height`] and the unit tests below pin.
//!
//! # NOT WIRED
//!
//! Every item in this module is inert, for one shared reason: retail's clip
//! lives on a **spawned helper actor** drawn from the `FUN_80020de0` pool, and
//! `engine-core`'s world model has neither that pool nor a per-actor clip
//! cursor. `World::try_field_ledge_hop` posts the `FieldLedgeHop` and stops.
//!
//! The concrete blocker is a storage one and it is named precisely so the next
//! pass does not re-derive it: driving [`advance_hop_session`] needs a
//! `cursor`/`extent` pair that survives between frames, and the only places it
//! can live are a new `World` field (`crates/engine-core/src/world/state.rs`)
//! or a new field on `FieldLedgeHop` / the actor's `ActorState`
//! (`crates/engine-core/src/world/types.rs`, `engine-vm`'s `move_vm`). All
//! three are outside this module's path set. Computing the arc without
//! somewhere to keep the cursor would be inventing state nothing reads, so it
//! is deliberately not done.

/// Fixed-point full-clip extent: retail's cursor runs `0 ..= 0x1000`.
pub const CLIP_FULL: i32 = 0x1000;

/// The landing point retail writes into the helper actor's `+0x24 / +0x26 /
/// +0x28` - the three stack half-words the caller builds at `sp+0x18`.
///
/// Mirrors `engine_core::world::FieldLedgeHop`'s `target_*` fields; kept
/// local so this module stays free of a dependency on the world model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HopTarget {
    /// Landing X (`+0x24`).
    pub x: i16,
    /// Landing Y (`+0x26`) - the floor height sampled one step ahead.
    pub y: i16,
    /// Landing Z (`+0x28`).
    pub z: i16,
}

/// The clip `FUN_801d2404` seeds on the helper actor it spawns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HopArc {
    /// Bezier start point - the player's `+0x14 / +0x16 / +0x18` at the
    /// instant the hop starts, copied verbatim into the helper.
    pub start: (i16, i16, i16),
    /// Bezier end point - the caller's landing triple.
    pub end: (i16, i16, i16),
    /// Bezier control point, stored at `+0x3C / +0x3E / +0x40`. X and Z are
    /// plain midpoints; Y carries the apex correction.
    pub control: (i16, i16, i16),
    /// Clip cursor `+0x9C`, always seeded to `0`.
    pub cursor: i16,
    /// Per-frame cursor step `+0x9E` = `0x1000 / frames`, or `0x1000` when
    /// `frames <= 0`.
    pub step: i16,
    /// The second helper's `+0x9E`: the raw frame count, stored unscaled.
    pub paired_frames: i16,
}

/// Signed midpoint exactly as retail computes it: `addu` then `sra 1`, i.e. a
/// floor-division by two rather than the truncating division Rust's `/` would
/// give for negative sums.
fn mid(a: i16, b: i16) -> i16 {
    ((a as i32 + b as i32) >> 1) as i16
}

/// Build the hop clip.
///
/// `start` is the player's live transform (`+0x14 / +0x16 / +0x18`), `target`
/// the landing triple, `apex` the `a1` height (`0x10` up / `0x18` down) and
/// `frames` the `a2` clip length (retail always passes `0x10`).
///
/// PORT: FUN_801d2404
// NOT WIRED: the helper-actor pool `FUN_801d2404` allocates from, and the
// per-frame advance `FUN_801d2298` that consumes the clip, have no
// counterpart in `engine-core`'s world model; wiring would edit
// `engine-core/src/world/**`, owned elsewhere.
pub fn build_hop_arc(start: (i16, i16, i16), target: HopTarget, apex: i16, frames: i16) -> HopArc {
    let end = (target.x, target.y, target.z);

    // `+0x3C / +0x3E / +0x40` - the plain midpoints, written first.
    let mid_x = mid(start.0, end.0);
    let mid_y = mid(start.1, end.1);
    let mid_z = mid(start.2, end.2);

    // `+0x3E` is then overwritten with the arc control point. Retail selects
    // `min(start.y, end.y)` with an `slt`/`move` pair, then evaluates
    // `mid + 2 * (hi - apex - mid)` with a `subu`/`subu`/`sll 1`/`addu` chain.
    let hi = start.1.min(end.1) as i32;
    let ctrl_y = (mid_y as i32 + (((hi - apex as i32) - mid_y as i32) << 1)) as i16;

    // `+0x9E`: `blez` guard first, so a non-positive frame count skips the
    // divide entirely and stores the full extent.
    let step = if frames > 0 {
        (CLIP_FULL / frames as i32) as i16
    } else {
        CLIP_FULL as i16
    };

    HopArc {
        start,
        end,
        control: (mid_x, ctrl_y, mid_z),
        cursor: 0,
        step,
        paired_frames: frames,
    }
}

/// The second record `FUN_801d25ec` chains behind the arc helper: an emitter
/// allocated from template `0x801F22AC`, back-linked to the arc helper and
/// carrying the two pointers and the class byte the caller supplied on the
/// stack.
///
/// Field-for-field, from the stores at `0x801D2770..0x801D27AC`:
///
/// | Offset | Source |
/// |---|---|
/// | `+0x90` | the arc helper allocated first |
/// | `+0x94` | the caller's `sp+0x38` word - the actor's encounter record |
/// | `+0x74` | the caller's `sp+0x3C` word - the emitter's asset pointer |
/// | `+0x5C` | the caller's `sp+0x40` byte, zero-extended |
/// | `+0x9C` | `0` |
/// | `+0x9E` | the raw `a3` frame count, unscaled |
/// | `+0x50` | `1` when the `a0` actor **is** the player, else `0` |
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HopEmitter {
    /// `+0x94` - the caller's first stack pointer. `+0x94` is the actor slot
    /// the encounter record is installed at (`docs/formats/encounter.md`), and
    /// the attached-sprite tick `FUN_801e4470` branches on it being non-null.
    pub encounter_record: u32,
    /// `+0x74` - the emitter's asset pointer.
    pub asset: u32,
    /// `+0x5C` - the class byte, `lbu` from `sp+0x40` so never sign-extended.
    pub class: u16,
    /// `+0x9E` - the frame count, stored raw rather than as `0x1000 / frames`.
    pub frames: i16,
    /// `+0x50` - the "owner is the player" flag retail derives with
    /// `xor` / `sltiu 1` against `_DAT_8007C364`.
    pub owner_is_player: bool,
}

/// What a spawn call produced: the arc clip, plus the emitter when the entry
/// chains one.
///
/// Retail returns the *record pointer* and signals failure with a null; the
/// port returns `None` from the spawners instead, since a pool exhaustion has
/// no other observable effect on the arc itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HopSpawn {
    /// The clip seeded on the `0x801F227C` helper.
    pub arc: HopArc,
    /// The chained `0x801F22AC` emitter, when the entry allocates one.
    pub emitter: Option<HopEmitter>,
}

/// Generic arc-hop spawn: retail `FUN_801d5780(src, &target, apex, frames)`
/// (field overlay `0897_xxx_dat`, file offset `0x6F68`, 57 instructions).
///
/// The four-argument standalone form of the family. It differs from
/// [`build_hop_arc`] in exactly two ways, both read off the disassembly rather
/// than inferred:
///
/// * the Bezier start point is the **`a0` actor's** `+0x14..+0x1B` block
///   (`lwl`/`lwr` pair at `0x801D57D8`), not the player context - so any actor
///   can be arced, and `a0 == 0` returns null at `0x801D57A4` before anything
///   is allocated;
/// * there is **no** second allocation and no movement lock: the body runs
///   straight from the `+0x9E` store to the epilogue.
///
/// PORT: FUN_801d5780
// NOT WIRED: see the module's `NOT WIRED` section - the clip needs a helper
// actor to live on, and `engine-core` has no pool to allocate one from.
pub fn spawn_arc_helper(
    src: Option<(i16, i16, i16)>,
    target: HopTarget,
    apex: i16,
    frames: i16,
) -> Option<HopSpawn> {
    let start = src?;
    Some(HopSpawn {
        arc: build_hop_arc(start, target, apex, frames),
        emitter: None,
    })
}

/// Arc-hop spawn with a chained emitter: retail
/// `FUN_801d25ec(src, &target, apex, frames, encounter, asset, class)`
/// (field overlay `0897_xxx_dat`, file offset `0x3DD4`).
///
/// The first half is [`spawn_arc_helper`] inlined verbatim. The second half
/// allocates from template `0x801F22AC` and fills the [`HopEmitter`] table
/// above.
///
/// The failure ordering matters and is the one thing a re-derivation gets
/// wrong: when the **emitter** allocation fails retail does not leave the arc
/// helper running. It sets the tear-down bit `8` in the helper's `+0x10`
/// (`0x801D27B0..0x801D27BC`) and returns null, so a pool exhaustion abandons
/// the whole spawn rather than leaving a headless clip - the port models that
/// as `None`.
///
/// PORT: FUN_801d25ec
// NOT WIRED: see the module's `NOT WIRED` section.
#[allow(clippy::too_many_arguments)]
pub fn spawn_arc_with_emitter(
    src: Option<(i16, i16, i16)>,
    src_is_player: bool,
    target: HopTarget,
    apex: i16,
    frames: i16,
    encounter_record: u32,
    asset: u32,
    class: u8,
) -> Option<HopSpawn> {
    let spawn = spawn_arc_helper(src, target, apex, frames)?;
    Some(HopSpawn {
        arc: spawn.arc,
        emitter: Some(HopEmitter {
            encounter_record,
            asset,
            class: u16::from(class),
            frames,
            owner_is_player: src_is_player,
        }),
    })
}

/// The per-frame phase global `_DAT_8007BDD8` the hop advance stamps.
///
/// Only the three values `FUN_801d2298` writes are named; the global itself
/// is wider than a hop.
pub mod hop_phase {
    /// Take-off frame (`+0x9C == 0`).
    pub const AIRBORNE: u32 = 6;
    /// The frame the cursor crosses `+0x9E`.
    pub const LANDING: u32 = 7;
    /// Tear-down, `+0x9C >= +0x9E + 6`.
    pub const GROUNDED: u32 = 1;
}

/// SFX cue ids the hop brackets itself with (`FUN_80035b50`).
pub mod hop_sfx {
    /// Take-off.
    pub const TAKE_OFF: u8 = 0x2A;
    /// Landing.
    pub const LAND: u8 = 0x29;
}

/// The live half of the clip: retail's `+0x9C` cursor against the `+0x9E`
/// extent on the **paired** record (the one whose `+0x9E` is the raw frame
/// count, not the `0x1000 / frames` step).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HopSession {
    /// `+0x9C`.
    pub cursor: i16,
    /// `+0x9E`.
    pub extent: i16,
}

/// Everything one `FUN_801d2298` tick writes outside the session itself.
///
/// Modelled as a record rather than as direct mutation because every one of
/// these writes lands on the *player context* `_DAT_8007C364`, which this
/// module deliberately does not reach into.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct HopTick {
    /// A write to `_DAT_8007BDD8`, when the tick makes one.
    pub phase: Option<u32>,
    /// A `FUN_80035b50` cue, when the tick fires one.
    pub sfx: Option<u8>,
    /// Bits ORed into the player actor's `+0x62` (`8` = anim-active).
    pub player_anim_set: u16,
    /// Bits cleared from the player actor's `+0x62`.
    pub player_anim_clear: u16,
    /// Bits ORed into the player actor's `+0x10`.
    pub player_flags_set: u32,
    /// Bits cleared from the player actor's `+0x10`.
    pub player_flags_clear: u32,
    /// The tick set the helper's own tear-down bit `8` - the clip is done.
    pub finished: bool,
}

/// Per-frame advance of a spawned hop: retail `FUN_801d2298(helper)`
/// (field overlay `0897_xxx_dat`, file offset `0x3A80`, 91 instructions).
///
/// `step` is `DAT_1F800393`, the per-frame delta scalar the free-movement step
/// loop also paces on, read as an **unsigned byte** (`lbu`).
///
/// The three phases and their exact guards, from the disassembly:
///
/// | Phase | Guard | Writes |
/// |---|---|---|
/// | take-off | `cursor == 0` | `+0x62 \|= 8`, phase `6`, `+0x10 \|= 0x200000`, SFX `0x2A` |
/// | crossing | `cursor < extent && cursor + step >= extent` | `+0x10 &= ~0x200000`, phase `7`, `+0x62 \|= 8` |
/// | end | `cursor + step >= extent + 6` | `+0x62 &= ~8`, `+0x10 &= ~0x80000`, phase `1`, helper `+0x10 \|= 8`, SFX `0x29` |
///
/// The cursor store at `0x801D233C` sits in a branch **delay slot**, so it
/// happens on every tick regardless of which guard fires, and it is
/// **unclamped**: the sum is stored as-is and the end phase compares against
/// `extent + 6` rather than against a saturated cursor. A port that clamps the
/// cursor to `extent` - as this repo's own locomotion page used to say retail
/// does - can never reach the end phase at a step of `1`, so the hop never
/// releases the movement lock.
///
/// PORT: FUN_801d2298
// NOT WIRED: see the module's `NOT WIRED` section - nothing in `engine-core`
// owns a `HopSession` between frames.
pub fn advance_hop_session(session: &mut HopSession, step: u8) -> HopTick {
    let mut tick = HopTick::default();

    if session.cursor == 0 {
        tick.player_anim_set |= 8;
        tick.phase = Some(hop_phase::AIRBORNE);
        tick.player_flags_set |= 0x0020_0000;
        tick.sfx = Some(hop_sfx::TAKE_OFF);
    }

    let before = session.cursor;
    // `addu` on the halfword then `sh` back: retail wraps rather than
    // saturates, and the compare that follows re-reads the stored halfword.
    let next = (before as u16).wrapping_add(u16::from(step)) as i16;
    session.cursor = next;

    if before < session.extent && next >= session.extent {
        tick.player_flags_clear |= 0x0020_0000;
        tick.phase = Some(hop_phase::LANDING);
        tick.player_anim_set |= 8;
    }

    if next >= session.extent.saturating_add(6) {
        tick.player_anim_clear |= 8;
        tick.player_flags_clear |= 0x0008_0000;
        tick.phase = Some(hop_phase::GROUNDED);
        tick.finished = true;
        tick.sfx = Some(hop_sfx::LAND);
    }

    tick
}

/// Evaluate the seeded quadratic Bezier at `t = num/den`, in the same
/// `0x1000`-scaled fixed point the clip cursor uses.
///
/// Provided so the arc's shape is testable without the per-frame advance:
/// retail's `FUN_801d2298` walks `+0x9C` from `0` to `+0x9E` and feeds it
/// through the same basis functions.
pub fn bezier_at(p0: i16, c: i16, p2: i16, cursor: i32) -> i32 {
    let t = cursor.clamp(0, CLIP_FULL) as i64;
    let u = CLIP_FULL as i64 - t;
    let full = CLIP_FULL as i64;
    // (u^2 * p0 + 2*u*t*c + t^2 * p2) / 0x1000^2 - widened because the
    // numerator alone needs 40-odd bits at full i16 range.
    let acc = u * u * p0 as i64 + 2 * u * t * c as i64 + t * t * p2 as i64;
    (acc / (full * full)) as i32
}

/// How far above the higher endpoint the seeded arc peaks, in world units.
///
/// Always equals the `apex` argument by construction - see the module docs.
pub fn hop_apex_height(arc: &HopArc) -> i32 {
    let hi = arc.start.1.min(arc.end.1) as i32;
    hi - bezier_at(arc.start.1, arc.control.1, arc.end.1, CLIP_FULL / 2)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(x: i16, y: i16, z: i16) -> HopTarget {
        HopTarget { x, y, z }
    }

    #[test]
    fn generic_spawner_refuses_a_null_owner() {
        assert!(spawn_arc_helper(None, target(0, 0, 0), 0x10, 0x10).is_none());
        assert!(
            spawn_arc_with_emitter(None, false, target(0, 0, 0), 0x10, 0x10, 1, 2, 3).is_none()
        );
    }

    #[test]
    fn generic_spawner_matches_the_player_form_arithmetic() {
        let start = (10, 40, 70);
        let t = target(90, 120, 150);
        let direct = build_hop_arc(start, t, 0x18, 0x10);
        let spawned = spawn_arc_helper(Some(start), t, 0x18, 0x10).unwrap();
        assert_eq!(spawned.arc, direct);
        assert!(spawned.emitter.is_none());
    }

    #[test]
    fn emitter_stores_the_raw_frame_count_not_the_scaled_step() {
        let s = spawn_arc_with_emitter(
            Some((0, 0, 0)),
            true,
            target(64, 0, 0),
            0x10,
            0x10,
            0xdead_beef,
            0xfeed_face,
            0x0c,
        )
        .unwrap();
        let e = s.emitter.unwrap();
        // `+0x9E` on the arc helper is `0x1000 / frames`; on the emitter it is
        // the raw `a3`.
        assert_eq!(s.arc.step, 0x100);
        assert_eq!(e.frames, 0x10);
        assert_eq!(e.class, 0x0c);
        assert_eq!(e.encounter_record, 0xdead_beef);
        assert_eq!(e.asset, 0xfeed_face);
        assert!(e.owner_is_player);
    }

    #[test]
    fn hop_tick_fires_take_off_only_on_the_zero_frame() {
        let mut s = HopSession {
            cursor: 0,
            extent: 0x10,
        };
        let first = advance_hop_session(&mut s, 1);
        assert_eq!(first.sfx, Some(hop_sfx::TAKE_OFF));
        assert_eq!(first.phase, Some(hop_phase::AIRBORNE));
        assert_eq!(first.player_flags_set, 0x0020_0000);
        assert_eq!(s.cursor, 1);
        let second = advance_hop_session(&mut s, 1);
        assert_eq!(second.sfx, None);
        assert_eq!(second.phase, None);
    }

    #[test]
    fn hop_tick_reaches_the_end_phase_because_the_cursor_is_unclamped() {
        let mut s = HopSession {
            cursor: 0,
            extent: 0x10,
        };
        let mut crossing = None;
        let mut end = None;
        for frame in 0..64 {
            let t = advance_hop_session(&mut s, 1);
            if t.phase == Some(hop_phase::LANDING) {
                crossing = Some(frame);
            }
            if t.finished {
                end = Some(frame);
                break;
            }
        }
        // Crossing on the frame the cursor first reaches `extent`, tear-down
        // six frames later.
        assert_eq!(crossing, Some(0x0f));
        assert_eq!(end, Some(0x15));
        let t = advance_hop_session(
            &mut HopSession {
                cursor: 0x10,
                extent: 0x10,
            },
            1,
        );
        assert_eq!(t.player_flags_clear, 0);
        assert!(!t.finished);
    }

    #[test]
    fn hop_tick_end_phase_releases_the_movement_lock() {
        let mut s = HopSession {
            cursor: 0x15,
            extent: 0x10,
        };
        let t = advance_hop_session(&mut s, 1);
        assert!(t.finished);
        assert_eq!(t.player_flags_clear, 0x0008_0000);
        assert_eq!(t.player_anim_clear, 8);
        assert_eq!(t.phase, Some(hop_phase::GROUNDED));
        assert_eq!(t.sfx, Some(hop_sfx::LAND));
    }

    #[test]
    fn midpoints_are_plain_for_x_and_z() {
        let arc = build_hop_arc((100, 0, 200), target(300, 0, 400), 0x10, 0x10);
        assert_eq!(arc.control.0, 200);
        assert_eq!(arc.control.2, 300);
    }

    #[test]
    fn step_is_full_extent_over_frame_count() {
        let arc = build_hop_arc((0, 0, 0), target(0, 0, 0), 0x10, 0x10);
        assert_eq!(arc.step, 0x100);
        assert_eq!(arc.cursor, 0);
        assert_eq!(arc.paired_frames, 0x10);
    }

    #[test]
    fn non_positive_frame_count_takes_the_blez_arm() {
        for frames in [0i16, -1, -0x10] {
            let arc = build_hop_arc((0, 0, 0), target(0, 0, 0), 0x10, frames);
            assert_eq!(arc.step, CLIP_FULL as i16, "frames = {frames}");
        }
    }

    #[test]
    fn arc_peaks_apex_units_above_the_higher_endpoint() {
        // PSX Y grows downward, so "higher" is the smaller value.
        for (p0y, p2y) in [(0i16, 0i16), (0, 200), (200, 0), (-64, 96), (96, -64)] {
            for apex in [0x10i16, 0x18] {
                let arc = build_hop_arc((0, p0y, 0), target(0, p2y, 0), apex, 0x10);
                assert_eq!(
                    hop_apex_height(&arc),
                    apex as i32,
                    "p0y={p0y} p2y={p2y} apex={apex}"
                );
            }
        }
    }

    #[test]
    fn endpoints_are_exact() {
        let arc = build_hop_arc((10, 40, 70), target(90, 120, 150), 0x10, 0x10);
        assert_eq!(bezier_at(arc.start.1, arc.control.1, arc.end.1, 0), 40);
        assert_eq!(
            bezier_at(arc.start.1, arc.control.1, arc.end.1, CLIP_FULL),
            120
        );
    }

    #[test]
    fn midpoint_uses_arithmetic_shift_not_truncating_division() {
        // -3 + 0 = -3; `sra 1` floors to -2, whereas `-3 / 2` truncates to -1.
        assert_eq!(mid(-3, 0), -2);
    }
}
