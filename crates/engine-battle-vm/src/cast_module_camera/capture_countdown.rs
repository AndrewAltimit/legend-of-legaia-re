//! The **arm countdowns** of the capture-class bodies whose camera arms are
//! not ported - how long each arm of a monster's special holds.
//!
//! Every capture-class module keeps one countdown word in its own image, and
//! most arms open on the same gate: drain the word by a per-arm expression of
//! the scratchpad frame step `*(0x1F800393)`, store it back, and return busy
//! (`bgtz` to the epilogue) while it stays positive. The tick that sees it at
//! or below zero runs the arm's writes, re-arms the word and advances the
//! phase. So an arm's dwell is its seed over its drain, and the body's phase
//! chain - ported per body in [`crate::cast_arm_ticks`] - may only run on the
//! tick the gate lets through.
//!
//! The seven bodies [`super::capture_camera_director`] covers carry their
//! countdown with their camera. This table is the countdown alone, for the
//! bodies whose camera is not ported: without it each arm took one tick and a
//! special that holds the stage for seconds in retail was over in a handful
//! of frames.
//!
//! ## Units
//!
//! Three drain forms appear, and the multiplier is baked per arm:
//!
//! | form | per engine tick |
//! |---|---|
//! | `*(0x1F800393) * *(0x1F80037D)` | [`DRAIN_PRODUCT`] (`8`) |
//! | `*(0x1F800393) << 1` | [`DRAIN_DOUBLE`] (`2`) |
//! | `*(0x1F800393)` | [`DRAIN_STEP`] (`1`) |
//!
//! The engine ticks once per vsync, so the frame step is `1` and the speed
//! scalar `*(0x1F80037D)` is [`super::SPEED_SCALAR`]. The seeds are either
//! scalar multiples (`scalar << k`, stored pre-multiplied below) or literals.
//!
//! ## Measured against retail
//!
//! `docs/subsystems/cast-module.md` (*The fourteen, measured*) carries a
//! PCSX-Redux capture of each body's per-arm dwell in module ticks. Dividing
//! each seed below by the capture's own per-tick drain (the product read
//! `32` in those runs, the bare step `4`) reproduces every counted arm:
//! PROT 0941's `0xB9` `64, 32, 64` against `65, 32, 64`; PROT 0943's `0xB5`
//! `64, 32, 64, 32` against `65, 32, 64, 32`; PROT 0943 `0x40` / PROT 0944
//! `0x53` `8, 40, 8, 32` against `9, 40, 8, 32`; PROT 0950's `0xAB`
//! `64, 16, 64, 8, 40, 16` against `65, 21, 64, 8, 40, 15+`; PROT 0950's
//! `0x5A` arm 3 `32` against `32`; PROT 0940's `0x50` `3, 64, 16` against
//! `3, 64, 19`. The one-tick excess on a first counted arm is the capture
//! window opening a tick early; the arms that disagree by more wait on
//! something else as well (a walk, a victim's clip), which the body's own
//! port carries.
//!
//! PROT 0956's `0x71` is the same shape (`1, 33, 32, 32, 1` measured against
//! `32, 32, 32`). Not covered: PROT 0962's three bodies, whose arms do not
//! gate on a module countdown of this shape (`0xA3` counts a word **up** to
//! `0x41`; the others wait on the scene).
//!
//! Provenance: the disassembly of each body at slot-B base `0x801F69D8`
//! (`see ghidra/scripts/funcs/overlay_cast_<label>_<entry>_<va>.txt`); the
//! per-row comments cite the gate and the re-arm stores.

use super::{ModuleCountdown, SPEED_SCALAR};
use crate::cast_arm_ticks as arms;

/// Drain per engine tick of the `step * scalar` product form.
pub const DRAIN_PRODUCT: i32 = SPEED_SCALAR;
/// Drain per engine tick of the `step << 1` form.
pub const DRAIN_DOUBLE: i32 = 2;
/// Drain per engine tick of the bare `step` form.
pub const DRAIN_STEP: i32 = 1;

/// What an arm does to the countdown word on the tick it lets the body run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CountdownWrite {
    /// `sw <value>, countdown`.
    Set(i32),
    /// `countdown += <value>` - the re-arm adds to whatever residue the
    /// drain left, which is how a dwell carries a tick of slack forward.
    Add(i32),
}

