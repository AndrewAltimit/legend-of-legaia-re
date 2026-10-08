use super::*;

#[test]
fn tally_stalls_each_row_until_it_has_faded_in() {
    let mut t = BakaTally::new([10, 0, 0, 0]);
    // Below the fade gate nothing moves.
    for _ in 0..(TALLY_FADE_GATE - 1) {
        t.tick(1, false);
    }
    assert_eq!(t.counters()[0], 10, "row stalls while it fades in");
    assert!(t.take_cues().is_empty(), "a stalled row is silent");
    t.tick(1, false);
    assert!(t.counters()[0] < 10, "row drains once faded in");
    assert_eq!(t.take_cues(), vec![BAKA_CUE_CURSOR]);
}

#[test]
fn tally_drains_rows_strictly_in_order_and_splits_score_from_gold() {
    let mut t = BakaTally::new([7, 5, 3, 100]);
    for _ in 0..4000 {
        if t.done() {
            break;
        }
        t.tick(1, false);
    }
    assert!(t.done(), "every row empties");
    assert_eq!(t.total(), 7 + 5 + 3, "score rows feed the total");
    assert_eq!(t.gold_drained(), 100, "the prize row feeds gold");
    // Later rows only start after earlier ones finish, so each row needs
    // its own fade-in: the run is longer than a single row's would be.
    let mut solo = BakaTally::new([0, 0, 0, 100]);
    let mut solo_frames = 0;
    while !solo.done() {
        solo.tick(1, false);
        solo_frames += 1;
    }
    assert!(solo_frames > TALLY_FADE_GATE);
}

#[test]
fn tally_fast_forward_latches_and_snaps_to_the_end() {
    let mut t = BakaTally::new([0, 0, 0, 460]);
    for _ in 0..TALLY_FADE_GATE {
        t.tick(1, false);
    }
    // One face-button frame latches the fast-forward for good.
    t.tick(1, true);
    assert!(t.done(), "the whole remainder moves in one step");
    assert_eq!(t.gold_drained(), 460);
}

#[test]
fn tally_gold_is_taken_incrementally_and_sums_to_the_prize() {
    let mut t = BakaTally::new([0, 0, 0, 100]);
    let mut banked = 0;
    let mut takes = 0;
    while !t.done() {
        t.tick(1, false);
        let got = t.take_gold();
        if got > 0 {
            banked += got;
            takes += 1;
        }
    }
    assert_eq!(banked, 100, "every coin reaches the host exactly once");
    assert!(
        takes > 1,
        "the prize arrives over several frames, not at once"
    );
    assert_eq!(t.take_gold(), 0, "nothing left to take");
}

#[test]
fn tally_drain_accelerates_then_ticks_out_one_at_a_time() {
    // Large remainder: a fifth per frame.
    assert_eq!(tally_drain_step(100, false), 20);
    assert_eq!(tally_drain_step(6, false), 1);
    // The 3..=5 band halves.
    assert_eq!(tally_drain_step(5, false), 2);
    assert_eq!(tally_drain_step(4, false), 2);
    assert_eq!(tally_drain_step(3, false), 1);
    // Below 3 the step is exactly one, which is what lands it on zero.
    assert_eq!(tally_drain_step(2, false), 1);
    assert_eq!(tally_drain_step(1, false), 1);
}

#[test]
fn tally_fast_forward_moves_the_whole_remainder() {
    assert_eq!(tally_drain_step(1234, true), 1234);
    assert_eq!(tally_drain_sequence(1234).iter().sum::<i32>(), 1234);
}

#[test]
fn tally_sequence_always_terminates_at_exactly_the_total() {
    for amount in [0, 1, 2, 3, 5, 6, 30, 460, 9999] {
        let steps = tally_drain_sequence(amount);
        assert_eq!(
            steps.iter().sum::<i32>(),
            amount,
            "tally of {amount} drains to exactly zero"
        );
        assert!(steps.iter().all(|&s| s > 0), "no zero-length step stalls");
    }
    assert!(tally_drain_sequence(0).is_empty());
    assert!(
        tally_drain_sequence(-5).is_empty(),
        "negative treated as empty"
    );
}

fn cfg(roster_id: usize, power: i32) -> FighterConfig {
    FighterConfig {
        roster_id,
        damage_mod: 100,
        def_tiers: [0, 0, 0],
        crit_chance: 0,
        atk_tiers: [0, 0, 0],
        attack_power: [0, power, power, power, 0],
        gold_reward: 30,
        ai_pattern: vec![1, 2, 3],
    }
}

#[test]
fn a_held_triangle_at_the_round_setup_sends_the_cameo_on() {
    let mut f = fight();
    f.set_held_pad(crate::baka_fighter_chrome::CAMEO_HOLD_MASK);
    f.tick(1);
    let c = f.cameo().expect("the cameo spawns on the held word");
    assert_eq!(c.phase, 1);
    // It walks, poses, walks off and retires by the retire phase.
    for _ in 0..crate::baka_fighter_chrome::CAMEO_RETIRE_PHASE {
        f.tick(1);
    }
    assert!(f.cameo().is_none(), "retired");
    // No held word, no cameo.
    let mut g = fight();
    g.tick(1);
    assert!(g.cameo().is_none());
}

fn fight() -> BakaFight {
    let mut f = BakaFight::new(cfg(0, 10), cfg(1, 10), 1);
    f.ai_controlled = [false, false]; // deterministic: drive both by hand
    f
}

