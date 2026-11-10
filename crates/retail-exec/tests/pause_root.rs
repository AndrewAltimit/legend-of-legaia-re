//! The pause menu's top level (PROT 0899): the command list `FUN_801CFD68`,
//! the money / play-time box `FUN_801D0148` and the party panel
//! `FUN_801D030C`, against `engine-ui`'s `field_menu_draws_for`,
//! `field_menu_info_draws_for` and `field_menu_icon_sprites_for`, on
//! `menu_painters`' rig.

use crate::common::Rng;
use crate::menu_painters::{Ev, hands, pictos, rect, rig, same_text};
use legaia_engine_ui::{
    FieldMenuPartyView, FieldMenuRowView, SaveMenuAtlasRects, field_menu_draws_for,
    field_menu_icon_sprites_for, field_menu_info_draws_for,
};
use psx_retail_exec::{A0, V0};
use std::cell::RefCell;
use std::rc::Rc;

const COMMANDS: u32 = 0x801C_FD68;
const MONEY: u32 = 0x801D_0148;
const PARTY: u32 = 0x801D_030C;
const FLAG_TEST: u32 = 0x8003_CE64;
const GAUGE: u32 = 0x8002_C69C;

const ROW_LABELS: [u32; 7] = [
    0x801C_E9D0,
    0x801C_E9D8,
    0x801C_E9E0,
    0x801C_E9E8,
    0x801C_E9F4,
    0x801C_EA00,
    0x801C_EA08,
];
const CURSOR: u32 = 0x801E_46BC;
const OP_PTR: u32 = 0x8007_B450;
const OP: u32 = 0x8013_2000;
/// The byte the last row's ink follows.
const LAST_ROW_GATE: u32 = 0x8007_B6A8;
const GOLD: u32 = 0x8008_459C;
const COINS: u32 = 0x8008_45A4;
const PLAY_FRAMES: u32 = 0x8008_4570;
const PARTY_COUNT: u32 = 0x8008_4594;
const ROSTER: u32 = 0x8008_4598;
const RECORDS: u32 = 0x8008_4708;
const STRIDE: u32 = 0x414;

/// A hook's log.
type Calls<T> = Rc<RefCell<Vec<T>>>;

fn text_only(log: &[Ev]) -> Vec<Ev> {
    log.iter()
        .filter(|e| matches!(e, Ev::Text(..) | Ev::Number(..) | Ev::Separator(..)))
        .cloned()
        .collect()
}

