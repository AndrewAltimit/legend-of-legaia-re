//! A retail spin replayed through the engine's slot machine, frame for frame.
//!
//! `minigame_slot_machine_retail_spin_decided` is recorded from
//! `minigame_slot_machine_retail_reels` (a retail, pad-only state at rest
//! before its first bet) by pressing Square, Circle, Cross, Square, Circle,
//! one every 20 vsyncs from vsync 20, each held four. Its RAM words are the
//! oracle:
//!
//! | word | at rest | spin decided |
//! |---|---|---|
//! | slot LCG `DAT_801D3C80` | `0x5C2E0B36` | `0xACF1768A` |
//! | BIOS `rand` seed `0x80009010` | `0xBE4DCD5A` | `0x579DA7FF` |
//! | state word `DAT_801D3C84` | `1` | `4`, `103` vsyncs after the load |
//! | reels `DAT_801D3CC0..` | - | rows `9`, `11`, `3` |
//! | claimed `DAT_801D3D20..` | - | `4`, `8`, `1` |
//! | net take `DAT_801D3D40` / balance `DAT_801D4114` | `0` / `9999999` | `6` / `9999996` |
//!
//! The animation counter `DAT_801D3C9C` moves `178` to `282` between them: one
//! machine frame a vsync. The machine the warp builds stands on the at-rest
//! LCG after `177` idle frames, the strips being the init literal's. Seated on
//! the capture's `rand` word and fed the presses two frames after the script
//! raised them, it lands every word above on the vsync retail did: the feature
//! roll, the spin-up, each stop's plan and landing search, the roll-in, the
//! claims and the losing evaluation.
//!
//! The payout table is synthetic: the spin pays nothing, so no payout byte
//! is read. Nothing here is disc data.

use legaia_engine_minigames::slot_machine::{SLOT_RNG_SEED, SlotMachine};
use legaia_engine_system::pad::{PACK_CIRCLE, PACK_CROSS, PACK_SQUARE};
use legaia_minigame_formats::minigame_slot_scene::PAYLINE_CENTRE_ROW_BIAS;
use legaia_minigame_formats::slot_payout::SlotPayoutTable;

/// The capture's slot LCG at rest.
const REST_LCG: u32 = 0x5C2E_0B36;
/// The capture's BIOS `rand` seed at rest.
const REST_RAND: u32 = 0xBE4D_CD5A;
/// The press train's lag: a pad word the script raises on vsync `t` is the
/// machine's edge on frame `t + 2`.
const PAD_LAG: u32 = 2;

/// The script's pad on vsync `t`: from vsync 20 a button every 20 vsyncs,
/// held four, taking turns Cross / Square / Circle by `t / 20`.
fn script_pad(t: i64) -> u16 {
    if t < 20 || t % 20 >= 4 {
        return 0;
    }
    [PACK_CROSS, PACK_SQUARE, PACK_CIRCLE][((t / 20) % 3) as usize]
}

/// A retail row `DAT_801D3CC0` in the engine's payline frame.
fn payline_pos(retail_row: i32) -> i32 {
    ((retail_row + PAYLINE_CENTRE_ROW_BIAS) % 20) << 8
}

#[test]
fn the_engine_lands_the_retail_spin_on_its_vsync() {
    let payouts = SlotPayoutTable {
        payouts: [2, 4, 6, 8, 10, 20, 30, 60, 100, 200],
    };
    let mut m = SlotMachine::new(payouts, SLOT_RNG_SEED, 9_999_999);
    let mut idle = 0;
    while m.rng_state() != REST_LCG {
        m.frame(0);
        idle += 1;
        assert!(idle < 10_000, "the at-rest LCG is never reached");
    }
    assert_eq!(idle, 177, "the counter's 178 less the init's own frame");
    assert_eq!(m.retail_state(), 1);
    m.seat_rand(REST_RAND);

    let mut prev = 0u16;
    let mut decided = None;
    for f in 0..104u32 {
        let pad = script_pad(i64::from(f) - i64::from(PAD_LAG));
        let edge = pad & !prev;
        prev = pad;
        m.frame(u32::from(edge));
        if decided.is_none() && m.retail_state() == 4 {
            decided = Some(f);
        }
    }
    // The capture's watch saved one vsync after the state word read `4`.
    assert_eq!(decided, Some(102), "the spin decides on retail's vsync");
    let row = m.frame_row();
    assert_eq!(
        row.reel_pos,
        [payline_pos(9), payline_pos(11), payline_pos(3)]
    );
    assert_eq!([m.claimed(0), m.claimed(1), m.claimed(2)], [4, 8, 1]);
    assert_eq!((row.net_take, row.balance), (6, 9_999_996));
    assert_eq!(row.line, -1, "no line paid");
    assert_eq!(row.lcg, 0xACF1_768A);
    assert_eq!(row.rand, 0x579D_A7FF);
}
