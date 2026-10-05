use super::*;

/// A host with a scripted `rand()` and a record of what it was asked to
/// apply.
struct Host {
    rolls: Vec<u32>,
    next: usize,
    applied: Vec<FatalOutcome>,
    reaction: bool,
}

impl Host {
    fn new(rolls: &[u32]) -> Self {
        Self {
            rolls: rolls.to_vec(),
            next: 0,
            applied: Vec::new(),
            reaction: true,
        }
    }
}

impl FatalDecisionHost for Host {
    fn rand(&mut self) -> u32 {
        let r = self.rolls.get(self.next).copied().unwrap_or(0);
        self.next += 1;
        r
    }
    fn apply_outcome(&mut self, outcome: FatalOutcome) -> bool {
        self.applied.push(outcome);
        self.reaction
    }
}

#[test]
fn the_three_casters_draw_over_8_12_and_16_slots() {
    assert_eq!(wheel_span(0x77), Some(8));
    assert_eq!(wheel_span(0x78), Some(12));
    assert_eq!(wheel_span(0x79), Some(16));
    assert_eq!(wheel_span(0x10), None);
    // `0x79`: rolls 17..=24 land on 1..=8; slot 7's roll 16 lands on 0, so
    // no forcing draw is made.
    let mut h = Host::new(&[17, 18, 19, 20, 21, 22, 23, 16, 99]);
    let s = fill_wheel(0x79, 0, false, [0; 8], || h.rand());
    assert_eq!(s, [1, 2, 3, 4, 5, 6, 7, 0]);
    assert_eq!(h.next, 8, "a wheel with a blank draws nothing more");
}

#[test]
fn a_wheel_with_no_blank_gets_one_forced() {
    // `0x77` (`% 8`): eight non-zero draws, then the forcing draw `11 % 8`
    // blanks slot 3.
    let mut h = Host::new(&[1, 2, 3, 4, 5, 6, 7, 1, 11]);
    let s = fill_wheel(0x77, 0, false, [0; 8], || h.rand());
    assert_eq!(s, [1, 2, 3, 0, 5, 6, 7, 1]);
    assert_eq!(h.next, 9);
}

#[test]
fn a_rotted_victim_has_stone_struck_off_the_wheel() {
    let mut h = Host::new(&[10, 10, 3, 4, 5, 6, 7, 1]);
    let s = fill_wheel(0x79, 0x0010, false, [0; 8], || h.rand());
    assert_eq!(&s[..2], &[0, 0]);
    // Without the Rot bit the two Stone slots stay, and a blank is forced.
    let mut h = Host::new(&[10, 10, 3, 4, 5, 6, 7, 1, 0]);
    let s = fill_wheel(0x79, 0, false, [0; 8], || h.rand());
    assert_eq!(s, [0, 10, 3, 4, 5, 6, 7, 1]);
}

#[test]
fn a_monster_victim_trades_the_party_only_outcomes() {
    let mut h = Host::new(&[10, 9, 14, 15, 0, 1, 2, 3]);
    let s = fill_wheel(0x79, 0, true, [0; 8], || h.rand());
    assert_eq!(s, [2, 1, 6, 7, 0, 1, 2, 3]);
}

#[test]
fn an_unlisted_caster_keeps_the_image_words() {
    let mut h = Host::new(&[5]);
    let s = fill_wheel(0x10, 0, false, [3, 3, 3, 3, 3, 3, 3, 3], || h.rand());
    // No draw per slot; the forcing draw `5 % 8` blanks slot 5.
    assert_eq!(s, [3, 3, 3, 3, 3, 0, 3, 3]);
}

#[test]
fn the_ring_sits_85_out_and_the_spread_grows_to_it() {
    assert_eq!(ring_offset(0), [0, 85]);
    assert_eq!(ring_offset(0x400), [85, 0]);
    assert_eq!(ring_offset(0x800), [0, -85]);
    // The spread at its arm-5 exit value `0x80` is the ring.
    assert_eq!(spread_offset(0, 0x80), [0, 85]);
    assert_eq!(spread_offset(0, 0), [0, 0]);
}

#[test]
fn the_snap_lands_on_the_boundary_it_crosses() {
    assert_eq!(snap_step(-0x400, 1), -0x400, "aligned stays");
    assert_eq!(snap_step(-0x3F0, 1), -0x3F4);
    assert_eq!(snap_step(-0x3FE, 1), -0x400);
    assert_eq!(landing_slot(-0x400), Some(6));
    assert_eq!(landing_slot(0), Some(4));
    assert_eq!(landing_slot(-0x3F0), None);
}