#[test]
fn the_top_level_windows_match() {
    differential!(prot(899, COMMANDS), prot(899, MONEY), prot(899, PARTY));
    let Some(mut rig) = rig() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let labels: Vec<String> = ROW_LABELS.iter().map(|&va| rig.string(va)).collect();
    let coin_row: Rc<RefCell<bool>> = Rc::default();
    let c = coin_row.clone();
    rig.cpu.hook(FLAG_TEST, move |r, _| {
        assert_eq!(r.r[A0], 8, "the coin row's flag");
        r.r[V0] = u32::from(*c.borrow());
    });
    rig.cpu.unhook(GAUGE);
    let gauges: Calls<(i32, i32, u32, u32)> = Rc::default();
    let g = gauges.clone();
    rig.cpu.hook(GAUGE, move |r, _| {
        g.borrow_mut().push((
            r.r[A0] as i32,
            r.r[psx_retail_exec::A1] as i32,
            r.r[psx_retail_exec::A2],
            r.r[psx_retail_exec::A3],
        ))
    });
    let tag = |n: u32| (n, 0, 1, 1);
    let atlas = SaveMenuAtlasRects {
        cursor: tag(1),
        label_lv: tag(0x0A),
        label_hp: tag(0x3F),
        label_mp: tag(0x40),
        icon_money: tag(0x62),
        label_time: tag(0x63),
        label_coin: tag(0x66),
        gauge_cap: tag(0x200),
        gauge_trough: tag(0x201),
        gauge_box: tag(0x202),
        gauge_tip: tag(0x203),
        gauge_digits: (0x300, 0, 6, 6),
        gauge_100: tag(0x204),
        gauge_fill: tag(0x205),
        ..Default::default()
    };
    rig.cpu.mem.wr32(OP_PTR, OP);
    let mut rng = Rng(0x524F_4F54);
    let (mut greys, mut coin_rows, mut capped, mut ailing) = (0, 0, 0, 0);
    for run in 0..4_000 {
        let (r1, r2, r3) = (rect(&mut rng), rect(&mut rng), rect(&mut rng));
        let row = rng.below(7);
        let flags = row | (rng.below(2) << 12);
        rig.cpu.mem.wr32(CURSOR, flags);
        // The two rows that can grey: row 5 under an entry context of kind
        // `0x0D`, row 6 while the gate byte is clear.
        let kind = [0u8, 0x0D, 0x49][rng.below(3) as usize];
        rig.cpu.mem.wr8(OP, kind);
        let has_context = rng.below(4) != 0;
        rig.cpu.mem.wr32(OP_PTR, if has_context { OP } else { 0 });
        let gate = rng.below(2) as u8;
        rig.cpu.mem.wr8(LAST_ROW_GATE, gate);
        let enabled = |i: usize| match i {
            5 => !(has_context && kind == 0x0D),
            6 => gate != 0,
            _ => true,
        };
        let gold = rng.below(100_000_000);
        let coins = rng.below(100_000_000);
        let frames = match rng.below(3) {
            0 => rng.next(),
            _ => rng.below(60 * 3600 * 120),
        };
        *coin_row.borrow_mut() = rng.below(2) == 1;
        let coin = *coin_row.borrow();
        rig.cpu.mem.wr32(GOLD, gold);
        rig.cpu.mem.wr32(COINS, coins);
        rig.cpu.mem.wr32(PLAY_FRAMES, frames);
        capped += u32::from(frames / 60 / 3600 >= 100);
        coin_rows += u32::from(coin);

        let n = 1 + rng.below(3);
        let mut pool = vec![0u32, 1, 2];
        let ids: Vec<u32> = (0..n)
            .map(|_| pool.remove(rng.below(pool.len() as u32) as usize))
            .collect();
        rig.cpu.mem.wr8(PARTY_COUNT, n as u8);
        let mut names = Vec::new();
        let mut stats = Vec::new();
        for (i, &id) in ids.iter().enumerate() {
            rig.cpu.mem.wr8(ROSTER + i as u32, id as u8);
            let rec = RECORDS + id * STRIDE;
            let name = format!("Hero{id}{}", rng.below(100));
            rig.poke_str(rec + 0x2A7, &name);
            let hp_max = 1 + rng.below(9999) as u16;
            let mp_max = rng.below(1000) as u16;
            let hp = rng.below(u32::from(hp_max) + 1) as u16;
            let mp = rng.below(u32::from(mp_max) + 1) as u16;
            let ap = rng.below(101) as u16;
            let status = if rng.below(4) == 0 {
                ailing += 1;
                1u16 << rng.below(16)
            } else {
                0
            };
            let level = 1 + rng.below(99) as u8;
            for (off, v) in [
                (0x104, hp_max),
                (0x106, hp),
                (0x108, mp_max),
                (0x10A, mp),
                (0x10E, ap),
                (0x12E, status),
            ] {
                rig.cpu.mem.wr16(rec + off, v);
            }
            rig.cpu.mem.wr8(rec + 0x130, level);
            names.push(name);
            stats.push((level, hp, hp_max, mp, mp_max, ap, status));
        }

        let mut log = rig.run(COMMANDS, r1);
        let list_hands = hands(&log);
        let money = rig.run(MONEY, r2);
        log.extend(money.clone());
        gauges.borrow_mut().clear();
        let party_log = rig.run(PARTY, r3);
        let what = format!("run {run} flags {flags:#x} kind {kind:#x} gate {gate} ids {ids:?}");

        let rows: Vec<FieldMenuRowView<'_>> = labels
            .iter()
            .enumerate()
            .map(|(i, l)| FieldMenuRowView {
                label: l,
                enabled: enabled(i),
            })
            .collect();
        greys += rows.iter().filter(|r| !r.enabled).count() as u32;
        let port = field_menu_draws_for(
            &rig.font,
            &rows,
            row as u8,
            gold,
            frames / 60,
            coin.then_some(coins),
            (r1.x, r1.y),
            (r2.x, r2.y),
        );
        same_text(
            &rig,
            &text_only(&log),
            &port,
            &format!("{what}: list and box"),
        );
        let party: Vec<FieldMenuPartyView<'_>> = names
            .iter()
            .zip(&stats)
            .map(
                |(name, &(level, hp, hp_max, mp, mp_max, ap, status))| FieldMenuPartyView {
                    name,
                    level,
                    hp,
                    hp_max,
                    mp,
                    mp_max,
                    ap,
                    status,
                },
            )
            .collect();
        let info = field_menu_info_draws_for(&rig.font, &party, (r3.x, r3.y));
        same_text(
            &rig,
            &text_only(&party_log),
            &info,
            &format!("{what}: party"),
        );

        let aps: Vec<u16> = stats.iter().map(|s| s.5).collect();
        let sprites = field_menu_icon_sprites_for(
            &atlas,
            row as u8,
            &aps,
            (r1.x, r1.y),
            (r2.x, r2.y),
            (r3.x, r3.y),
            coin,
            (0, 0),
            1,
        );
        let port_hands: Vec<(i32, i32)> = sprites
            .iter()
            .filter(|s| s.src.0 == 1)
            .map(|s| (s.dst.0, s.dst.1))
            .collect();
        assert_eq!(
            port_hands,
            list_hands.iter().map(|h| (h.x, h.y)).collect::<Vec<_>>(),
            "{what}: the hand"
        );
        for h in &list_hands {
            assert_eq!(u32::from(h.variant), ((flags >> 12) ^ 1) & 1, "{what}");
        }
        let mut port_icons: Vec<(u32, i32, i32)> = sprites
            .iter()
            .filter(|s| s.src.0 != 1 && s.src.0 < 0x200)
            .map(|s| (s.src.0, s.dst.0, s.dst.1))
            .collect();
        let mut want: Vec<(u32, i32, i32)> = pictos(&money)
            .iter()
            .chain(pictos(&party_log).iter())
            .map(|p| (u32::from(p.id), p.x, p.y))
            .collect();
        port_icons.sort();
        want.sort();
        assert_eq!(port_icons, want, "{what}: pictograms");
        let caps: Vec<(i32, i32)> = sprites
            .iter()
            .filter(|s| s.src.0 == 0x200)
            .map(|s| (s.dst.0, s.dst.1))
            .collect();
        let bars: Vec<(i32, i32)> = gauges.borrow().iter().map(|g| (g.0, g.1)).collect();
        assert_eq!(caps, bars, "{what}: the AP bars");
        assert_eq!(
            gauges
                .borrow()
                .iter()
                .map(|g| (g.2, g.3))
                .collect::<Vec<_>>(),
            aps.iter().map(|&a| (1, u32::from(a))).collect::<Vec<_>>(),
            "{what}: the AP values"
        );
    }
    assert!(
        greys > 1000 && coin_rows > 1000 && capped > 100 && ailing > 500,
        "{greys} {coin_rows} {capped} {ailing}"
    );
}
