//! The battle's **near-camera ghost pass** - `FUN_8004DC68`, run once per
//! battle frame by the frame driver `FUN_80046A20` (`jal` at `0x80047124`,
//! behind the `gp[+0x330]` sign gate, just ahead of the tint SM
//! `FUN_80050120`).
//!
//! Every write it makes is to the pool actor's `+0x8` word, and only to the
//! bits `0x83000000` ([`GHOST_BITS`]): set them (`lui v1,0x8300 ; or`) or
//! clear them (`lui v1,0x7cff ; ori 0xffff ; and`). The tint pass
//! `FUN_8004A908` copies `+0x8 & 0xFF000000` into the top byte of the render
//! node's `+0x74` colour word (`0x8004AA44..0x8004AA50`), which is the draw's
//! mode byte: bit 31 raises semi-transparency and bits 24/25 select the blend
//! rule, so `0x83` draws the body with PSX blend mode `3` (`B + F/4`) - a faint
//! ghost of itself.
//!
//! Which bodies go ghost (see `ghidra/scripts/funcs/8004dc68.txt`):
//!
//! 1. **Near the camera** (`0x8004DCA8..0x8004DED8`). The pass forms a point
//!    `P` on the camera's view axis - the focus trio (`0x80089118` /
//!    `0x80089120`, stored negated) pulled back by `dist * 25 / 128` along the
//!    yaw (`0x8007B792`), `dist` being the eye-space depth `0x800840C0` - and,
//!    for each of pool slots `0..=6` carrying a record (`+0x22C != 0`),
//!    measures `|dx| * |sin b| + |dz| * |cos b|` with `b` the bearing
//!    `FUN_80019B28` returns from `P` to the actor, plus a half turn. That sum
//!    is the planar distance `|P - actor|`; within `dist / 4` the body is set,
//!    beyond it cleared. A set body is cleared again when it is the acting
//!    actor, when the command flow byte `ctx[+6]` is below `0x1F` or one of
//!    `0x32` / `0x6E` / `0xFE`, when the action state `ctx[+7]` is below
//!    `0x0B`, and - for the acting actor's target `+0x1DD` - also when
//!    `ctx[+6]` is `0x64` / `0x65`, or `0xFF` with the actor's category
//!    `+0x1DE` in `1..=3`. The target's other test falls through into the
//!    general one; nothing sets bits in this loop past those gates.
//! 2. **Whole-side scopes** (`0x8004DEE0..0x8004DF68`): a target byte of `8` or
//!    more clears the party (unless it is `9`) and the monsters (unless it is
//!    `8`).
//! 3. **Nothing ghosts** (`0x8004DF6C..0x8004DFFC`) while the actor runs
//!    (category `5`), on a pre-emptive round (`ctx[+0x290] == 1`), in action
//!    state `0x0B`, or once the battle has ended (`0x8007BD71 == 0xFE`) with
//!    `ctx[+0x26B]` raised: all seven slots are cleared.
//! 4. **Target selection** (`0x8004E000..0x8004E10C`, action states
//!    `0x28..=0x2E`): the acting side's whole row is set - slots `0..=2` for
//!    a party actor, `3..=6` for a monster - then the actor and its target
//!    (when below `8`) are cleared. The allies fade while the actor picks, and
//!    the one it picks stays solid.

/// The `+0x8` bits the pass owns.
pub const GHOST_BITS: u32 = 0x8300_0000;

/// One pool slot as the pass reads it (retail slot numbering: party `0..=2`,
/// monsters `3..=6`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct GhostSlot {
    /// `+0x22C != 0` - the slot carries a battle record.
    pub present: bool,
    /// `+0x34` world X.
    pub x: i16,
    /// `+0x38` world Z.
    pub z: i16,
    /// `+0x8` - the flag word the pass edits.
    pub flag_word: u32,
}

