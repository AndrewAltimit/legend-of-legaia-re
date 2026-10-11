use super::*;

fn tail_rec(combo: &[u8]) -> [u8; 0xD0] {
    let mut r = [0u8; 0xD0];
    r[..combo.len()].copy_from_slice(combo);
    r
}

/// A scripted `rand()`: the listed draws in order, then `fallback`.
fn script(draws: &[i32], fallback: i32) -> impl FnMut() -> i32 + '_ {
    let mut it = draws.iter();
    move || it.next().copied().unwrap_or(fallback)
}

#[test]
fn the_tail_stops_at_once_below_the_spirit_gate() {
    // Record 0xB is a two-arrow art; Spirit 0x12 fails `r%7 + 0x12 <
    // budget` for every draw.
    let mut recs = vec![[0u8; 0xD0]; 0x0B];
    recs.push(tail_rec(&[1, 2]));
    let mut q = vec![0x0C, 0x0D, 0x0C, 0x0D];
    let input = ArtsTailInput {
        char_id: 1,
        spirit: 0x12,
        ability_high: 0,
        learned: &[0],
        miracle_marker: true,
        records: &recs,
        art_count: recs.len() as u8,
    };
    // Draws: opening %5 = 0 -> art 0xB; gate %7; step %2.
    let out = insert_arts(&mut q, &input, script(&[0, 0, 0], 0));
    assert!(out.inserted.is_empty());
    assert_eq!(out.draws, 3, "opening roll, gate roll, the common step");
    assert_eq!(q, vec![0x0C, 0x0D, 0x0C, 0x0D], "queue untouched");
    assert_eq!(out.budget_left, 0x12);
}

#[test]
fn an_accepted_art_lands_on_the_free_regions_tail() {
    // Art 0xB = arrows [2, 1]: need counts only byte 1 (one `1`), and it
    // is spliced as [0x0D, 0x0C] over the last two slots.
    let mut recs = vec![[0u8; 0xD0]; 0x0B];
    recs.push(tail_rec(&[2, 1]));
    let mut q = vec![0x0C, 0x0C, 0x0D, 0x0E];
    let input = ArtsTailInput {
        char_id: 1,
        spirit: 100,
        ability_high: 0,
        learned: &[0],
        miracle_marker: true,
        records: &recs,
        art_count: recs.len() as u8,
    };
    // opening 0 -> 0xB; gate 0 (0x12 < 100); chance 0 (< 50); refill two
    // head slots: dir 0 (have 2-1=1), dir 2 (have 1); re-seed; step.
    let out = insert_arts(&mut q, &input, script(&[0, 0, 0, 0, 2, 0, 0], 0));
    assert_eq!(out.inserted.len(), 1);
    let ins = out.inserted[0];
    assert_eq!((ins.art, ins.at, ins.len, ins.clipped), (0x0B, 2, 2, false));
    assert_eq!(ins.cost, 2 * i32::from(TAIL_COST_FIRST));
    assert_eq!(q, vec![0x0C, 0x0E, 0x0D, 0x0C]);
    assert_eq!(out.budget_left, 100 - 22);
}

#[test]
fn a_rejected_art_skips_twice_its_plus_0x0b_run() {
    // Art 0xB is not learned; its +0x0B run is two bytes long, so the
    // walk skips 4 records, then steps +1 -> 0x10 = the count: done.
    let mut recs = vec![[0u8; 0xD0]; 0x0B];
    let mut r = [0u8; 0xD0];
    r[0] = 1;
    r[1] = 1;
    r[0x0B] = 5;
    r[0x0C] = 6;
    recs.push(r);
    recs.resize(recs.len() + 4, [0u8; 0xD0]);
    let mut q = vec![0x0C, 0x0C, 0x0C];
    let input = ArtsTailInput {
        char_id: 1,
        spirit: 100,
        ability_high: 0,
        learned: &[],
        miracle_marker: true,
        records: &recs,
        art_count: recs.len() as u8,
    };
    let out = insert_arts(&mut q, &input, script(&[0, 0, 0, 0], 0));
    assert!(out.inserted.is_empty());
    // opening, gate, chance, step - then 0xB + 4 + 1 = 0x10 >= 0x10.
    assert_eq!(out.draws, 4);
}