/// The resolution SM's banner tail: the first frame the foe is down with
/// the player untouched raises PERFECT!!, once; a hit on the player turns
/// the next round's win into YOU WIN!; the banners come down when the
/// cabinet leaves the decided round.
#[test]
fn a_decided_round_raises_its_result_banner_once() {
    use crate::baka_fighter_chrome::{RESULT_PERFECT_SPRITE, RESULT_WIN_SPRITE};
    let mut f = fight();
    f.f[1].hp = 0;
    f.tick_result_banner();
    let ids: Vec<u16> = f.chrome.result_sprites().iter().map(|a| a.id).collect();
    assert_eq!(
        ids,
        vec![RESULT_PERFECT_SPRITE],
        "untouched win is PERFECT!!"
    );
    f.chrome.clear_result();
    f.tick_result_banner();
    assert!(
        f.chrome.result_sprites().is_empty(),
        "latched: once per round"
    );

    let mut g = fight();
    g.apply_damage(0);
    g.f[0].hp = g.f[0].hp.max(1);
    g.f[1].hp = 0;
    g.tick_result_banner();
    let ids: Vec<u16> = g.chrome.result_sprites().iter().map(|a| a.id).collect();
    assert_eq!(ids[0], RESULT_WIN_SPRITE, "a hit player's win is YOU WIN!");
}

/// Every announcer line the chrome starts over a whole match was on a
/// prestage list drained before it - the list the world queues at entry
/// and after each tick - so an asynchronous host has it staged in time.
#[test]
fn every_announcer_line_is_prestaged_before_it_fires() {
    let mut f = BakaFight::new(cfg(0, 10), cfg(1, 10), 1).with_intro_card();
    f.ai_controlled = [true, true];
    let mut listed = f.take_xa_prestage();
    assert!(!listed.is_empty(), "the entry list");
    let mut fired = 0;
    let mut rounds = 0;
    for _ in 0..20_000 {
        if f.match_over() {
            break;
        }
        f.tick(1);
        if let Some(c) = f.chrome_frame().xa {
            assert!(listed.contains(&c), "{c:?} fired before it was listed");
            fired += 1;
        }
        rounds = rounds.max(f.round());
        listed.extend(f.take_xa_prestage());
    }
    assert!(fired > 0, "the chrome started no line");
    assert!(rounds > 0, "the match never advanced a round");
    // Nothing is listed twice.
    let mut dedup = listed.clone();
    dedup.dedup();
    assert_eq!(dedup.len(), listed.len());
}

#[test]
fn a_decided_exchange_queues_the_hit_cue_and_a_draw_queues_none() {
    let mut f = fight();
    // Undecided: nobody has chosen, so no damage and no cue.
    f.tick(1);
    assert!(f.take_cues().is_empty(), "no exchange, no cue");

    // 2 beats 1 -> slot 1 wins, damage lands on slot 0, cue 9 fires once.
    f.choose(0, BakaAttack::A);
    f.choose(1, BakaAttack::B);
    f.tick(1);
    assert_eq!(f.take_cues(), vec![BAKA_CUE_HIT]);
    // Drained.
    assert!(f.take_cues().is_empty());

    // A draw (same type both sides) resolves without applying damage.
    f.choose(0, BakaAttack::A);
    f.choose(1, BakaAttack::A);
    f.tick(1);
    assert!(
        f.take_cues().is_empty(),
        "a drawn exchange applies no damage, so fires no hit cue"
    );
}

#[test]
fn beats_relation_matches_the_dump() {
    // 2 beats 1, 3 beats 2, 1 beats 3.
    for (a, b, w) in [
        (BakaAttack::A, BakaAttack::B, 1usize),
        (BakaAttack::B, BakaAttack::C, 1),
        (BakaAttack::C, BakaAttack::A, 1),
        (BakaAttack::B, BakaAttack::A, 0),
        (BakaAttack::C, BakaAttack::B, 0),
        (BakaAttack::A, BakaAttack::C, 0),
    ] {
        let mut f = fight();
        assert!(f.choose(0, a));
        assert!(f.choose(1, b));
        f.tick(1);
        let r = f.last_exchange().expect("resolved");
        assert_eq!(r.winner, w, "{a:?} vs {b:?}");
        assert!(!r.draw);
        // Loser took damage; base formula: hit=10, guard=100 →
        // 10*100*0x20/100 = 320, first hit combo bonus (0-1)*0x40 = -64.
        assert_eq!(r.damage, 320 - 64);
        assert_eq!(f.hp(w ^ 1), HP_START - 256);
    }
}

#[test]
fn same_type_is_a_draw_damaging_both() {
    let mut f = fight();
    assert!(f.choose(0, BakaAttack::B));
    assert!(f.choose(1, BakaAttack::B));
    f.tick(1);
    let r = f.last_exchange().expect("resolved");
    assert!(r.draw);
    assert_eq!(f.hp(0), HP_START - 256);
    assert_eq!(f.hp(1), HP_START - 256);
}

/// Throw the special the way the auto-finisher does (it is never a
/// [`BakaFight::choose`] option).
fn throw_special(f: &mut BakaFight, slot: usize) {
    f.f[slot].chosen = Some(BakaAttack::Special);
    f.commit(slot, BakaAttack::Special);
}

#[test]
fn the_special_is_not_a_choice() {
    // Retail's type 4 has no button: a host asking for it is refused,
    // and the exchange stays open.
    let mut f = fight();
    assert!(!f.choose(0, BakaAttack::Special));
    assert_eq!(f.chosen(0), None);
    f.tick(1);
    assert!(f.last_exchange().is_none());
    assert_eq!(f.round_wins(0), 0);
}

#[test]
fn special_beats_everything_with_fighter0_priority() {
    let mut f = striking_fight(16, &[2], &[1]);
    throw_special(&mut f, 0);
    throw_special(&mut f, 1);
    let mut ticks = 0;
    while f.last_exchange().is_none() {
        f.tick(1);
        ticks += 1;
        assert!(ticks < 10, "the specials strike");
    }
    let r = f.last_exchange().expect("resolved");
    assert_eq!(r.winner, 0);
    // Special power is 0: the hit itself is the combo term only.
    assert_eq!(f.hp(0), HP_START);
}

