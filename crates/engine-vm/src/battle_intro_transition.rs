//! The field-to-battle transition overlay's state machine and its shared
//! textured-quad builder.
//!
//! The state machine is live: `legaia_engine_core::World::tick_encounter`
//! drives it once per frame for as long as the encounter session sits in its
//! `Transition` phase, which is the same window retail runs this overlay in.
//! The quad builder is not - see [`build_intro_quad`].
//!
//! The transition is its own overlay (PROT 0979 `field_battle_intro`; see
//! `docs/subsystems/cutscene.md` § "Field-to-battle transition"). It runs the
//! 3D camera spin between leaving the field and the battle scene coming up,
//! and it does two jobs at once: sequence the battle handoff (mesh assembly,
//! BGM, scene bundle, the game-mode write) and drive one of five visual
//! styles.
//!
//! ## The dump caveat this module used to carry is retired
//!
//! An earlier note here said `overlay_field_battle_intro_801cf5bc.txt`
//! "reports `size=752 bytes, 188 instructions`" and that its disassembly
//! "stops at `0x801CF8A8`, on a branch **delay slot** rather than a `jr ra`",
//! and on that basis three things were deliberately left unported as
//! decompiled-C-only: the `ctx+0x2A == 3` completion arm's calls, the
//! game-mode handoff write `_DAT_8007B83C = 0x14`, and the per-style fade
//! ramp.
//!
//! That was true of the dump it was written against and is not true of the
//! dump in the corpus. **The caveat went stale because the dump got longer.**
//! It has since been re-extracted by an extent walker that reports its own
//! completeness, and now reads `size=1528 bytes, 382 instructions` /
//! `extent=walk with frontier + frame-boundary rule: complete, 0 return(s)
//! crossed`, ending on a real `jr ra` at `0x801CFBAC` with the stack
//! adjustment in its delay slot. Nothing about the function changed; the
//! window onto it did, and the caveat kept asserting a truncation that had
//! stopped existing.
//!
//! All three withheld items are therefore disassembly-grounded now and are
//! ported: the completion arm and the mode write below, and both tail
//! switches in [`crate::battle_intro_styles`] ([`IntroStyle`] and
//! `intro_fade`). This is a general hazard rather than a one-off - see
//! `docs/tooling/dump-corpus-integrity.md` § "A caveat outlives the dump it
//! was written against".
//!
//! Provenance: `see ghidra/scripts/funcs/overlay_field_battle_intro_801cf5bc.txt`
//! and `overlay_field_battle_intro_801cf1b0.txt`.
//!
//! The handoff tail's four calls, and the routine that fills the formation
//! cell it reads, are named where they are used and cited once here:
//!
//! REF: FUN_80062004 - the libsnd volume shim (`SsSeqSetVol(slot, ch 0, vol,
//! ramp)` via `FUN_80061EDC`), phase 0's second audio call.
//! REF: FUN_80026740 - actor sound-source pan-reset + voice-stop.
//! REF: FUN_800266E0 - actor sound-source teardown.
//! REF: FUN_8001E54C - the asset / SEQ installer.
//! REF: FUN_80026478 - actor sound-source attach, i.e. the battle track's
//! actual start.
//! REF: FUN_8005567C - the battle-id to formation-cell expander that fills
//! `DAT_8007BD0C`.

// ---------------------------------------------------------------------------
// FUN_801CF5BC - transition tick
// ---------------------------------------------------------------------------

/// Number of handled phases in the entity's `+0x22` phase counter. Retail
/// bounds it with `sltiu v0, v1, 0x8`; anything `>= 8` skips the switch
/// entirely and falls into the shared post-switch block.
pub const TRANSITION_PHASES: u16 = 8;

/// The transition entity's fields this kernel reads and writes.
///
/// | field | entity offset |
/// |---|---|
/// | `phase` | `+0x22` |
/// | `elapsed` | `+0x1A` |
/// | `ready` | `+0x2A` |
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TransitionEntity {
    /// `+0x22` - the handoff phase counter.
    pub phase: i16,
    /// `+0x1A` - the camera-spin frame counter.
    pub elapsed: i16,
    /// `+0x2A` - the ready bitfield. Bit `0` is raised near the end of the
    /// spin, bit `1` by phase 7; `3` means both.
    pub ready: u16,
}

/// The globals the tick reads. Named for what they gate rather than for their
/// address, with the address given so the read is checkable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TransitionGlobals {
    /// `_DAT_8007B880` - the battle id. `-1` means "no id yet", `0` selects
    /// the default battle, and any positive value indexes the per-battle
    /// bundle. Negative values short-circuit several phases.
    pub battle_id: i32,
    /// `DAT_8007BD60` - the per-battle flags byte. Only bit `0x80` is read.
    pub battle_flags: u8,
    /// `DAT_8007B648` - the mesh-assembly completion byte. Phase 1 holds
    /// until it reads exactly `0x80`.
    pub assembly_state: u8,
    /// `DAT_8007B64B` - the alternate-default-bundle flag, read only when
    /// `battle_id == 0`.
    pub alt_default_bundle: u8,
    /// `DAT_8007B8C2` - selects which of the two phase-5 lookups runs.
    pub phase5_selector: i16,
    /// `DAT_801D2458` - the total intro duration the spin counter is measured
    /// against.
    pub total_duration: i32,
    /// `DAT_80070536` (i16) - the field-BGM sequencer voice id. The address
    /// is `0x8007052C + 0xA`: byte `+0xA` of the field-BGM sound-source actor
    /// (the same actor [`TransitionEffect::StopFieldBgmVoice`] targets), which
    /// is the bound voice id `FUN_80026478` keys its resume on. Runtime-written
    /// when the field track attaches; the static SCUS image holds `0`.
    pub bgm_voice_id: i16,
    /// `_DAT_8007B910` - the **live audio level** (`0..255`, cold reset
    /// `0xD7`; see `docs/subsystems/battle-action.md` § "The `_DAT_8007B910`
    /// ramps are an audio duck"). Phase 0 narrows it to libsnd's `0..0x7F`
    /// with retail's own `<< 15` then arithmetic `>> 16` (i.e. a halve).
    pub audio_level: i32,
    /// `DAT_8007BAAC` - the bundle handle phase 2 loads into and the
    /// completion arm installs the battle sequence from. Read in the
    /// post-switch block, not in the phase switch.
    pub bundle_handle: u32,
    /// `DAT_8007BD0C` - slot 0 of the resolved formation cell (the cell
    /// `FUN_8005567C` fills). The completion arm compares it against
    /// [`FORMATION_RESET_BATTLE_ID`].
    pub formation_slot0: u8,
}