/// One arm's countdown half.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArmCountdown {
    /// The module phase `ctx[+0x279]` the arm runs on.
    pub arm: u8,
    /// The per-tick drain of the arm's gate; `None` for an arm that runs
    /// ungated (it may still seed the word).
    pub drain: Option<i32>,
    /// The store the arm makes as it passes.
    pub write: Option<CountdownWrite>,
}

impl ArmCountdown {
    /// One pass of the arm's gate: drain, then report whether it holds (the
    /// word is still positive - the `bgtz` every gate here uses).
    pub fn holds(&self, cd: &mut ModuleCountdown) -> bool {
        match self.drain {
            Some(d) => {
                cd.0 -= d;
                cd.0 > 0
            }
            None => false,
        }
    }

    /// The arm passed (the body ran and moved the phase): its re-arm.
    pub fn pass(&self, cd: &mut ModuleCountdown) {
        match self.write {
            Some(CountdownWrite::Set(v)) => cd.0 = v,
            Some(CountdownWrite::Add(v)) => cd.0 += v,
            None => {}
        }
    }
}

const fn gate(arm: u8, drain: i32, write: Option<CountdownWrite>) -> ArmCountdown {
    ArmCountdown {
        arm,
        drain: Some(drain),
        write,
    }
}

const fn seed(arm: u8, value: i32) -> ArmCountdown {
    ArmCountdown {
        arm,
        drain: None,
        write: Some(CountdownWrite::Set(value)),
    }
}

const fn add(v: i32) -> Option<CountdownWrite> {
    Some(CountdownWrite::Add(v))
}

const S: i32 = SPEED_SCALAR;
const P: i32 = DRAIN_PRODUCT;

/// PROT 0940 `0x50` / `0xAE` (`0x801F78B8`, word `0x801F864C`): arm 0 seeds
/// `scalar * 12` (`0x801F7A7C`); arm 1's product gate (`0x801F7AA8`) adds the
/// literal `0x200` (`0x801F7B50`); arm 2 drains `step << 1` (`0x801F7B78`) -
/// shaking the caster off its seat by the shortfall each pass - and adds
/// `scalar << 6` (`0x801F7CE8`) past its `bgtz` (`0x801F7CCC`); arm 3's product
/// gate (`0x801F81DC`) latches `0xFF`.
pub const GLARE_DIVIDE_SPLIT: &[ArmCountdown] = &[
    seed(0, S * 12),
    gate(1, P, add(0x200)),
    gate(2, DRAIN_DOUBLE, add(S << 6)),
    gate(3, P, None),
];

/// PROT 0941 `0x51` (`0x801F730C`, word `0x801F83EC`): arm 0 seeds
/// `scalar << 5` (`0x801F7448`); arm 1 is the walk and drains nothing; arm 2's
/// gate (`0x801F7B9C`) adds `scalar << 6` (`0x801F7BEC`); arm 3's gate
/// (`0x801F7CEC`) latches `0xFF`.
pub const STEAL: &[ArmCountdown] = &[seed(0, S << 5), gate(2, P, add(S << 6)), gate(3, P, None)];

/// PROT 0941 `0xB9` (`0x801F6A04`, word `0x801F83EC`): seed `scalar << 8`
/// (`0x801F6B2C`); gates at `0x801F6BE8` / `6EAC` / `71C8` / `72B8` re-arm
/// `scalar << 7`, `scalar << 8`, `scalar << 6` (`0x801F6C34`, `714C`, `7278`).
pub const STEAL_SWEEP: &[ArmCountdown] = &[
    seed(0, S << 8),
    gate(1, P, add(S << 7)),
    gate(2, P, add(S << 8)),
    gate(3, P, add(S << 6)),
    gate(4, P, None),
];

/// PROT 0943 `0x40` (`0x801F6EF4`) and PROT 0944 `0x53` (`0x801F7470`) - the
/// two Curse bodies share the shape over their own words (`0x801F7A04`,
/// `0x801F8360`): seed `scalar << 5`, then product gates re-arming
/// `scalar * 0xA0`, `scalar << 5`, `scalar << 7` (PROT 0943: `0x801F7054`;
/// gates `0x801F718C` / `72AC` / `73E8` / `74E8`; re-arms `0x801F7274` /
/// `7370` / `74A8`).
pub const CURSE: &[ArmCountdown] = &[
    seed(0, S << 5),
    gate(1, P, add(S * 0xA0)),
    gate(2, P, add(S << 5)),
    gate(3, P, add(S << 7)),
    gate(4, P, None),
];