fn victim() -> FatalVictim {
    FatalVictim {
        hp: 301,
        max_hp: 400,
        mp: 41,
        max_mp: 60,
        atk: 1,
        atk_base: 90,
        udf: 50,
        udf_base: 51,
        ldf: 3,
        ldf_base: 1,
        ..Default::default()
    }
}

#[test]
fn hp_outcomes_halve_or_empty_the_pool_and_skip_a_stone_victim() {
    let mut v = victim();
    let s = apply_outcome_to(&mut v, FatalOutcome::HalveHp);
    assert_eq!((v.hp, v.max_hp, v.hp_bar_delta), (150, 200, 151));
    assert_eq!(s.popup, Some((151, false)));
    assert!(s.reaction);

    let mut v = victim();
    let s = apply_outcome_to(&mut v, FatalOutcome::Death);
    assert_eq!((v.hp, v.hp_bar_delta), (0, 301));
    assert!(s.reaction);

    let mut v = victim();
    v.status = 4;
    let before = v;
    let s = apply_outcome_to(&mut v, FatalOutcome::Death);
    assert_eq!(v, before, "Stone shields the HP and MP arms");
    assert!(!s.reaction);
}

#[test]
fn mp_outcomes_move_the_readout_and_the_max() {
    let mut v = victim();
    let s = apply_outcome_to(&mut v, FatalOutcome::HalveMp);
    assert_eq!((v.mp, v.max_mp, v.mp_readout), (20, 30, 21));
    assert_eq!(s.popup, Some((21, false)));

    let mut v = victim();
    apply_outcome_to(&mut v, FatalOutcome::DrainMp);
    assert_eq!((v.mp, v.max_mp, v.mp_readout), (0, 0, 41));
}

#[test]
fn status_outcomes_or_their_bits_and_two_cancel_the_queued_action() {
    for (o, bits, cancels) in [
        (FatalOutcome::Venom, 0x0001, false),
        (FatalOutcome::Toxic, 0x0002, false),
        (FatalOutcome::Rot, 0x0038, false),
        (FatalOutcome::Curse, 0x1000, false),
        (FatalOutcome::Numb, 0x0400, true),
        (FatalOutcome::Stone, 0x0004, true),
    ] {
        let mut v = victim();
        v.status = 0x0100;
        let s = apply_outcome_to(&mut v, o);
        assert_eq!(v.status, 0x0100 | bits, "{o:?}");
        assert_eq!(s.status_set, bits);
        assert_eq!(s.cancels_action, cancels);
        // The two that cancel skip the reaction arm.
        assert_eq!(s.reaction, !cancels, "{o:?}");
    }
}

#[test]
fn stat_outcomes_halve_with_a_floor_of_one() {
    let mut v = victim();
    apply_outcome_to(&mut v, FatalOutcome::HalveAtk);
    assert_eq!((v.atk, v.atk_base), (1, 45));
    let mut v = victim();
    apply_outcome_to(&mut v, FatalOutcome::HalveDef);
    assert_eq!((v.udf, v.udf_base, v.ldf, v.ldf_base), (25, 25, 1, 1));
}

#[test]
fn the_full_heal_clears_every_status_and_fills_hp() {
    let mut v = victim();
    v.status = 0x1C7F;
    let s = apply_outcome_to(&mut v, FatalOutcome::FullHeal);
    assert_eq!((v.hp, v.status, v.hp_bar_delta), (400, 0, -99));
    assert_eq!(s.popup, Some((99, true)));
    assert!(s.status_cleared && !s.reaction);
}

#[test]
fn nothing_and_the_host_outcomes_store_nothing() {
    let mut v = victim();
    let s = apply_outcome_to(&mut v, FatalOutcome::Nothing);
    assert_eq!(v, victim());
    assert!(!s.reaction);
    for o in [FatalOutcome::StealItem, FatalOutcome::GoldTithe] {
        let mut v = victim();
        let s = apply_outcome_to(&mut v, o);
        assert_eq!(v, victim());
        assert!(s.reaction);
    }
    assert_eq!(gold_tithe(1005), 905);
    assert_eq!(gold_tithe(9), 9);
}

fn view(party: bool) -> FatalDecisionView {
    FatalDecisionView {
        caster: ModuleSeat {
            x: 0,
            y: 0,
            z: 600,
            facing: 0,
        },
        victim: ModuleSeat {
            x: 0,
            y: 0,
            z: -600,
            facing: 0x800,
        },
        victim_seat_y: -100,
        victim_radius: 640,
        depth: 0x1200,
        caster_monster: 0x79,
        victim_is_party: party,
        ..Default::default()
    }
}