/// The battle-start cue id phase 0 writes into `_DAT_8007B6D8` when it takes
/// the plain arm (`0x1F`), and `0x4D` when it takes the flagged arm. Both are
/// stored as halfwords.
///
/// `_DAT_8007B6D8` is **slot 0 of the 4-slot pending SFX ring** the battle
/// funnel `FUN_8004FE5C` appends into (`engine-core::sfx_cue`), so the value
/// is already in the drainer's resolved ring-id space - which for ids below
/// `0x64` is the static SFX descriptor table index itself
/// (`docs/formats/sfx-table.md`; both `0x1F` and `0x4D` are populated
/// category-0 descriptors). The store bypasses the funnel and bumps no
/// counter, so a second store in the same tick **overwrites** slot 0 - the
/// last cue written is the one that plays.
pub const AUDIO_CUE_PLAIN: u16 = 0x1F;
/// See [`AUDIO_CUE_PLAIN`].
pub const AUDIO_CUE_FLAGGED: u16 = 0x4D;

/// Third argument of the phase-0 `SsSeqSetVol` call: the volume ramp length,
/// `li a2, 0x64` at `0x801CF658` (loaded in the branch delay slot, so it is
/// in place for both the default-battle and positive-id arms).
pub const BGM_LEVEL_RAMP_TICKS: i32 = 100;

/// Base scene-bundle id phase 2 loads: `battle_id + 0x36F`, or `0x36F`/`0x370`
/// when `battle_id == 0` depending on [`TransitionGlobals::alt_default_bundle`].
pub const BATTLE_BUNDLE_BASE: i32 = 0x36F;

/// The bundle-load flag word phase 2 pushes as the fifth argument of
/// `FUN_8001FC00` (`lui 0x3; ori 0x2000`).
pub const BATTLE_BUNDLE_FLAGS: u32 = 0x0003_2000;

/// One observable side effect of a transition tick. Retail performs these as
/// direct calls; the port surfaces them so a host can order them exactly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransitionEffect {
    /// Phase 0: a halfword store into `_DAT_8007B6D8` - slot 0 of the pending
    /// SFX ring, in resolved ring-id space (see [`AUDIO_CUE_PLAIN`]). Retail
    /// issues this **up to twice** in one tick - once in the `battle_id == -1`
    /// pre-arm and once in the arm the reloaded id selects - so the effect
    /// list preserves both writes in order rather than collapsing them; a
    /// consumer honouring the slot-0 overwrite plays only the **last** one.
    SetAudioCue { cue: u16 },
    /// Phase 0: `FUN_80062004(DAT_80070536, level, 100)` -
    /// `SsSeqSetVol(voice, channel 0, vol, ramp)` via `FUN_80061EDC`
    /// (`docs/reference/re-settled-threads.md` § "`_DAT_8007B910` is the live
    /// audio level"). Not a cue play: it re-applies the live audio level
    /// (halved into libsnd's `0..0x7F`, retail's `<< 15` then arithmetic
    /// `>> 16`) to the field-BGM voice over a [`BGM_LEVEL_RAMP_TICKS`]-tick
    /// ramp, undoing any active duck as the spin starts. A negative battle id
    /// never reaches this call.
    ReapplyBgmLevel { voice_id: i16, level: i16 },
    /// Phase 1: `FUN_80052770()` - the battle-mesh assembly step.
    AssembleBattleMeshes,
    /// Phase 2: `FUN_800567A8("battle_bgm_%d", battle_id)`.
    LoadBattleBgm { battle_id: i32 },
    /// Phase 2: `FUN_8001FC00(bundle_id, 5, DAT_8007BAAC, 0, flags)`.
    LoadBattleBundle { bundle_id: i32, flags: u32 },
    /// Phase 5: the `DAT_8007B8C2 == 0` arm,
    /// `FUN_8003E6BC("brule_xxx", DAT_8007B9AC)`.
    Phase5Lookup,
    /// Phase 5: the `DAT_8007B8C2 != 0` arm,
    /// `FUN_8003EB98(0x384, DAT_8007B9AC, 1)`, whose result is shifted left
    /// by 11 before the rounding step.
    Phase5AltLookup,
    /// Phases 3 and 6: `FUN_8003DE7C(1)`. The phase advances only when it
    /// returns zero, so the phase holds while this reports "busy".
    WaitForLoad,
    /// Post-switch, `0x801CF8AC`: `FUN_80026740(0x8007052C)` - the field
    /// BGM's sound source is pan-reset and its voice stopped, six frames
    /// before the intro's end. Fires only once both ready bits are up and the
    /// battle id is non-negative.
    StopFieldBgmVoice,
    /// Post-switch, `0x801CF904`: `FUN_800266E0(0x8007051C)` - teardown of the
    /// sequencer slot the field track was playing on.
    ReleaseFieldBgmSlot,
    /// Post-switch, `0x801CF924`: `FUN_8001E54C(5, DAT_8007BAAC, 0)` - the
    /// asset/SEQ installer, on the same bundle handle phase 2's
    /// [`TransitionEffect::LoadBattleBundle`] read into. This is where the
    /// battle track is *installed*, as against phase 2 where it is *loaded*.
    InstallBattleSequence { bundle_handle: u32 },
    /// Post-switch, `0x801CF92C`: `FUN_80026478(0x8007056C)` - attach the
    /// sound source, i.e. the battle track actually starts here.
    StartBattleBgmVoice,
}