/// PROT 0943 `0xB5` (`0x801F6A04`, word `0x801F7A04`): the one body whose
/// drain is the bare step (`lbu 0x7f` / `subu` at `0x801F6B94`) and whose
/// seeds are literals - `0x100` (`0x801F6B04`), then `+0x80`, `+0x100`,
/// `+0x80` (`0x801F6BC4`, `6D90`, `6E40`).
pub const CURSE_MP_DRAIN: &[ArmCountdown] = &[
    seed(0, 0x100),
    gate(1, DRAIN_STEP, add(0x80)),
    gate(2, DRAIN_STEP, add(0x100)),
    gate(3, DRAIN_STEP, add(0x80)),
    gate(4, DRAIN_STEP, None),
];

/// PROT 0950 `0x5A` (`0x801F79F8`, word `0x801F86B0`): arm 1 is the walk;
/// arm 2 seeds `scalar << 7` (`0x801F7DD8`); arm 3's product gate
/// (`0x801F7F54`) is the strike. Arm 4 drains only a positive word
/// (`blez` at `0x801F80C0`), which arm 3 never re-arms, so it waits on the
/// victim's clip alone.
pub const ROLLING_FLARE: &[ArmCountdown] = &[seed(2, S << 7), gate(3, P, None)];

/// PROT 0950 `0xAB` (`0x801F6A24`, word `0x801F86B0`), fourteen arms: seed
/// `scalar << 8` (`0x801F6B44`), then a product gate on every arm, re-arming
/// as listed (arm 3 `0x801F6EE4` / `6F00`, arm 5 `0x801F7068` / `7084`, arm 6
/// `0x801F711C` / `7174`, arm 9 `0x801F7470` / `7494` among them).
pub const ROLLING_FLARE_SWEEP: &[ArmCountdown] = &[
    seed(0, S << 8),
    gate(1, P, add(S << 6)),
    gate(2, P, add(S << 8)),
    gate(3, P, add(S << 5)),
    gate(4, P, add(S * 0xA0)),
    gate(5, P, add(S << 6)),
    gate(6, P, add(S << 5)),
    gate(7, P, add(S * 24)),
    gate(8, P, add(S << 6)),
    gate(9, P, add(S << 6)),
    gate(10, P, add(S << 6)),
    gate(11, P, add(S << 6)),
    gate(12, P, add(S << 6)),
    gate(13, P, None),
];

/// PROT 0956 `0x71` (`0x801F7298`, word `0x801F86A0`): seed `scalar << 7`
/// (`0x801F7470`), product gates at `0x801F7578` / `76C8` / `7C24`, the
/// first two re-arming `scalar << 7` (`0x801F75F0`, `76FC`). Arm 3 then also
/// waits for its victim to settle before it latches `0xFF`.
pub const WATER_HAZARD: &[ArmCountdown] = &[
    seed(0, S << 7),
    gate(1, P, add(S << 7)),
    gate(2, P, add(S << 7)),
    gate(3, P, None),
];

/// PROT 0948 (Cross Beam `0x58`, `0x801F69F0`, word `0x801F8854`): arm 0
/// seeds `scalar << 6` (`0x801F6B68`); product gates at `0x801F6CE8`,
/// `6E68`, `6F24` and `7070` re-arm `scalar * 0x60`, `scalar << 7`,
/// `scalar << 6` and `scalar * 12` (`0x801F6E3C`, `6EF0`, `6F74`, `7090`);
/// arm 5 drains only a positive word (`blez` at `0x801F70B4`, gate
/// `0x801F70D8`) and then waits for its victim to settle. Arm 3 calls the
/// beam builder `0x801F726C` ahead of its gate, so the beam draws on every
/// tick the arm holds.
pub const CROSS_BEAM: &[ArmCountdown] = &[
    seed(0, S << 6),
    gate(1, P, add(S * 0x60)),
    gate(2, P, add(S << 7)),
    gate(3, P, add(S << 6)),
    gate(4, P, add(S * 12)),
    gate(5, P, None),
];

// ---------------------------------------------------------------------------
// The phase-chain bodies (`crate::cast_module_ticks::chain_bodies`)
// ---------------------------------------------------------------------------

