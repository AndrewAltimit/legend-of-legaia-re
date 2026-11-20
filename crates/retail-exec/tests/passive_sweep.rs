//! The party walk of the per-frame actor pass `FUN_8004CE2C`
//! (`0x8004CE6C..0x8004D018`) against `World::tick_battle_passive_sweep`: the
//! Maximum AP pin and the guard accessories' clear, over every guard
//! combination, a status word split between the port's raw half and its
//! tracker, and fallen members.

use crate::common::{Rng, scus_cpu};
use legaia_engine_battle_vm::status_effects::StatusKind;
use legaia_engine_core::world::World;
use legaia_save::Party;
use psx_retail_exec::Stop;

const ENTRY: u32 = 0x8004_CE2C;
/// The first instruction past the party walk.
const WALK_END: u32 = 0x8004_D01C;
const CTX_PTR: u32 = 0x8007_BD24;
const CTX: u32 = 0x8012_0000;
const ACTOR_TABLE: u32 = 0x801C_9370;
const ACTOR: u32 = 0x8013_0000;
const ACTOR_STRIDE: u32 = 0x400;
const SEATS: u32 = 0x8007_BD10;
const RECORDS: u32 = 0x8008_4140;
const STRIDE: u32 = 0x414;

/// Tracked kinds and the word bit each packs to, for splitting a word.
const TRACKED: [(StatusKind, u16); 6] = [
    (StatusKind::Venom, 0x0001),
    (StatusKind::Toxic, 0x0002),
    (StatusKind::Stone, 0x0004),
    (StatusKind::Numb, 0x0400),
    (StatusKind::Sleep, 0x0800),
    (StatusKind::Curse, 0x1000),
];

#[test]
fn the_passive_sweep_matches_retail() {
    differential!(scus(ENTRY));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.hook(WALK_END, |_, _| {});
    cpu.mem.wr32(CTX_PTR, CTX);
    let mut rng = Rng(0x4755_4152);
    let (mut cleared, mut pinned, mut fallen_cleared) = (0, 0, 0);
    for round in 0..20000 {
        let n = 1 + rng.below(3);
        let mut w = World::new();
        w.load_party(Party::zeroed(3));
        w.enter_battle(3, 2);
        w.party.party_count = n as u8;
        cpu.mem.wr8(CTX, n as u8);
        let mut want = Vec::new();
        for s in 0..n {
            let guards = (rng.next() & 0x1FC0_0000) & [0, 0xFFFF_FFFF][rng.below(2) as usize];
            let word0 = guards | (rng.next() & 0x003F_FFFF);
            let word1 = rng.next() & [0x400, 0, 0xFFFF_FBFF, 0xFFFF_FFFF][rng.below(4) as usize];
            let status = (rng.next() as u16) & [0x1C7F, 0x1FFF, 0xFFFF, 0][rng.below(4) as usize];
            let hp = [0u16, 1, 250][rng.below(3) as usize];
            let gauge = [0u16, 37, 100][rng.below(3) as usize];

            let actor = ACTOR + s * ACTOR_STRIDE;
            cpu.mem.wr32(ACTOR_TABLE + s * 4, actor);
            cpu.mem.wr8(SEATS + s, s as u8 + 1);
            cpu.mem.wr32(RECORDS + s * STRIDE + 0x6BC, word0);
            cpu.mem.wr32(RECORDS + s * STRIDE + 0x6C0, word1);
            cpu.mem.wr16(actor + 0x14C, hp);
            cpu.mem.wr16(actor + 0x16E, status);
            cpu.mem.wr16(actor + 0x170, gauge);

            let mut bits = [0u8; legaia_save::ABILITY_BITS_LEN];
            bits[0..4].copy_from_slice(&word0.to_le_bytes());
            bits[4..8].copy_from_slice(&word1.to_le_bytes());
            w.party.roster.members[s as usize].set_ability_bits(bits);
            let a = &mut w.actors[s as usize].battle;
            a.hp = hp;
            a.liveness = hp;
            a.spirit_gauge = gauge;
            // Half the time the tracked kinds sit in the tracker, as a hit's
            // status arm leaves them; the rest stays in the raw half.
            let mut raw = status;
            if rng.below(2) == 0 {
                for (kind, bit) in TRACKED {
                    if status & bit != 0 {
                        w.battle.status_effects.apply(s as u8, kind);
                        raw &= !bit;
                    }
                }
            }
            w.actors[s as usize].battle.field_flags = raw;
            want.push((status, gauge, hp, word0));
        }

        assert_eq!(cpu.call(ENTRY, &[]), Stop::Returned, "round {round}");
        w.tick_battle_passive_sweep();

        for s in 0..n {
            let actor = ACTOR + s * ACTOR_STRIDE;
            let retail = (cpu.mem.rd16(actor + 0x16E), cpu.mem.rd16(actor + 0x170));
            let port = (
                w.raw_status_word(s as u8),
                w.actors[s as usize].battle.spirit_gauge,
            );
            let (status, gauge, hp, word0) = want[s as usize];
            assert_eq!(
                retail, port,
                "round {round} seat {s}: status {status:#06x} gauge {gauge} hp {hp} word0 {word0:#x}"
            );
            if retail.0 != status {
                cleared += 1;
                fallen_cleared += i32::from(hp == 0);
            }
            pinned += i32::from(retail.1 == 100 && gauge != 100);
        }
    }
    assert!(
        cleared > 3000 && pinned > 1000 && fallen_cleared > 500,
        "{cleared} {pinned} {fallen_cleared}"
    );
}