#[test]
fn noa_steps_over_records_0xd_and_0xe() {
    let mut recs = vec![[0u8; 0xD0]; 0x0F];
    recs.push(tail_rec(&[1, 1]));
    let mut q = vec![0x0C, 0x0C, 0x0C];
    let input = ArtsTailInput {
        char_id: 2,
        spirit: 100,
        ability_high: TAIL_COST_HALVING_BIT,
        learned: &[4],
        miracle_marker: true,
        records: &recs,
        art_count: recs.len() as u8,
    };
    // opening %5 = 2 -> 0xD -> Noa's +2 -> 0xF.
    let out = insert_arts(&mut q, &input, script(&[2, 0, 0, 0, 0, 0], 0));
    assert_eq!(out.inserted.len(), 1);
    assert_eq!(out.inserted[0].art, 0x0F);
    // The halving bit: 0xB >> 1 = 5 per input.
    assert_eq!(out.inserted[0].cost, 2 * 5);
}

#[test]
fn gate_needs_the_bit_set_and_the_status_clear() {
    assert!(gate_selects_auto_fill(AUTO_FILL_ABILITY_BIT, 0));
    assert!(!gate_selects_auto_fill(0, 0));
    assert!(!gate_selects_auto_fill(AUTO_FILL_ABILITY_BIT, 0x0004));
    assert!(!gate_selects_auto_fill(AUTO_FILL_ABILITY_BIT, 0x0400));
    // Bits outside the mask do not veto.
    assert!(gate_selects_auto_fill(AUTO_FILL_ABILITY_BIT, 0x0380));
}

#[test]
fn only_character_two_gets_the_higher_floor() {
    assert_eq!(auto_fill_floor(1), 4);
    assert_eq!(auto_fill_floor(2), 6);
    assert_eq!(auto_fill_floor(3), 4);
    assert_eq!(auto_fill_floor(0), 4);
}

#[test]
fn stop_roll_fires_on_every_seventh_value() {
    assert!(auto_fill_should_stop(0));
    assert!(auto_fill_should_stop(7));
    assert!(auto_fill_should_stop(70));
    for r in 1..7 {
        assert!(!auto_fill_should_stop(r), "r={r}");
    }
}

#[test]
fn auto_fill_bails_on_an_empty_learned_list() {
    let mut n = 0;
    let out = auto_fill_queue(1, &[], || {
        n += 1;
        1
    });
    assert!(out.bytes.is_empty());
    assert_eq!(n, 0, "the count gate precedes every draw");
}

#[test]
fn auto_fill_biases_each_kept_art_and_discards_the_ones_under_the_floor() {
    // learned = [2, 9]: 2 is under the floor of 4, 9 is over it.
    let learned = [2u8, 9];
    // Draws, in call order: (stop=1, pick=0) -> discard;
    // (stop=1, pick=1) -> keep; (stop=7) -> stop.
    let mut script = vec![1, 0, 1, 1, 7].into_iter();
    let out = auto_fill_queue(1, &learned, || script.next().unwrap());
    assert_eq!(out.discarded, 1);
    assert_eq!(out.bytes, vec![9 + ART_ACTION_BIAS]);
}

#[test]
fn auto_fill_stops_at_the_queue_limit() {
    let learned = [9u8];
    // Never stop, always pick index 0.
    let out = auto_fill_queue(1, &learned, || 1);
    assert_eq!(out.bytes.len(), AUTO_FILL_QUEUE_LIMIT);
    assert!(out.bytes.iter().all(|&b| b == 9 + ART_ACTION_BIAS));
}

#[test]
fn floor_six_rejects_arts_a_floor_of_four_would_keep() {
    let learned = [5u8];
    let keep = auto_fill_queue(1, &learned, {
        let mut n = 0;
        move || {
            n += 1;
            if n > 2 { 7 } else { 1 }
        }
    });
    assert_eq!(keep.bytes, vec![5 + ART_ACTION_BIAS]);
    let reject = auto_fill_queue(2, &learned, {
        let mut n = 0;
        move || {
            n += 1;
            if n > 2 { 7 } else { 1 }
        }
    });
    assert!(reject.bytes.is_empty());
    assert_eq!(reject.discarded, 1);
}

