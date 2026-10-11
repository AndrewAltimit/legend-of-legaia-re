//! The impact effect pair, its keyframe index and the positional sprite blit.

// ---------------------------------------------------------------------------
// Impact effect pair, positional cue, special-attack afterimage
// ---------------------------------------------------------------------------

/// One effect-part spawn the impact pair emits: a world position, a Euler
/// rotation and the rodata VA of the template `FUN_80021B04` is handed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImpactSpawn {
    pub pos: (i16, i16, i16),
    pub rot: (i16, i16, i16),
    /// Rodata VA of the spawn template.
    pub template: u32,
    /// Fixed-point scale (`0x1000` = 1.0).
    pub scale: i32,
}

/// Templates the impact pair picks by slot: `[slot 0, slot 1]`, first the
/// un-rotated pair then the yawed pair.
pub const IMPACT_TEMPLATE_A: [u32; 2] = [0x801D_B8FC, 0x801D_B960];
pub const IMPACT_TEMPLATE_B: [u32; 2] = [0x801D_BBA4, 0x801D_BBD4];
/// Yaw the second spawn takes, by slot - the two sides mirror.
pub const IMPACT_YAW: [i16; 2] = [-0x400, 0x400];
/// Z lift applied unless the special latch `DAT_801DBF50` is up.
pub const IMPACT_Z_LIFT: i16 = 0x32;

// Called from the duel's booking arms (`crate::baka_fighter::BakaFight`'s
// `spawn_impact`, run by `tick_with_input` - the one duel tick the native
// window, the browser play page and the minigames page all drive). The
// spawns seat `crate::baka_impact_fx` parts, which the duel surface
// (`legaia_engine_minigame_scenes::baka_duel_scene`) draws.
/// PORT: FUN_801d4df8 - the per-slot **impact effect pair**.
///
/// Retail calls it on a decided exchange. It first zeroes the fighter's
/// `+0x38` accumulator (`&DAT_801dbff4[slot * 0xa8]`), then places both
/// spawns at the fighter's world position offset by the current action
/// keyframe's TRS: X **added** when the facing flag `&DAT_801dbfe4[slot]` is
/// set and **subtracted** when it is clear, Y added, and Z added with a
/// further [`IMPACT_Z_LIFT`] taken off unless the special latch
/// `DAT_801DBF50` is up. `reset_keyframe` forces the keyframe index to `0`
/// rather than the fighter's live `&DAT_801dc054[slot]`
/// ([`impact_keyframe_index`]).
///
/// Both spawns go through the shared part-spawn API at scale `0x1000`; the
/// first uses [`IMPACT_TEMPLATE_A`] with no rotation, the second
/// [`IMPACT_TEMPLATE_B`] with the slot's [`IMPACT_YAW`]. The two slots take
/// **opposite** yaws, which is what mirrors the effect across the arena.
///
/// It is an effect spawn, not an animation play: nothing here touches the
/// fighter's action id or frame cursor.
pub fn impact_effect_pair(
    slot: usize,
    actor_pos: (i16, i16, i16),
    keyframe_trs: (i16, i16, i16),
    facing_flag: bool,
    special_latch: bool,
) -> [ImpactSpawn; 2] {
    let s = slot & 1;
    let x = if facing_flag {
        actor_pos.0.wrapping_add(keyframe_trs.0)
    } else {
        actor_pos.0.wrapping_sub(keyframe_trs.0)
    };
    let y = actor_pos.1.wrapping_add(keyframe_trs.1);
    let mut z = actor_pos.2.wrapping_add(keyframe_trs.2);
    if !special_latch {
        z = z.wrapping_sub(IMPACT_Z_LIFT);
    }
    let pos = (x, y, z);
    [
        ImpactSpawn {
            pos,
            rot: (0, 0, 0),
            template: IMPACT_TEMPLATE_A[s],
            scale: 0x1000,
        },
        ImpactSpawn {
            pos,
            rot: (0, IMPACT_YAW[s], 0),
            template: IMPACT_TEMPLATE_B[s],
            scale: 0x1000,
        },
    ]
}

/// Which keyframe index the impact pair reads - `0` when the caller asks for
/// a reset, otherwise the fighter's live cursor `&DAT_801dc054[slot]`.
///
/// REF: FUN_801d4df8 (`0x801D4E54..0x801D4E5C`)
pub fn impact_keyframe_index(live_cursor: i32, reset_keyframe: bool) -> i32 {
    if reset_keyframe { 0 } else { live_cursor }
}

/// One VRAM-to-VRAM sprite blit: the source `RECT` the helper stacks, plus
/// the destination it hands `MoveImage`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpriteBlit {
    /// Source `RECT.x` (`0x801D6628`..`0x801D6638`).
    pub src_x: i16,
    /// Source `RECT.y` (`0x801D6644`..`0x801D664C`).
    pub src_y: i16,
    /// The rect's fixed `(w, h)` (`0x801D6610` / `0x801D6618`).
    pub size: (i16, i16),
    /// Destination `x`, the call's `a1` (`0x801D6624`..`0x801D6648`).
    pub dst_x: i32,
    /// Destination `y`, the call's `a2`.
    pub dst_y: i32,
}

/// Source-`x` base the table entry's first byte is added to.
pub const BLIT_SRC_X_BASE: i16 = 0x340;
/// Source-`y` base the table entry's second byte is added to.
pub const BLIT_SRC_Y_BASE: i16 = 0x80;
/// Destination `y` the blit always writes to.
pub const BLIT_DST_Y: i32 = 0x86;

// Called from the duel surface's VRAM build
// (`legaia_engine_minigame_scenes::baka_duel_scene::BakaDuelAssets::apply_wink`), for the row the
// cameo's frame leaves showing ([`CameoPose::blit_index`]); the surface bumps
// its generation when the row changes so each host re-uploads the edited
// VRAM.
/// PORT: FUN_801d65f8 - the **sprite-blit helper**.
///
/// Builds a VRAM source `RECT` out of the 4-byte record at
/// `&DAT_801dbe84 + index * 4`: byte `0` shifted right two and added to
/// [`BLIT_SRC_X_BASE`], byte `1` added to [`BLIT_SRC_Y_BASE`], with the
/// rect's `(w, h) = (6, 0x18)`, and hands it to
/// `FUN_80058490(&rect, 0x340, 0x86)`. That callee is **`MoveImage`** - it
/// validates the string `"MoveImage"` at `0x800156EC`, packs the destination
/// as `(y << 16) | x` and issues a VRAM-to-VRAM blit packet. An earlier
/// reading here made it a positional SFX play, so the two rect halves wore
/// `pitch` / `pan` names and the destination `y` wore `voice`; nothing in the
/// 49-instruction callee touches a sound path.
///
/// `mode` is the routine's first argument, and only `0` is a defined call:
/// the table pointer *and* the rect's `(w, h)` are written **only** inside
/// the `mode == 0` arm, so any other value reads the table through an
/// uninitialised register. Retail has no such call site; this port returns
/// `None` rather than inventing behaviour for it.
pub fn sprite_blit(mode: i32, entry: [u8; 2]) -> Option<SpriteBlit> {
    if mode != 0 {
        return None;
    }
    Some(SpriteBlit {
        src_x: (entry[0] >> 2) as i16 + BLIT_SRC_X_BASE,
        src_y: entry[1] as i16 + BLIT_SRC_Y_BASE,
        size: (6, 0x18),
        dst_x: BLIT_SRC_X_BASE as i32,
        dst_y: BLIT_DST_Y,
    })
}