const ROUTINE_END: u32 = 0x8004_D9FC;
const SEATS_TABLE: u32 = 0x8007_BD10;
const FORMATION: u32 = 0x8007_BD0C;
const MONSTER_TABLES: u32 = 0x801C_9348;
const IMPACT_TABLE: u32 = 0x801F_53D4;
const EFFECT_AT: u32 = 0x801E_1D98;
const RAND: u32 = 0x8005_6798;
const NODES: u32 = 0x8014_0000;
const RECORDS_AT: u32 = 0x8014_8000;
const MONSTER_RECORD: u32 = 0x8014_C000;

/// The pass's arms past the party walk, run from its entry
/// (`0x8004D01C..0x8004D530`): each character's clip-tag arms keyed on the
/// acting actor's committed record `+0x77` and its node cursor `+0x68`
/// (`clip_impact`, `clip_impact_acting_selector`, `vahn_render_arm`,
/// `noa_status_arm`, `monster_render_arm`), the Jette shatter hook
/// (`jette_shatter_hook`) and Dohati's growth (`dohati_growth`).
#[test]
fn the_clip_tag_arms_write_what_retail_writes() {
    use legaia_engine_battle_vm::battle_impact_fx as fx;
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;

    differential!(scus(ENTRY));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.stub_outside = Some((ENTRY, ROUTINE_END));
    let state = Rc::new(Cell::new(0u32));
    let stream = state.clone();
    cpu.hook(RAND, move |r, _| {
        let next = stream.get().wrapping_mul(0x41C6_4E6D).wrapping_add(0x3039);
        stream.set(next);
        r.r[2] = (next >> 16) & 0x7FFF;
    });
    let effects = Rc::new(RefCell::new(Vec::new()));
    let sink = effects.clone();
    cpu.hook(EFFECT_AT, move |r, _| {
        sink.borrow_mut().push((r.r[4], r.r[5]))
    });
    let clean = cpu.mem.clone();
    let tint = [0x1234_5678u32, 0x0ABC_DEF0];
    let mut rng = Rng(0x434C_4950);
    let mut fired = std::collections::HashMap::<&str, u32>::new();
    for round in 0..60000 {
        let acting = rng.below(8) as u8;
        let target = loop {
            let t = rng.below(8) as u8;
            if t != acting {
                break t;
            }
        };
        // Roster ids past Gala (Terra's `4`) and the empty `0` take no arm.
        let ids: [u8; 3] =
            std::array::from_fn(|_| [1u8, 1, 2, 2, 3, 3, 4, 0][rng.below(8) as usize]);
        let key = [0x18u8, 0x16, 0x17, 0x67, 0x2B, 0x29, 0x2D, 0x3B, 0x05][rng.below(9) as usize];
        let cursor = [
            0u16, 0x1F, 0x20, 0x3F, 0x40, 0x50, 0x51, 0x80, 0x81, 0x8F, 0x90, 0xA0, 0xA1,
        ]
        .into_iter()
        .chain([0xAF, 0xB0, 0xE0, 0xF0, 0xF1, 0xFFF0])
        .nth(rng.below(19) as usize)
        .unwrap();
        let hit = rng.below(2) as u8;
        let scripted = rng.below(4) == 0;
        let first = [0x12u8, 0xA7, 0xAA, 0xAA, 0x8A, 0x8A][rng.below(6) as usize];
        let action_state =
            [0x00u8, 0x1E, 0x1F, 0x20, 0x32, 0x3B, 0x3C, 0x50][rng.below(8) as usize];
        let bound = [0u8, 0x13, 5][rng.below(3) as usize];
        let target_clip = [0u8, 2, 4][rng.below(3) as usize];
        let target_cursor = [0i16, 0x10, 0xDF, 0xE0, 0x100][rng.below(5) as usize];
        let gauge = [0u16, 0x19, 0x32, 0x33, 0x80][rng.below(5) as usize];
        let seed = rng.next();

        cpu.mem = clean.clone();
        cpu.mem.wr32(CTX_PTR, CTX);
        cpu.mem.wr8(CTX, 3);
        cpu.mem.wr8(CTX + 7, action_state);
        cpu.mem.wr8(CTX + 0x13, acting);
        cpu.mem.wr8(CTX + 0x287, u8::from(scripted));
        cpu.mem.wr8(FORMATION, first);
        cpu.mem.wr32(IMPACT_TABLE, tint[0]);
        cpu.mem.wr32(IMPACT_TABLE + 4, tint[1]);
        for (i, id) in ids.iter().enumerate() {
            cpu.mem.wr8(SEATS_TABLE + i as u32, *id);
        }
        for s in 0..8u32 {
            let a = ACTOR + s * ACTOR_STRIDE;
            cpu.mem.wr32(ACTOR_TABLE + s * 4, a);
            cpu.mem.wr32(a + 0x22C, NODES + s * 0x100);
            cpu.mem
                .wr32(NODES + s * 0x100 + 0x4C, RECORDS_AT + s * 0x100);
        }
        for k in 0..5 {
            cpu.mem.wr32(MONSTER_TABLES + 4 * k, MONSTER_RECORD);
        }
        let a = ACTOR + u32::from(acting) * ACTOR_STRIDE;
        let t = ACTOR + u32::from(target) * ACTOR_STRIDE;
        let a_node = NODES + u32::from(acting) * 0x100;
        let t_node = NODES + u32::from(target) * 0x100;
        cpu.mem
            .wr8(RECORDS_AT + u32::from(acting) * 0x100 + 0x77, key);
        cpu.mem.wr16(a_node + 0x68, cursor);
        cpu.mem.wr8(a + 0x1DD, target);
        cpu.mem.wr8(a + 0x1F4, hit);
        cpu.mem.wr8(a + 0x21B, bound);
        cpu.mem.wr8(a + 0x21C, 9);
        cpu.mem.wr8(a + 0x21F, 9);
        cpu.mem.wr8(t + 0x1D9, target_clip);
        cpu.mem.wr16(t_node + 0x68, target_cursor as u16);
        cpu.mem.wr8(t_node + 0x6D, 9);
        cpu.mem.wr16(t + 0x1BA, 0x777);
        cpu.mem.wr8(t + 0x21C, 9);
        cpu.mem.wr8(t + 0x21D, 9);
        cpu.mem.wr8(t + 0x21F, 9);
        let seat3 = ACTOR + 3 * ACTOR_STRIDE;
        cpu.mem.wr16(seat3 + 0x170, gauge);
        state.set(seed);
        effects.borrow_mut().clear();
        assert_eq!(cpu.call(ENTRY, &[]), Stop::Returned, "round {round}");
        let m = &mut cpu.mem;
        let retail = (
            (m.rd8(a + 0x21C), m.rd8(a + 0x21F)),
            (m.rd32(t + 4), m.rd32(t + 0xC), m.rd16(t + 0x16E)),
            (m.rd8(t + 0x21C), m.rd8(t + 0x21D), m.rd8(t + 0x21F)),
            (m.rd8(t_node + 0x6D), m.rd16(t + 0x1BA)),
        );
        let dohati = (
            m.rd16(seat3 + 0x170),
            m.rd16(NODES + 3 * 0x100 + 0x72),
            m.rd8(MONSTER_RECORD + 0x1F),
            m.rd16(NODES + 3 * 0x100 + 0x58) as i16,
        );

        // The port's kernels, from the same bytes.
        let mut port = (
            (9u8, 9u8),
            (0u32, 0u32, 0u16),
            (9u8, 9u8, 9u8),
            (9u8, 0x777u16),
        );
        let port_state = Cell::new(seed);
        let mut port_effects = Vec::new();
        if acting < 3 {
            let char_id = ids[usize::from(acting)];
            if let Some(sel) = fx::clip_impact_acting_selector(char_id, key) {
                port.0.1 = sel;
            }
            if let Some(w) = fx::clip_impact(char_id, key, cursor) {
                port.1.0 = tint[usize::from(w.impact_selector - 1)];
                port.1.1 = 0x1000;
                port.2.2 = w.impact_selector;
                if w.freeze_target {
                    port.2.1 = 0;
                }
                if w.effect_at_target {
                    port_effects.push((t + 0x3C, u32::from(fx::GALA_EFFECT_ARG)));
                }
                *fired.entry("impact").or_default() += 1;
            }
            if let Some(f) = fx::vahn_render_arm(char_id, key, cursor) {
                port.0.0 = f;
                *fired.entry("vahn").or_default() += 1;
            }
            if fx::noa_status_arm(char_id, key, hit, scripted, first, || {
                let n = port_state
                    .get()
                    .wrapping_mul(0x41C6_4E6D)
                    .wrapping_add(0x3039);
                port_state.set(n);
                (n >> 16) & 0x7FFF
            }) {
                port.1.2 |= fx::NOA_STATUS_BITS;
                *fired.entry("noa").or_default() += 1;
            }
        } else if let Some((mine, theirs)) = fx::monster_render_arm(key, bound) {
            port.0.0 = mine;
            port.2.0 = theirs;
            *fired.entry("monster").or_default() += 1;
        }
        match fx::jette_shatter_hook(
            first,
            (3..7).contains(&target),
            action_state,
            target_clip,
            target_cursor,
        ) {
            fx::JetteShatter::Keep => {}
            fx::JetteShatter::Set(v) => {
                port.3 = (fx::JETTE_SHATTER_ROW, v);
                *fired.entry("jette set").or_default() += 1;
            }
            fx::JetteShatter::Clear => {
                port.3 = (0, 0);
                *fired.entry("jette clear").or_default() += 1;
            }
        }
        let at = format!(
            "round {round}: acting {acting} target {target} ids {ids:?} key {key:#x} cursor \
             {cursor:#x} hit {hit} scripted {scripted} first {first:#x} state {action_state:#x} \
             bound {bound:#x} target clip {target_clip} cursor {target_cursor:#x}"
        );
        assert_eq!(retail, port, "{at}");
        assert_eq!(*effects.borrow(), port_effects, "effects, {at}");
        assert_eq!(state.get(), port_state.get(), "rand, {at}");
        if first == fx::DOHATI_MONSTER_ID {
            let g = gauge.min(fx::DOHATI_GAUGE_CLAMP);
            let want = match fx::dohati_growth(g, action_state) {
                Some(d) => (g, d.render_scale, d.size_class, d.radius),
                None => (g, 0, 0, 0),
            };
            assert_eq!(dohati, want, "dohati, {at}");
            *fired.entry("dohati").or_default() += 1;
        }
    }
    for arm in [
        "impact",
        "vahn",
        "noa",
        "monster",
        "jette set",
        "jette clear",
        "dohati",
    ] {
        assert!(
            fired.get(arm).copied().unwrap_or(0) > 200,
            "{arm}: {fired:?}"
        );
    }
}