#[test]
fn weight_family_comes_off_the_monster_type_byte() {
    assert_eq!(weight_family(2), WeightFamily::TypeTwo);
    assert_eq!(weight_family(3), WeightFamily::TypeThree);
    for t in [0u8, 1, 4, 0xFF] {
        assert_eq!(weight_family(t), WeightFamily::None, "type {t}");
    }
}

#[test]
fn no_family_leaves_every_command_at_the_default_weight() {
    assert_eq!(
        command_weight(&[0x12, 0x1C, 0, 0], WeightFamily::None),
        WEIGHT_DEFAULT
    );
}

#[test]
fn the_weight_ladder_walks_default_then_one_band_then_both() {
    use WeightFamily::TypeTwo;
    // Nothing in either band.
    assert_eq!(command_weight(&[0x20, 0x21, 0, 0], TypeTwo), WEIGHT_DEFAULT);
    // Low band only.
    assert_eq!(command_weight(&[0x11, 0x20, 0, 0], TypeTwo), WEIGHT_LOW);
    // High band only.
    assert_eq!(command_weight(&[0x1B, 0x20, 0, 0], TypeTwo), WEIGHT_HIGH);
    // Low then high, and high then low, both land on 2.
    assert_eq!(command_weight(&[0x11, 0x1B, 0, 0], TypeTwo), WEIGHT_BOTH);
    assert_eq!(command_weight(&[0x1B, 0x11, 0, 0], TypeTwo), WEIGHT_BOTH);
}

#[test]
fn the_other_family_uses_the_other_two_bands() {
    use WeightFamily::TypeThree;
    // `..=0x10` is the low band here, so 0x11 misses it entirely.
    assert_eq!(command_weight(&[0x11, 0, 0, 0], TypeThree), WEIGHT_DEFAULT);
    assert_eq!(command_weight(&[0x10, 0, 0, 0], TypeThree), WEIGHT_LOW);
    assert_eq!(command_weight(&[0x16, 0, 0, 0], TypeThree), WEIGHT_HIGH);
    assert_eq!(command_weight(&[0x1A, 0x05, 0, 0], TypeThree), WEIGHT_BOTH);
    // 0x1B is past the high band for this family.
    assert_eq!(command_weight(&[0x1B, 0, 0, 0], TypeThree), WEIGHT_DEFAULT);
}

#[test]
fn a_zero_byte_ends_the_ladder_scan() {
    use WeightFamily::TypeTwo;
    // The 0x1B after the terminator is never seen.
    assert_eq!(command_weight(&[0x11, 0, 0x1B, 0x1B], TypeTwo), WEIGHT_LOW);
}

#[test]
fn the_ladder_reads_at_most_four_bytes() {
    use WeightFamily::TypeTwo;
    let bytes = [0x20, 0x20, 0x20, 0x20];
    assert_eq!(command_weight(&bytes, TypeTwo), WEIGHT_DEFAULT);
    // A fifth byte in a band cannot be reached.
    assert_eq!(command_weight(&[0x20, 0x20, 0x20, 0x20], TypeTwo), 8);
}

fn cmd(id: u8, cost: u8, bytes: [u8; 4], guard: u16) -> ArtsCommand {
    ArtsCommand {
        id,
        cost,
        bytes,
        guard,
    }
}

#[test]
fn pool_repeats_each_command_by_its_weight_and_tracks_the_cheapest() {
    let commands = [
        cmd(0x0C, 20, [0x11, 0, 0, 0], 0),    // low band -> weight 1
        cmd(0x0D, 8, [0x1B, 0, 0, 0], 0),     // high band -> weight 4
        cmd(0x0E, 30, [0x11, 0x1B, 0, 0], 0), // both -> weight 2
    ];
    let pool = build_candidate_pool(&commands, WeightFamily::TypeTwo, 0);
    assert_eq!(pool.min_cost, 8);
    assert_eq!(pool.entries.iter().filter(|&&c| c == 0x0C).count(), 1);
    assert_eq!(pool.entries.iter().filter(|&&c| c == 0x0D).count(), 4);
    assert_eq!(pool.entries.iter().filter(|&&c| c == 0x0E).count(), 2);
    assert!(!pool.overflowed);
}