/// What one tick of `FUN_801CF5BC` decided, apart from the entity writes it
/// already applied.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TransitionTick {
    /// Host calls in retail order.
    pub effects: Vec<TransitionEffect>,
    /// Whether the tick cleared `_DAT_8007B92C` / `_DAT_8007B930` a second
    /// time in the post-switch block (it always clears them on entry).
    ///
    /// Retail follows the clear with a **floor at zero** on each accumulator
    /// (`0x801CF81C`..`0x801CF844`, `bgez` then `sw zero`). The port does not
    /// model that because it does not own the two accumulators; the flag is
    /// the whole of what this kernel exposes about them.
    pub cleared_spin_accumulators: bool,
    /// The tick wrote the master game-mode handoff `_DAT_8007B83C = 0x14`
    /// (enter battle) at `0x801CF8F8`.
    ///
    /// The store sits in the **delay slot** of the branch that tests the
    /// battle id, so it lands whether or not the id is negative - only the
    /// audio handoff behind it is skipped. Gated on the intro clock having
    /// run past its full duration *and* [`TransitionEntity::ready`] being
    /// exactly `3`.
    pub entered_battle_mode: bool,
    /// The tick reset the battle id `_DAT_8007B880` to zero because the
    /// resolved formation cell's first slot `DAT_8007BD0C` reads
    /// [`FORMATION_RESET_BATTLE_ID`] (`0x801CF934`..`0x801CF94C`).
    pub cleared_battle_id: bool,
}

/// The master game-mode value the completion arm writes into `_DAT_8007B83C`.
pub const MASTER_MODE_ENTER_BATTLE: u16 = 0x14;

/// The formation-cell slot-0 monster id (`DAT_8007BD0C`) that makes the
/// completion arm zero the battle id instead of leaving it set.
pub const FORMATION_RESET_BATTLE_ID: u8 = 0xA6;

/// Frames before the intro's end at which the field BGM's voice is stopped
/// (`DAT_801D2458 - 6 < elapsed`).
pub const FIELD_BGM_STOP_LEAD: i32 = 6;

/// Answers the host must supply because the kernel cannot call into the
/// overlay's load/wait routines itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TransitionResponses {
    /// Return value of `FUN_8003DE7C(1)` for phases 3 / 6. Zero advances the
    /// phase; non-zero holds it.
    pub load_busy: i32,
}

/// Round a `q12`-style product toward zero, the way retail does it with
/// `if (x < 0) x += 0xFFF; x >>= 12`.
fn shr_toward_zero_12(x: i32) -> i32 {
    if x < 0 { x + 0xfff } else { x }.wrapping_shr(12)
}

/// Round toward zero by 256 (`if (x < 0) x += 0xFF; x >>= 8`).
fn shr_toward_zero_8(x: i32) -> i32 {
    if x < 0 { x + 0xff } else { x }.wrapping_shr(8)
}

/// Round toward zero by 1024 (`if (x < 0) x += 0x3FF; x >>= 10`).
fn shr_toward_zero_10(x: i32) -> i32 {
    if x < 0 { x + 0x3ff } else { x }.wrapping_shr(10)
}

