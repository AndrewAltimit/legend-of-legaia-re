//! Battle init `FUN_80055B6C`, run whole with its calls stubbed, against the
//! pieces of it the port carries.
//!
//! - **The species-order rebuild** (`0x80055C7C..0x80055D2C`) against
//!   `battle_seats::rebuild_species_order`: the four formation cells
//!   `DAT_8007BD0C..0F` and whether `rand()` is drawn, over one, two and
//!   three species with gaps.
//! - **The stage override** (`0x80055D2C..0x80055D44`), read off the rebuilt
//!   first cell, against `battle_stage_module::battle_init_stage_override`.
//! - **The battle tally** `0x80084140 + 0x564`, the rumble seed
//!   (`0x8007BD34` / `0x8007BD48` / `0x8007BD4C`) and the held entry fade
//!   (the template handed to `FUN_80024E80`) against `World::enter_battle`.

use crate::common::{Rng, scus_cpu};
use legaia_engine_battle::battle_seats::rebuild_species_order;
use legaia_engine_battle::battle_stage_module::battle_init_stage_override;
use legaia_engine_core::world::World;
use legaia_engine_system::fade::{FadeState, FadeTemplate};
use legaia_save::Party;
use psx_retail_exec::{A0, A1, Stop, V0};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

const BATTLE_INIT: u32 = 0x8005_5B6C;
const BATTLE_INIT_END: u32 = 0x8005_60B4;
const RAND: u32 = 0x8005_6798;
const ALLOC: u32 = 0x8001_7888;
const STREAM_PUMP: u32 = 0x8005_2770;
const SPAWN_FADE: u32 = 0x8002_4E80;
const CELLS: u32 = 0x8007_BD0C;
const STAGE_WORD: u32 = 0x8007_B7FC;
const STREAM_BYTE: u32 = 0x8007_B648;
const STAGE_ID: u32 = 0x8007_B64A;
const WHITE_INTRO: u32 = 0x8007_B8FC;
const TALLY: u32 = 0x8008_4140 + 0x564;
const RUMBLE_HOLD: u32 = 0x8007_BD34;
const RUMBLE_RATES: u32 = 0x8007_BD48;
const CTX: u32 = 0x8010_0000;

