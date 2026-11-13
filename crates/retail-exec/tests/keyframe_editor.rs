//! The cabinet overlay's developer keyframe editor `FUN_801D4FC8` (PROT 0976)
//! against `baka_fighter_chrome::editor_tick`, one tick at a time over seeded
//! globals, the editor actor, the fighter actor it seats, a `0x11`-row action
//! table (the editor's `+0x48` and every fighter's `0x801DB8B8` slot) and the
//! two clip tables its `+0x4C` bind reads.
//!
//! Retail reaches the routine only from the cabinet's developer arm, which the
//! shipped disc never enters (`dev_rows`); the interpreter runs it anyway. The
//! keyframe lookup `FUN_801D6E5C` runs for real, as do the two dev-menu stubs
//! `FUN_8002B98C` / `FUN_8002B984`. The debug text, the debug print, the
//! re-pose and the marker spawn are hooked; the marker's position is compared.
//! The editor's globals, both actors' fields and the whole table are compared.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_minigames::baka_fighter_chrome::{
    EditorAction, EditorActor, EditorGlobals, EditorKey, EditorKeyEdit, editor_tick,
};
use psx_retail_exec::{A0, A2, A3, Stop};
use std::cell::RefCell;
use std::rc::Rc;

const OVERLAY_BASE: u32 = 0x801C_E818;
const EDITOR_TICK: u32 = 0x801D_4FC8;
const FIGHTER_PTR: u32 = 0x801D_BFB4;
const ACTION_TABLES: u32 = 0x801D_B8B8;
const CLIPS_LOW: u32 = 0x8007_B888;
const CLIPS_HIGH: u32 = 0x8007_B840;
const EDITOR: u32 = 0x8012_0000;
const FIGHTER: u32 = 0x8012_1000;
const TABLE: u32 = 0x8015_0000;
const CLIP_LOW: u32 = 0x8016_0000;
const CLIP_HIGH: u32 = 0x8017_0000;
const ROWS: usize = 0x11;
const CLIPS: u32 = 0x40;

fn rd_globals(m: &mut psx_retail_exec::Memory) -> EditorGlobals {
    let h = |m: &mut psx_retail_exec::Memory, a: u32| m.rd16(a) as i16;
    EditorGlobals {
        fighter: h(m, 0x801D_BF04),
        fighter_at_entry: h(m, 0x801D_BF08),
        offset: [h(m, 0x801D_BF0A), h(m, 0x801D_BF0C), h(m, 0x801D_BF0E)],
        has_key: h(m, 0x801D_BF10),
        frame: h(m, 0x801D_BF12),
        cursor_raw: h(m, 0x801D_BF14),
        action: h(m, 0x801D_BF16),
        prev_action: h(m, 0x801D_BF18),
        speed: h(m, 0x801D_BF1A),
        power: h(m, 0x801D_BF1C),
        prev_frame: h(m, 0x801D_BF1E),
        match_timer: m.rd32(0x801D_BF44) as i32,
        action_base: h(m, 0x801D_C078),
        marker_key: m.rd32(0x801D_C0FC) as i32,
        paused: h(m, 0x801D_C13C),
        key: m.rd32(0x801D_C148) as i32,
        key_sel: m.rd32(0x801D_C14C) as i32,
    }
}

fn wr_globals(m: &mut psx_retail_exec::Memory, g: &EditorGlobals) {
    let h = |m: &mut psx_retail_exec::Memory, a: u32, v: i16| m.wr16(a, v as u16);
    h(m, 0x801D_BF04, g.fighter);
    h(m, 0x801D_BF08, g.fighter_at_entry);
    for i in 0..3 {
        h(m, 0x801D_BF0A + 2 * i as u32, g.offset[i]);
    }
    h(m, 0x801D_BF10, g.has_key);
    h(m, 0x801D_BF12, g.frame);
    h(m, 0x801D_BF14, g.cursor_raw);
    h(m, 0x801D_BF16, g.action);
    h(m, 0x801D_BF18, g.prev_action);
    h(m, 0x801D_BF1A, g.speed);
    h(m, 0x801D_BF1C, g.power);
    h(m, 0x801D_BF1E, g.prev_frame);
    m.wr32(0x801D_BF44, g.match_timer as u32);
    h(m, 0x801D_C078, g.action_base);
    m.wr32(0x801D_C0FC, g.marker_key as u32);
    h(m, 0x801D_C13C, g.paused);
    m.wr32(0x801D_C148, g.key as u32);
    m.wr32(0x801D_C14C, g.key_sel as u32);
}

fn rd_actor(m: &mut psx_retail_exec::Memory, a: u32) -> EditorActor {
    EditorActor {
        flags: m.rd32(a + 0x10),
        pos: [
            m.rd16(a + 0x14) as i16,
            m.rd16(a + 0x16) as i16,
            m.rd16(a + 0x18) as i16,
        ],
        anim: m.rd16(a + 0x5C) as i16,
        anim_prev: m.rd16(a + 0x5E) as i16,
        cursor: m.rd16(a + 0x68) as i16,
        rate: m.rd16(a + 0x6A) as i16,
    }
}