/// Run one tick of the field-to-battle transition state machine.
/// `FUN_801CF5BC`.
///
/// Phase bodies, all read off the printed disassembly:
///
/// | phase | body |
/// |---|---|
/// | `0` | Writes the battle-start cue into SFX-ring slot 0 and, for a non-negative id, re-applies the live audio level to the field-BGM voice; then advances. A negative battle id other than the `-1` sentinel skips both and advances anyway. |
/// | `1` | Runs the mesh assembly every frame; falls into the phase-3 body once `assembly_state == 0x80`, otherwise holds. |
/// | `2` | Loads the battle BGM, then the battle scene bundle (skipped for a negative battle id), then advances. |
/// | `3`, `6` | Holds while the load-wait reports busy; advances when it reports idle. |
/// | `4` | Advance only. |
/// | `5` | One of two lookups, whose result is rounded to a `0x800` grid into `_DAT_8007B9DC`; then advances. |
/// | `7` | Raises ready bit `1`. Does **not** advance. |
///
/// Phase `>= 8` runs no body at all.
///
/// The switch's phase-to-address mapping is the one Ghidra's switch recovery
/// reports; the jump table itself is data at `0x801CE870` and is not in the
/// dump. Each phase *body* is disassembly-grounded.
///
/// After the switch comes the **post-switch block**, which runs on every tick
/// regardless of phase, and then the **handoff tail**:
///
/// | clock test | effect |
/// |---|---|
/// | `elapsed >= 4` | clears the two spin accumulators |
/// | `total - 0x1E < elapsed` | raises ready bit `0` |
/// | `total - 6 < elapsed`, `ready == 3`, id `>= 0` | stops the field BGM voice |
/// | `total < elapsed`, `ready == 3` | writes the game-mode handoff, then swaps the BGM slot over to the battle track |
///
/// Both tail arms compare `ready` for **equality** with `3`, so the two ready
/// bits gate the handoff jointly - which is why phase 7 resting without
/// advancing (bit `1`) and the spin clock running out (bit `0`) both have to
/// happen before a battle opens.
///
/// What this kernel does **not** carry is the pair of tail switches on the
/// style selector; those are [`crate::battle_intro_styles::IntroStyle`] and
/// [`crate::battle_intro_styles::intro_fade`], kept with the styles they
/// dispatch to rather than here.
///
/// PORT: FUN_801CF5BC
pub fn tick_transition(
    entity: &mut TransitionEntity,
    g: &TransitionGlobals,
    responses: &TransitionResponses,
) -> TransitionTick {
    let mut out = TransitionTick::default();
    let mut advance = false;

    if (entity.phase as u16) < TRANSITION_PHASES {
        match entity.phase {
            0 => {
                // Pre-arm, taken only for the "no battle id yet" sentinel.
                // The plain value is stored in the branch **delay slot**, so
                // it lands whether or not the flag bit is set; the flagged
                // value then overwrites it.
                if g.battle_id == -1 {
                    out.effects.push(TransitionEffect::SetAudioCue {
                        cue: AUDIO_CUE_PLAIN,
                    });
                    if (g.battle_flags & 0x80) != 0 {
                        out.effects.push(TransitionEffect::SetAudioCue {
                            cue: AUDIO_CUE_FLAGGED,
                        });
                    }
                }
                // Retail re-reads the battle id here rather than reusing the
                // register from the pre-arm.
                if g.battle_id >= 0 {
                    let cue = if g.battle_id == 0 {
                        AUDIO_CUE_PLAIN
                    } else {
                        AUDIO_CUE_FLAGGED
                    };
                    out.effects.push(TransitionEffect::SetAudioCue { cue });
                    out.effects.push(TransitionEffect::ReapplyBgmLevel {
                        voice_id: g.bgm_voice_id,
                        level: (g.audio_level.wrapping_shl(15) >> 16) as i16,
                    });
                }
                // Every arm - including the `blez` negative-id one - advances.
                advance = true;
            }
            1 => {
                out.effects.push(TransitionEffect::AssembleBattleMeshes);
                if g.assembly_state == 0x80 {
                    out.effects.push(TransitionEffect::WaitForLoad);
                    if responses.load_busy == 0 {
                        advance = true;
                    }
                }
            }
            2 => {
                out.effects.push(TransitionEffect::LoadBattleBgm {
                    battle_id: g.battle_id,
                });
                if g.battle_id >= 0 {
                    let bundle_id = if g.battle_id == 0 {
                        if g.alt_default_bundle == 0 {
                            BATTLE_BUNDLE_BASE
                        } else {
                            BATTLE_BUNDLE_BASE + 1
                        }
                    } else {
                        g.battle_id + BATTLE_BUNDLE_BASE
                    };
                    out.effects.push(TransitionEffect::LoadBattleBundle {
                        bundle_id,
                        flags: BATTLE_BUNDLE_FLAGS,
                    });
                }
                advance = true;
            }
            3 | 6 => {
                out.effects.push(TransitionEffect::WaitForLoad);
                if responses.load_busy == 0 {
                    advance = true;
                }
            }
            4 => advance = true,
            5 => {
                if g.phase5_selector == 0 {
                    out.effects.push(TransitionEffect::Phase5Lookup);
                } else {
                    out.effects.push(TransitionEffect::Phase5AltLookup);
                }
                advance = true;
            }
            _ => {
                // Phase 7.
                entity.ready |= 2;
            }
        }
    }

    if advance {
        entity.phase = entity.phase.wrapping_add(1);
    }

    // Post-switch block, all disassembly-grounded.
    if entity.elapsed >= 4 {
        out.cleared_spin_accumulators = true;
    }
    let clock = i32::from(entity.elapsed);
    if (g.total_duration - 0x1E) < clock {
        entity.ready |= 1;
    }

    // The handoff tail. Both arms test `ready == 3` for equality, not for the
    // bits - a `ready` of 1 or 2 alone reaches neither.
    let ready = entity.ready == 3;
    if (g.total_duration - FIELD_BGM_STOP_LEAD) < clock && ready && g.battle_id >= 0 {
        out.effects.push(TransitionEffect::StopFieldBgmVoice);
    }
    if g.total_duration < clock && ready {
        // The mode write is in the delay slot of the battle-id test, so it
        // lands on both arms; only the audio handoff is skipped.
        out.entered_battle_mode = true;
        if g.battle_id >= 0 {
            out.effects.push(TransitionEffect::ReleaseFieldBgmSlot);
            // Retail re-reads the battle id here rather than reusing the
            // register it just branched on.
            if g.battle_id >= 0 {
                out.effects.push(TransitionEffect::InstallBattleSequence {
                    bundle_handle: g.bundle_handle,
                });
                out.effects.push(TransitionEffect::StartBattleBgmVoice);
            }
        }
        if g.formation_slot0 == FORMATION_RESET_BATTLE_ID {
            out.cleared_battle_id = true;
        }
    }

    out
}

// ---------------------------------------------------------------------------
// FUN_801CF1B0 - transition quad builder
// ---------------------------------------------------------------------------

/// Byte stride of one record in the transition sprite table at `0x801D1EC4`.
pub const INTRO_QUAD_DESC_STRIDE: usize = 0x14;

/// Mask that splits the caller's packed key into its descriptor index.
/// The remaining high bits (`key / 0x400`, rounded toward zero) are the mode.
pub const INTRO_QUAD_INDEX_MASK: i32 = 0x3FF;

/// GPU primitive code base for a Gouraud-shaded textured quad (`POLY_GT4`).
/// The `abr` bit is OR-ed in one place left: `code = (abr << 1) | 0x3C`.
pub const POLY_GT4_CODE: u8 = 0x3C;

/// The CLUT the builder substitutes for the descriptor's own when the packed
/// key's mode field is exactly `2`.
pub const INTRO_QUAD_MODE2_CLUT: u16 = 0x7D0F;

/// The OT depth the builder writes back into `DAT_801D245C` after every emit,
/// so a caller-chosen depth is consumed exactly once.
pub const INTRO_QUAD_DEFAULT_OT_DEPTH: u32 = 3;

