//! The bite roll and interval ladder: hit type, water tile class and the pad nudge.

// --- Bite roll and interval ladder (FUN_801D26CC) --------------------------

/// Bite cadence in frames while the debug readouts are up - the override
/// that makes the fish bite almost immediately.
pub const BITE_INTERVAL_DEBUG: i32 = 0x20;

/// Bite cadence for a cast **above** the ladder's only live threshold.
pub const BITE_INTERVAL_NEAR: i32 = 1000;

/// Bite cadence at exactly [`BITE_LADDER_PIVOT`] - the ladder's untouched
/// initial value.
pub const BITE_INTERVAL_PIVOT: i32 = 0x200;

/// Bite cadence below the pivot.
pub const BITE_INTERVAL_FAR: i32 = 2000;

/// The single distance the interval ladder actually discriminates on.
pub const BITE_LADDER_PIVOT: i32 = 200;

/// Strike credit the far band **replaces** the whole credit base with.
pub const BITE_FAR_CREDIT: i32 = -100;

/// Bite cadence for a cast metric of `distance` (`DAT_801D9280`).
///
/// This is the **modulus** of the per-frame strike roll: retail's
/// `(rand() % interval) < credit`, so a larger interval is a rarer bite.
///
/// The retail ladder is six `slti`/`bne` pairs writing the same register in
/// **ascending** threshold order, so every earlier arm is overwritten by a
/// later one that is true whenever it is. The four intermediate cadences
/// (`200`, `350`, `400`, `500`) are therefore unreachable: only the
/// `>= 201` arm, the `<= 199` arm and the untouched initial value survive.
/// The port reproduces the reachable behaviour and names the dead arms in
/// [`BITE_LADDER_DEAD_ARMS`] rather than pretending they run.
///
/// PORT: FUN_801d26cc (bite-interval ladder)
pub fn bite_interval(distance: i32, debug: bool) -> i32 {
    if debug {
        return BITE_INTERVAL_DEBUG;
    }
    if distance > BITE_LADDER_PIVOT {
        BITE_INTERVAL_NEAR
    } else if distance < BITE_LADDER_PIVOT {
        BITE_INTERVAL_FAR
    } else {
        BITE_INTERVAL_PIVOT
    }
}

/// The four `(threshold, cadence)` arms the ladder's write order makes
/// unreachable, kept so the dead range is documented rather than lost.
pub const BITE_LADDER_DEAD_ARMS: [(i32, i32); 4] = [(401, 200), (351, 350), (301, 400), (251, 500)];

/// The far band's strike-credit **override**, if it applies.
///
/// This rides on the same comparison as [`bite_interval`], and it is not a
/// bias: retail writes `li s1, -0x64` into the register that already holds
/// the credit base (`countdown + 2`, or `0x40` on a cadence match), so the
/// base is *replaced*, not offset. Everything added after the ladder - the
/// water-class bonus and the pad nudges - still lands on top of the `-100`,
/// which is why a shallow cast cannot strike at all: those add at most
/// `0x1E + 3`, far short of zero.
///
/// PORT: FUN_801d26cc (far-band credit override)
#[inline]
pub fn bite_credit_override(distance: i32) -> Option<i32> {
    (distance < BITE_LADDER_PIVOT).then_some(BITE_FAR_CREDIT)
}

/// Upper bound (inclusive) of each random band, most common first. A draw of
/// `rand() & 0xFFF` picks the **last** band whose bound it exceeds.
pub const HIT_TYPE_BANDS: [(u32, u8); 4] = [(0x0C00, 3), (0x0E70, 2), (0x0F38, 1), (0x0FFF, 0)];

/// Cast-band roll used when the scripted picker declines.
///
/// Retail seeds `3`, then overwrites with `2`, `1`, `0` as the draw passes
/// `0xC00`, `0xE70` and `0xF38`, so the bands are heavily skewed toward `3`
/// (3073/4096) and `0` is a 199-in-4096 tail.
///
/// PORT: FUN_801d26cc (hit-type roll)
pub fn roll_hit_type(draw: u32) -> u8 {
    let d = draw & 0xFFF;
    let mut band = 3;
    if d > 0x0C00 {
        band = 2;
    }
    if d > 0x0E70 {
        band = 1;
    }
    if d > 0x0F38 {
        band = 0;
    }
    band
}

/// Minimum cast metric below which the bite countdown is forced to zero
/// (`DAT_801D9280 < 100`).
pub const BITE_SUPPRESS_BELOW: i32 = 100;

/// Water-tile class flags read out of `_DAT_8007B8F4` after the walk-grid
/// probe reports the `0x4000` water bit, with the `(countdown bonus, weight)`
/// pair each one installs. Retail tests them in this order without `else`,
/// so the highest set bit wins.
pub const WATER_TILE_CLASSES: [(u32, i32, i32); 3] =
    [(0x04, 0x1E, 100), (0x08, 0x14, 300), (0x10, 0x14, 500)];

/// Resolve the water-tile class bonus for a probe result.
///
/// Returns `None` when no class bit is set, which leaves the countdown bonus
/// and the fish weight at their defaults (`0` and `10`).
///
/// PORT: FUN_801d26cc (water-tile class)
// Wired through [`LureActor::probe`] on both fishing hosts. Its input is the
// `_DAT_8007B8F4` class word, and the previous note here had that word's
// producer wrong: it is **not** a read taken "after the walk-grid probe
// reports the `0x4000` water bit". The water gate is bit `0x4000` of the
// `+0x8000` per-tile cell word (`andi v1,v1,0x4000` at `0x801D3374`, over a
// halfword loaded through the scratchpad scene pointer at `0x801D3330`), and
// the class word is then rebuilt by `FUN_800180EC` - the region-kind mask
// [`legaia_engine_field::field_regions::refresh_region_attributes`] already ports - called
// at `0x801D3384` with the same tile pair. `FUN_801D7030`'s probe is a
// separate read with a separate consequence.
pub fn water_tile_class(flags: u32) -> Option<(i32, i32)> {
    let mut got = None;
    for (bit, bonus, weight) in WATER_TILE_CLASSES {
        if flags & bit != 0 {
            got = Some((bonus, weight));
        }
    }
    got
}

/// Default `(countdown bonus, weight)` outside every water class.
pub const WATER_TILE_DEFAULT: (i32, i32) = (0, 10);

/// Pad bits that each shorten the bite countdown by one frame while held
/// (`_DAT_8007B874`): the two D-pad bits and the two shoulder bits, the
/// latter pair tested as one mask.
pub const BITE_NUDGE_MASKS: [u32; 3] = [0x8000, 0x2000, 0x00C0];

/// Count this frame's pad nudges into the strike credit.
///
/// Retail loads the newly-pressed word `_DAT_8007B874` once and adds one to
/// the credit register per mask that hits (`0x801D343C..0x801D3468`): D-pad
/// left, D-pad right, and the two reel bits tested together - so both reel
/// buttons pressed on one frame are one nudge, and the cast press is none.
///
/// PORT: FUN_801d26cc (pad nudge)
///
/// Wired on all three hosts: `World::tick_fishing` counts the play hosts'
/// nudge through it off the engine pad's newly-pressed edges, rotated into
/// the packed layout, and the minigames page's `fishing_pond_tick` export
/// takes the packed pressed word and counts through it too.
pub fn bite_pad_nudge(pad: u32) -> i32 {
    BITE_NUDGE_MASKS.iter().filter(|&&m| pad & m != 0).count() as i32
}
