use super::*;

struct Host {
    draws: Vec<i32>,
    cursor: usize,
    yaw: i16,
    scale: u16,
    pool_free: usize,
    spawns: Vec<(u32, SpawnRequest)>,
}

impl Host {
    fn new(draws: Vec<i32>) -> Self {
        Self {
            draws,
            cursor: 0,
            yaw: 0,
            scale: 0x1000,
            pool_free: usize::MAX,
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
    fn parent_yaw(&self) -> i16 {
        self.yaw
    }
    fn parent_scale(&self) -> u16 {
        self.scale
    }
    fn spawn(&mut self, record_addr: u32, request: SpawnRequest) -> Option<u32> {
        if self.pool_free == 0 {
            return None;
        }
        self.pool_free = self.pool_free.saturating_sub(1);
        self.spawns.push((record_addr, request));
        Some(0x8010_0000)
    }
}

// -- the mode fork ------------------------------------------------------

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

// -- the arm constants --------------------------------------------------

#[test]
fn the_two_arms_differ_only_in_the_documented_constants() {
    for (w, n) in WIDE_BLOCKS.iter().zip(NARROW_BLOCKS.iter()) {
        assert_eq!(w.diagonal, n.diagonal);
        assert_eq!(w.sine_shift, n.sine_shift);
        assert_eq!(w.yaw_mod, n.yaw_mod);
        assert_eq!(w.yaw_bias, n.yaw_bias);
        assert_eq!(w.spread_mod, n.spread_mod);
        assert_eq!(w.spread_bias, n.spread_bias);
        assert_eq!(w.tail_mod, n.tail_mod);
        assert_eq!(
            w.scale_halved, n.scale_halved,
            "the lhu>>1 / lh fork is per block, not per arm"
        );
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
fn the_two_arms_use_different_records() {
    assert_ne!(
        BurstMode::Wide.record_addr(),
        BurstMode::Narrow.record_addr()
    );
    assert_eq!(BurstMode::Wide.record_addr(), 0x801F_5DA4);
    assert_eq!(BurstMode::Narrow.record_addr(), 0x801F_5D0C);
}

#[test]
fn each_trigger_sits_immediately_before_the_record_it_fires() {
    // The trigger is nine words (two header + WAIT/EXT/WAIT/HALT), and the
    // record starts one word past its end - the tail is packed with a
    // single alignment word between records.
    for mode in [BurstMode::Wide, BurstMode::Narrow] {
        let gap = mode.record_addr() - mode.trigger_addr();
        assert_eq!(gap, 0x14, "{mode:?}: trigger is 18 bytes + 2 of padding");
    }
}

// -- angles -------------------------------------------------------------

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
fn the_two_arms_are_a_quarter_turn_apart_and_evenly_spaced() {
    // The property the constants encode: four spawn directions per block,
    // one full turn between them, and the diagonal arm offset by an eighth.
    let card: Vec<u32> = (0..ITERATIONS).map(|i| lut_index(i, false)).collect();
    let diag: Vec<u32> = (0..ITERATIONS).map(|i| lut_index(i, true)).collect();
    for w in card.windows(2) {
        assert_eq!(w[1] - w[0], LUT_TURN / ITERATIONS);
    }
    for w in diag.windows(2) {
        assert_eq!(w[1] - w[0], LUT_TURN / ITERATIONS);
    }
    for (c, d) in card.iter().zip(diag.iter()) {
        assert_eq!(d - c, LUT_TURN / 8);
    }
}

#[test]
fn only_the_diagonal_arm_masks() {
    // Retail's cardinal arm is a bare `sll $s1, $s2, 0xb`. Inside the loop
    // bound the two agree; outside it they must not, or the mask has been
    // added to an arm that has none.
    for i in 0..ITERATIONS {
        assert!(lut_index(i, false) < LUT_TURN);
    }
    assert_eq!(lut_index(4, false), 4096, "cardinal arm does not wrap");
    assert_eq!(lut_index(4, true), 512, "diagonal arm does");
}

// -- arithmetic kernels -------------------------------------------------

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

/// The magic-multiply reciprocals, reproduced exactly as retail computes
/// them and checked against plain truncating division.
///
/// `q = (hi(x * magic) >> shift) - (x >> 31)`, or with the operand added
/// back before the shift for the `0x88888889` magic-with-add form. Every
/// (magic, shift, divisor) triple in the two arms is here; the shift is the
/// part that has been misread before, so the table carries it explicitly.
#[test]
fn every_magic_multiply_is_the_divide_it_is_taken_for() {
    fn hi(a: i32, b: i32) -> i32 {
        (((a as i64) * (b as i64)) >> 32) as i32
    }
    fn q_plain(x: i32, magic: i32, shift: u32) -> i32 {
        (hi(x, magic) >> shift) - (x >> 31)
    }
    fn q_add(x: i32, magic: i32, shift: u32) -> i32 {
        (hi(x, magic).wrapping_add(x) >> shift) - (x >> 31)
    }

    #[allow(clippy::type_complexity)]
    let cases: &[(&str, i32, u32, i32, fn(i32, i32, u32) -> i32)] = &[
        ("blk0 yaw", 0x7F80_7F81u32 as i32, 7, 257, q_plain),
        ("blk0 cos wide", 0x2AAA_AAAB, 3, 48, q_plain),
        ("blk0 cos narrow", 0x2AAA_AAAB, 4, 96, q_plain),
        ("blk0 spread", 0x30C3_0C31, 2, 21, q_plain),
        ("blk0 tail", 0x3E0F_83E1, 3, 33, q_plain),
        ("blk1 yaw", 0x0FE0_3F81, 3, 129, q_plain),
        ("blk1 cos wide", 0x38E3_8E39, 4, 72, q_plain),
        ("blk1 cos narrow", 0x38E3_8E39, 5, 144, q_plain),
        ("blk1 spread", 0x8888_8889u32 as i32, 3, 15, q_add),
        ("blk1 tail", 0x5397_829D, 4, 49, q_plain),
        ("blk2 cos wide", 0x2AAA_AAAB, 4, 96, q_plain),
        ("blk2 cos narrow", 0x2AAA_AAAB, 5, 192, q_plain),
        ("blk2 spread", 0x2E8B_A2E9, 1, 11, q_plain),
        ("blk2 tail", 0x7878_7879, 3, 17, q_plain),
    ];

    let boundaries = [
        i32::MIN,
        i32::MIN + 1,
        -1_073_741_824,
        -65_537,
        -65_536,
        -32_769,
        -32_768,
        32_767,
        32_768,
        65_535,
        65_536,
        1_073_741_823,
        i32::MAX - 1,
        i32::MAX,
    ];
    for &(label, magic, shift, d, form) in cases {
        for x in (-40_000i32..40_000).chain(boundaries) {
            assert_eq!(form(x, magic, shift), x / d, "{label}: /{d} at x={x}");
        }
        // And the modulo the port actually uses agrees with retail's
        // `x - q*d` over the same band.
        for x in -40_000i32..40_000 {
            assert_eq!(
                reciprocal_mod(x, d),
                x - form(x, magic, shift) * d,
                "{label}: modulo at x={x}"
            );
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

// -- the loop -----------------------------------------------------------

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
fn an_exhausted_pool_still_consumes_the_whole_rng_stream() {
    // Retail's two post-spawn `mfhi` chains sit between the `jal`s with no
    // branch on the returned pointer, so a null return does not shorten the
    // draw stream. A host that mirrors the RNG must see all 36.
    let mut h = Host::new(vec![3; 128]);
    h.pool_free = 5;
    let out = run_burst(&mut h, 0);
    assert_eq!(out.len(), 12, "every block still produces a request");
    assert_eq!(h.spawns.len(), 5, "only five fit in the pool");
    assert_eq!(h.cursor, ITERATIONS as usize * DRAWS_PER_ITERATION);
}

#[test]
fn every_spawn_carries_the_arms_record() {
    let mut h = Host::new(vec![0; 128]);
    run_burst(&mut h, 1);
    assert!(
        h.spawns
            .iter()
            .all(|(t, _)| *t == BurstMode::Narrow.record_addr())
    );
}

// -- per-block values ---------------------------------------------------

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
fn the_yaw_folds_the_sine_the_jitter_and_the_bias() {
    let mut h = Host::new(vec![0; 8]);
    h.yaw = 1000;
    let b = WIDE_BLOCKS[0];
    // Iteration 0 -> LUT index 0 -> sin 0, cos 4096.
    let req = run_block(&mut h, b, 0, 0);
    // yaw = 1000 + 0 + (0 % 257) + (-0x80)
    assert_eq!(req.yaw, 1000 - 0x80);
    // spread = 4096/48 + (0 % 21) - 0x0A
    assert_eq!(req.spread, (4096 / 48) - 0x0A);
    // tail = (0 % 33) + 0x70
    assert_eq!(req.tail, 0x70);
}

#[test]
fn the_child_heading_is_the_yaw_modulo_one_turn() {
    // The seater masks `rot[1]` to 12 bits, so a yaw that overflows the
    // circle still lands inside it - including from the negative side,
    // which a signed remainder would get wrong.
    let mut h = Host::new(vec![0; 8]);
    for start in [-4000i16, -1, 0, 1, 4095, 4096, 20000] {
        h.yaw = start;
        h.cursor = 0;
        let req = run_block(&mut h, WIDE_BLOCKS[0], 0, 0);
        assert!(
            req.child_heading() < LUT_TURN as u16,
            "start={start} heading={}",
            req.child_heading()
        );
    }
}

#[test]
fn blocks_zero_and_one_halve_the_parent_scale_and_block_two_does_not() {
    // Retail's fork is `lhu >> 1` vs `lh`, which only diverge for a parent
    // whose +0x72 has the top bit set - so probe there, not just at 0x1000.
    for parent in [0u16, 1, 0x1000, 0x7FFF, 0x8000, 0xFFFF] {
        assert_eq!(
            WIDE_BLOCKS[0].child_scale(parent),
            (parent >> 1) as i32,
            "parent={parent:#06X}"
        );
        assert_eq!(WIDE_BLOCKS[1].child_scale(parent), (parent >> 1) as i32);
        assert_eq!(
            WIDE_BLOCKS[2].child_scale(parent),
            parent as i16 as i32,
            "block 2 sign-extends and does not halve"
        );
    }
    // The two forms really are distinguishable, or the test is vacuous.
    assert_ne!(
        WIDE_BLOCKS[0].child_scale(0x8000),
        WIDE_BLOCKS[2].child_scale(0x8000)
    );
}

#[test]
fn a_burst_carries_two_distinct_child_scales() {
    let mut h = Host::new(vec![0; 128]);
    h.scale = 0x1000;
    let out = run_burst(&mut h, 0);
    for (i, req) in out.iter().enumerate() {
        let want = if i % SPAWNS_PER_ITERATION == 2 {
            0x1000
        } else {
            0x0800
        };
        assert_eq!(req.scale, want, "block {} of iteration {}", i % 3, i / 3);
    }
}

#[test]
fn the_same_draws_place_narrow_spawns_tighter_than_wide() {
    let draws = vec![7i32; 128];
    let mut hw = Host::new(draws.clone());
    let mut hn = Host::new(draws);
    let wide = run_burst(&mut hw, 0);
    let narrow = run_burst(&mut hn, 1);
    assert_eq!(wide.len(), narrow.len());
    // Yaw and scale are arm-independent; the spread and tail are not.
    for (w, n) in wide.iter().zip(narrow.iter()) {
        assert_eq!(w.yaw, n.yaw);
        assert_eq!(w.scale, n.scale);
        assert!(n.tail < w.tail);
    }
}

#[test]
fn draw_order_is_yaw_then_spread_then_tail() {
    // Distinct draws make the ordering observable.
    let mut h = Host::new(vec![100, 200, 300]);
    h.yaw = 0;
    let b = WIDE_BLOCKS[0];
    let req = run_block(&mut h, b, 0, 0);
    assert_eq!(
        req.yaw,
        (reciprocal_mod(100, b.yaw_mod) + b.yaw_bias) as i16
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
    assert_ne!(req.yaw, req.spread);
    assert_ne!(req.spread, req.tail);
}

// -- the record parser --------------------------------------------------

/// Build a synthetic image around a hand-assembled record so the parser is
/// exercised without disc data. Real-image coverage is the disc-gated
/// `battle_burst_records` test.
fn image_with(base_va: u32, record_va: u32, words: &[u16]) -> Vec<u8> {
    let off = (record_va - base_va) as usize;
    let mut img = vec![0xAAu8; off + words.len() * 2 + 64];
    for (i, w) in words.iter().enumerate() {
        img[off + i * 2..off + i * 2 + 2].copy_from_slice(&w.to_le_bytes());
    }
    img
}

#[test]
fn a_record_walk_stops_at_halt_and_keeps_the_halt_word() {
    let base = 0x801C_E818;
    let va = BurstMode::Wide.record_addr();
    // [model_sel=-1][flags=0] 0x39 a b c | 0x06 v | 0x08
    let words = [0xFFFF, 0x0000, 0x39, 1, 2, 3, 0x06, 9, 0x08, 0x1234, 0x5678];
    let img = image_with(base, va, &words);
    let rec = BurstRecord::parse(&img, base, BurstMode::Wide).expect("parses");
    assert_eq!(rec.model_sel, -1);
    assert_eq!(rec.reserved, 0);
    assert_eq!(rec.opcode_sequence(), vec![0x39, 0x06, 0x08]);
    assert_eq!(
        rec.program.last().copied(),
        Some(0x08),
        "the terminator is part of the program"
    );
    assert_eq!(rec.byte_len(), (2 + 7) * 2);
    assert!(
        !rec.program.contains(&0x1234),
        "nothing past HALT is captured"
    );
}

#[test]
fn a_record_that_never_halts_is_rejected() {
    let base = 0x801C_E818;
    let va = BurstMode::Wide.record_addr();
    // 0x47 is past the dispatcher's bound check - retail ends the loop
    // there, but it is not a terminator, so the walk must refuse it.
    let words = [0xFFFF, 0x0000, 0x47, 0x47, 0x47];
    let img = image_with(base, va, &words);
    assert!(BurstRecord::parse(&img, base, BurstMode::Wide).is_none());
}

#[test]
fn an_address_outside_the_image_is_rejected() {
    let base = 0x801C_E818;
    let img = vec![0u8; 16];
    assert!(BurstRecord::parse(&img, base, BurstMode::Wide).is_none());
    // A record address below the base underflows rather than wrapping.
    assert!(BurstRecord::parse_at(&img, base, base - 4).is_none());
}

#[test]
fn a_trigger_record_is_the_same_format_as_a_stager_record() {
    // Structural: the trigger shape the two arms share is
    // [model_sel][flags] WAIT_SET 0 / 0x17 mode / WAIT_SET 0 / HALT.
    let base = 0x801C_E818;
    let va = BurstMode::Wide.trigger_addr();
    let words = [0xFFFF, 0x0000, 0x09, 0, 0x17, 0, 0x09, 0, 0x08];
    let img = image_with(base, va, &words);
    let rec = BurstRecord::parse_at(&img, base, va).expect("parses");
    assert_eq!(rec.opcode_sequence(), vec![0x09, 0x17, 0x09, 0x08]);
    assert_eq!(rec.byte_len(), 18, "nine words including the header");
    assert_eq!(
        rec.byte_len() + 2,
        (BurstMode::Wide.record_addr() - va) as usize,
        "the stager record starts one word past the trigger's end"
    );
}