/// The bare frame step `*(0x1F800393)` as a seed operand: two of PROT
/// 0935's re-arms read it (`lbu 0x7f`) where every other seed reads the
/// scalar. One per engine tick.
const T: i32 = DRAIN_STEP;

/// An ungated arm that adds to the word as it passes.
const fn bump(arm: u8, value: i32) -> ArmCountdown {
    ArmCountdown {
        arm,
        drain: None,
        write: Some(CountdownWrite::Add(value)),
    }
}

/// PROT 0935 (Earthquake `0x4A`, word `0x801F82BC`). Arm 1 seeds
/// `scalar << 6` (`0x801F6CB4`) once the caster's `+0x21B` leaves `0xFF`;
/// arm 2's product gate (`0x801F6FF4`) re-arms nothing; arm 3 adds
/// `step << 8` (`0x801F7198`) once the caster model's `+0x68` reaches
/// `0x160`; arm 4's gate (`0x801F76C8`) adds `step << 8` again
/// (`0x801F7734`); arm 5's gate (`0x801F7A70`) is the hit; arm 6 waits on the
/// whole row's clips. The two non-countdown holds (arms 1 and 3) and the row
/// settle are not carried.
pub const EARTHQUAKE: &[ArmCountdown] = &[
    seed(1, S << 6),
    gate(2, P, None),
    bump(3, T << 8),
    gate(4, P, add(T << 8)),
    gate(5, P, None),
];

/// PROT 0936 (Hyper Crush `0x4B`, word `0x801F8174`): seed `scalar * 0x60`
/// (`0x801F6C1C`), product gates re-arming `scalar * 0x60`, `scalar * 0x18`,
/// `scalar << 6` (`0x801F6E6C`, `6F2C`, `6FB4`). Arm 4 holds on its own
/// tick counter `0x801F817C` reaching `6` (`0x801F72D4`) while it drains the
/// word untested every tick (`0x801F7268`); the table runs it in one tick and
/// carries the six drains forward as a negative re-arm. Then gates at
/// `0x801F752C` / `78A0` / `7AC8` re-arming `scalar * 0xC0` and
/// `scalar << 6`; arm 8 waits on the party row's clips (not carried).
pub const HYPER_CRUSH: &[ArmCountdown] = &[
    seed(0, S * 0x60),
    gate(1, P, add(S * 0x60)),
    gate(2, P, add(S * 0x18)),
    gate(3, P, add(S << 6)),
    bump(4, -6 * P),
    gate(5, P, add(S * 0xC0)),
    gate(6, P, add(S << 6)),
    gate(7, P, None),
];

/// PROT 0937 (Hyper Lightning `0x4C`, word `0x801F8090`): seed
/// `scalar * 0x60` (`0x801F6BB4`), product gates re-arming `scalar * 0x60`,
/// `scalar << 6`, `<< 5`, `<< 7`, `* 0x60` (`0x801F6E04`, `6F14`, `708C`,
/// `7100`, `744C`). Arm 7 runs its four hits on a second word, `0x801F8094`,
/// which arm 6 zeroes (`0x801F754C`): the first lands at once and each
/// re-arms `scalar << 3` (`0x801F7680`), then the arm waits for the victim
/// to settle. The table carries that cadence on the one word - arm 6 sets
/// it to the three gaps instead of adding the `scalar << 7` retail adds to
/// a word arm 7 never reads.
pub const HYPER_LIGHTNING: &[ArmCountdown] = &[
    seed(0, S * 0x60),
    gate(1, P, add(S * 0x60)),
    gate(2, P, add(S << 6)),
    gate(3, P, add(S << 5)),
    gate(4, P, add(S << 7)),
    gate(5, P, add(S * 0x60)),
    gate(6, P, Some(CountdownWrite::Set(3 * (S << 3)))),
    gate(7, P, None),
];

/// PROT 0939 (Spore Gas, word `0x801F7AF0`): seed `scalar << 7`
/// (`0x801F6BA8`), product gates re-arming `scalar * 0x50`, `<< 6`, `<< 7`
/// (`0x801F6C9C`, `6ED0`, `6FA8`). Arm 4 leaves on the main word alone
/// (`0x801F740C`); its per-hit word `0x801F7AF4` paces the hits inside it.
/// Arm 5 waits for the victim to settle (not carried).
pub const SPORE_GAS: &[ArmCountdown] = &[
    seed(0, S << 7),
    gate(1, P, add(S * 0x50)),
    gate(2, P, add(S << 6)),
    gate(3, P, add(S << 7)),
    gate(4, P, None),
];

