//! The pause Magic screen's four sub-screens (PROT 0899) frame by frame
//! against `engine-menus`' `SpellMenuSession`: the caster picker
//! `FUN_801D8F10` (`0x0E`), the spell list's confirm dispatch `FUN_801D9110`
//! (`0x0F`), the group flow `FUN_801D9280` (`0x10`) and the single-target
//! pick `FUN_801D9594` (`0x11`), each with the list kernel `FUN_801D688C`
//! live beneath it (`menu_subscreens` declares the kernel).
//!
//! The window VM, the window-list tick and the two cue entries are logged and
//! stubbed; the applier `FUN_800402F4` and the aggregator `FUN_80042558` are
//! stubbed (their arithmetic is `field_magic`'s), the relevance broadcast
//! `FUN_8003053C` answers a scripted verdict, and the cost kernel
//! `FUN_80035394` and the target validator `FUN_8003FB10` run live. The
//! list window itself is the stubbed tick, so on `0x0F` its result word
//! `0x8007BB94` and staged id `0x8007BB88` are scripted from the row the
//! port's cursor stands on.

use crate::common::{Rng, disc, scus, scus_cpu};
use legaia_engine_battle::retail_magic::seru_magic_catalog_from_scus;
use legaia_engine_battle::spells::SpellCatalog;
use legaia_engine_menus::equip_session::RETAIL_RASERU_EQUIP_BYTE;
use legaia_engine_menus::spell_menu::{
    CasterSlot, SpellMenuEvent, SpellMenuInput, SpellMenuPhase, SpellMenuSession, TargetRow,
};
use psx_retail_exec::{A0, Cpu, Stop, V0};
use std::cell::RefCell;
use std::rc::Rc;

const OVERLAY_BASE: u32 = 0x801C_E818;
const CASTERS: u32 = 0x801D_8F10;
const SPELL_LIST: u32 = 0x801D_9110;
const GROUP: u32 = 0x801D_9280;
const SINGLE: u32 = 0x801D_9594;
const VM: u32 = 0x801D_6628;
const CUE: u32 = 0x8003_5BD0;
const NAV_CUE: u32 = 0x8003_5B50;
const WINDOW_TICK: u32 = 0x8003_1D00;
const APPLIER: u32 = 0x8004_02F4;
const AGGREGATOR: u32 = 0x8004_2558;
const AFFECTS: u32 = 0x8003_053C;

const SCREEN: u32 = 0x801E_46A4;
const STEP: u32 = 0x801E_46AC;
const CASTER_CURSOR: u32 = 0x801E_46C8;
const TARGET_CURSOR: u32 = 0x801E_46C4;
const BUSY: u32 = 0x8007_BB80;
const EDGE: u32 = 0x8007_B874;
const REPEAT: u32 = 0x8007_BB84;
const CONFIRM_MASK: u32 = 0x8008_46D0;
const CANCEL_MASK: u32 = 0x8008_46D4;
const LIST_CONFIRM_MASK: u32 = 0x801E_F0F0;
const LIST_CANCEL_MASK: u32 = 0x801E_F0F4;
const PARTY_COUNT: u32 = 0x8008_4594;
const ROSTER: u32 = 0x8008_4598;
const RECORDS: u32 = 0x8008_4708;
const STRIDE: u32 = 0x414;
/// The Ra-Seru equip byte per character, `0x8007B424`.
const RASERU_TABLE: u32 = 0x8007_B424;
const SPELL_STATS: u32 = 0x8007_54C8;
/// The list window's result word, staged id, selected row and scroll top.
const LIST_RESULT: u32 = 0x8007_BB94;
const STAGED: u32 = 0x8007_BB88;
const LIST_ROW: u32 = 0x8007_BB98;
const LIST_TOP: u32 = 0x8007_BB90;
/// The applier's level-up mailbox the group / single commit seeds with `0xFF`.
const MAILBOX: u32 = 0x8007_BB70;

#[derive(Default)]
struct Log {
    scripts: usize,
    cues: Vec<u32>,
    nav_cues: Vec<u32>,
    applied: Vec<(u32, u32, u32, u32)>,
}

