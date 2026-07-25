//! The two-mode battle effect burst.
//!
//! PORT: FUN_801F30C4
//!
//! `(record, mode)`. A radial spawn burst: four iterations around the compass,
//! three spawns per iteration, each placed by a trig term plus a bounded random
//! jitter. `mode` picks one of two parameter sets; anything but `0` or `1`
//! returns immediately.
//!
//! Read from a disassembly of the mapped `0898` image at base `0x801CE818`.
//! `disasm-overlay-fn.py` cannot be used here - it stops at the first
//! unconditional `j` and reports 18 instructions for this entry - so the span
//! `0x801F30C4..0x801F398C` was disassembled with raw capstone. It really does
//! end where the cast audio-cue dispatcher `func_0x801F3990` begins:
//! `0x801F3988` is the `jr ra` and `0x801F3990` a clean `addiu sp, sp, -0x20`
//! prologue. 563 instructions.
//!
//! ## The entry is one loop written twice
//!
//! The three-way fork on the second argument reaches two loop bodies of 258
//! instructions each. Diffed instruction by instruction they are **identical
//! except for ten constants**, three of which are only branch targets shifted
//! by the arm offset. The seven real differences are:
//!
//! | | arm `0` ([`BurstMode::Wide`]) | arm `1` ([`BurstMode::Narrow`]) |
//! |---|---|---|
//! | spawn table (x3) | `0x801F5DA4` | `0x801F5D0C` |
//! | cosine divisors | `/48`, `/72`, `/96` | `/96`, `/144`, `/192` |
//! | tail offsets | `+0x70`, `+0xA8`, `+0x38` | `+0x30`, `+0x48`, `+0x18` |
//! | loop latch | `beqz` out, `j` back | `bnez` back |
//!
//! Two exact relations fall out, and [`arm_invariants_hold`] checks them rather
//! than leaving them as prose: every narrow cosine divisor is **twice** its wide
//! counterpart (one extra `sra`), and every narrow tail offset is exactly
//! **3/7** of its wide counterpart. So `mode` selects the same burst at a
//! smaller radius, not a different effect.
//!
//! ## Angles
//!
//! Block `0` indexes the trig LUTs at `iteration * 1024` - the four cardinals -
//! while blocks `1` and `2` share `(iteration * 1024 + 512) & 0xFFF`, the four
//! diagonals. Block `2` reuses block `1`'s index register rather than
//! recomputing it.
//!
//! ## Reciprocal divides
//!
//! Eleven per arm, and **every one was checked against plain division** over
//! `0..300000` plus the 32-bit signed boundary before being written down - a
//! reciprocal that is nearly the divide it looks like is the classic way this
//! goes silently wrong. Two hand-readings were wrong before the check and are
//! recorded here in their corrected form: `0x2AAAAAAB >> 3` is `/48` (not `/6`,
//! which is the constant read without its shift), and `0x2E8BA2E9 >> 1` is
//! `/11`. `0x88888889` is the **signed magic-with-add** form (`mfhi`, `addu` the
//! original, then `sra 3`) and needs signed arithmetic to reproduce; it is
//! `/15`. All of them are used as `x - (x / d) * d`, i.e. a modulo, except the
//! three cosine divides.
//!
//! # NOT WIRED
//!
//! No engine caller, and two prerequisites remain - both stated rather than
//! guessed at:
//!
//! * **`FUN_80050ED4` is not decoded.** It takes `(record + 0x14, scratch,
//!   table, record[+0x72] >> 1)` and returns a record pointer the burst then
//!   writes two halfwords into. What it allocates is open, so it is a
//!   [`BurstHost`] method rather than a port.
//! * **The two tables are disc data with no parser.** `0x801F5DA4` and
//!   `0x801F5D0C` live in `0898`'s tail. Their *addresses* are the parameter -
//!   [`BurstMode::table_addr`] carries those - but none of their bytes are
//!   reproduced here and none should be committed.