const LOAD_IMAGE: u32 = 0x8005_83C8;
const PALETTES: u32 = CTX + 0x894;

/// The pass's status CLUT arms (`0x8004D6A8..0x8004D9B8`) run from its entry
/// against `battle_status_clut::StatusClutState::step`: a latched Stone seat's
/// whole row greyed onto VRAM row `481 + slot`, then each latched Rot limb's
/// window of the same pristine copy turned blue, over every roster id's
/// windows. Retail's `LoadImage` calls are applied to a copy of the pristine
/// rows and held against the port's VRAM.
#[test]
fn the_status_clut_arms_upload_what_retail_uploads() {
    use legaia_engine_battle::battle_status_clut::{
        PARTY_CLUT_ENTRIES, PARTY_CLUT_ROW_BASE, RotClutWindows, StatusClutState,
    };
    use std::cell::RefCell;
    use std::rc::Rc;

    differential!(scus(ENTRY));
    let (Some(mut cpu), Some(scus)) = (scus_cpu(), crate::common::scus()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let windows = RotClutWindows::from_scus(&scus).expect("rot windows");
    cpu.stub_outside = Some((ENTRY, ROUTINE_END));
    let uploads = Rc::new(RefCell::new(Vec::new()));
    let sink = uploads.clone();
    cpu.hook(LOAD_IMAGE, move |r, m| {
        let rect = r.r[4];
        let (x, y, w) = (m.rd16(rect), m.rd16(rect + 2), m.rd16(rect + 4));
        let words: Vec<u16> = (0..u32::from(w)).map(|i| m.rd16(r.r[5] + 2 * i)).collect();
        sink.borrow_mut().push((x, y, words));
    });
    let clean = cpu.mem.clone();
    let mut rng = Rng(0x434C_5554);
    let (mut stoned, mut rotted) = (0, 0);
    for round in 0..3000 {
        let ids: [u8; 3] = std::array::from_fn(|_| 1 + rng.below(4) as u8);
        let status: [u16; 3] = std::array::from_fn(|_| {
            (rng.next() as u16) & [0x3C, 0x04, 0x38, 0x0C][rng.below(4) as usize]
        });
        let latch: [u8; 3] = std::array::from_fn(|_| rng.below(2) as u8);
        let limb_latch: [[u8; 3]; 3] =
            std::array::from_fn(|_| std::array::from_fn(|_| rng.below(2) as u8));
        let palette: Vec<Vec<u16>> = (0..3)
            .map(|_| {
                (0..PARTY_CLUT_ENTRIES)
                    .map(|_| (rng.next() as u16) | 1)
                    .collect()
            })
            .collect();

        cpu.mem = clean.clone();
        cpu.mem.wr32(CTX_PTR, CTX);
        cpu.mem.wr8(CTX, 3);
        cpu.mem.wr8(CTX + 0x13, 5);
        for s in 0..8u32 {
            let a = ACTOR + s * ACTOR_STRIDE;
            cpu.mem.wr32(ACTOR_TABLE + s * 4, a);
            cpu.mem.wr32(a + 0x22C, NODES + s * 0x100);
            cpu.mem
                .wr32(NODES + s * 0x100 + 0x4C, RECORDS_AT + s * 0x100);
        }
        cpu.mem.wr8(ACTOR + 5 * ACTOR_STRIDE + 0x1DD, 6);
        for s in 0..3usize {
            let a = ACTOR + s as u32 * ACTOR_STRIDE;
            cpu.mem.wr8(SEATS_TABLE + s as u32, ids[s]);
            cpu.mem.wr16(a + 0x14C, 1);
            cpu.mem.wr16(a + 0x16E, status[s]);
            cpu.mem.wr8(a + 0x220, latch[s]);
            for n in 0..3 {
                cpu.mem.wr8(a + 0x221 + n as u32, limb_latch[s][n]);
            }
            for (k, c) in palette[s].iter().enumerate() {
                cpu.mem.wr16(PALETTES + s as u32 * 0x1E0 + 2 * k as u32, *c);
            }
        }
        uploads.borrow_mut().clear();
        assert_eq!(cpu.call(ENTRY, &[]), Stop::Returned, "round {round}");
        let mut retail_rows = palette.clone();
        for (x, y, words) in uploads.borrow().iter() {
            let row = &mut retail_rows[usize::from(*y - PARTY_CLUT_ROW_BASE)];
            for (i, w) in words.iter().enumerate() {
                row[usize::from(*x) + i] = *w;
            }
        }

        let mut vram = psx_tim::Vram::new();
        let mut clut = StatusClutState::default();
        clut.set_rot_windows(std::array::from_fn(|s| windows.row(ids[s])));
        for s in 0..3usize {
            let bytes: Vec<u8> = palette[s].iter().flat_map(|c| c.to_le_bytes()).collect();
            vram.write_clut_row(0, PARTY_CLUT_ROW_BASE + s as u16, &bytes);
            if latch[s] != 0 {
                clut.arm(s as u8, status[s] & 4 != 0);
            }
            let mut rot = 0;
            for n in 0..3 {
                if limb_latch[s][n] != 0 {
                    rot |= status[s] & (8 << n);
                }
            }
            clut.arm_rot(s as u8, rot);
        }
        clut.step(&mut vram);
        for s in 0..3usize {
            let port: Vec<u16> = (0..PARTY_CLUT_ENTRIES)
                .map(|x| vram.pixel(x, usize::from(PARTY_CLUT_ROW_BASE) + s))
                .collect();
            assert_eq!(
                retail_rows[s], port,
                "round {round} slot {s}: id {} status {:#x} latch {} limbs {:?}",
                ids[s], status[s], latch[s], limb_latch[s]
            );
            stoned += i32::from(latch[s] != 0 && status[s] & 4 != 0);
            rotted += i32::from((0..3).any(|n| limb_latch[s][n] != 0 && status[s] & (8 << n) != 0));
        }
    }
    assert!(stoned > 1000 && rotted > 2000, "{stoned} {rotted}");
}