struct Rig {
    cpu: Cpu,
    log: Rc<RefCell<Log>>,
    /// What `FUN_8003053C` answers, per spell id.
    affects: Rc<RefCell<[bool; 256]>>,
    catalog: SpellCatalog,
}

fn rig() -> Option<Rig> {
    let (mut cpu, d, scus) = (scus_cpu()?, disc()?, scus()?);
    cpu.mem.load(
        OVERLAY_BASE,
        &d.read_entry_footprint(899).expect("PROT 0899"),
    );
    let log = Rc::new(RefCell::new(Log::default()));
    let l = log.clone();
    cpu.hook(VM, move |_, _| l.borrow_mut().scripts += 1);
    let l = log.clone();
    cpu.hook(CUE, move |r, _| l.borrow_mut().cues.push(r.r[A0]));
    let l = log.clone();
    cpu.hook(NAV_CUE, move |r, _| l.borrow_mut().nav_cues.push(r.r[A0]));
    cpu.stub(WINDOW_TICK, 0);
    let l = log.clone();
    cpu.hook(APPLIER, move |r, _| {
        l.borrow_mut()
            .applied
            .push((r.r[A0], r.r[A0 + 1], r.r[A0 + 2], r.r[A0 + 3]))
    });
    cpu.stub(AGGREGATOR, 0);
    let affects = Rc::new(RefCell::new([true; 256]));
    let a = affects.clone();
    cpu.hook(AFFECTS, move |r, _| {
        r.r[V0] = u32::from(a.borrow()[(r.r[A0] & 0xFF) as usize])
    });
    let catalog = seru_magic_catalog_from_scus(&scus).expect("the Seru magic catalog");
    Some(Rig {
        cpu,
        log,
        affects,
        catalog,
    })
}

/// One generated party, seated in RAM and handed to the port.
struct Party {
    ids: Vec<u32>,
    casters: Vec<CasterSlot>,
    targets: Vec<TargetRow>,
}

/// The field-castable Seru spells and a few the field refuses.
const SPELL_POOL: [u8; 11] = [
    0x81, 0x82, 0x83, 0x84, 0x85, 0x86, 0x87, 0x88, 0x89, 0x8A, 0x8B,
];

fn seat_party(rig: &mut Rig, rng: &mut Rng, spells_always: bool) -> Party {
    let n = 1 + rng.below(3);
    let mut pool = vec![0u32, 1, 2];
    let mut ids = Vec::new();
    for _ in 0..n {
        ids.push(pool.remove(rng.below(pool.len() as u32) as usize));
    }
    let m = &mut rig.cpu.mem;
    m.wr8(PARTY_COUNT, n as u8);
    for (i, &id) in ids.iter().enumerate() {
        m.wr8(ROSTER + i as u32, id as u8);
    }
    let mut casters = Vec::new();
    let mut targets = Vec::new();
    for &id in &ids {
        let rec = RECORDS + id * STRIDE;
        let hp_max = 1 + rng.below(999) as u16;
        let hp = match rng.below(4) {
            0 => 0,
            1 => hp_max,
            _ => rng.below(u32::from(hp_max)) as u16,
        };
        let mp_max = rng.below(300) as u16;
        let mp = [0, mp_max, rng.below(u32::from(mp_max) + 1) as u16][rng.below(3) as usize];
        m.wr16(rec + 0x104, hp_max);
        m.wr16(rec + 0x106, hp);
        m.wr16(rec + 0x108, mp_max);
        m.wr16(rec + 0x10A, mp);
        m.wr32(rec + 0xF4, [0, 0x10, 0x20, 0x30][rng.below(4) as usize]);
        let count = if spells_always {
            1 + rng.below(6)
        } else {
            [0, 1, 1 + rng.below(6)][rng.below(3) as usize]
        };
        m.wr8(rec + 0x13C, count as u8);
        let mut spells = Vec::new();
        for k in 0..count {
            let s = SPELL_POOL[rng.below(SPELL_POOL.len() as u32) as usize];
            m.wr8(rec + 0x13D + k, s);
            m.wr8(rec + 0x161 + k, 1 + rng.below(9) as u8);
            spells.push(s);
        }
        let seat = m.rd16(RASERU_TABLE + id * 2) as i16;
        assert_eq!(
            seat, RETAIL_RASERU_EQUIP_BYTE[id as usize],
            "the Ra-Seru seat table"
        );
        let raseru = if spells_always || rng.below(4) != 0 {
            1 + rng.below(0x40) as u8
        } else {
            0
        };
        m.wr8(rec + 0x196 + seat as u32, raseru);
        casters.push(CasterSlot {
            slot: id as u8,
            name: String::new(),
            hp,
            mp,
            hp_max,
            mp_max,
            level: 1,
            spell_levels: Vec::new(),
            spells,
            ability_bits: m.rd32(rec + 0xF4),
            ra_seru_missing: raseru == 0,
        });
        targets.push(TargetRow {
            slot: id as u8,
            name: String::new(),
            hp,
            hp_max,
        });
    }
    Party {
        ids,
        casters,
        targets,
    }
}