fn wr_actor(m: &mut psx_retail_exec::Memory, a: u32, s: &EditorActor) {
    m.wr32(a + 0x10, s.flags);
    for i in 0..3 {
        m.wr16(a + 0x14 + 2 * i as u32, s.pos[i] as u16);
    }
    m.wr16(a + 0x5C, s.anim as u16);
    m.wr16(a + 0x5E, s.anim_prev as u16);
    m.wr16(a + 0x68, s.cursor as u16);
    m.wr16(a + 0x6A, s.rate as u16);
}

/// The table as the port models it, laid over the seeded bytes.
fn table_bytes(seed: &[u8], rows: &[EditorAction]) -> Vec<u8> {
    let mut b = seed.to_vec();
    for (i, r) in rows.iter().enumerate() {
        let o = i * 0x60;
        b[o + 4..o + 8].copy_from_slice(&r.speed.to_le_bytes());
        b[o + 0x18..o + 0x1C].copy_from_slice(&r.power.to_le_bytes());
        b[o + 0x1C..o + 0x20].copy_from_slice(&(r.keys.len() as u32).to_le_bytes());
        for (k, key) in r.keys.iter().enumerate() {
            let ko = o + 0x20 + 8 * k;
            for j in 0..3 {
                b[ko + 2 * j..ko + 2 * j + 2].copy_from_slice(&key.offset[j].to_le_bytes());
            }
            b[ko + 6..ko + 8].copy_from_slice(&key.frame.to_le_bytes());
        }
    }
    b
}

#[derive(Debug, Default)]
struct Seen {
    marker: Vec<([i16; 3], u32, u32)>,
    slot_edits: u32,
}