#[test]
fn special_won_exchange_never_moves_hp_and_never_heals() {
    // The regression at the value, not just the sign. The special's action
    // record carries power 0 (all 17 fighters on the disc), so the raw
    // kernel total for a special on a fresh combo is the bare combo term
    // `(0-1)*0x40 = -64` - which used to be *applied*, healing the foe
    // from 3200 to 3264 ("you hit -64"). The kernel's HP write is
    // `hp > 0`-gated (`overlay_baka_fighter_801d3b18.txt` `0x801d3e58`),
    // so a special-won exchange lands exactly zero HP change.
    let mut f = striking_fight(16, &[9], &[1, 3]);
    throw_special(&mut f, 0);
    assert!(f.choose(1, BakaAttack::A));
    let mut ticks = 0;
    while f.last_exchange().is_none() {
        f.tick(1);
        ticks += 1;
        assert!(ticks < 10, "the special's first strike lands");
    }
    let r = f.last_exchange().expect("resolved");
    assert_eq!(r.winner, 0, "the special wins the exchange");
    assert!(!r.special_round_win, "first of two strikes - no round win");
    assert_eq!(r.damage, 0, "a special-won exchange's HP delta is zero");
    assert_eq!(f.hp(1), HP_START, "the foe is neither damaged nor healed");
    assert_eq!(f.hp(0), HP_START);
}

#[test]
fn a_knockout_throws_the_finisher_and_its_last_strike_takes_the_round() {
    // Attacks strike on frame 2, the special on frames 1 and 3.
    let mut f = striking_fight(16, &[2], &[1, 3]);
    f.f[1].hp = 1;
    assert!(f.choose(0, BakaAttack::B)); // B beats A
    assert!(f.choose(1, BakaAttack::A));
    let mut ticks = 0;
    while f.hp(1) != 0 {
        f.tick(1);
        ticks += 1;
        assert!(ticks < 10, "the knockout lands");
    }
    // Retail's auto-finisher gate: the round is not over at the KO; the
    // winner throws the special on its own.
    assert_eq!(f.phase(), MatchPhase::Fighting);
    assert_eq!(f.chosen(0), Some(BakaAttack::Special));
    assert_eq!(f.round_wins(0), 0);
    // The downed foe cannot act while the finisher plays.
    assert!(!f.can_choose(1));
    assert!(!f.choose(1, BakaAttack::C));
    for _ in 0..40 {
        f.tick(1);
        if f.round_wins(0) == 1 {
            break;
        }
    }
    assert_eq!(
        f.round_wins(0),
        1,
        "the finisher's last strike credits the round"
    );
    assert!(f.last_exchange().unwrap().special_round_win);
    assert!(matches!(f.phase(), MatchPhase::RoundOver(0)));
}

#[test]
fn a_bare_config_knockout_ends_the_round_on_the_spot() {
    let mut f = fight();
    f.f[1].hp = 1;
    assert!(f.choose(0, BakaAttack::B));
    assert!(f.choose(1, BakaAttack::A));
    f.tick(1);
    assert_eq!(f.hp(1), 0);
    assert_eq!(f.round_wins(0), 1);
    assert!(matches!(f.phase(), MatchPhase::RoundOver(0)));
}

#[test]
fn attack_on_idle_opponent_never_lands() {
    let mut f = fight();
    assert!(f.choose(0, BakaAttack::A));
    for _ in 0..10 {
        f.tick(1);
    }
    assert!(f.last_exchange().is_none());
    assert_eq!(f.hp(1), HP_START);
}

#[test]
fn combo_bonus_escalates_on_consecutive_hits() {
    let mut f = fight();
    // Hit fighter 1 twice; second hit carries combo=1 → bonus 0.
    assert!(f.choose(0, BakaAttack::B));
    assert!(f.choose(1, BakaAttack::A));
    f.tick(1);
    let first = f.last_exchange().unwrap().damage;
    // Cooldown: fighter-0 win leaves slot 0 free, slot 1 at 200 (decays
    // 16/frame → ~13 frames).
    for _ in 0..13 {
        f.tick(1);
    }
    assert!(f.choose(0, BakaAttack::B));
    assert!(f.choose(1, BakaAttack::A));
    f.tick(1);
    let second = f.last_exchange().unwrap().damage;
    assert_eq!(second, first + COMBO_DAMAGE_STEP);
}

#[test]
fn fighter_pack_entry_folds_the_party_slots() {
    assert_eq!(fighter_pack_entry(0), (0, 0x4B6));
    assert_eq!(fighter_pack_entry(2), (2, 0x4B8));
    // Roster 3 is the first ladder fighter and folds back onto entry 0.
    assert_eq!(fighter_pack_entry(3), (0, 0x4B6));
    assert_eq!(fighter_pack_entry(16), (13, 0x4C3));
}

#[test]
fn fighter_pack_walk_stops_at_a_zero_header() {
    let mut buf = Vec::new();
    let mut chunk = |kind: u8, len: usize| {
        let hdr = ((kind as u32) << 24) | len as u32;
        buf.extend_from_slice(&hdr.to_le_bytes());
        buf.extend(std::iter::repeat_n(0xAAu8, len));
    };
    chunk(0, 8);
    chunk(9, 4);
    buf.extend_from_slice(&[0u8; 4]);
    let chunks = fighter_pack_chunks(&buf);
    assert_eq!(
        chunks,
        vec![
            FighterChunk {
                kind: 0,
                offset: 4,
                len: 8
            },
            FighterChunk {
                kind: 9,
                offset: 16,
                len: 4
            },
        ]
    );
}

#[test]
fn the_walk_rounds_a_chunk_length_down_to_a_word() {
    // A 5-byte chunk advances 4 + 4, not 4 + 5 - the retail stride
    // truncates the size before adding the header. So the next header is
    // read at offset 8, one byte inside the first chunk's own payload.
    let mut buf = vec![0x05, 0x00, 0x00, 0x00];
    buf.extend_from_slice(&[0xAA; 4]);
    buf.extend_from_slice(&[0x09, 0x00, 0x00, 0x06]);
    buf.extend_from_slice(&[0u8; 12]);
    let chunks = fighter_pack_chunks(&buf);
    assert_eq!(
        chunks,
        vec![
            FighterChunk {
                kind: 0,
                offset: 4,
                len: 5
            },
            FighterChunk {
                kind: 6,
                offset: 12,
                len: 9
            },
        ]
    );
}

