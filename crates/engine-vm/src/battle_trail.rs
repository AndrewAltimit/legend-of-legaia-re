//! Weapon-trail trigger + sweep/band schedule - the simulation half of the
//! swept `POLY_G4` weapon trail ordinary arts swings leave behind a party
//! character's blade.
//!
//! PORT: FUN_8005112C (per-character signature-clip trigger)
//! PORT: FUN_80048310 (sweep schedule + band colour ladder; the projected
//! quad emission half is `legaia_engine_ui::battle_trail`)
//!
//! # The retail chain
//!
//! The per-actor battle draw tick (`FUN_800480D8`) calls the trigger
//! `FUN_8005112C` for a party seat whose FX colour word is armed. The
//! trigger fires only while the **committed action record's `+0x77`
//! identity byte** (the same byte the equipment attach scan matches - see
//! `legaia_asset::monster_archive::MonsterAnimation::attach_key`) equals a
//! per-character constant, i.e. only during that character's hand-picked
//! swing clips. It then calls the sweep driver `FUN_80048310` with a
//! **base object index**, a point count of `3` and an RGB tint.
//!
//! The driver saves the anim cursor `actor[+0x68]`, and up to 16 times:
//! re-decodes the pose at the current cursor (`FUN_8004998C`), copies the
//! three control points' decoded positions (object origins `base..base+3`
//! out of the pose pool), and rewinds the cursor by `2 * record[+0x78]` -
//! i.e. **two display frames per sweep step** (`FUN_80047430` advances
//! `rate` per frame). The rewind stops at the start of the clip. With at
//! least two captured steps it emits gouraud bands between consecutive
//! steps ([`band_schedule`]) through the GTE quad emitter `FUN_800485BC`.
//!
//! # Engine mapping
//!
//! The engine does not re-decode poses at rewound cursors; it already keeps
//! the per-frame pose-history ring the after-image ghosts sample
//! (`engine-core`'s `BattleGhostFrame`, retail's own `FUN_80047430` ring).
//! Sweep step `k` = the pose `2k` frames ago, which is exactly the retail
//! rewind under a constant rate. The ring records each frame's committed
//! clip identity so the sweep stops at the clip boundary, the engine
//! equivalent of retail's cursor-underflow stop.

/// Number of control points every retail trigger passes (`a2 = 3` at all
/// four `FUN_8005112C` call sites). The emitter's scratch fits 4.
pub const TRAIL_POINTS: usize = 3;

/// Sweep-step budget (`slti v0,s0,0x10` at `0x80048360`).
pub const MAX_SWEEP_STEPS: usize = 16;

/// Display frames between consecutive sweep steps: the driver rewinds the
/// cursor by `2 * rate` per step (`0x800483E4..0x800483FC`) while the anim
/// tick advances `rate` per frame.
pub const SWEEP_FRAMES_PER_STEP: usize = 2;

/// One row of the retail trigger table (`FUN_8005112C`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TrailTrigger {
    /// Roster character id in retail's `DAT_8007BD10` space
    /// (1 = Vahn, 2 = Noa, 3 = Gala).
    pub character_id: u8,
    /// The committed record's `+0x77` identity byte that fires this row.
    pub attach_key: u8,
    /// First control-point object index (the weapon bone chain; the trail
    /// reads objects `base_part .. base_part + TRAIL_POINTS`).
    pub base_part: usize,
    /// Trail tint `[r, g, b]` (the retail `0x00RRGGBB` word split).
    pub rgb: [u8; 3],
}

/// The four retail trigger rows, verbatim from the `FUN_8005112C` ladder:
/// Vahn fires on clip `0x29` (base object `0x0C`, red `0x802040`), Noa on
/// `0x1E` (base `0x04`, pale green `0x80FFC0`) and `0x2A` (base `0x0A`,
/// green `0x208040`), Gala on `0x64` (base `0x06`, blue `0x204080`).
pub const TRAIL_TRIGGERS: [TrailTrigger; 4] = [
    TrailTrigger {
        character_id: 1,
        attach_key: 0x29,
        base_part: 0x0C,
        rgb: [0x80, 0x20, 0x40],
    },
    TrailTrigger {
        character_id: 2,
        attach_key: 0x1E,
        base_part: 0x04,
        rgb: [0x80, 0xFF, 0xC0],
    },
    TrailTrigger {
        character_id: 2,
        attach_key: 0x2A,
        base_part: 0x0A,
        rgb: [0x20, 0x80, 0x40],
    },
    TrailTrigger {
        character_id: 3,
        attach_key: 0x64,
        base_part: 0x06,
        rgb: [0x20, 0x40, 0x80],
    },
];

/// Look up the trigger row for a character playing a clip with the given
/// `+0x77` identity byte. `None` = no trail this clip (the common case).
pub fn trail_trigger(character_id: u8, attach_key: u8) -> Option<&'static TrailTrigger> {
    TRAIL_TRIGGERS
        .iter()
        .find(|t| t.character_id == character_id && t.attach_key == attach_key)
}

