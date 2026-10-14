//! Small-party seating: a party of one or two against one to three monsters.
//!
//! Retail seats monster `k` at pool slot `3 + k` whatever the party size; the
//! engine seats it at `party_count + k`. Every test here puts the acting or
//! targeted monster on an engine slot below `3` and checks that the retail
//! `slot < 3` test it reaches reads it as a monster
//! ([`crate::battle_slot::SeatMap`]).

use super::*;
use crate::battle_slot::SeatMap;

/// `pc` party seats then `mc` monster seats, every actor alive at 100 HP,
/// `active` acting with `category`.
fn seated(pc: u8, mc: u8, active: u8, category: ActionCategory) -> (BattleActionCtx, RecHost) {
    let (ctx, mut host) = fresh(category, active);
    let n = usize::from(pc + mc);
    host.actors.truncate(n);
    host.party_count = pc;
    host.slot_count = pc + mc;
    for a in &mut host.actors {
        a.max_hp = 100;
        a.hp = 100;
        a.liveness = 1;
    }
    (ctx, host)
}

/// Every (party size, monster count, monster index) a small party meets.
fn small_party_monsters() -> impl Iterator<Item = (u8, u8, u8)> {
    (1..=3u8).flat_map(|pc| (1..=3u8).flat_map(move |mc| (0..mc).map(move |k| (pc, mc, k))))
}

/// `FUN_801F3990`'s enemy leg (`slot >= 3`) on every monster seat: the cast
/// cue is the enemy band `0x20C`, never a character's `0xF8 + 0x10 * id`
/// band - which a raw engine slot `1` / `2` took on a small party.
#[test]
fn a_small_partys_monster_casts_on_the_enemy_cue_leg() {
    for (pc, mc, k) in small_party_monsters() {
        let slot = pc + k;
        let (mut ctx, mut host) = seated(pc, mc, slot, ActionCategory::Magic);
        ctx.action_state = ActionState::SpiritWait.as_byte();
        host.actors[usize::from(slot)].cast_class = 0;
        step(&mut host, &mut ctx);
        let sfx: Vec<_> = host
            .take()
            .into_iter()
            .filter(|e| matches!(e, Event::Sfx(_)))
            .collect();
        assert_eq!(sfx, vec![Event::Sfx(0x20C)], "pc {pc} mc {mc} monster {k}");
    }
    // The party side keeps its per-character band.
    let (mut ctx, mut host) = seated(1, 2, 0, ActionCategory::Magic);
    ctx.action_state = ActionState::SpiritWait.as_byte();
    step(&mut host, &mut ctx);
    assert!(host.take().contains(&Event::Sfx(0x108)));
}

/// The applier's class-`1` cue group walks retail's monster row `3..7` for an
/// enemy-wide target (`param_3 == 9`) and the party row `0..3` otherwise. Over
/// the engine's seating it must land on exactly the seated monsters (or
/// party members), whatever the party size.
#[test]
fn a_group_cue_lands_on_every_seat_of_its_row() {
    const CLASS_1_STATE: u32 = 0x000F_FFFF;
    for pc in 1..=3u8 {
        for mc in 1..=3u8 {
            for (target, want_monsters) in [(9u8, true), (8u8, false)] {
                let (mut ctx, mut host) = seated(pc, mc, 0, ActionCategory::Magic);
                host.cue_tables = Some((vec![0; 256], Vec::new()));
                ctx.action_state = ActionState::SpiritFireDamage.as_byte();
                ctx.frame_timer = 0;
                let a = &mut host.actors[0];
                a.cast_class = 1;
                a.cast_sub_class = 0;
                a.active_target = target;
                step(&mut host, &mut ctx);
                let seats = SeatMap::new(pc);
                for s in 0..pc + mc {
                    let monster = !seats.is_party(s);
                    let lit = host.actors[usize::from(s)].render_color == CLASS_1_STATE;
                    assert_eq!(
                        lit,
                        monster == want_monsters,
                        "pc {pc} mc {mc} target {target} slot {s}"
                    );
                }
            }
        }
    }
}

/// `FUN_801EC3E4`'s equipment fold runs on the player branch only
/// (`slot < 3` on the pool slot): a small party's monster never folds.
#[test]
fn a_small_partys_monster_never_takes_the_arms_fold() {
    for (pc, mc, k) in small_party_monsters() {
        let slot = pc + k;
        let (ctx, mut host) = seated(pc, mc, slot, ActionCategory::Attack);
        let a = &mut host.actors[usize::from(slot)];
        a.current_anim = 0x0C;
        a.atk_working = 100;
        host.equip_atk.insert(slot, [0, 0, 30, 0, 0]);
        let hit = hit_event_admits(
            ActionState::AttackChain.as_byte(),
            &[0x0C, 0, 0, 0],
            &[6, 0, 0, 0],
            0,
            5,
        )
        .expect("a swing entry admits its single hit");
        assert_eq!(
            fold_weapon_atk_on_hit(&mut host, slot, ctx.action_state, &hit),
            None,
            "pc {pc} mc {mc} monster {k}"
        );
        assert_eq!(host.actors[usize::from(slot)].atk_working, 100);
    }
    // A party seat still folds.
    let (ctx, mut host) = seated(2, 1, 1, ActionCategory::Attack);
    host.actors[1].current_anim = 0x0C;
    host.equip_atk.insert(1, [0, 0, 30, 0, 0]);
    let hit = hit_event_admits(
        ActionState::AttackChain.as_byte(),
        &[0x0C, 0, 0, 0],
        &[6, 0, 0, 0],
        0,
        5,
    )
    .unwrap();
    assert_eq!(
        fold_weapon_atk_on_hit(&mut host, 1, ctx.action_state, &hit),
        Some(15)
    );
}

/// The settle gate and the HP ramp, swept: a monster-side live-HP write with
/// no readout seed never holds state `0x51`, on any monster of any small
/// party, and a party target's outstanding ramp always does.
#[test]
fn the_settle_gate_reads_every_small_party_seat_by_side() {
    for (pc, mc, k) in small_party_monsters() {
        let monster = pc + k;
        let (ctx, mut host) = seated(pc, mc, 0, ActionCategory::Attack);
        host.actors[0].active_target = monster;
        // Nighto's shape: the live HP zeroed, the readout left standing.
        let m = &mut host.actors[usize::from(monster)];
        m.hp_display = Some(100);
        m.hp = 0;
        assert!(
            !crate::battle_action::hp_bar_drain_pending(&host, &ctx),
            "pc {pc} mc {mc} monster {k}"
        );
        // A seeded monster ramp settles in one frame (the monster arm), a
        // party ramp a quarter at a time.
        let m = &mut host.actors[usize::from(monster)];
        m.hp = 60;
        m.hp_bar_pending = 40;
        host.actors[0].hp = 60;
        host.actors[0].hp_display = Some(100);
        host.actors[0].hp_bar_pending = 40;
        crate::battle_action::tick_hp_bars(&mut host);
        assert_eq!(host.actors[usize::from(monster)].hp_display, Some(60));
        assert_eq!(host.actors[0].hp_display, Some(90), "pc {pc}: 40 / 4");
    }
    for pc in 1..=3u8 {
        let (ctx, mut host) = seated(pc, 1, 0, ActionCategory::Attack);
        let party = pc - 1;
        host.actors[0].active_target = party;
        let p = &mut host.actors[usize::from(party)];
        p.hp = 60;
        p.hp_display = Some(100);
        p.hp_bar_pending = 40;
        assert!(crate::battle_action::hp_bar_drain_pending(&host, &ctx));
    }
}