/// PROT 0942 `0xAA` (Power Up, `0x801F69F4`, word `0x801F88B4`): seed
/// `scalar * 0x180` (`0x801F6B24`), product gates re-arming `scalar * 0x60`,
/// `<< 7`, `* 0x180`, `<< 6`, `<< 7` (`0x801F6DD0`, `7150`, `72B8`, `76F4`,
/// `7B58`), terminal gate `0x801F7C20`.
pub const POWER_UP_AA: &[ArmCountdown] = &[
    seed(0, S * 0x180),
    gate(1, P, add(S * 0x60)),
    gate(2, P, add(S << 7)),
    gate(3, P, add(S * 0x180)),
    gate(4, P, add(S << 6)),
    gate(5, P, add(S << 7)),
    gate(6, P, None),
];

/// PROT 0947 (V Windhash, word `0x801F7B40`): seed `scalar << 5`
/// (`0x801F6BB8`), product gates re-arming `<< 7`, `<< 6`, `<< 6`,
/// `* 12` (`0x801F6DE8`, `6F38`, `7448`, `7724`); arm 3 drains at the top
/// (`0x801F7104`) and tests late (`0x801F73A8`). Arm 5 drains only a
/// positive word (`blez` at `0x801F7748`) and then waits for the victim to
/// settle (not carried).
pub const V_WINDHASH: &[ArmCountdown] = &[
    seed(0, S << 5),
    gate(1, P, add(S << 7)),
    gate(2, P, add(S << 6)),
    gate(3, P, add(S << 6)),
    gate(4, P, add(S * 12)),
    gate(5, P, None),
];

/// PROT 0956 `0x75` (`0x801F69D8`, word `0x801F86A0`): seed `scalar << 8`
/// (`0x801F6B60`), product gates re-arming `<< 8` and `<< 9`
/// (`0x801F6E50`, `70CC`). Arm 3 leaves on whichever comes first: its ramp
/// `0x801F86A8 += (scalar * step) << 2` (`0x801F712C`, zeroed by arm 2)
/// passing `0x1000`, or the countdown and a settled victim. The ramp always
/// wins - `0x1000` at four products a tick against `scalar << 9` at one - so
/// the table hands arm 3 the ramp's span at the ramp's rate.
pub const PARALYZING_WAVE: &[ArmCountdown] = &[
    seed(0, S << 8),
    gate(1, P, add(S << 8)),
    gate(2, P, Some(CountdownWrite::Set(0x1000))),
    gate(3, P << 2, None),
];

/// PROT 0959 (Megaton Press, `0x801F69F0`, word `0x801F9624`): seed
/// `scalar * 12` (`0x801F6C6C`) and a product gate on every arm, re-arming as
/// listed (`0x801F6D5C` .. `0x801F8114`). Arm `0x0F` drains at
/// `0x801F7EB0` and tests at `0x801F7F44`, with its repeat hits keyed off a
/// threshold word in between; arm `0x11`'s gate (`0x801F8140`) is
/// terminal.
pub const MEGATON_PRESS: &[ArmCountdown] = &[
    seed(0, S * 12),
    gate(1, P, add(S << 6)),
    gate(2, P, add(S << 7)),
    gate(3, P, add(S << 6)),
    gate(4, P, add(S << 6)),
    gate(5, P, add(S << 6)),
    gate(6, P, add(S * 0x3C)),
    gate(7, P, add(S << 2)),
    gate(8, P, add(S * 0xE0)),
    gate(9, P, add(S << 7)),
    gate(0x0A, P, add(S * 0x60)),
    gate(0x0B, P, add(S << 6)),
    gate(0x0C, P, add(S << 4)),
    gate(0x0D, P, add(S << 5)),
    gate(0x0E, P, add(S << 8)),
    gate(0x0F, P, add(S << 7)),
    gate(0x10, P, add(S << 6)),
    gate(0x11, P, None),
];