/// One frame's pad: the retail edge / repeat words and the port's input.
fn pad(rng: &mut Rng) -> (u32, u32, SpellMenuInput) {
    let mut i = SpellMenuInput::default();
    let (mut edge, mut repeat) = (0, 0);
    match rng.below(10) {
        // Start, which no arm of these screens reads: the list kernel tests
        // the edge word against the two masks alone.
        9 => edge |= 0x800,
        0 | 1 => i.down = true,
        2 => i.up = true,
        3 => i.cross = true,
        4 => i.circle = true,
        5 => i.left = true,
        6 => i.right = true,
        7 => {
            i.cross = true;
            i.down = true;
        }
        _ => {}
    }
    if i.cross {
        edge |= 0x40;
    }
    if i.circle {
        edge |= 0x20;
    }
    if i.up {
        repeat |= 0x1000;
    }
    if i.down {
        repeat |= 0x4000;
    }
    // Left / Right reach the retail list kernel as nothing it reads.
    if i.left {
        repeat |= 0x8000;
    }
    if i.right {
        repeat |= 0x2000;
    }
    (edge, repeat, i)
}

impl Rig {
    fn frame(&mut self, entry: u32, edge: u32, repeat: u32) {
        let m = &mut self.cpu.mem;
        m.wr32(CONFIRM_MASK, 0x40);
        m.wr32(CANCEL_MASK, 0x20);
        m.wr32(LIST_CONFIRM_MASK, 0x40);
        m.wr32(LIST_CANCEL_MASK, 0x20);
        m.wr32(BUSY, 0);
        m.wr32(EDGE, edge);
        m.wr32(REPEAT, repeat);
        *self.log.borrow_mut() = Log::default();
        assert_eq!(self.cpu.call(entry, &[]), Stop::Returned, "{entry:#x}");
    }

    fn rd(&mut self, a: u32) -> u32 {
        self.cpu.mem.rd32(a)
    }
}

fn session(rig: &Rig, p: &Party) -> SpellMenuSession {
    let unaffected: Vec<u8> = (0..=255u8)
        .filter(|&s| !rig.affects.borrow()[usize::from(s)])
        .collect();
    SpellMenuSession::new(p.casters.clone(), p.targets.clone(), rig.catalog.clone())
        .with_unaffected_spells(unaffected)
}