/// One 20-byte record of the transition sprite table at `0x801D1EC4`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct IntroQuadDesc {
    /// `+0x00` - `q12` size scale applied to `w` / `h` before the caller's own.
    pub size_q12: i32,
    /// `+0x04` - base tpage word; the mode's step is added as `step << 5`.
    pub tpage: u16,
    /// `+0x06` - CLUT word (overridden for mode `2`).
    pub clut: u16,
    /// `+0x08` / `+0x09` - top-left texel.
    pub u0: u8,
    /// See [`IntroQuadDesc::u0`].
    pub v0: u8,
    /// `+0x0A` / `+0x0B` - texel extent, also the pre-scale quad extent.
    pub w: u8,
    /// See [`IntroQuadDesc::w`].
    pub h: u8,
    /// `+0x0C..+0x0E` - the colour of the quad's **top** edge (vertices 0/1).
    pub top: [u8; 3],
    /// `+0x0F` - semi-transparency mode, used only when the packed key's mode
    /// field is zero.
    pub abr: u8,
    /// `+0x10..+0x12` - the colour of the quad's **bottom** edge (vertices
    /// 2/3). Top and bottom differing is what makes the quad a gradient.
    pub bottom: [u8; 3],
    /// `+0x13` - tpage step, used only when the packed key's mode field is
    /// zero.
    pub tpage_step: u8,
}

impl IntroQuadDesc {
    /// Decode one record from its 20 little-endian bytes.
    pub fn parse(bytes: &[u8]) -> Option<Self> {
        let b: &[u8; INTRO_QUAD_DESC_STRIDE] =
            bytes.get(..INTRO_QUAD_DESC_STRIDE)?.try_into().ok()?;
        Some(Self {
            size_q12: i32::from_le_bytes([b[0], b[1], b[2], b[3]]),
            tpage: u16::from_le_bytes([b[4], b[5]]),
            clut: u16::from_le_bytes([b[6], b[7]]),
            u0: b[8],
            v0: b[9],
            w: b[0x0a],
            h: b[0x0b],
            top: [b[0x0c], b[0x0d], b[0x0e]],
            abr: b[0x0f],
            bottom: [b[0x10], b[0x11], b[0x12]],
            tpage_step: b[0x13],
        })
    }
}

/// One corner of the built quad.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct IntroQuadVertex {
    /// Screen X.
    pub x: i16,
    /// Screen Y.
    pub y: i16,
    /// Texel U.
    pub u: u8,
    /// Texel V.
    pub v: u8,
    /// Vertex colour, already scaled by the caller's intensity.
    pub rgb: [u8; 3],
}

/// A built `POLY_GT4`, in the vertex order retail writes it: `0` top-left,
/// `1` top-right, `2` bottom-left, `3` bottom-right.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct IntroQuad {
    /// GPU primitive code byte, `(abr << 1) | 0x3C`.
    pub code: u8,
    /// Texture page word, `desc.tpage + (tpage_step << 5)`.
    pub tpage: u16,
    /// CLUT word.
    pub clut: u16,
    /// The four corners.
    pub verts: [IntroQuadVertex; 4],
    /// OT depth this primitive was linked at - the value `DAT_801D245C` held
    /// on entry. The builder then resets that global to
    /// [`INTRO_QUAD_DEFAULT_OT_DEPTH`].
    pub ot_depth: u32,
}

/// Where the caller's `(x, y)` sits relative to the quad. `param_1` of
/// `FUN_801CF1B0`: zero centres, non-zero anchors the top-left.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntroQuadAnchor {
    /// `param_1 == 0` - the quad is centred on `(x, y)`, each half-extent
    /// rounded toward zero.
    Centre,
    /// `param_1 != 0` - `(x, y)` is the top-left corner.
    TopLeft,
}

/// The seven caller-supplied arguments of `FUN_801CF1B0`, plus the OT depth it
/// picks up from the global `DAT_801D245C`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IntroQuadRequest {
    /// `param_1`.
    pub anchor: IntroQuadAnchor,
    /// `param_2`.
    pub x: i16,
    /// `param_3`.
    pub y: i16,
    /// `param_4` - the packed `(mode << 10) | index` key.
    pub key: i32,
    /// `param_5` - the `/ 256` colour intensity.
    pub intensity: i32,
    /// `param_6` - `q12` horizontal scale.
    pub scale_x: i32,
    /// `param_7` - `q12` vertical scale.
    pub scale_y: i32,
    /// `DAT_801D245C` on entry.
    pub ot_depth: u32,
}