/// PROT 0960 `0xA6` (Neo Star Slash, `0x801F69D8`, word `0x801F8E50`):
/// seed `scalar << 5` (`0x801F6B44`); arm 1's gate (`0x801F6B8C`) **sets**
/// `scalar * 0x60` (`0x801F6BC8`); the rest add `scalar * 0xC0`, `* 0x1C`,
/// `<< 7`, `* 0xA0` (`0x801F6D44`, `6EB0`, `6FA4`, `70D8`). Arm 6's gate
/// (`0x801F73FC`) is followed by a party-row settle (not carried).
pub const NEO_STAR_SLASH: &[ArmCountdown] = &[
    seed(0, S << 5),
    gate(1, P, Some(CountdownWrite::Set(S * 0x60))),
    gate(2, P, add(S * 0xC0)),
    gate(3, P, add(S * 0x1C)),
    gate(4, P, add(S << 7)),
    gate(5, P, add(S * 0xA0)),
    gate(6, P, None),
];

/// PROT 0961 (Dead End Crisis / Final Crisis, `0x801F69D8`, word
/// `0x801F82E4`): seed `scalar * 0xC0` (`0x801F6C88`), product gates
/// re-arming `* 0xE0` (both formation paths), `* 0x180`, `<< 7`, `<< 6`,
/// `<< 8` (`0x801F6D84` / `6E10`, `6FE4`, `7344`, `7508`, `7768`); arm 6's
/// gate (`0x801F77C0`) stores `0xFF`.
pub const DEAD_END_CRISIS: &[ArmCountdown] = &[
    seed(0, S * 0xC0),
    gate(1, P, add(S * 0xE0)),
    gate(2, P, add(S * 0x180)),
    gate(3, P, add(S << 7)),
    gate(4, P, add(S << 6)),
    gate(5, P, add(S << 8)),
    gate(6, P, None),
];

/// PROT 0963 (Genocidal Cannon `0xB3`, `0x801F6A20`, word `0x801F92C8`):
/// the band's one body that drains the bare step on every arm and re-arms
/// literals - seed `0x20` (`0x801F6BC0`), then `+0x60`, `+0x100`, `+0x100`
/// (arm 3, which jumps to arm 6), `+0x40`, `+0x30`, `+0x20`, `+0x30`,
/// `+0x20`, `+0x70`, `+0x80`, `+0x10`, `+0x80`, `+0x100`, `+0x80`
/// (`0x801F6CC0` .. `0x801F801C`); arm `0x11`'s gate (`0x801F80E0`) is
/// terminal.
pub const GENOCIDAL_CANNON: &[ArmCountdown] = &[
    seed(0, 0x20),
    gate(1, DRAIN_STEP, add(0x60)),
    gate(2, DRAIN_STEP, add(0x100)),
    gate(3, DRAIN_STEP, add(0x100)),
    gate(6, DRAIN_STEP, add(0x40)),
    gate(7, DRAIN_STEP, add(0x30)),
    gate(8, DRAIN_STEP, add(0x20)),
    gate(9, DRAIN_STEP, add(0x30)),
    gate(0x0A, DRAIN_STEP, add(0x20)),
    gate(0x0B, DRAIN_STEP, add(0x70)),
    gate(0x0C, DRAIN_STEP, add(0x80)),
    gate(0x0D, DRAIN_STEP, add(0x10)),
    gate(0x0E, DRAIN_STEP, add(0x80)),
    gate(0x0F, DRAIN_STEP, add(0x100)),
    gate(0x10, DRAIN_STEP, add(0x80)),
    gate(0x11, DRAIN_STEP, None),
];

/// PROT 0964 `0xB0..=0xB2` (`0x801F69D8`, word `0x801F9B74`): seed
/// `scalar << 8` (`0x801F6C98`), arm 1's gate re-arming `scalar << 7`
/// (`0x801F6E94`) before the selector fork, and the same six re-arms in each
/// variant run (`2..`, `0x32..`, `0x64..`): `* 0x60`, `<< 8`, `<< 5`,
/// `* 0x70`, `* 0xC0`, `<< 7` (the last through the tail at `0x801F87E8` all
/// three share), then the shared terminal gate (`0x801F8840`).
pub const ELEMENT_STRIKE: &[ArmCountdown] = &[
    seed(0, S << 8),
    gate(1, P, add(S << 7)),
    gate(2, P, add(S * 0x60)),
    gate(3, P, add(S << 8)),
    gate(4, P, add(S << 5)),
    gate(5, P, add(S * 0x70)),
    gate(6, P, add(S * 0xC0)),
    gate(7, P, add(S << 7)),
    gate(8, P, None),
    gate(0x32, P, add(S * 0x60)),
    gate(0x33, P, add(S << 8)),
    gate(0x34, P, add(S << 5)),
    gate(0x35, P, add(S * 0x70)),
    gate(0x36, P, add(S * 0xC0)),
    gate(0x37, P, add(S << 7)),
    gate(0x38, P, None),
    gate(0x64, P, add(S * 0x60)),
    gate(0x65, P, add(S << 8)),
    gate(0x66, P, add(S << 5)),
    gate(0x67, P, add(S * 0x70)),
    gate(0x68, P, add(S * 0xC0)),
    gate(0x69, P, add(S << 7)),
    gate(0x6A, P, None),
];