#[test]
fn the_caster_picker_steps_as_retail_steps() {
    differential!(prot(899, CASTERS));
    let Some(mut rig) = rig() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut rng = Rng(0x4341_5354);
    let (mut entered, mut buzzed, mut cancelled, mut dead_entered) = (0, 0, 0, 0);
    for episode in 0..6000 {
        let p = seat_party(&mut rig, &mut rng, false);
        let mut port = session(&rig, &p);
        let start = rng.below(p.ids.len() as u32);
        rig.cpu.mem.wr32(CASTER_CURSOR, start);
        rig.cpu.mem.wr32(SCREEN, 0x0E);
        rig.cpu.mem.wr32(STEP, 1);
        // The port's cursor seats where retail's does.
        for _ in 0..start {
            port.tick(SpellMenuInput {
                down: true,
                ..Default::default()
            });
        }
        assert_eq!(u32::from(port.cursor()), start);
        for frame in 0..30 {
            let (edge, repeat, input) = pad(&mut rng);
            rig.frame(CASTERS, edge, repeat);
            let events = port.tick(input);
            let what = format!(
                "episode {episode} frame {frame} {input:?} party {:?}",
                p.casters
                    .iter()
                    .map(|c| (c.slot, c.hp, c.spells.len(), c.ra_seru_missing))
                    .collect::<Vec<_>>()
            );
            let screen = rig.rd(SCREEN);
            let cues = rig.log.borrow().cues.clone();
            let refused = events
                .iter()
                .any(|e| matches!(e, SpellMenuEvent::InvalidConfirm { .. }));
            assert_eq!(cues.contains(&0x23), refused, "{what}: buzz {cues:?}");
            match port.phase() {
                SpellMenuPhase::CharSelect { cursor } => {
                    assert_eq!(screen, 0x0E, "{what}");
                    assert_eq!(
                        rig.rd(CASTER_CURSOR) & 0xFFF,
                        u32::from(*cursor),
                        "{what}: cursor"
                    );
                    buzzed += u32::from(refused);
                }
                SpellMenuPhase::SpellSelect { caster, cursor } => {
                    assert_eq!(screen, 0x0F, "{what}");
                    assert_eq!(cues, [0x20], "{what}");
                    assert_eq!(rig.rd(CASTER_CURSOR) & 0xFFF, u32::from(*caster));
                    assert_eq!(*cursor, 0, "{what}");
                    entered += 1;
                    dead_entered += u32::from(p.casters[usize::from(*caster)].hp == 0);
                    break;
                }
                SpellMenuPhase::Done(_) => {
                    assert_eq!(screen, 1, "{what}: back to the pause list");
                    cancelled += 1;
                    break;
                }
                other => panic!("{what}: {other:?}"),
            }
        }
    }
    assert!(
        entered > 1000 && buzzed > 1000 && cancelled > 1000 && dead_entered > 100,
        "{entered} {buzzed} {cancelled} {dead_entered}"
    );
}

/// Step the port's spell list onto `row` of the caster it entered with.
fn to_row(port: &mut SpellMenuSession, row: usize) {
    for _ in 0..row {
        port.tick(SpellMenuInput {
            down: true,
            ..Default::default()
        });
    }
    assert_eq!(usize::from(port.cursor()), row);
}

/// Enter the port's spell list for the caster at roster position `at`.
fn enter_list(port: &mut SpellMenuSession, at: usize) {
    for _ in 0..at {
        port.tick(SpellMenuInput {
            down: true,
            ..Default::default()
        });
    }
    port.tick(SpellMenuInput {
        cross: true,
        ..Default::default()
    });
    assert!(
        matches!(port.phase(), SpellMenuPhase::SpellSelect { .. }),
        "{:?}",
        port.phase()
    );
}