/// The camera and battle-context reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct GhostInputs {
    /// `0x8007B792` - the camera yaw, 12-bit units.
    pub yaw: u16,
    /// The camera focus X/Z, **un-negated** (retail stores the negation at
    /// `0x80089118` / `0x80089120`).
    pub focus_x: i32,
    pub focus_z: i32,
    /// `0x800840C0` - the eye-space depth.
    pub dist: i32,
    /// `ctx[+0x13]` - the acting slot.
    pub acting: u8,
    /// The acting actor's `+0x1DD` target byte.
    pub target: u8,
    /// The acting actor's `+0x1DE` category.
    pub category: u8,
    /// `ctx[+6]` - the command-flow byte.
    pub flow: u8,
    /// `ctx[+7]` - the action state.
    pub state: u8,
    /// `ctx[+0x290]`.
    pub formation: u8,
    /// `0x8007BD71` - the battle running / ended signal.
    pub battle_end: u8,
    /// `ctx[+0x26B]`.
    pub ctx_26b: u8,
}

fn clear(s: &mut GhostSlot) {
    s.flag_word &= !GHOST_BITS;
}

fn set(s: &mut GhostSlot) {
    s.flag_word |= GHOST_BITS;
}

/// Run the pass over pool slots `0..=6` (`slots` shorter than seven is
/// walked as far as it goes). `sin` / `cos` are the `_DAT_8007B81C` /
/// `_DAT_8007B7F8` tables (`1 << 12` fixed point); `bearing` is
/// `FUN_80019B28(a0 = p1z, a1 = p1x, a2 = p2z, a3 = p2x)`.
///
/// PORT: FUN_8004DC68
pub fn camera_ghost_pass(
    i: &GhostInputs,
    slots: &mut [GhostSlot],
    sin: impl Fn(i32) -> i32,
    cos: impl Fn(i32) -> i32,
    bearing: impl Fn(i32, i32, i32, i32) -> u16,
) {
    let n = slots.len().min(7);
    // 0x8004DCA8..0x8004DD4C: P, with `(dist * trig) * 25 >> 19`.
    let back = i32::from(i.yaw.wrapping_neg() & 0xFFF);
    let px = i.focus_x - (i.dist.wrapping_mul(sin(back)).wrapping_mul(25) >> 19);
    let pz = i.focus_z - (i.dist.wrapping_mul(cos(back)).wrapping_mul(25) >> 19);
    let radius = i.dist >> 2;
    for (k, s) in slots.iter_mut().enumerate().take(n) {
        if !s.present {
            continue;
        }
        let b = (i32::from(bearing(pz, px, i32::from(s.z), i32::from(s.x))) + 0x800) & 0xFFF;
        let dx = (px - i32::from(s.x)).abs();
        let dz = (pz - i32::from(s.z)).abs();
        let reach = ((dx * sin(b)) >> 12).abs() + ((dz * cos(b)) >> 12).abs();
        if radius < reach {
            clear(s);
            continue;
        }
        set(s);
        let k = k as u8;
        let target_cleared = k == i.target
            && (matches!(i.flow, 0x64 | 0x65) || (i.flow == 0xFF && (1..=3).contains(&i.category)));
        let keep = k != i.acting
            && !target_cleared
            && i.flow >= 0x1F
            && !matches!(i.flow, 0x6E | 0x32 | 0xFE)
            && i.state >= 0x0B;
        if !keep {
            clear(s);
        }
    }
    if i.target >= 8 {
        if i.target != 9 {
            slots.iter_mut().take(n.min(3)).for_each(clear);
        }
        if i.target != 8 {
            slots.iter_mut().take(n).skip(3).for_each(clear);
        }
    }
    if i.category == 5
        || i.formation == 1
        || i.state == 0x0B
        || (i.battle_end == 0xFE && i.ctx_26b != 0)
    {
        slots.iter_mut().take(n).for_each(clear);
    }
    if (0x28..=0x2E).contains(&i.state) {
        let row = if i.acting < 3 { 0..3 } else { 3..7 };
        for k in row {
            if let Some(s) = slots.get_mut(k) {
                set(s);
            }
        }
        if let Some(s) = slots.get_mut(usize::from(i.acting)) {
            clear(s);
        }
        if i.target < 8
            && let Some(s) = slots.get_mut(usize::from(i.target))
        {
            clear(s);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sin(a: i32) -> i32 {
        (((a as f64) * std::f64::consts::TAU / 4096.0).sin() * 4096.0) as i32
    }
    fn cos(a: i32) -> i32 {
        sin(a + 0x400)
    }
    fn bearing(p1z: i32, p1x: i32, p2z: i32, p2x: i32) -> u16 {
        crate::battle_action::bearing_12bit_approx(p1z as i16, p1x as i16, p2z as i16, p2x as i16)
    }

    fn inputs() -> GhostInputs {
        GhostInputs {
            yaw: 0,
            focus_x: 0,
            focus_z: 0,
            dist: 4000,
            acting: 0,
            target: 3,
            category: 3,
            flow: 0x40,
            state: 0x20,
            formation: 0,
            battle_end: 0xFF,
            ctx_26b: 0,
        }
    }

    fn slots_at(pos: &[(i16, i16)]) -> Vec<GhostSlot> {
        pos.iter()
            .map(|&(x, z)| GhostSlot {
                present: true,
                x,
                z,
                flag_word: 0,
            })
            .collect()
    }

    #[test]
    fn only_a_body_near_the_view_point_ghosts() {
        let i = inputs();
        // P sits `dist * 25 / 128` back along the yaw from the focus: at yaw
        // 0 that is (0, -781). Slot 1 stands on it, slot 2 far off.
        let mut s = slots_at(&[(0, -800), (0, -760), (2000, 2000)]);
        camera_ghost_pass(&i, &mut s, sin, cos, bearing);
        // Slot 0 is the acting actor: never ghosted.
        assert_eq!(s[0].flag_word, 0);
        assert_eq!(s[1].flag_word, GHOST_BITS);
        assert_eq!(s[2].flag_word, 0);
    }

    #[test]
    fn early_flow_states_and_runs_ghost_nothing() {
        for i in [
            GhostInputs {
                flow: 0x10,
                ..inputs()
            },
            GhostInputs {
                state: 0x05,
                ..inputs()
            },
            GhostInputs {
                category: 5,
                ..inputs()
            },
        ] {
            let mut s = slots_at(&[(500, 0), (0, -760)]);
            s[1].flag_word = 0x0012_3456 | GHOST_BITS;
            camera_ghost_pass(&i, &mut s, sin, cos, bearing);
            assert_eq!(s[1].flag_word, 0x0012_3456, "low bits survive the clear");
        }
    }

    #[test]
    fn target_selection_ghosts_the_actors_allies_and_keeps_the_target_solid() {
        let i = GhostInputs {
            state: 0x28,
            acting: 1,
            target: 4,
            ..inputs()
        };
        let far = (3000, 3000);
        let mut s = slots_at(&[far; 7]);
        camera_ghost_pass(&i, &mut s, sin, cos, bearing);
        let ghosted: Vec<bool> = s.iter().map(|s| s.flag_word == GHOST_BITS).collect();
        assert_eq!(ghosted, [true, false, true, false, false, false, false]);
    }

    #[test]
    fn a_whole_side_scope_clears_that_side() {
        let i = GhostInputs {
            target: 9,
            acting: 5,
            ..inputs()
        };
        let mut s = slots_at(&[(0, -760); 7]);
        camera_ghost_pass(&i, &mut s, sin, cos, bearing);
        // Scope 9 keeps the party's bits and clears the monsters'.
        assert!(s[..3].iter().all(|s| s.flag_word == GHOST_BITS));
        assert!(s[3..].iter().all(|s| s.flag_word == 0));
    }
}