/// Iterations each arm runs (`slti $v0, $s2, 4`).
pub const ITERATIONS: u32 = 4;
/// Spawn blocks per iteration - three `FUN_80050ED4` calls.
pub const SPAWNS_PER_ITERATION: usize = 3;
/// Draws per iteration (`FUN_80056798` x3 per block).
pub const DRAWS_PER_ITERATION: usize = 9;
/// Stride the burst advances the returned record pointer by before its second
/// store (`addiu $s0, $s0, 0x80`).
pub const RECORD_STRIDE: i32 = 0x80;
/// Bytes copied from `record[+0x24]` into the stack scratch by the `lwl`/`lwr`
/// pair at the top of every block.
pub const SCRATCH_BYTES: usize = 8;
/// Offset of the halfword inside that scratch the placement term lands on
/// (`sh $a2, 0x12($sp)` against a scratch at `sp+0x10`).
pub const SCRATCH_PLACE_OFFSET: usize = 2;

/// Which parameter set the second argument selects.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BurstMode {
    /// `mode == 0`.
    Wide,
    /// `mode == 1`.
    Narrow,
}

impl BurstMode {
    /// Resolve the second argument. Retail falls straight to the epilogue for
    /// anything else, so the burst is a no-op.
    pub const fn from_arg(mode: u32) -> Option<Self> {
        match mode {
            0 => Some(BurstMode::Wide),
            1 => Some(BurstMode::Narrow),
            _ => None,
        }
    }

    /// The spawn-table address this arm hands `FUN_80050ED4`. The bytes behind
    /// it are disc data and are deliberately not modelled.
    pub const fn table_addr(self) -> u32 {
        match self {
            BurstMode::Wide => 0x801F_5DA4,
            BurstMode::Narrow => 0x801F_5D0C,
        }
    }

    /// The three spawn blocks, in the order the loop runs them.
    pub const fn blocks(self) -> [SpawnBlock; SPAWNS_PER_ITERATION] {
        match self {
            BurstMode::Wide => WIDE_BLOCKS,
            BurstMode::Narrow => NARROW_BLOCKS,
        }
    }
}

/// One spawn block's constants.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SpawnBlock {
    /// `true` when the block indexes the LUTs on the diagonals
    /// (`(iter * 1024 + 512) & 0xFFF`) rather than the cardinals
    /// (`iter * 1024`).
    pub diagonal: bool,
    /// Right shift applied to the sine term as a **signed** divide (the
    /// `bgez` / `addiu (1 << n) - 1` rounding pair precedes it).
    pub sine_shift: u32,
    /// Modulus of the placement jitter (`rand % this`).
    pub place_mod: i32,
    /// Bias added after the placement jitter.
    pub place_bias: i32,
    /// Divisor applied to the cosine term.
    pub cosine_div: i32,
    /// Modulus of the first post-spawn store's jitter.
    pub spread_mod: i32,
    /// Bias added after that jitter. The store lands at `spawn[+0x3E]`.
    pub spread_bias: i32,
    /// Modulus of the second post-spawn store's jitter.
    pub tail_mod: i32,
    /// Bias added after that jitter. The store lands at
    /// `spawn[+0x80 + 0x18]`.
    pub tail_bias: i32,
}

/// Arm `0`'s three blocks.
pub const WIDE_BLOCKS: [SpawnBlock; SPAWNS_PER_ITERATION] = [
    SpawnBlock {
        diagonal: false,
        sine_shift: 3,
        place_mod: 257,
        place_bias: -0x80,
        cosine_div: 48,
        spread_mod: 21,
        spread_bias: -0x0A,
        tail_mod: 33,
        tail_bias: 0x70,
    },
    SpawnBlock {
        diagonal: true,
        sine_shift: 4,
        place_mod: 129,
        place_bias: -0x40,
        cosine_div: 72,
        spread_mod: 15,
        spread_bias: -0x07,
        tail_mod: 49,
        tail_bias: 0xA8,
    },
    SpawnBlock {
        diagonal: true,
        sine_shift: 3,
        place_mod: 257,
        place_bias: -0x80,
        cosine_div: 96,
        spread_mod: 11,
        spread_bias: -0x05,
        tail_mod: 17,
        tail_bias: 0x38,
    },
];