/// Walk the body from arm 0 to its return, pressing confirm on `press_at`
/// (a tick count into arm 6), and collect every pass.
fn walk(party: bool, press_at: Option<u32>, host: &mut Host) -> Vec<(u8, FatalDecisionPass)> {
    let mut st = FatalDecisionState::default();
    let mut ctx = CastModuleCtx::default();
    let mut out = Vec::new();
    let mut in_six = 0u32;
    for _ in 0..20_000 {
        let mut v = view(party);
        if ctx.phase == 6 {
            v.confirm = press_at == Some(in_six);
            in_six += 1;
        }
        let phase = ctx.phase;
        let p = fatal_decision_tick(&mut st, &mut ctx, &v, host);
        let done = p.done;
        out.push((phase, p));
        if done {
            return out;
        }
    }
    panic!("the body never returned");
}

#[test]
fn a_party_victim_stops_the_wheel_with_confirm() {
    // Slot values 1..=7 and a blank; the wheel lands on whatever sits at
    // `0x800` once the spin stops.
    let mut host = Host::new(&[17, 18, 19, 20, 21, 22, 23, 16]);
    let passes = walk(true, Some(30), &mut host);
    let phases: Vec<u8> = passes.iter().map(|(p, _)| *p).collect();
    // Every arm runs, in order, and the body ends on `0xFF`.
    let mut seen: Vec<u8> = phases.clone();
    seen.dedup();
    assert_eq!(seen, [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 0xFF]);
    // Arm 6 held exactly until the press.
    assert_eq!(phases.iter().filter(|&&p| p == 6).count(), 31);
    // The prompt opens leaving arm 5 and closes leaving arm 6.
    let prompts: Vec<(u8, bool)> = passes
        .iter()
        .filter_map(|(ph, p)| p.prompt.map(|b| (*ph, b)))
        .collect();
    assert_eq!(prompts, [(5, true), (6, false)]);
    // Spawns: the backdrop, eight icons, the three landing records.
    let spawns: Vec<(u8, usize)> = passes
        .iter()
        .filter(|(_, p)| !p.spawns.is_empty())
        .map(|(ph, p)| (*ph, p.spawns.len()))
        .collect();
    assert_eq!(spawns, [(0, 1), (4, 8), (7, 3)]);
    // The banner and the applied outcome name the same landed slot.
    let banner = passes.iter().find_map(|(_, p)| p.banner);
    let applied = passes.iter().find_map(|(_, p)| p.applied);
    assert!(banner.is_some());
    assert_eq!(banner, applied);
    assert_eq!(host.applied, applied.into_iter().collect::<Vec<_>>());
    // The cues: the open, then the landing.
    let cues: Vec<u16> = passes.iter().filter_map(|(_, p)| p.cue).collect();
    assert_eq!(cues[0], OPEN_CUE);
    assert!(cues[1] == STOP_CUE || cues[1] == FULL_HEAL_STOP_CUE);
}

#[test]
fn an_unpressed_party_wheel_runs_out_its_twenty_seconds() {
    let mut host = Host::new(&[17, 18, 19, 20, 21, 22, 23, 16]);
    let passes = walk(true, None, &mut host);
    // `scalar * 1200` drained `scalar` a tick.
    assert_eq!(passes.iter().filter(|(p, _)| *p == 6).count(), 1200);
}

#[test]
fn a_monster_victims_wheel_stops_itself() {
    let mut host = Host::new(&[17, 18, 19, 20, 21, 22, 23, 16]);
    let passes = walk(false, None, &mut host);
    assert!(passes.iter().all(|(_, p)| p.prompt.is_none()));
    assert_eq!(passes.iter().filter(|(p, _)| *p == 6).count(), 64);
}

#[test]
fn an_outcome_without_a_reaction_skips_arm_11() {
    let mut host = Host::new(&[17, 18, 19, 20, 21, 22, 23, 16]);
    host.reaction = false;
    let passes = walk(true, Some(0), &mut host);
    assert_eq!(host.applied.len(), 1);
    assert!(passes.iter().all(|(ph, _)| *ph != 11));
    assert!(passes.iter().all(|(_, p)| !p.react));
}

#[test]
fn the_landed_icon_grows_while_the_others_open_out() {
    let mut host = Host::new(&[17, 18, 19, 20, 21, 22, 23, 16]);
    let passes = walk(true, Some(5), &mut host);
    let nine: Vec<&FatalDecisionPass> = passes
        .iter()
        .filter(|(ph, _)| *ph == 9)
        .map(|(_, p)| p)
        .collect();
    assert!(!nine.is_empty());
    for p in &nine {
        let (i, d) = p.grow.expect("the landed icon grows every pass");
        assert_eq!(d, DELTA as i16);
        assert!(p.place[i].is_none());
        assert_eq!(p.place.iter().filter(|o| o.is_some()).count(), 7);
    }
}