#[test]
fn the_duel_ticks_the_round_chrome() {
    let mut f = fight();
    // No banner is running before a round ends.
    f.tick(1);
    assert!(!f.chrome().busy());
    f.end_round(0, false);
    assert!(f.chrome().busy());
    assert_eq!(f.chrome().sprites().len(), 1);
    f.tick(1);
    assert_eq!(
        f.chrome_frame().xa.map(|x| x.clip),
        Some(0x1F),
        "the round-announce line fires on the banner's first frame"
    );
}

#[test]
fn the_duel_ticks_the_cabinet_shell_and_its_hud() {
    let mut f = fight();
    // The duel sits in the cabinet's own duel state, match phase active.
    assert_eq!(f.cabinet().state(), crate::baka_cabinet::ST_DUEL);
    assert_eq!(f.cabinet().match_phase(), 2);
    f.f[1].combo = 5;
    f.tick(1);
    let frame = f.cabinet_frame();
    assert!(frame.draw_arena && frame.draw_hud);
    let hud = frame.hud.as_ref().expect("the duel band draws the HUD");
    // Both fighters open at full HP, so both bars are full width.
    assert_eq!(hud.bars[0].x1 - hud.bars[0].x0, 0x64);
    assert_eq!(hud.bars[1].x1 - hud.bars[1].x0, 0x64);
    // The crossed combo sides put slot 1's streak on the player's counter.
    assert_eq!(
        hud.combo_level[0],
        crate::baka_cabinet::combo_counter_level(5)
    );
    // The pip rows are sized by the best-of-3 target.
    assert_eq!(hud.pips[0].len(), ROUND_WIN_TARGET as usize);
}

#[test]
fn the_cabinet_shell_banks_the_rung_prize_and_advances_the_stage() {
    let mut f = fight();
    let stage_before = f.cabinet().stage();
    f.f[0].round_wins = ROUND_WIN_TARGET - 1;
    f.end_round(0, false);
    // The cabinet's round bracket - counted from the deciding exchange -
    // has to run out before it reads the win.
    for _ in 0..0xB6 {
        f.tick(1);
    }
    assert_eq!(f.cabinet().stage(), stage_before + 1);
    assert!(f.cabinet().all_clear() || f.cabinet().stage() > 0);
}

#[test]
fn the_running_max_combo_tracks_slot_ones_hits_taken() {
    let mut f = fight();
    f.f[1].combo = 4;
    f.tick(1);
    assert_eq!(f.max_combo(), 4);
    // It is a maximum, so a reset streak does not lower it.
    f.f[1].combo = 0;
    f.tick(1);
    assert_eq!(f.max_combo(), 4);
}

#[test]
fn without_tables_there_is_no_score_channel_at_all() {
    let mut f = fight();
    f.f[1].combo = 6;
    f.tick(1);
    f.end_round(0, false);
    assert_eq!(f.score_rows(), [0, 0, 0]);
    // ...and the tally therefore opens on the coin row alone.
    let mut f = fight();
    f.f[0].round_wins = ROUND_WIN_TARGET - 1;
    f.end_round(0, false);
    assert_eq!(f.tally().unwrap().counters()[..3], [0, 0, 0]);
}

#[test]
fn the_perfect_bonus_is_a_literal_not_a_table_read() {
    // Supplying empty tables turns the channel on; the combo lookup then
    // misses but the full-HP bonus still lands, because retail spells it
    // as an immediate rather than a table cell.
    let mut f = fight().with_score_tables(BakaScoreTables::default());
    f.f[1].combo = 6;
    f.tick(1);
    f.end_round(0, false);
    assert_eq!(f.score_rows(), [0, 0, BAKA_PERFECT_BONUS]);

    // A winner below full HP falls back to the (empty) health table.
    let mut f = fight().with_score_tables(BakaScoreTables::default());
    f.f[0].hp = HP_START - 1;
    f.end_round(0, false);
    assert_eq!(f.score_rows(), [0, 0, 0]);
}

#[test]
fn score_rows_accumulate_once_the_tables_are_supplied() {
    let mut f = fight().with_score_tables(BakaScoreTables {
        combo_bonus: (0..20).map(|i| i * 10).collect(),
        health_bonus: vec![0; 16],
    });
    f.f[1].combo = 6;
    f.tick(1);
    f.end_round(0, false);
    // Combo row takes combo_bonus[6]; the winner is at full HP, so the
    // bonus row takes the flat perfect bonus.
    assert_eq!(f.score_rows(), [0, 60, BAKA_PERFECT_BONUS]);
}

#[test]
fn ko_ends_the_round_and_two_rounds_take_the_match() {
    let mut f = fight();
    let mut rounds = 0;
    let mut guard = 0;
    let mut was_over = false;
    while !f.match_over() {
        guard += 1;
        assert!(guard < 10_000, "match terminates");
        match f.phase() {
            MatchPhase::Fighting => {
                was_over = false;
                f.choose(0, BakaAttack::B);
                f.choose(1, BakaAttack::A);
            }
            MatchPhase::RoundOver(w) => {
                assert_eq!(w, 0);
                rounds += u32::from(!was_over);
                was_over = true;
            }
            MatchPhase::MatchOver(_) => {}
        }
        f.tick(1);
    }
    assert_eq!(f.winner(), Some(0));
    assert_eq!(f.round_wins(0), ROUND_WIN_TARGET);
    assert_eq!(rounds, 1, "second round win ends the match directly");
    assert_eq!(f.gold_reward(), 30);
}