/// Arm `1`'s three blocks - identical but for the cosine divisors and the tail
/// biases.
pub const NARROW_BLOCKS: [SpawnBlock; SPAWNS_PER_ITERATION] = [
    SpawnBlock {
        cosine_div: 96,
        tail_bias: 0x30,
        ..WIDE_BLOCKS[0]
    },
    SpawnBlock {
        cosine_div: 144,
        tail_bias: 0x48,
        ..WIDE_BLOCKS[1]
    },
    SpawnBlock {
        cosine_div: 192,
        tail_bias: 0x18,
        ..WIDE_BLOCKS[2]
    },
];

/// The trig-LUT element index a block uses on a given iteration.
pub const fn lut_index(iteration: u32, diagonal: bool) -> u32 {
    if diagonal {
        ((iteration << 10) + 0x200) & 0xFFF
    } else {
        (iteration << 10) & 0xFFF
    }
}

/// Retail's signed divide-by-power-of-two: bias a negative numerator by
/// `(1 << shift) - 1` before the arithmetic shift so it truncates toward zero.
pub const fn signed_shift_div(value: i32, shift: u32) -> i32 {
    let biased = if value < 0 {
        value + ((1 << shift) - 1)
    } else {
        value
    };
    biased >> shift
}

/// The modulo the reciprocal chains compute. Retail forms `x - (x / d) * d`
/// with a magic multiply; every constant was verified against plain division
/// before this was written, so the port uses the division directly.
pub const fn reciprocal_mod(draw: i32, divisor: i32) -> i32 {
    if divisor == 0 {
        return 0;
    }
    draw - (draw / divisor) * divisor
}

/// What a block hands `FUN_80050ED4`, plus the two values it writes afterwards.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SpawnRequest {
    /// The scratch halfword after the placement term is folded in.
    pub place: i16,
    /// Value stored at `spawn[+0x3E]`.
    pub spread: i16,
    /// Value stored at `spawn[+0x80 + 0x18]`.
    pub tail: i16,
}

/// Everything the burst reads that is not one of its own constants.
pub trait BurstHost {
    /// `func_0x80056798` - the SCUS RNG.
    fn rand(&mut self) -> i32;
    /// `sin[index]`, via the pointer at `_DAT_8007B81C`.
    fn sin(&self, index: u32) -> i16;
    /// `cos[index]`, via the pointer at `_DAT_8007B7F8`.
    fn cos(&self, index: u32) -> i16;
    /// The scratch halfword the block starts from - the second halfword of the
    /// eight bytes copied out of `record[+0x24]`.
    fn scratch_place(&self) -> i16;
    /// `FUN_80050ED4(record + 0x14, scratch, table, record[+0x72] >> 1)`.
    /// Returns the spawned record pointer; the burst writes two halfwords into
    /// it. Not decoded - see the module's `NOT WIRED` note.
    fn spawn(&mut self, table: u32, request: SpawnRequest) -> u32;
}