/// Build one transition quad. `FUN_801CF1B0`.
///
/// [`IntroQuadRequest::key`] packs two things: the low ten bits index the
/// descriptor table, and the rest (`key / 0x400`, rounded toward zero) is a
/// **mode**. Mode `0` means "use the descriptor's own `abr` and tpage step";
/// any other mode forces `abr = 1` (semi-transparent) and uses the mode value
/// itself as the tpage step. Mode `2` additionally replaces the CLUT with
/// [`INTRO_QUAD_MODE2_CLUT`].
///
/// `intensity` scales every vertex colour by `c * intensity / 256`;
/// `scale_x` / `scale_y` are `q12` multipliers applied on top of the
/// descriptor's own `q12` size. All four divisions round toward zero, which is
/// what retail's `if (x < 0) x += mask` pre-bias does.
///
/// PORT: FUN_801CF1B0
///
/// WIRED. Its one caller [`crate::battle_intro_styles::tick_curtain`] draws
/// end to end: `legaia_engine_render::battle_intro::BattleIntro` owns the
/// `0x14`-stride descriptor table - parsed off PROT 0979 by
/// `IntroQuadTable::parse_overlay`, so the overlay the older note called
/// unloaded is read - ticks it from the live transition clock, and converts
/// every quad built here into a screen primitive. The native window reaches
/// the pair through `take_battle_intro_frame`.
pub fn build_intro_quad(req: &IntroQuadRequest, table: &[IntroQuadDesc]) -> Option<IntroQuad> {
    let IntroQuadRequest {
        anchor,
        x,
        y,
        key,
        intensity,
        scale_x,
        scale_y,
        ot_depth,
    } = *req;
    let mode = shr_toward_zero_10(key);
    let desc = *table.get((key & INTRO_QUAD_INDEX_MASK) as usize)?;

    let (abr, tpage_step) = if mode == 0 {
        (desc.abr as i32, desc.tpage_step as i32)
    } else {
        (1, mode)
    };

    let shade = |c: u8| -> u8 { shr_toward_zero_8(c as i32 * intensity) as u8 };
    let top = [shade(desc.top[0]), shade(desc.top[1]), shade(desc.top[2])];
    let bottom = [
        shade(desc.bottom[0]),
        shade(desc.bottom[1]),
        shade(desc.bottom[2]),
    ];

    let w = shr_toward_zero_12(shr_toward_zero_12(desc.w as i32 * desc.size_q12) * scale_x);
    let h = shr_toward_zero_12(shr_toward_zero_12(desc.h as i32 * desc.size_q12) * scale_y);

    let (x0, y0, x1, y1) = match anchor {
        IntroQuadAnchor::TopLeft => (x, y, x.wrapping_add(w as i16), y.wrapping_add(h as i16)),
        IntroQuadAnchor::Centre => {
            // Retail halves with `srl 31; addu; sra 1` - a toward-zero /2.
            let hw = (w / 2) as i16;
            let hh = (h / 2) as i16;
            (
                x.wrapping_sub(hw),
                y.wrapping_sub(hh),
                x.wrapping_add(hw),
                y.wrapping_add(hh),
            )
        }
    };

    let (u0, v0) = (desc.u0, desc.v0);
    let (u1, v1) = (desc.u0.wrapping_add(desc.w), desc.v0);
    let (u2, v2) = (desc.u0, desc.v0.wrapping_add(desc.h));
    let (u3, v3) = (desc.u0.wrapping_add(desc.w), desc.v0.wrapping_add(desc.h));

    Some(IntroQuad {
        code: ((abr as u8) << 1) | POLY_GT4_CODE,
        tpage: desc.tpage.wrapping_add((tpage_step as u16) << 5),
        clut: if mode == 2 {
            INTRO_QUAD_MODE2_CLUT
        } else {
            desc.clut
        },
        verts: [
            IntroQuadVertex {
                x: x0,
                y: y0,
                u: u0,
                v: v0,
                rgb: top,
            },
            IntroQuadVertex {
                x: x1,
                y: y0,
                u: u1,
                v: v1,
                rgb: top,
            },
            IntroQuadVertex {
                x: x0,
                y: y1,
                u: u2,
                v: v2,
                rgb: bottom,
            },
            IntroQuadVertex {
                x: x1,
                y: y1,
                u: u3,
                v: v3,
                rgb: bottom,
            },
        ],
        ot_depth,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn globals() -> TransitionGlobals {
        TransitionGlobals {
            battle_id: 4,
            total_duration: 200,
            ..Default::default()
        }
    }

    #[test]
    fn phase_zero_fires_the_cue_and_advances() {
        let mut e = TransitionEntity::default();
        let g = TransitionGlobals {
            bgm_voice_id: 0x12,
            audio_level: 4,
            ..globals()
        };
        let tick = tick_transition(&mut e, &g, &TransitionResponses::default());
        assert_eq!(e.phase, 1);
        assert_eq!(
            tick.effects,
            vec![
                TransitionEffect::SetAudioCue {
                    cue: AUDIO_CUE_FLAGGED
                },
                TransitionEffect::ReapplyBgmLevel {
                    voice_id: 0x12,
                    // 4 << 15 == 0x20000; >> 16 == 2 - the retail halve into
                    // libsnd's 0..0x7F volume range.
                    level: 2
                }
            ]
        );
    }

    #[test]
    fn phase_zero_sentinel_id_arms_the_cue_then_advances_without_playing() {
        let mut e = TransitionEntity::default();
        let g = TransitionGlobals {
            battle_id: -1,
            battle_flags: 0x80,
            ..globals()
        };
        let tick = tick_transition(&mut e, &g, &TransitionResponses::default());
        assert_eq!(e.phase, 1);
        assert_eq!(
            tick.effects,
            vec![
                TransitionEffect::SetAudioCue {
                    cue: AUDIO_CUE_PLAIN
                },
                TransitionEffect::SetAudioCue {
                    cue: AUDIO_CUE_FLAGGED
                },
            ],
            "the delay-slot store lands, then the flagged one overwrites it"
        );
    }

    #[test]
    fn phase_zero_other_negative_id_does_nothing_but_advance() {
        let mut e = TransitionEntity::default();
        let g = TransitionGlobals {
            battle_id: -5,
            ..globals()
        };
        let tick = tick_transition(&mut e, &g, &TransitionResponses::default());
        assert_eq!(e.phase, 1);
        assert!(tick.effects.is_empty(), "blez arm skips both stores");
    }

    #[test]
    fn phase_one_holds_until_assembly_reports_0x80() {
        let mut e = TransitionEntity {
            phase: 1,
            ..Default::default()
        };
        let tick = tick_transition(&mut e, &globals(), &TransitionResponses::default());
        assert_eq!(e.phase, 1, "holds");
        assert_eq!(tick.effects, vec![TransitionEffect::AssembleBattleMeshes]);

        let g = TransitionGlobals {
            assembly_state: 0x80,
            ..globals()
        };
        let tick = tick_transition(&mut e, &g, &TransitionResponses::default());
        assert_eq!(e.phase, 2);
        assert_eq!(
            tick.effects,
            vec![
                TransitionEffect::AssembleBattleMeshes,
                TransitionEffect::WaitForLoad
            ]
        );
    }

    #[test]
    fn phase_two_bundle_id_arms() {
        let mut e = TransitionEntity {
            phase: 2,
            ..Default::default()
        };
        let tick = tick_transition(&mut e, &globals(), &TransitionResponses::default());
        assert!(tick.effects.contains(&TransitionEffect::LoadBattleBundle {
            bundle_id: 0x36F + 4,
            flags: BATTLE_BUNDLE_FLAGS,
        }));

        // battle_id 0 with the alt flag set picks 0x370.
        let mut e = TransitionEntity {
            phase: 2,
            ..Default::default()
        };
        let g = TransitionGlobals {
            battle_id: 0,
            alt_default_bundle: 1,
            ..globals()
        };
        let tick = tick_transition(&mut e, &g, &TransitionResponses::default());
        assert!(tick.effects.contains(&TransitionEffect::LoadBattleBundle {
            bundle_id: 0x370,
            flags: BATTLE_BUNDLE_FLAGS,
        }));

        // A negative id loads the BGM but no bundle.
        let mut e = TransitionEntity {
            phase: 2,
            ..Default::default()
        };
        let g = TransitionGlobals {
            battle_id: -3,
            ..globals()
        };
        let tick = tick_transition(&mut e, &g, &TransitionResponses::default());
        assert_eq!(tick.effects.len(), 1);
        assert_eq!(e.phase, 3);
    }

    #[test]
    fn wait_phases_hold_while_busy() {
        for phase in [3i16, 6] {
            let mut e = TransitionEntity {
                phase,
                ..Default::default()
            };
            tick_transition(&mut e, &globals(), &TransitionResponses { load_busy: 1 });
            assert_eq!(e.phase, phase);
            tick_transition(&mut e, &globals(), &TransitionResponses::default());
            assert_eq!(e.phase, phase + 1);
        }
    }

    #[test]
    fn phase_seven_sets_ready_bit_and_does_not_advance() {
        let mut e = TransitionEntity {
            phase: 7,
            ..Default::default()
        };
        tick_transition(&mut e, &globals(), &TransitionResponses::default());
        assert_eq!(e.phase, 7);
        assert_eq!(e.ready, 2);
    }

    #[test]
    fn out_of_range_phase_runs_no_body() {
        let mut e = TransitionEntity {
            phase: 9,
            ..Default::default()
        };
        let tick = tick_transition(&mut e, &globals(), &TransitionResponses::default());
        assert_eq!(e.phase, 9);
        assert!(tick.effects.is_empty());
    }

    #[test]
    fn ready_bit_zero_raised_near_the_end_of_the_spin() {
        let g = TransitionGlobals {
            total_duration: 100,
            ..globals()
        };
        let mut e = TransitionEntity {
            phase: 4,
            elapsed: 70,
            ..Default::default()
        };
        tick_transition(&mut e, &g, &TransitionResponses::default());
        assert_eq!(e.ready, 0, "100 - 0x1E == 70, not < 70");

        e.elapsed = 71;
        tick_transition(&mut e, &g, &TransitionResponses::default());
        assert_eq!(e.ready, 1);
    }

    /// The entity state the handoff tail needs: rested at phase 7 (ready bit
    /// 1) with the clock past the duration (ready bit 0).
    fn at_completion(total: i32, elapsed: i16) -> TransitionEntity {
        assert!(i32::from(elapsed) > total, "test setup must clear the gate");
        TransitionEntity {
            phase: 7,
            elapsed,
            ready: 3,
        }
    }

    #[test]
    fn the_handoff_needs_both_ready_bits_not_just_either() {
        let g = TransitionGlobals {
            total_duration: 100,
            ..globals()
        };
        // Bit 1 without bit 0: rested at phase 7, but the spin clock has not
        // passed `total - 0x1E`, so the post-switch never raises bit 0.
        let mut e = TransitionEntity {
            phase: 7,
            elapsed: 50,
            ..Default::default()
        };
        let tick = tick_transition(&mut e, &g, &TransitionResponses::default());
        assert_eq!(e.ready, 2);
        assert!(!tick.entered_battle_mode);

        // Bit 0 without bit 1: the clock is spent but the machine is still
        // walking the phases, so phase 7's arm has not run.
        let mut e = TransitionEntity {
            phase: 4,
            elapsed: 200,
            ..Default::default()
        };
        let tick = tick_transition(&mut e, &g, &TransitionResponses::default());
        assert_eq!(e.ready, 1);
        assert!(!tick.entered_battle_mode);

        // Both: the tail reads `ready` *after* the switch and the post-switch
        // OR, so a phase-7 tick with a spent clock raises both bits and
        // completes inside the same tick rather than on the next one.
        let mut e = TransitionEntity {
            phase: 7,
            elapsed: 200,
            ..Default::default()
        };
        let tick = tick_transition(&mut e, &g, &TransitionResponses::default());
        assert_eq!(e.ready, 3);
        assert!(tick.entered_battle_mode);
    }

    #[test]
    fn the_mode_write_lands_even_for_a_negative_battle_id() {
        // The store is in the delay slot of the id test, so only the audio
        // handoff behind it is skipped.
        let g = TransitionGlobals {
            total_duration: 100,
            battle_id: -1,
            ..globals()
        };
        let mut e = at_completion(100, 200);
        let tick = tick_transition(&mut e, &g, &TransitionResponses::default());
        assert!(tick.entered_battle_mode);
        assert!(
            !tick
                .effects
                .iter()
                .any(|f| matches!(f, TransitionEffect::StartBattleBgmVoice)),
            "a negative id skips the whole audio handoff"
        );
    }

    #[test]
    fn the_bgm_swap_runs_in_retail_order() {
        let g = TransitionGlobals {
            total_duration: 100,
            battle_id: 4,
            bundle_handle: 0xDEAD_BEEF,
            ..globals()
        };
        let mut e = at_completion(100, 200);
        let tick = tick_transition(&mut e, &g, &TransitionResponses::default());
        assert_eq!(
            tick.effects,
            vec![
                TransitionEffect::StopFieldBgmVoice,
                TransitionEffect::ReleaseFieldBgmSlot,
                TransitionEffect::InstallBattleSequence {
                    bundle_handle: 0xDEAD_BEEF
                },
                TransitionEffect::StartBattleBgmVoice,
            ],
            "stop the field voice, release its slot, install the battle SEQ, start it"
        );
    }

    #[test]
    fn the_field_voice_stops_six_frames_before_the_handoff() {
        let g = TransitionGlobals {
            total_duration: 100,
            battle_id: 0,
            ..globals()
        };
        // 100 - 6 == 94: not `< 94`, so nothing yet.
        let mut e = TransitionEntity {
            phase: 7,
            elapsed: 94,
            ready: 3,
        };
        let tick = tick_transition(&mut e, &g, &TransitionResponses::default());
        assert!(tick.effects.is_empty());
        // One frame later the stop fires, and the mode write still does not.
        let mut e = TransitionEntity {
            phase: 7,
            elapsed: 95,
            ready: 3,
        };
        let tick = tick_transition(&mut e, &g, &TransitionResponses::default());
        assert_eq!(tick.effects, vec![TransitionEffect::StopFieldBgmVoice]);
        assert!(!tick.entered_battle_mode);
    }

    #[test]
    fn one_formation_resets_the_battle_id_and_the_others_do_not() {
        let mk = |slot0: u8| {
            let g = TransitionGlobals {
                total_duration: 100,
                battle_id: 4,
                formation_slot0: slot0,
                ..globals()
            };
            let mut e = at_completion(100, 200);
            tick_transition(&mut e, &g, &TransitionResponses::default()).cleared_battle_id
        };
        assert!(mk(FORMATION_RESET_BATTLE_ID));
        assert!(!mk(FORMATION_RESET_BATTLE_ID.wrapping_sub(1)));
        assert!(!mk(0));
    }

    fn desc() -> IntroQuadDesc {
        IntroQuadDesc {
            size_q12: 0x1000,
            tpage: 0x0020,
            clut: 0x1234,
            u0: 8,
            v0: 16,
            w: 32,
            h: 64,
            top: [0xFF, 0x80, 0x00],
            abr: 1,
            bottom: [0x00, 0x40, 0xFF],
            tpage_step: 3,
        }
    }

    fn req(anchor: IntroQuadAnchor, x: i16, y: i16, key: i32, intensity: i32) -> IntroQuadRequest {
        IntroQuadRequest {
            anchor,
            x,
            y,
            key,
            intensity,
            scale_x: 0x1000,
            scale_y: 0x1000,
            ot_depth: 3,
        }
    }

    #[test]
    fn quad_top_left_anchor_and_uvs() {
        let table = [desc()];
        let q = build_intro_quad(
            &IntroQuadRequest {
                ot_depth: 5,
                ..req(IntroQuadAnchor::TopLeft, 10, 20, 0, 0x100)
            },
            &table,
        )
        .unwrap();
        assert_eq!((q.verts[0].x, q.verts[0].y), (10, 20));
        assert_eq!((q.verts[3].x, q.verts[3].y), (42, 84));
        assert_eq!((q.verts[0].u, q.verts[0].v), (8, 16));
        assert_eq!((q.verts[1].u, q.verts[1].v), (40, 16));
        assert_eq!((q.verts[2].u, q.verts[2].v), (8, 80));
        assert_eq!((q.verts[3].u, q.verts[3].v), (40, 80));
        assert_eq!(q.ot_depth, 5);
    }

    #[test]
    fn quad_centre_anchor_halves_the_extents() {
        let table = [desc()];
        let q =
            build_intro_quad(&req(IntroQuadAnchor::Centre, 100, 100, 0, 0x100), &table).unwrap();
        assert_eq!((q.verts[0].x, q.verts[0].y), (84, 68));
        assert_eq!((q.verts[3].x, q.verts[3].y), (116, 132));
    }

    #[test]
    fn quad_gradient_is_top_then_bottom() {
        let table = [desc()];
        let q = build_intro_quad(&req(IntroQuadAnchor::TopLeft, 0, 0, 0, 0x80), &table).unwrap();
        // Half intensity.
        assert_eq!(q.verts[0].rgb, [0x7F, 0x40, 0x00]);
        assert_eq!(q.verts[1].rgb, q.verts[0].rgb);
        assert_eq!(q.verts[2].rgb, [0x00, 0x20, 0x7F]);
        assert_eq!(q.verts[3].rgb, q.verts[2].rgb);
    }

    #[test]
    fn mode_zero_uses_descriptor_abr_and_step() {
        let table = [desc()];
        let q = build_intro_quad(&req(IntroQuadAnchor::TopLeft, 0, 0, 0, 0x100), &table).unwrap();
        assert_eq!(q.code, (1 << 1) | POLY_GT4_CODE);
        assert_eq!(q.tpage, 0x20 + (3 << 5));
        assert_eq!(q.clut, 0x1234);
    }

    #[test]
    fn non_zero_mode_forces_abr_one_and_uses_mode_as_step() {
        let table = [desc()];
        // mode 2: abr forced to 1, step 2, CLUT replaced.
        let q =
            build_intro_quad(&req(IntroQuadAnchor::TopLeft, 0, 0, 2 << 10, 0x100), &table).unwrap();
        assert_eq!(q.code, (1 << 1) | POLY_GT4_CODE);
        assert_eq!(q.tpage, 0x20 + (2 << 5));
        assert_eq!(q.clut, INTRO_QUAD_MODE2_CLUT);
    }

    #[test]
    fn desc_parse_round_trips_field_offsets() {
        let mut raw = [0u8; INTRO_QUAD_DESC_STRIDE];
        raw[0..4].copy_from_slice(&0x1000i32.to_le_bytes());
        raw[4..6].copy_from_slice(&0x0020u16.to_le_bytes());
        raw[6..8].copy_from_slice(&0x1234u16.to_le_bytes());
        raw[8] = 8;
        raw[9] = 16;
        raw[0x0a] = 32;
        raw[0x0b] = 64;
        raw[0x0c..0x0f].copy_from_slice(&[0xFF, 0x80, 0x00]);
        raw[0x0f] = 1;
        raw[0x10..0x13].copy_from_slice(&[0x00, 0x40, 0xFF]);
        raw[0x13] = 3;
        assert_eq!(IntroQuadDesc::parse(&raw), Some(desc()));
        assert_eq!(IntroQuadDesc::parse(&raw[..8]), None);
    }
}