/// A decided round holds - HP, round counter and result banner as they
/// were - for the cabinet's `0xB5`-frame round timer, counted from the
/// deciding exchange (`FUN_801D3468` advances it only once the round is
/// decided; the duel state reads it at `0x801D0620`), however long the
/// fight ran before the KO.
#[test]
fn a_decided_round_holds_for_the_result_timer_from_the_ko() {
    let mut f = fight();
    // A long fight first: the timer must not have been counting.
    for _ in 0..0x200 {
        f.tick(1);
    }
    f.f[1].hp = 0;
    f.end_round(0, false);
    assert!(matches!(f.phase(), MatchPhase::RoundOver(0)));
    let round = f.round;
    let mut held = 0;
    while matches!(f.phase(), MatchPhase::RoundOver(_)) {
        assert_eq!(f.f[1].hp, 0, "the KO'd HP stays down while held");
        assert_eq!(f.round, round);
        f.tick(1);
        held += 1;
        assert!(held < 0x400, "the round restarts");
    }
    assert!((0xB5..=0xB8).contains(&held), "held {held} ticks");
    assert_eq!(f.round, round + 1);
}

#[test]
fn hp_tier_keying_shifts_the_multipliers() {
    let mut player = cfg(0, 10);
    player.atk_tiers = [0, 50, 100]; // stronger as HP drops
    let mut f = BakaFight::new(player, cfg(1, 10), 1);
    f.ai_controlled = [false, false];
    // Drop fighter 0 into the low band by rigging HP directly.
    f.f[0].hp = HP_TIER_MID - 1;
    f.choose(0, BakaAttack::B);
    f.choose(1, BakaAttack::A);
    f.tick(1);
    // hit = 10 + 10*100/100 = 20 → 20*100*0x20/100 = 640, combo -64.
    assert_eq!(f.last_exchange().unwrap().damage, 640 - 64);
}

#[test]
fn comeback_crit_replaces_damage_with_power_shift() {
    let mut player = cfg(0, 10);
    player.crit_chance = 100; // always
    let mut f = BakaFight::new(player, cfg(1, 10), 1);
    f.ai_controlled = [false, false];
    // Put fighter 0 in the crit HP band and let it take a hit → rolls.
    f.f[0].hp = CRIT_HP_BAND - 1;
    f.choose(0, BakaAttack::A);
    f.choose(1, BakaAttack::B);
    f.tick(1);
    assert!(f.f[0].crit_pending, "comeback crit armed");
    // Fighter 0's next winning hit crits: dmg = power << 7 = 1280.
    for _ in 0..13 {
        f.tick(1);
    }
    f.choose(0, BakaAttack::B);
    f.choose(1, BakaAttack::A);
    f.tick(1);
    let r = f.last_exchange().unwrap();
    assert!(r.critical);
    assert_eq!(r.damage, 10 << 7);
}

#[test]
fn ai_pattern_plays_backward_after_seeding() {
    let mut opp = cfg(1, 10);
    opp.ai_pattern = vec![1, 2, 3];
    let mut f = BakaFight::new(cfg(0, 10), opp, 7);
    // Force the seeded-pattern branch by draining picks: over many picks
    // the backward walk must appear (3 → 2 → 1 as types C, B, A).
    let mut seen_backward = false;
    for _ in 0..64 {
        f.f[1].ai_cursor = 0;
        // Find a pick that seeds (roll % 6 >= 3): after seeding, cursor
        // is len-1 and the pick is the LAST symbol (3 → C).
        let pick = f.ai_pick(1);
        if f.f[1].ai_cursor == 2 {
            assert_eq!(pick, BakaAttack::C, "seeded pick = last symbol");
            assert_eq!(f.ai_pick(1), BakaAttack::B);
            assert_eq!(f.ai_pick(1), BakaAttack::A);
            assert_eq!(f.f[1].ai_cursor, 0);
            seen_backward = true;
            break;
        }
    }
    assert!(seen_backward, "the scripted pattern branch fired");
}

#[test]
fn hud_widget_quad_scales_centres_and_mirrors() {
    let w = legaia_asset::baka_opponents::BakaHudWidget {
        scale: 0x2000, // 2.0 in 20.12: half-extent = cell (w*0x2000>>13 = w)
        texpage: 0x19,
        clut: 0x7AB0,
        u: 8,
        v: 16,
        w: 32,
        h: 16,
        rgb_top: [0x80, 0x40, 0xFF],
        semi: 1,
        rgb_bottom: [0x10, 0x20, 0x30],
        abr: 1,
    };
    let q = hud_widget_quad(&w, 160, 120, 0x100, 0x1000, false);
    // scale 0x2000 -> half = cell size; size 0x1000 = 1.0.
    assert_eq!((q.x0, q.x1), (160 - 32, 160 + 31));
    assert_eq!((q.y0, q.y1), (120 - 16, 120 + 15));
    // brightness 0x100 = identity on the colour channels.
    assert_eq!(q.rgb_top, [0x80, 0x40, 0xFF]);
    assert_eq!(q.rgb_bottom, [0x10, 0x20, 0x30]);
    // Inclusive UV cell + poly code + ABR fold.
    assert_eq!(q.uv, [(8, 16), (39, 16), (8, 31), (39, 31)]);
    assert_eq!(q.poly_code, 0x3E);
    assert_eq!(q.tpage_attr, 0x19 + 0x20);
    // Half brightness halves the channels (round toward zero).
    let dim = hud_widget_quad(&w, 160, 120, 0x80, 0x1000, false);
    assert_eq!(dim.rgb_top, [0x40, 0x20, 0x7F]);
    // The mirror latch swaps the texture columns only.
    let m = hud_widget_quad(&w, 160, 120, 0x100, 0x1000, true);
    assert_eq!(m.uv, [(39, 16), (8, 16), (39, 31), (8, 31)]);
    assert_eq!((m.x0, m.x1), (q.x0, q.x1));
}