/// Run one spawn block and return what it produced.
///
/// Draw order is retail's: the placement draw before the spawn call, then the
/// spread draw, then the tail draw.
pub fn run_block(
    host: &mut impl BurstHost,
    block: SpawnBlock,
    iteration: u32,
    table: u32,
) -> SpawnRequest {
    let idx = lut_index(iteration, block.diagonal);

    let draw = host.rand();
    let sine = signed_shift_div(host.sin(idx) as i32, block.sine_shift);
    let place = (host.scratch_place() as i32)
        .wrapping_add(sine)
        .wrapping_add(reciprocal_mod(draw, block.place_mod))
        .wrapping_add(block.place_bias) as i16;

    let request_place = place;
    let draw = host.rand();
    let spread = ((host.cos(idx) as i32) / block.cosine_div)
        .wrapping_add(reciprocal_mod(draw, block.spread_mod))
        .wrapping_add(block.spread_bias) as i16;

    let draw = host.rand();
    let tail = reciprocal_mod(draw, block.tail_mod).wrapping_add(block.tail_bias) as i16;

    let req = SpawnRequest {
        place: request_place,
        spread,
        tail,
    };
    host.spawn(table, req);
    req
}

/// The whole burst (`FUN_801F30C4`).
///
/// Returns the requests in the order they were issued, or an empty vector when
/// `mode` selects neither arm.
pub fn run_burst(host: &mut impl BurstHost, mode: u32) -> Vec<SpawnRequest> {
    let Some(mode) = BurstMode::from_arg(mode) else {
        return Vec::new();
    };
    let blocks = mode.blocks();
    let table = mode.table_addr();
    let mut out = Vec::with_capacity(ITERATIONS as usize * SPAWNS_PER_ITERATION);
    for iteration in 0..ITERATIONS {
        for block in blocks {
            out.push(run_block(host, block, iteration, table));
        }
    }
    out
}