/// One gouraud band between sweep steps `seg` and `seg + 1`: the leading
/// edge (step `seg`) draws `lead`, the trailing edge `tail`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TrailBand {
    pub seg: usize,
    pub lead: [u8; 3],
    pub tail: [u8; 3],
}

/// The retail band emission order for a sweep of `steps` captured poses
/// (`FUN_80048310` `0x80048418..0x8004858C`), `rgb` = the trigger tint.
/// Empty below two steps (`slti v0,s0,0x2` at `0x8004840C`).
///
/// With `n = steps - 1` segments, retail emits - in this order, all
/// semi-transparent so they stack additively:
///
/// 1. segment 0 white -> `0x808080` (the hot leading edge),
/// 2. segment 1 `0x7F7F7F` -> black,
/// 3. every segment `k` in `0..n` with the tint faded linearly along the
///    sweep: `rgb * (n-k)/n -> rgb * (n-k-1)/n` (per channel, truncating
///    division - the exact `mult`/`divu` ladder at `0x8004847C..0x80048534`).
///
/// One divergence, disclosed: retail emits the segment-1 band even for a
/// two-step sweep, where step 2 was never captured this call - it reads
/// whatever a previous sweep left in the scratch buffer. The port skips a
/// band whose far edge was not captured instead of reproducing a stale
/// read.
pub fn band_schedule(steps: usize, rgb: [u8; 3]) -> Vec<TrailBand> {
    if steps < 2 {
        return Vec::new();
    }
    let n = steps - 1;
    let mut out = Vec::with_capacity(n + 2);
    out.push(TrailBand {
        seg: 0,
        lead: [0xFF; 3],
        tail: [0x80; 3],
    });
    if steps > 2 {
        out.push(TrailBand {
            seg: 1,
            lead: [0x7F; 3],
            tail: [0x00; 3],
        });
    }
    let scale = |m: usize| rgb.map(|c| ((c as usize * m) / n) as u8);
    for k in 0..n {
        out.push(TrailBand {
            seg: k,
            lead: scale(n - k),
            tail: scale(n - k - 1),
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_trigger_table_is_the_fun_8005112c_ladder() {
        // Vahn's single row.
        let v = trail_trigger(1, 0x29).expect("Vahn 0x29");
        assert_eq!((v.base_part, v.rgb), (0x0C, [0x80, 0x20, 0x40]));
        // Noa has two rows - the 0x1E arm is a plain `jal` (not a tail
        // call), so both clips fire independently.
        let n1 = trail_trigger(2, 0x1E).expect("Noa 0x1E");
        assert_eq!((n1.base_part, n1.rgb), (0x04, [0x80, 0xFF, 0xC0]));
        let n2 = trail_trigger(2, 0x2A).expect("Noa 0x2A");
        assert_eq!((n2.base_part, n2.rgb), (0x0A, [0x20, 0x80, 0x40]));
        let g = trail_trigger(3, 0x64).expect("Gala 0x64");
        assert_eq!((g.base_part, g.rgb), (0x06, [0x20, 0x40, 0x80]));
        // No cross-talk: another character's clip id fires nothing.
        assert!(trail_trigger(1, 0x1E).is_none());
        assert!(trail_trigger(3, 0x29).is_none());
        // Terra (4) has no arm in the ladder.
        assert!(trail_trigger(4, 0x29).is_none());
    }

    #[test]
    fn a_short_sweep_emits_nothing() {
        assert!(band_schedule(0, [0x80, 0x20, 0x40]).is_empty());
        assert!(band_schedule(1, [0x80, 0x20, 0x40]).is_empty());
    }

    #[test]
    fn a_two_step_sweep_is_lead_band_plus_one_tint_band() {
        let bands = band_schedule(2, [0x80, 0x20, 0x40]);
        assert_eq!(
            bands,
            vec![
                TrailBand {
                    seg: 0,
                    lead: [0xFF; 3],
                    tail: [0x80; 3]
                },
                // n = 1: the tint band runs full tint -> zero.
                TrailBand {
                    seg: 0,
                    lead: [0x80, 0x20, 0x40],
                    tail: [0, 0, 0]
                },
            ]
        );
    }

    #[test]
    fn the_full_ladder_fades_the_tint_linearly_along_the_sweep() {
        let rgb = [0x80, 0x40, 0x20];
        let bands = band_schedule(5, rgb);
        // Two lead bands + n = 4 tint bands.
        assert_eq!(bands.len(), 6);
        assert_eq!(bands[0].seg, 0);
        assert_eq!(bands[1].seg, 1);
        // Tint bands cover segments 0..4, newest first and brightest first.
        let tint = &bands[2..];
        for (k, b) in tint.iter().enumerate() {
            assert_eq!(b.seg, k);
            let m = 4 - k;
            assert_eq!(b.lead, rgb.map(|c| ((c as usize * m) / 4) as u8));
            assert_eq!(b.tail, rgb.map(|c| ((c as usize * (m - 1)) / 4) as u8));
        }
        // The oldest band's far edge fades to black - the trail vanishes
        // rather than ending on a hard edge.
        assert_eq!(tint[3].tail, [0, 0, 0]);
        assert_eq!(tint[0].lead, rgb);
    }
}