#[test]
fn the_keyframe_editor_ticks_as_retail_does() {
    differential!(prot(976, EDITOR_TICK));
    let (Some(mut cpu), Some(d)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem
        .load(OVERLAY_BASE, &d.read_entry(976).expect("PROT 0976"));
    let seen = Rc::new(RefCell::new(Seen::default()));
    // The debug text, the debug print and the re-pose.
    for va in [0x8001_C93C, 0x8001_7D98, 0x8002_04F8] {
        cpu.stub(va, 0);
    }
    {
        let seen = seen.clone();
        cpu.hook(0x8002_1B04, move |r, m| {
            let p = r.r[A0];
            let pos = [m.rd16(p) as i16, m.rd16(p + 2) as i16, m.rd16(p + 4) as i16];
            seen.borrow_mut().marker.push((pos, r.r[A2], r.r[A3]));
        });
    }
    for va in [0x801D_57BC, 0x801D_58E0] {
        let seen = seen.clone();
        cpu.hook(va, move |_, _| seen.borrow_mut().slot_edits += 1);
    }
    // Two clip tables, each clip naming its frame count at `+0x02`.
    let mut rng = Rng(0x801D_4FC8);
    let mut frames_of = std::collections::BTreeMap::<(bool, u32), u16>::new();
    for (high, base) in [(false, CLIP_LOW), (true, CLIP_HIGH)] {
        for i in 0..CLIPS {
            let off = 0x1000 + i * 0x10;
            cpu.mem.wr32(base + i * 4, off);
            let n = [0, 1, 2, 8, rng.below(40) as u16][rng.below(5) as usize];
            cpu.mem.wr16(base + off + 2, n);
            frames_of.insert((high, i), n);
        }
    }
    cpu.mem.wr32(CLIPS_LOW, CLIP_LOW);
    cpu.mem.wr32(CLIPS_HIGH, CLIP_HIGH);
    // The lookup reads the table through the fighter cursor before it wraps,
    // so the slots either side of `0..0x11` name it too.
    for f in -1i32..=0x12 {
        cpu.mem
            .wr32(ACTION_TABLES.wrapping_add((4 * f) as u32), TABLE);
    }
    cpu.mem.wr32(FIGHTER_PTR, FIGHTER);

    let mut tally = std::collections::BTreeMap::<&str, u32>::new();
    for round in 0..20_000 {
        // The table: speed and power words, up to eight keys each.
        let mut seed = vec![0u8; ROWS * 0x60];
        for b in seed.iter_mut() {
            *b = rng.below(256) as u8;
        }
        let mut rows = Vec::new();
        for i in 0..ROWS {
            let o = i * 0x60;
            let n = rng.below(9) as usize;
            let keys: Vec<EditorKey> = (0..n)
                .map(|_| EditorKey {
                    offset: [
                        rng.below(400) as i16 - 200,
                        rng.below(400) as i16 - 200,
                        rng.below(400) as i16 - 200,
                    ],
                    frame: rng.below(24) as i16 - 2,
                })
                .collect();
            let w = |o: usize| i32::from_le_bytes(seed[o..o + 4].try_into().unwrap());
            rows.push(EditorAction {
                speed: w(o + 4),
                power: w(o + 0x18),
                keys,
            });
        }
        let seed_bytes = table_bytes(&seed, &rows);
        cpu.mem.load(TABLE, &seed_bytes);

        let action = rng.below(ROWS as u32) as i16;
        let frame = rng.below(26) as i16 - 2;
        let fighter_cursor = rng.below(0x13) as i16 - 1;
        let anim_index = rng.below(CLIPS) as i16;
        let mut g = EditorGlobals {
            fighter: fighter_cursor,
            fighter_at_entry: if rng.below(4) == 0 {
                rng.below(0x11) as i16
            } else {
                fighter_cursor
            },
            offset: [rng.next() as i16, rng.next() as i16, rng.next() as i16],
            has_key: rng.below(2) as i16,
            frame,
            cursor_raw: rng.next() as i16,
            action,
            prev_action: if rng.below(2) == 0 {
                action
            } else {
                rng.below(ROWS as u32) as i16
            },
            speed: rng.next() as i16,
            power: rng.next() as i16,
            prev_frame: if rng.below(3) == 0 {
                frame
            } else {
                rng.below(26) as i16 - 2
            },
            match_timer: match rng.below(8) {
                0 => rng.below(1000) as i32 - 100,
                _ => 400 + rng.below(100) as i32,
            },
            action_base: rng.below(0x30) as i16,
            marker_key: rng.next() as i32,
            paused: [0, 1, -1][rng.below(3) as usize],
            key: rng.next() as i32,
            key_sel: rng.next() as i32,
        };
        let high = rng.below(3) == 0;
        let anim = anim_index | if high { 0x400 } else { 0 };
        let mut editor = EditorActor {
            flags: rng.next(),
            pos: [rng.next() as i16, rng.next() as i16, rng.next() as i16],
            anim,
            anim_prev: if rng.below(2) == 0 {
                g.action_base + action + 1
            } else {
                rng.below(0x40) as i16
            },
            cursor: rng.below(800) as i16 - 200,
            rate: rng.next() as i16,
        };
        let mut fighter = EditorActor {
            flags: rng.next(),
            pos: [rng.next() as i16; 3],
            ..Default::default()
        };
        wr_globals(&mut cpu.mem, &g);
        wr_actor(&mut cpu.mem, EDITOR, &editor);
        cpu.mem.wr32(EDITOR + 0x48, TABLE);
        cpu.mem.wr32(EDITOR + 0x4C, 0);
        wr_actor(&mut cpu.mem, FIGHTER, &fighter);
        *seen.borrow_mut() = Seen::default();
        assert_eq!(cpu.call(EDITOR_TICK, &[EDITOR]), Stop::Returned);

        let t = editor_tick(
            &mut g,
            &mut editor,
            &mut fighter,
            &mut rows,
            |a| frames_of[&(a >= 0x400, (a & 0x3FF) as u32)],
            |_| {},
        );
        let at = format!("round {round}");
        assert_eq!(rd_globals(&mut cpu.mem), g, "globals, {at}");
        assert_eq!(rd_actor(&mut cpu.mem, EDITOR), editor, "editor, {at}");
        let f = rd_actor(&mut cpu.mem, FIGHTER);
        assert_eq!(
            (f.flags, f.pos),
            (fighter.flags, fighter.pos),
            "fighter, {at}"
        );
        assert_eq!(
            cpu.mem.read(TABLE, ROWS * 0x60),
            table_bytes(&seed_bytes, &rows),
            "table, {at}"
        );
        let seen = seen.borrow();
        assert_eq!(seen.slot_edits, 0, "{at}");
        assert_eq!(t.edit, EditorKeyEdit::None, "{at}");
        if !t.retired {
            let base = if t.clip_anim >= 0x400 {
                CLIP_HIGH
            } else {
                CLIP_LOW
            };
            let off = cpu.mem.rd32(base + 4 * (t.clip_anim as u32 & 0x3FF));
            assert_eq!(cpu.mem.rd32(EDITOR + 0x4C), base + off, "clip bind, {at}");
        }
        let want: Vec<([i16; 3], u32, u32)> =
            t.marker.iter().map(|&p| (p, 0x801D_B8FC, 0x1000)).collect();
        assert_eq!(seen.marker, want, "marker, {at}");
        let arm = match () {
            _ if t.retired => "retired",
            _ if t.marker.is_some() => "marker",
            _ if g.paused != 0 && g.has_key != 0 => "held on a key",
            _ if g.paused != 0 => "held off a key",
            _ => "playing",
        };
        *tally.entry(arm).or_default() += 1;
        if !t.retired && g.fighter != g.fighter_at_entry {
            *tally.entry("re-entry").or_default() += 1;
        }
    }
    for arm in [
        "retired",
        "marker",
        "held on a key",
        "held off a key",
        "playing",
        "re-entry",
    ] {
        assert!(
            tally.get(arm).copied().unwrap_or(0) > 200,
            "{arm}: {tally:?}"
        );
    }
}