/// The two exact relations between the arms, as a runtime check rather than a
/// claim in prose: every narrow cosine divisor is twice the wide one, and every
/// narrow tail offset is exactly three sevenths of the wide one.
pub fn arm_invariants_hold() -> bool {
    WIDE_BLOCKS.iter().zip(NARROW_BLOCKS.iter()).all(|(w, n)| {
        n.cosine_div == w.cosine_div * 2
            && w.tail_bias * 3 % 7 == 0
            && n.tail_bias == w.tail_bias * 3 / 7
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Host {
        draws: Vec<i32>,
        cursor: usize,
        place: i16,
        spawns: Vec<(u32, SpawnRequest)>,
    }

    impl Host {
        fn new(draws: Vec<i32>) -> Self {
            Self {
                draws,
                cursor: 0,
                place: 0,
                spawns: Vec::new(),
            }
        }
    }

    impl BurstHost for Host {
        fn rand(&mut self) -> i32 {
            let v = self.draws.get(self.cursor).copied().unwrap_or(0);
            self.cursor += 1;
            v
        }
        fn sin(&self, index: u32) -> i16 {
            let rad = (index & 0xFFF) as f64 * std::f64::consts::TAU / 4096.0;
            (rad.sin() * 4096.0).round() as i16
        }
        fn cos(&self, index: u32) -> i16 {
            let rad = (index & 0xFFF) as f64 * std::f64::consts::TAU / 4096.0;
            (rad.cos() * 4096.0).round() as i16
        }
        fn scratch_place(&self) -> i16 {
            self.place
        }
        fn spawn(&mut self, table: u32, request: SpawnRequest) -> u32 {
            self.spawns.push((table, request));
            0x8010_0000
        }
    }

    #[test]
    fn only_zero_and_one_select_an_arm() {
        assert_eq!(BurstMode::from_arg(0), Some(BurstMode::Wide));
        assert_eq!(BurstMode::from_arg(1), Some(BurstMode::Narrow));
        for m in [2u32, 3, 0xFF, 0xFFFF_FFFF] {
            assert_eq!(BurstMode::from_arg(m), None, "mode {m}");
        }
    }

    #[test]
    fn an_unselected_mode_spawns_nothing_and_draws_nothing() {
        let mut h = Host::new(vec![1; 64]);
        assert!(run_burst(&mut h, 7).is_empty());
        assert_eq!(h.cursor, 0, "the fork precedes every draw");
        assert!(h.spawns.is_empty());
    }

    #[test]
    fn the_two_arms_differ_only_in_the_documented_constants() {
        for (w, n) in WIDE_BLOCKS.iter().zip(NARROW_BLOCKS.iter()) {
            assert_eq!(w.diagonal, n.diagonal);
            assert_eq!(w.sine_shift, n.sine_shift);
            assert_eq!(w.place_mod, n.place_mod);
            assert_eq!(w.place_bias, n.place_bias);
            assert_eq!(w.spread_mod, n.spread_mod);
            assert_eq!(w.spread_bias, n.spread_bias);
            assert_eq!(w.tail_mod, n.tail_mod);
            assert_ne!(w.cosine_div, n.cosine_div);
            assert_ne!(w.tail_bias, n.tail_bias);
        }
    }

    #[test]
    fn narrow_is_wide_at_half_the_cosine_and_three_sevenths_the_offset() {
        assert!(arm_invariants_hold());
        // Spelled out, so a future edit to the tables trips this too.
        assert_eq!(WIDE_BLOCKS.map(|b| b.cosine_div), [48, 72, 96]);
        assert_eq!(NARROW_BLOCKS.map(|b| b.cosine_div), [96, 144, 192]);
        assert_eq!(WIDE_BLOCKS.map(|b| b.tail_bias), [0x70, 0xA8, 0x38]);
        assert_eq!(NARROW_BLOCKS.map(|b| b.tail_bias), [0x30, 0x48, 0x18]);
    }

    #[test]
    fn the_two_arms_use_different_tables() {
        assert_ne!(BurstMode::Wide.table_addr(), BurstMode::Narrow.table_addr());
        assert_eq!(BurstMode::Wide.table_addr(), 0x801F_5DA4);
        assert_eq!(BurstMode::Narrow.table_addr(), 0x801F_5D0C);
    }

    #[test]
    fn block_zero_walks_the_cardinals_and_the_rest_the_diagonals() {
        assert_eq!(
            (0..ITERATIONS)
                .map(|i| lut_index(i, false))
                .collect::<Vec<_>>(),
            vec![0, 1024, 2048, 3072]
        );
        assert_eq!(
            (0..ITERATIONS)
                .map(|i| lut_index(i, true))
                .collect::<Vec<_>>(),
            vec![512, 1536, 2560, 3584]
        );
        assert!(!WIDE_BLOCKS[0].diagonal);
        assert!(WIDE_BLOCKS[1].diagonal && WIDE_BLOCKS[2].diagonal);
    }

    #[test]
    fn lut_index_stays_inside_the_twelve_bit_table() {
        for i in 0..ITERATIONS {
            for d in [false, true] {
                assert!(lut_index(i, d) <= 0xFFF);
            }
        }
    }

    #[test]
    fn signed_shift_div_truncates_toward_zero() {
        assert_eq!(signed_shift_div(16, 3), 2);
        assert_eq!(signed_shift_div(15, 3), 1);
        assert_eq!(signed_shift_div(-16, 3), -2);
        // The bias is what makes this differ from a bare `sra`.
        assert_eq!(signed_shift_div(-15, 3), -1);
        assert_eq!(-15i32 >> 3, -2, "a bare sra would round away from zero");
        for v in -5000i32..5000 {
            for sh in [3u32, 4] {
                assert_eq!(signed_shift_div(v, sh), v / (1 << sh), "v={v} sh={sh}");
            }
        }
    }

    #[test]
    fn reciprocal_mod_matches_plain_modulo() {
        for d in [257i32, 129, 21, 15, 11, 33, 49, 17] {
            for v in 0i32..2000 {
                assert_eq!(reciprocal_mod(v, d), v % d, "v={v} d={d}");
            }
        }
        assert_eq!(reciprocal_mod(5, 0), 0, "no divide-by-zero trap");
    }

    #[test]
    fn a_burst_issues_twelve_spawns_and_thirty_six_draws() {
        let mut h = Host::new(vec![0; 128]);
        let out = run_burst(&mut h, 0);
        assert_eq!(out.len(), (ITERATIONS as usize) * SPAWNS_PER_ITERATION);
        assert_eq!(out.len(), 12);
        assert_eq!(h.cursor, 12 * 3);
        assert_eq!(h.cursor, ITERATIONS as usize * DRAWS_PER_ITERATION);
        assert_eq!(h.spawns.len(), 12);
    }

    #[test]
    fn every_spawn_carries_the_arms_table() {
        let mut h = Host::new(vec![0; 128]);
        run_burst(&mut h, 1);
        assert!(
            h.spawns
                .iter()
                .all(|(t, _)| *t == BurstMode::Narrow.table_addr())
        );
    }

    #[test]
    fn the_tail_value_is_bounded_by_its_modulus_and_bias() {
        for mode in [BurstMode::Wide, BurstMode::Narrow] {
            for b in mode.blocks() {
                for draw in 0i32..200 {
                    let t = reciprocal_mod(draw, b.tail_mod) + b.tail_bias;
                    assert!(t >= b.tail_bias);
                    assert!(t < b.tail_bias + b.tail_mod);
                }
            }
        }
    }

    #[test]
    fn narrow_tails_land_strictly_below_wide_tails() {
        // The 3/7 scaling means every narrow band sits under its wide twin.
        for (w, n) in WIDE_BLOCKS.iter().zip(NARROW_BLOCKS.iter()) {
            assert!(n.tail_bias + n.tail_mod <= w.tail_bias + w.tail_mod);
            assert!(n.tail_bias < w.tail_bias);
        }
    }

    #[test]
    fn placement_folds_the_sine_the_jitter_and_the_bias() {
        let mut h = Host::new(vec![0; 8]);
        h.place = 1000;
        let b = WIDE_BLOCKS[0];
        // Iteration 0 -> LUT index 0 -> sin 0, cos 4096.
        let req = run_block(&mut h, b, 0, 0);
        // place = 1000 + 0 + (0 % 257) + (-0x80)
        assert_eq!(req.place, 1000 - 0x80);
        // spread = 4096/48 + (0 % 21) - 0x0A
        assert_eq!(req.spread, (4096 / 48) - 0x0A);
        // tail = (0 % 33) + 0x70
        assert_eq!(req.tail, 0x70);
    }

    #[test]
    fn the_same_draws_place_narrow_spawns_tighter_than_wide() {
        let draws = vec![7i32; 128];
        let mut hw = Host::new(draws.clone());
        let mut hn = Host::new(draws);
        let wide = run_burst(&mut hw, 0);
        let narrow = run_burst(&mut hn, 1);
        assert_eq!(wide.len(), narrow.len());
        // Placement is arm-independent; the spread and tail are not.
        for (w, n) in wide.iter().zip(narrow.iter()) {
            assert_eq!(w.place, n.place);
            assert!(n.tail < w.tail);
        }
    }

    #[test]
    fn draw_order_is_place_then_spread_then_tail() {
        // Distinct draws make the ordering observable.
        let mut h = Host::new(vec![100, 200, 300]);
        h.place = 0;
        let b = WIDE_BLOCKS[0];
        let req = run_block(&mut h, b, 0, 0);
        assert_eq!(
            req.place,
            (reciprocal_mod(100, b.place_mod) + b.place_bias) as i16
        );
        assert_eq!(
            req.spread,
            (4096 / b.cosine_div + reciprocal_mod(200, b.spread_mod) + b.spread_bias) as i16
        );
        assert_eq!(
            req.tail,
            (reciprocal_mod(300, b.tail_mod) + b.tail_bias) as i16
        );
        // And the three draws really were distinct, so the order is observable.
        assert_ne!(req.place, req.spread);
        assert_ne!(req.spread, req.tail);
    }
}