/// The two ways the duel page's widget extent has been computed, side by
/// side. The page used to do `(cell * scale) / 0x1000 / 2` in floating
/// point and drop the `size` argument entirely; retail is
/// `((cell * scale) >> 13) * size >> 12` with both shifts rounding toward
/// zero. This pins the two places they part company, so a future host that
/// re-rolls the arithmetic fails here rather than drifting a pixel.
#[test]
fn the_widget_extent_is_integer_and_size_scaled() {
    let w = legaia_asset::baka_opponents::BakaHudWidget {
        // An odd product: `w * scale >> 13` truncates where a float
        // division would not.
        scale: 0x1800,
        texpage: 0x19,
        clut: 0x7AB0,
        u: 0,
        v: 0,
        w: 21,
        h: 9,
        rgb_top: [0x80, 0x80, 0x80],
        semi: 0,
        rgb_bottom: [0x80, 0x80, 0x80],
        abr: 0,
    };
    // The float route: 21 * 0x1800 / 0x1000 / 2 = 15.75, so a page that
    // divides gets a 31.5-pixel span. The retail route truncates twice.
    let q = hud_widget_quad(&w, 100, 50, 0x80, 0x1000, false);
    assert_eq!((q.x0, q.x1), (100 - 15, 100 + 14), "half-extent truncates");
    assert_eq!((q.y0, q.y1), (50 - 6, 50 + 5));

    // The `size` term the float route dropped: half size halves the quad.
    let half = hud_widget_quad(&w, 100, 50, 0x80, 0x800, false);
    assert_eq!((half.x0, half.x1), (100 - 7, 100 + 6));
    assert_ne!((half.x0, half.x1), (q.x0, q.x1));

    // The UV span stays the cell regardless of the drawn extent - the
    // inclusive `u ..= u + w - 1` a host must add one back onto.
    assert_eq!(half.uv[0], (0, 0));
    assert_eq!(half.uv[3], (20, 8));
}

#[test]
fn center_effect_spawn_is_screen_centre_at_unit_scale() {
    let s = center_effect_spawn(0x2A);
    assert_eq!((s.x, s.y, s.scale, s.sprite_id), (0xA0, 0x78, 0x1000, 0x2A));
}

#[test]
fn keyframe_lookup_matches_the_range_and_fixed_point() {
    // Whole-frame keyframe indices; the query is << 4 fixed point.
    let frames = [0i16, 4, 10, 22, 30];
    // 22 << 4 = 0x160; a range straddling it matches its index (3).
    assert_eq!(keyframe_in_range(&frames, 21 << 4, 23 << 4), Some(3));
    // Exact single-frame query still resolves via the >>4 fold.
    assert_eq!(keyframe_in_range(&frames, 10 << 4, 10 << 4), Some(2));
    // First match wins when several fall in range.
    assert_eq!(keyframe_in_range(&frames, 0, 30 << 4), Some(0));
    // Nothing in the gap between 10 and 22.
    assert_eq!(keyframe_in_range(&frames, 15 << 4, 20 << 4), None);
    // Inverted range is rejected before the shift.
    assert_eq!(keyframe_in_range(&frames, 30 << 4, 0,), None);
    // No sub-keyframes -> no match.
    assert_eq!(keyframe_in_range(&[], 0, 100), None);
}

#[test]
fn keyframe_lookup_rounds_the_query_toward_zero() {
    let frames = [0i16, 1];
    // 0x0f >> 4 rounds to 0 (toward zero), so frame 0 is in [0, 0].
    assert_eq!(keyframe_in_range(&frames, 0, 0xF), Some(0));
    // 0x10 >> 4 = 1, so the low bound now excludes frame 0.
    assert_eq!(keyframe_in_range(&frames, 0x10, 0x1F), Some(1));
}

#[test]
fn right_aligned_number_suppresses_leading_zeros_but_always_draws_units() {
    // Zero draws exactly one '0' glyph in the units place.
    let z = right_aligned_number_cells(0);
    assert_eq!(z.len(), 1);
    assert_eq!(z[0].cell, DIGIT_FIELD_CELLS - 1);
    assert_eq!((z[0].digit, z[0].widget, z[0].u), (0, NUMBER_WIDGET, 0));

    // 42 draws "4" then "2" in the two rightmost cells, right-aligned.
    let n = right_aligned_number_cells(42);
    let digits: Vec<u8> = n.iter().map(|c| c.digit).collect();
    assert_eq!(digits, vec![4, 2]);
    assert_eq!(n[0].cell, 6);
    assert_eq!(n[1].cell, 7);
    // u = digit * 8; x steps by the 8px cell stride.
    assert_eq!(n[0].u, 4 * 8);
    assert_eq!(n[1].u, 2 * 8);
    assert_eq!(n[0].x_offset, 6 * NUMBER_CELL_STRIDE);
    assert_eq!(n[1].x_offset, 7 * NUMBER_CELL_STRIDE);
}

#[test]
fn coin_strip_uses_widget_47_and_its_own_cell_geometry() {
    let c = coin_digit_cells(305);
    let digits: Vec<u8> = c.iter().map(|d| d.digit).collect();
    assert_eq!(digits, vec![3, 0, 5]);
    for cell in &c {
        assert_eq!(cell.widget, COIN_WIDGET);
        // u = 0x58 + digit*0x10; x steps by the 16px coin cell stride.
        assert_eq!(cell.u, COIN_U_BASE + cell.digit * 0x10);
        assert_eq!(cell.x_offset, cell.cell as i16 * COIN_CELL_STRIDE);
    }
}

#[test]
fn single_digit_cell_patches_the_8px_u_column() {
    let d = single_digit_cell(7);
    assert_eq!((d.widget, d.digit, d.u), (NUMBER_WIDGET, 7, 7 * 8));
}

#[test]
fn number_field_never_exceeds_eight_cells() {
    // 8-digit maximum fills the whole field; a 9th place would overflow it,
    // matching the fixed 10^7 top divisor.
    let full = right_aligned_number_cells(98_765_432);
    assert_eq!(full.len(), DIGIT_FIELD_CELLS);
    assert_eq!(
        full.iter().map(|c| c.digit).collect::<Vec<_>>(),
        vec![9, 8, 7, 6, 5, 4, 3, 2]
    );
}