const CHAIN_0942: u32 = crate::cast_module_ticks::POWER_UP_AA_CHAIN.body;
const CHAIN_0956: u32 = crate::cast_module_ticks::WATER_HAZARD_75_CHAIN.body;
const CHAIN_0959: u32 = crate::cast_module_ticks::MEGATON_PRESS_CHAIN.body;
const CHAIN_0960: u32 = crate::cast_module_ticks::NEO_STAR_SLASH_CHAIN.body;
const CHAIN_0961: u32 = crate::cast_module_ticks::DEAD_END_CRISIS_CHAIN.body;
const CHAIN_0963: u32 = crate::cast_module_ticks::GENOCIDAL_CANNON_CHAIN.body;
const CHAIN_0964: u32 = crate::cast_module_ticks::ELEMENT_STRIKE_CHAIN.body;

/// The countdown table of a capture-class body, keyed on `(entry, body)` as
/// the trampoline map is. `None` for a body with no table here (one a camera
/// director paces, or one whose arms do not gate on the module word).
pub fn capture_arm_countdowns(entry: u32, body: u32) -> Option<&'static [ArmCountdown]> {
    Some(match (entry, body) {
        (940, arms::GLARE_DIVIDE_SPLIT_TICK) => GLARE_DIVIDE_SPLIT,
        (941, arms::STEAL_TICK) => STEAL,
        (941, arms::STEAL_SWEEP_TICK) => STEAL_SWEEP,
        (943, arms::CURSE_SINGLE_TICK) | (944, arms::GUILTY_CROSS_CURSE_TICK) => CURSE,
        (943, arms::CURSE_MP_DRAIN_TICK) => CURSE_MP_DRAIN,
        (950, arms::ROLLING_FLARE_TICK) => ROLLING_FLARE,
        (950, arms::ROLLING_FLARE_SWEEP_TICK) => ROLLING_FLARE_SWEEP,
        (956, arms::WATER_HAZARD_TICK) => WATER_HAZARD,
        (948, super::SINGLE_BODY) => CROSS_BEAM,
        (935, super::SINGLE_BODY) => EARTHQUAKE,
        (936, super::SINGLE_BODY) => HYPER_CRUSH,
        (937, super::SINGLE_BODY) => HYPER_LIGHTNING,
        (939, super::SINGLE_BODY) => SPORE_GAS,
        (947, super::SINGLE_BODY) => V_WINDHASH,
        (942, CHAIN_0942) => POWER_UP_AA,
        (956, CHAIN_0956) => PARALYZING_WAVE,
        (959, CHAIN_0959) => MEGATON_PRESS,
        (960, CHAIN_0960) => NEO_STAR_SLASH,
        (961, CHAIN_0961) => DEAD_END_CRISIS,
        (963, CHAIN_0963) => GENOCIDAL_CANNON,
        (964, CHAIN_0964) => ELEMENT_STRIKE,
        _ => return None,
    })
}