#[test]
fn the_spell_list_routes_a_confirmed_row_as_retail_routes_it() {
    differential!(prot(899, SPELL_LIST));
    let Some(mut rig) = rig() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut rng = Rng(0x5350_4C4C);
    let mut seen = [0u32; 3];
    for case in 0..15000 {
        *rig.affects.borrow_mut() = [true; 256];
        let p = seat_party(&mut rig, &mut rng, true);
        let at = rng.below(p.ids.len() as u32) as usize;
        let caster = &p.casters[at];
        let mut port = session(&rig, &p);
        enter_list(&mut port, at);
        let rows = port.current_spell_rows();
        let Some(row) = (0..rows.len()).find(|&r| rows[r].admissible && rng.below(2) == 0) else {
            continue;
        };
        to_row(&mut port, row);
        let spell = caster.spells[row];
        // Retail's entry frame, then the frame the list window answers.
        rig.cpu.mem.wr32(CASTER_CURSOR, at as u32);
        rig.cpu.mem.wr32(SCREEN, 0x0F);
        rig.cpu.mem.wr32(STEP, 0);
        rig.cpu.mem.wr32(LIST_ROW, 0xAB);
        rig.cpu.mem.wr32(LIST_TOP, 0xCD);
        rig.frame(SPELL_LIST, 0, 0);
        assert_eq!(
            (rig.rd(LIST_ROW), rig.rd(LIST_TOP)),
            (0, 0),
            "case {case}: the list reopens on its first row"
        );
        assert_eq!(rig.rd(CASTER_CURSOR), at as u32 | 0x1000);
        let answer = [2u32, 3][rng.below(2) as usize];
        rig.cpu.mem.wr32(STAGED, u32::from(spell));
        rig.cpu.mem.wr32(STEP, 2);
        rig.cpu.mem.wr32(LIST_RESULT, answer);
        rig.frame(SPELL_LIST, 0, 0);
        let input = SpellMenuInput {
            cross: answer == 2,
            circle: answer == 3,
            ..Default::default()
        };
        port.tick(input);
        let screen = rig.rd(SCREEN);
        let what = format!("case {case} spell {spell:#x} answer {answer}");
        match port.phase() {
            SpellMenuPhase::CharSelect { cursor } => {
                assert_eq!(screen, 0x0E, "{what}");
                assert_eq!(usize::from(*cursor), at, "{what}");
                assert_eq!(rig.rd(LIST_RESULT), 4, "{what}: the list parked");
                seen[0] += 1;
            }
            SpellMenuPhase::GroupConfirm { spell_id, .. } => {
                assert_eq!((screen, *spell_id), (0x10, spell), "{what}");
                seen[1] += 1;
            }
            SpellMenuPhase::TargetSelect {
                spell_id, cursor, ..
            } => {
                assert_eq!((screen, *spell_id, *cursor), (0x11, spell, 0), "{what}");
                seen[2] += 1;
            }
            other => panic!("{what}: {other:?}"),
        }
        let flag = rig.cpu.mem.rd8(SPELL_STATS + u32::from(spell) * 12 + 2);
        if answer != 2 {
            continue;
        }
        assert_eq!(
            legaia_engine_menus::spell_menu::spell_targets_group(flag),
            screen == 0x10,
            "{what}"
        );
    }
    assert!(seen.iter().all(|&n| n > 500), "{seen:?}");
}