// Synthetic (non-Sony) score tables: distinct values so an off-by-one in
// the index math is visible. Sizes match the retail overlay tables.
const COMBO_TBL: [i32; 20] = [
    0, 10, 20, 30, 40, 50, 60, 70, 80, 90, 100, 110, 120, 130, 140, 150, 160, 170, 180, 190,
];
const HEALTH_TBL: [i16; 11] = [0, 100, 200, 300, 400, 500, 600, 700, 800, 900, 1000];

#[test]
fn combo_index_clamps_at_nineteen() {
    assert_eq!(baka_combo_index(0), 0);
    assert_eq!(baka_combo_index(19), 19);
    // 20 and above pin to 0x13 (the `slti ..,0x14` boundary).
    assert_eq!(baka_combo_index(20), BAKA_COMBO_MAX);
    assert_eq!(baka_combo_index(255), BAKA_COMBO_MAX);
}

#[test]
fn round_score_indexes_combo_bonus() {
    let s = baka_round_score(5, &COMBO_TBL, 0, &HEALTH_TBL);
    assert_eq!(s.combo_gain, 50);
    // A 25-hit combo saturates at slot 19.
    let s = baka_round_score(25, &COMBO_TBL, 0, &HEALTH_TBL);
    assert_eq!(s.combo_gain, 190);
}

#[test]
fn round_score_pays_flat_perfect_bonus_at_full_hp() {
    // End-of-round HP still at HP_START (0xc80) is the perfect-clear path.
    let s = baka_round_score(0, &COMBO_TBL, HP_START, &HEALTH_TBL);
    assert_eq!(s.bonus_gain, BAKA_PERFECT_BONUS);
}

#[test]
fn round_score_scales_bonus_by_health_band() {
    // hp / 0x140 (floor): 0x140 -> slot 1, 0x280 -> slot 2, 0x3ff -> slot 3.
    assert_eq!(
        baka_round_score(0, &COMBO_TBL, 0x140, &HEALTH_TBL).bonus_gain,
        100
    );
    assert_eq!(
        baka_round_score(0, &COMBO_TBL, 0x280, &HEALTH_TBL).bonus_gain,
        200
    );
    assert_eq!(
        baka_round_score(0, &COMBO_TBL, 0x3FF, &HEALTH_TBL).bonus_gain,
        300
    );
    // Just below full HP takes the table path, not the perfect bonus.
    let almost = baka_round_score(0, &COMBO_TBL, HP_START - 1, &HEALTH_TBL);
    assert_ne!(almost.bonus_gain, BAKA_PERFECT_BONUS);
}

#[test]
fn round_score_out_of_range_index_is_inert() {
    // Empty tables never panic; both increments fall back to zero.
    let s = baka_round_score(5, &[], 0x500, &[]);
    assert_eq!(s, BakaRoundScore::default());
}

// --- The strike clock (retail's `+0x0C` keyframe gate) ------------------

/// A strike table where every attack plays at `speed` 16ths of a frame
/// per tick and strikes on `frames`; the special strikes on `special`.
fn strike_table(speed: i32, frames: &[i16], special: &[i16]) -> StrikeTable {
    let mut t = StrikeTable {
        speed: [speed; legaia_asset::baka_opponents::ACTIONS_PER_FIGHTER],
        ..StrikeTable::default()
    };
    for a in 1..=3 {
        t.frames[a] = frames.to_vec();
    }
    t.frames[4] = special.to_vec();
    t
}

fn striking_fight(speed: i32, frames: &[i16], special: &[i16]) -> BakaFight {
    let tab = strike_table(speed, frames, special);
    let mut f = BakaFight::new(cfg(0, 10), cfg(1, 10), 1).with_strike_tables([tab.clone(), tab]);
    f.ai_controlled = [false, false];
    f
}

#[test]
fn strike_clock_lands_on_the_keyframe_crossing_the_tick_after_commit() {
    // Speed 16 = one whole frame per tick. Strike on frame 3.
    let mut c = StrikeClock::default();
    c.commit();
    // Commit tick: no lookup (retail's ran against the old clip), step.
    c.step(&[3], 16, STRIKE_RATE_DIVISOR, None, 1);
    assert_eq!((c.prev, c.cursor, c.state), (0, 16, StrikeState::Armed));
    // Ranges [0,1], [1,2] miss; [2,3] holds frame 3... the lookup runs on
    // the range the LAST step covered, so frame 3 lands on the tick whose
    // pre-step range is [2 << 4, 3 << 4].
    c.step(&[3], 16, STRIKE_RATE_DIVISOR, None, 1); // looks up [0, 1]
    c.step(&[3], 16, STRIKE_RATE_DIVISOR, None, 1); // [1, 2]
    assert_eq!(c.state, StrikeState::Armed);
    c.step(&[3], 16, STRIKE_RATE_DIVISOR, None, 1); // [2, 3] -> lands
    assert_eq!(c.state, StrikeState::Landed);
    assert_eq!(c.landed, Some(0));
    // Once landed it stays landed (the lookup is skipped) until consumed.
    c.step(&[3], 16, STRIKE_RATE_DIVISOR, None, 1);
    assert_eq!(c.state, StrikeState::Landed);
}

#[test]
fn strike_step_is_speed_times_the_divisor_over_eight() {
    // `+0x6A = record[+4] * DAT_1F80037D >> 3`, divisor 8, times the
    // frame step the clip selector multiplies in.
    let mut c = StrikeClock::default();
    c.step(&[], 5, STRIKE_RATE_DIVISOR, None, 2);
    assert_eq!(c.cursor, 10);
    c.step(&[], -3, STRIKE_RATE_DIVISOR, None, 1);
    assert_eq!(c.cursor, 7, "a negative speed rounds toward zero");
    // The special's divisor halves it: 5 * 4 >> 3 = 2.
    c.step(
        &[],
        5,
        crate::baka_fighter_chrome::SPECIAL_RATE_DIVISOR,
        None,
        1,
    );
    assert_eq!(c.cursor, 9);
    // A double-step clip (record `+1` bit 0) at n = 1 doubles it back.
    let dbl = ClipHeader {
        frames: 30,
        double_step: true,
        n: 1,
    };
    c.step(
        &[],
        5,
        crate::baka_fighter_chrome::SPECIAL_RATE_DIVISOR,
        Some(dbl),
        1,
    );
    assert_eq!(c.cursor, 13);
    assert_eq!(ClipHeader::from_record_words(0x0114, 30, 0x0201), dbl);
}