#[test]
fn battle_init_is_retails() {
    differential!(scus(BATTLE_INIT));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.stub_outside = Some((BATTLE_INIT, BATTLE_INIT_END));
    let draws = Rc::new(Cell::new((0u32, 0u32)));
    let d = draws.clone();
    cpu.hook(RAND, move |r, _| {
        let (value, n) = d.get();
        r.r[V0] = value;
        d.set((value, n + 1));
    });
    cpu.stub(ALLOC, CTX);
    // The stream pump runs until the stage stream reports itself done.
    cpu.hook(STREAM_PUMP, |_, m| m.wr8(STREAM_BYTE, 0xFF));
    let fades = Rc::new(RefCell::new(Vec::new()));
    let f = fades.clone();
    cpu.hook(SPAWN_FADE, move |r, m| {
        let w: Vec<i16> = (0..13).map(|i| m.rd16(r.r[A0] + 2 * i) as i16).collect();
        f.borrow_mut().push(FadeTemplate {
            kind: w[0],
            duration: w[1],
            start_rgb: [w[3], w[4], w[5]],
            end_rgb: [w[7], w[8], w[9]],
            mode: [w[10], w[11], r.r[A1] as i16],
        });
        r.r[V0] = 0x8015_0000;
    });

    let mut rng = Rng(0x4249_4E49);
    let (mut swapped, mut trios, mut cort, mut failures) = (0, 0, 0, Vec::new());
    for case in 0..6000 {
        // One to three species over the four cells, with gaps.
        let pool: [u8; 3] = [
            1 + rng.below(0xFE) as u8,
            1 + rng.below(0xFE) as u8,
            if rng.below(8) == 0 {
                0xB5
            } else {
                1 + rng.below(0xFE) as u8
            },
        ];
        let species = 1 + rng.below(3) as usize;
        let mut cells: [u8; 4] = std::array::from_fn(|_| {
            if rng.below(5) == 0 {
                0
            } else {
                pool[rng.below(species as u32) as usize]
            }
        });
        if rng.below(10) == 0 {
            cells[0] = 0xB5;
        }
        let rand = rng.next() & 0x7FFF;
        let tally = rng.next();
        let white = rng.below(2);
        let stage_before = 0x40 + rng.below(0x40) as u8;
        for (i, c) in cells.iter().enumerate() {
            cpu.mem.wr8(CELLS + i as u32, *c);
        }
        cpu.mem.wr16(STAGE_WORD, rng.below(2) as u16 * 7);
        cpu.mem.wr8(STREAM_BYTE, 0xFF);
        cpu.mem.wr8(STAGE_ID, stage_before);
        cpu.mem.wr32(WHITE_INTRO, white);
        cpu.mem.wr32(TALLY, tally);
        for a in [RUMBLE_HOLD, RUMBLE_RATES, RUMBLE_RATES + 4] {
            cpu.mem.wr32(a, rng.next());
        }
        draws.set((rand, 0));
        fades.borrow_mut().clear();
        assert_eq!(cpu.call(BATTLE_INIT, &[]), Stop::Returned, "case {case}");

        let retail_cells: [u8; 4] = std::array::from_fn(|i| cpu.mem.rd8(CELLS + i as u32));
        let mut port_cells = cells;
        let mut drawn = 0;
        let rewrote = rebuild_species_order(&mut port_cells, || {
            drawn += 1;
            rand
        });
        if (retail_cells, draws.get().1) != (port_cells, drawn) && failures.len() < 8 {
            failures.push(format!(
                "case {case}: {cells:02x?} rand {rand:#x}: retail {retail_cells:02x?} ({} draws) \
                 port {port_cells:02x?} ({drawn})",
                draws.get().1
            ));
        }
        swapped += usize::from(rewrote);
        let distinct = {
            let mut v: Vec<u8> = cells.iter().copied().filter(|&c| c != 0).collect();
            v.sort_unstable();
            v.dedup();
            v.len()
        };
        trios += usize::from(rewrote && distinct == 3);

        let stage = battle_init_stage_override(port_cells[0]).unwrap_or(stage_before);
        assert_eq!(cpu.mem.rd8(STAGE_ID), stage, "case {case}: {cells:02x?}");
        cort += usize::from(stage == 2);

        let mut w = World::new();
        w.load_party(Party::zeroed(3));
        w.party.record_tally.battles = tally;
        w.rumble.battle_decay.hold = 0x55;
        w.rumble.battle_decay.pulse_rate = 0x55;
        w.rumble.battle_decay.level_rate = 0x55;
        w.battle.entry_fade_white = white != 0;
        w.enter_battle(3, 2);
        assert_eq!(cpu.mem.rd32(TALLY), w.party.record_tally.battles);
        assert_eq!(
            [RUMBLE_HOLD, RUMBLE_RATES, RUMBLE_RATES + 4].map(|a| cpu.mem.rd32(a) as i32),
            [
                w.rumble.battle_decay.hold,
                w.rumble.battle_decay.pulse_rate,
                w.rumble.battle_decay.level_rate
            ]
        );
        // The held fade: one quad until the stage loads.
        let fades = fades.borrow();
        assert_eq!(fades.len(), 1);
        let held = FadeState::load(&fades[0]);
        assert!(held.visible() && held.holds_at_end());
        let [r, g, b] = held.rgb();
        assert_eq!(
            w.battle_entry_fade_draw(),
            Some((
                u32::from(r) << 16 | u32::from(g) << 8 | u32::from(b),
                held.kind as u8,
                held.mode[2] as u32
            )),
            "case {case}: white {white}"
        );
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert!(
        swapped > 1000 && trios > 100 && cort > 200,
        "{swapped} {trios} {cort}"
    );
}