#[test]
fn a_guard_hit_drops_a_command_but_still_lowers_the_min_cost() {
    let commands = [
        cmd(0x0C, 4, [0x11, 0, 0, 0], 0x0080),
        cmd(0x0D, 20, [0x11, 0, 0, 0], 0),
    ];
    let pool = build_candidate_pool(&commands, WeightFamily::TypeTwo, 0x0080);
    assert!(!pool.entries.contains(&0x0C), "guarded out");
    assert!(pool.entries.contains(&0x0D));
    assert_eq!(
        pool.min_cost, 4,
        "the min-cost scan runs before the guard test"
    );
}

#[test]
fn the_default_weight_can_outgrow_retails_scratch() {
    // Four commands, no family match, weight 8 apiece = 32 pushes into a
    // 16-byte buffer.
    let commands: Vec<ArtsCommand> = (FIRST_COMMAND..LAST_COMMAND_EXCL)
        .map(|id| cmd(id, 10, [0x20, 0, 0, 0], 0))
        .collect();
    let pool = build_candidate_pool(&commands, WeightFamily::None, 0);
    assert!(pool.overflowed);
    assert_eq!(pool.entries.len(), CANDIDATE_SCRATCH);
}

#[test]
fn spend_is_skipped_when_the_gauge_cannot_cover_the_cheapest() {
    let pool = CandidatePool {
        entries: vec![0x0C, 0x0C],
        min_cost: 20,
        overflowed: false,
    };
    let mut n = 0;
    let out = spend_gauge(
        &pool,
        19,
        |_| 20,
        || {
            n += 1;
            0
        },
    );
    assert!(out.queue.is_empty());
    assert_eq!(out.gauge_left, 19);
    assert_eq!(n, 0, "the gate precedes the first draw");
}

#[test]
fn spend_consumes_the_gauge_and_stops_below_the_floor() {
    let pool = CandidatePool {
        entries: vec![0x0C, 0x0D, 0x0E, 0x0F],
        min_cost: 10,
        overflowed: false,
    };
    // Draw slots 0, 1, 2, 3 in order.
    let mut seq = (0..8).cycle();
    let out = spend_gauge(&pool, 25, |_| 10, || seq.next().unwrap());
    assert_eq!(out.queue.len(), 2, "25 AP buys two 10-cost commands");
    assert_eq!(out.gauge_left, 5);
}

#[test]
fn spend_stops_once_every_entry_is_consumed() {
    let pool = CandidatePool {
        entries: vec![0x0C, 0x0D],
        min_cost: 1,
        overflowed: false,
    };
    let mut seq = (0..2).cycle();
    let out = spend_gauge(&pool, 1000, |_| 1, || seq.next().unwrap());
    assert_eq!(out.queue.len(), 2);
    assert_eq!(out.gauge_left, 998);
}

#[test]
fn a_pick_the_gauge_cannot_afford_is_refused_without_spending() {
    let pool = CandidatePool {
        entries: vec![0x0C, 0x0D],
        min_cost: 5,
        overflowed: false,
    };
    // Slot 0 is a 100-cost command the gauge cannot buy; slot 1 costs 5.
    let mut seq = [0i32, 0, 1].into_iter().chain(std::iter::repeat(1));
    let out = spend_gauge(
        &pool,
        9,
        |c| if c == 0x0C { 100 } else { 5 },
        || seq.next().unwrap(),
    );
    assert_eq!(out.queue, vec![0x0D]);
    assert_eq!(out.gauge_left, 4);
}

#[test]
fn target_roll_lands_in_the_monster_slot_band() {
    for draw in 0..12 {
        let slot = roll_target_slot(5, draw);
        assert!((3..8).contains(&slot), "draw {draw} -> slot {slot}");
    }
    assert_eq!(roll_target_slot(1, 999), 3);
}

#[test]
fn liveness_predicate_matches_the_hp_halfword_test() {
    assert!(slot_is_live(&FleeActor {
        hp: 1,
        max_hp: 1,
        atk: 0
    }));
    assert!(!slot_is_live(&FleeActor::default()));
}