#[test]
fn a_special_commit_slows_the_round_and_spawns_its_afterimage() {
    let mut f = striking_fight(16, &[20], &[30, 40]);
    assert_eq!(f.rate_divisor(), STRIKE_RATE_DIVISOR);
    throw_special(&mut f, 0);
    assert_eq!(
        f.rate_divisor(),
        crate::baka_fighter_chrome::SPECIAL_RATE_DIVISOR
    );
    f.tick(1);
    let ghosts = f.afterimages();
    assert_eq!(ghosts.len(), 1);
    assert_eq!(ghosts[0].0, 0, "it trails the special's thrower");
    assert_eq!(ghosts[0].1.passes.len(), 2);
    // With a staged clip header the ghosts expire on the special clip's
    // end rather than with the exchange.
    let mut hdr = [None; legaia_asset::baka_opponents::ACTIONS_PER_FIGHTER];
    hdr[legaia_asset::baka_opponents::ACTION_SPECIAL] = Some(ClipHeader {
        frames: 2,
        double_step: false,
        n: 1,
    });
    f.set_clip_headers(0, hdr);
    for _ in 0..20 {
        f.tick(1);
    }
    assert!(
        f.afterimages().is_empty(),
        "both ghosts ran off a 2-frame clip"
    );
}

#[test]
fn a_decided_exchange_waits_for_the_winners_strike() {
    // Strike on frame 2 at one frame per tick.
    let mut f = striking_fight(16, &[2], &[1, 3]);
    assert!(f.choose(0, BakaAttack::B)); // B beats A: slot 0 wins
    assert!(f.choose(1, BakaAttack::A));
    let mut ticks = 0;
    while f.last_exchange().is_none() {
        f.tick(1);
        ticks += 1;
        assert!(ticks < 10, "the strike lands");
    }
    // Commit tick + [0,1] + [1,2] -> booked on the third tick.
    assert_eq!(ticks, 3);
    let r = f.last_exchange().unwrap();
    assert_eq!(r.winner, 0);
    assert_eq!(f.hp(1), HP_START - 256);
}

#[test]
fn a_losers_strike_alone_books_nothing() {
    // Slot 0 (the loser, A vs B) commits first and strikes; slot 1 has
    // not chosen yet, then chooses later - the exchange waits for slot
    // 1's own strike rather than booking on slot 0's.
    let mut f = striking_fight(16, &[1], &[1]);
    assert!(f.choose(0, BakaAttack::A));
    for _ in 0..4 {
        f.tick(1);
    }
    assert_eq!(f.strike_clock(0).state, StrikeState::Landed);
    assert!(f.last_exchange().is_none());
    assert!(f.choose(1, BakaAttack::B));
    f.tick(1); // commit tick
    assert!(f.last_exchange().is_none(), "slot 1 has not struck yet");
    f.tick(1); // [0,1] -> slot 1 lands
    let r = f.last_exchange().expect("booked on the winner's strike");
    assert_eq!(r.winner, 1);
}

#[test]
fn the_special_wins_the_round_only_on_its_last_strike() {
    // Special strikes on frames 1 and 3.
    let mut f = striking_fight(16, &[2], &[1, 3]);
    throw_special(&mut f, 0);
    let mut first = None;
    for t in 0..10 {
        f.tick(1);
        if let Some(r) = f.last_exchange() {
            match first {
                None => first = Some((t, r)),
                Some((t0, r0)) if r.special_round_win => {
                    // Retail's final-strike test: landed == count - 1.
                    assert_eq!(f.round_wins(0), 1);
                    assert!(t > t0, "the second strike is later");
                    assert!(!r0.special_round_win);
                    return;
                }
                Some(_) => {}
            }
        }
    }
    panic!("the special's last strike never landed: {first:?}");
}

#[test]
fn from_tables_runs_the_strike_clock() {
    use legaia_asset::baka_opponents::{BakaActionSet, BakaOpponent, BakaSubKeyframe};
    let opp = |i| BakaOpponent {
        index: i,
        gold_reward: 10,
        damage_mod: 100,
        def_tiers: [0; 3],
        crit_chance: 0,
        atk_tiers: [0; 3],
        stand_off: 0,
        ai_pattern: vec![],
    };
    let kf = |frame| BakaSubKeyframe {
        offset: [0; 3],
        frame,
    };
    let act = |i| BakaActionSet {
        index: i,
        power: [0, 10, 10, 10, 0, 0, 0, 0, 0],
        keyframes: [0, 1, 1, 1, 2, 0, 0, 0, 0],
        speed: [16; 9],
        sub_keyframes: [
            vec![],
            vec![kf(4)],
            vec![kf(4)],
            vec![kf(4)],
            vec![kf(1), kf(5)],
            vec![],
            vec![],
            vec![],
            vec![],
        ],
    };
    let mut f =
        BakaFight::from_tables(&[opp(0), opp(1)], &[act(0), act(1)], 0, 1, 3).expect("fight");
    f.ai_controlled = [false, false];
    assert!(f.choose(0, BakaAttack::B));
    assert!(f.choose(1, BakaAttack::A));
    for _ in 0..4 {
        f.tick(1);
        assert!(f.last_exchange().is_none(), "strike frame 4 not reached");
    }
    f.tick(1);
    assert_eq!(f.last_exchange().map(|r| r.winner), Some(0));
}