#[test]
fn the_group_and_target_flows_commit_and_come_back_as_retail_does() {
    differential!(prot(899, GROUP), prot(899, SINGLE), prot(899, SPELL_LIST));
    let Some(mut rig) = rig() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut rng = Rng(0x4752_4F55);
    let (mut commits, mut stays, mut leaves, mut cancels, mut refusals) = (0, 0, 0, 0, 0);
    for episode in 0..6000 {
        let mut verdicts = [true; 256];
        for s in SPELL_POOL {
            verdicts[usize::from(s)] = rng.below(5) != 0;
        }
        *rig.affects.borrow_mut() = verdicts;
        let p = seat_party(&mut rig, &mut rng, true);
        let at = rng.below(p.ids.len() as u32) as usize;
        let mut port = session(&rig, &p);
        enter_list(&mut port, at);
        let rows = port.current_spell_rows();
        let Some(row) = (0..rows.len()).find(|&r| rows[r].admissible) else {
            continue;
        };
        to_row(&mut port, row);
        port.tick(SpellMenuInput {
            cross: true,
            ..Default::default()
        });
        let (entry, spell) = match port.phase() {
            SpellMenuPhase::GroupConfirm { spell_id, .. } => (GROUP, *spell_id),
            SpellMenuPhase::TargetSelect { spell_id, .. } => (SINGLE, *spell_id),
            other => panic!("episode {episode}: {other:?}"),
        };
        let screen = if entry == GROUP { 0x10 } else { 0x11 };
        rig.cpu.mem.wr32(CASTER_CURSOR, at as u32 | 0x1000);
        rig.cpu.mem.wr32(STAGED, u32::from(spell));
        rig.cpu.mem.wr32(SCREEN, screen);
        rig.cpu.mem.wr32(STEP, 0);
        rig.frame(entry, 0, 0);
        let caster_rec = RECORDS + p.ids[at] * STRIDE;
        for frame in 0..30 {
            let what = format!("episode {episode} frame {frame} spell {spell:#x} {entry:#x}");
            let (edge, repeat, input) = pad(&mut rng);
            rig.frame(entry, edge, repeat);
            let events = port.tick(input);
            let cues = rig.log.borrow().cues.clone();
            let refused = events
                .iter()
                .any(|e| matches!(e, SpellMenuEvent::InvalidConfirm { .. }));
            assert_eq!(cues.contains(&0x23), refused, "{what}: buzz {cues:?}");
            refusals += u32::from(refused);
            let now = rig.rd(SCREEN);
            match port.phase().clone() {
                SpellMenuPhase::SpellSelect { cursor, .. } => {
                    assert_eq!(now, 0x0F, "{what}");
                    // The list's own entry frame decides the row it opens on.
                    rig.cpu.mem.wr32(STEP, 0);
                    rig.frame(SPELL_LIST, 0, 0);
                    assert_eq!(rig.rd(LIST_ROW), u32::from(cursor), "{what}: list row");
                    cancels += 1;
                    break;
                }
                SpellMenuPhase::GroupConfirm { .. } => {
                    assert_eq!((now, rig.rd(STEP)), (0x10, 1), "{what}");
                }
                SpellMenuPhase::TargetSelect { cursor, .. } => {
                    assert_eq!((now, rig.rd(STEP)), (0x11, 1), "{what}");
                    assert_eq!(rig.rd(TARGET_CURSOR) & 0xFFF, u32::from(cursor), "{what}");
                }
                SpellMenuPhase::Done(_) => {
                    // Committed: retail's next frame applies and debits.
                    assert_eq!((now, rig.rd(STEP)), (screen, 2), "{what}");
                    let target_row = rig.rd(TARGET_CURSOR) & 0xFFF;
                    rig.frame(entry, 0, 0);
                    assert_eq!(rig.log.borrow().applied.len(), 1, "{what}");
                    assert_eq!(rig.cpu.mem.rd16(MAILBOX), 0xFF, "{what}");
                    assert_eq!(rig.rd(STEP), 4, "{what}");
                    // The host rebuilds the session off the debited records.
                    let mut casters = p.casters.clone();
                    casters[at].mp = rig.cpu.mem.rd16(caster_rec + 0x10A);
                    let mut next = session(
                        &rig,
                        &Party {
                            ids: p.ids.clone(),
                            casters,
                            targets: p.targets.clone(),
                        },
                    );
                    next.resume_cast(at as u8, spell, target_row as u8);
                    rig.frame(entry, 0, 0);
                    let back = rig.rd(SCREEN);
                    commits += 1;
                    match next.phase() {
                        SpellMenuPhase::GroupConfirm { .. } => {
                            assert_eq!((back, rig.rd(STEP)), (0x10, 1), "{what}");
                            stays += 1;
                        }
                        SpellMenuPhase::TargetSelect { cursor, .. } => {
                            assert_eq!((back, rig.rd(STEP)), (0x11, 1), "{what}");
                            assert_eq!(rig.rd(TARGET_CURSOR) & 0xFFF, u32::from(*cursor));
                            stays += 1;
                        }
                        SpellMenuPhase::SpellSelect { cursor, .. } => {
                            assert_eq!(back, 0x0F, "{what}");
                            rig.cpu.mem.wr32(STEP, 0);
                            rig.frame(SPELL_LIST, 0, 0);
                            assert_eq!(rig.rd(LIST_ROW), u32::from(*cursor), "{what}: list row");
                            leaves += 1;
                        }
                        other => panic!("{what}: {other:?}"),
                    }
                    break;
                }
                other => panic!("{what}: {other:?}"),
            }
        }
    }
    assert!(
        commits > 500 && stays > 300 && leaves > 30 && cancels > 500 && refusals > 100,
        "{commits} {stays} {leaves} {cancels} {refusals}"
    );
}