/// The arm of `table` that runs on `phase`, if it has a countdown half.
pub fn arm_countdown(table: &[ArmCountdown], phase: u8) -> Option<ArmCountdown> {
    table.iter().copied().find(|a| a.arm == phase)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Walk a table the way the host does - one tick per call, the body
    /// advancing one arm whenever the gate lets it run - and return each
    /// arm's dwell in ticks.
    fn dwells(table: &[ArmCountdown], arms: u8) -> Vec<u32> {
        let mut cd = ModuleCountdown::default();
        let mut out = Vec::new();
        for phase in 0..arms {
            let mut ticks = 0;
            loop {
                ticks += 1;
                let a = arm_countdown(table, phase);
                if a.is_some_and(|a| a.holds(&mut cd)) {
                    continue;
                }
                if let Some(a) = a {
                    a.pass(&mut cd);
                }
                break;
            }
            out.push(ticks);
        }
        out
    }

    /// At the capture's drain (product `32` = four times the engine's), each
    /// counted arm's dwell is the measured one; at the engine's own step it
    /// is four times that, i.e. the same vsyncs.
    #[test]
    fn the_steal_sweep_holds_its_measured_arms() {
        assert_eq!(dwells(STEAL_SWEEP, 5), vec![1, 256, 128, 256, 64]);
    }

    #[test]
    fn the_curse_bodies_hold_their_measured_arms() {
        // Measured at a product of 32: 9, 40, 8, 32.
        assert_eq!(dwells(CURSE, 5), vec![1, 32, 160, 32, 128]);
        // Measured at a bare step of 4: 65, 32, 64, 32.
        assert_eq!(dwells(CURSE_MP_DRAIN, 5), vec![1, 256, 128, 256, 128]);
    }

    #[test]
    fn the_rolling_flare_sweep_holds_every_arm() {
        let d = dwells(ROLLING_FLARE_SWEEP, 14);
        // Measured 65, 21, 64, 8, 40, 15+ for arms 1..6 at 32 a tick.
        assert_eq!(&d[1..7], &[256, 64, 256, 32, 160, 64]);
    }

    #[test]
    fn the_steal_walk_arm_is_ungated() {
        let d = dwells(STEAL, 4);
        assert_eq!(d, vec![1, 1, 32, 64]);
    }

    #[test]
    fn the_water_hazard_holds_its_measured_arms() {
        // Measured 1, 33, 32, 32, 1 at 32 a tick.
        assert_eq!(dwells(WATER_HAZARD, 4), vec![1, 128, 128, 128]);
    }

    /// Every capture-class phase-chain body carries a countdown table, keyed
    /// the way the band looks it up, and every row names an arm the body has.
    /// PROT 0919 (Spoon) is a summon-band module, whose length the stager
    /// decides, and stays ungated.
    #[test]
    fn every_capture_chain_body_gates_its_own_arms() {
        use crate::cast_module_ticks::{capture_trampoline_for, chain_bodies};
        let mut n = 0;
        for b in chain_bodies().iter().filter(|b| b.prot_entry != 919) {
            let key = if capture_trampoline_for(b.prot_entry).is_some() {
                b.body
            } else {
                super::super::SINGLE_BODY
            };
            let t = capture_arm_countdowns(b.prot_entry, key)
                .unwrap_or_else(|| panic!("PROT {:04} has no countdown table", b.prot_entry));
            for a in t {
                assert!(
                    b.arm(a.arm).is_some(),
                    "PROT {:04}: row for arm {:#04X}, which the body does not have",
                    b.prot_entry,
                    a.arm
                );
            }
            n += 1;
        }
        assert_eq!(n, 13);
    }

    #[test]
    fn the_chain_body_arms_hold_for_their_seeds() {
        // PROT 0961: `scalar * 0xC0` over a product of 8 is 192 vsyncs.
        assert_eq!(dwells(DEAD_END_CRISIS, 3), vec![1, 192, 224]);
        // PROT 0963 drains the bare step: its literals are vsyncs.
        assert_eq!(dwells(GENOCIDAL_CANNON, 3), vec![1, 0x20, 0x60]);
        // PROT 0956's arm 3 runs on the ramp: 0x1000 at 32 a vsync.
        assert_eq!(dwells(PARALYZING_WAVE, 4), vec![1, 256, 256, 128]);
        // PROT 0937's four hits: the first at once, three gaps of 8.
        assert_eq!(dwells(HYPER_LIGHTNING, 8)[7], 24);
    }

    #[test]
    fn the_cross_beam_holds_its_beam_arm_for_128_vsyncs() {
        let d = dwells(CROSS_BEAM, 6);
        assert_eq!(d, vec![1, 64, 96, 128, 64, 12]);
    }

    #[test]
    fn a_body_with_a_camera_director_has_no_table_here() {
        assert!(capture_arm_countdowns(944, arms::GUILTY_CROSS_TICK).is_none());
        assert!(capture_arm_countdowns(940, arms::GLARE_DIVIDE_BLIND_TICK).is_none());
    }
}
