//! The name-based scene-change packet `FUN_8001FD44`, with the index resolver
//! `FUN_8001D7F8` it calls run for real over the CDNAME define table the
//! loader `FUN_8001D8FC` builds, against what the port takes from the same
//! request: field op `0x3F` through `World::step_field` (the name, the entry
//! tile and the facing it parks) and the scene window the name resolves to
//! (`cdname::block_range_for_name`, which `Scene::load` frames).
//!
//! The run is the shipped arm, `_DAT_8007B8C2 != 0` (`0x8001FDD4`), the
//! build-mode word `main` writes from `FUN_8003F084` at `0x80015F08`. That arm
//! copies the active buffer `0x80084548` into `0x80084558` *after* the
//! resolver has overwritten it with the staged name, so the buffer called the
//! previous-scene label carries the destination's own name - lower-cased in
//! its first eight bytes, as the resolver leaves it - which is what every
//! block on the library cards holds there.
//!
//! The BIOS `strcpy` / `strcmp` thunks are answered here, the spawn of the
//! streaming actor `FUN_80020DE0` is hooked on both of its answers.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_core::world::World;
use psx_retail_exec::{A0, A1, Stop, V0};
use std::cell::RefCell;
use std::rc::Rc;

const PACKET: u32 = 0x8001_FD44;
const STRCPY: u32 = 0x8005_6758;
const STRCMP: u32 = 0x8005_6738;
const PRINTF: u32 = 0x8005_67A8;
const SPAWN: u32 = 0x8002_0DE0;
const TABLE: u32 = 0x8008_8758;
const STAGED: u32 = 0x8007_050C;
const ACTIVE: u32 = 0x8008_4548;
const SECOND: u32 = 0x8008_4558;
const DEV_PENDING: u32 = 0x8009_15C8;
const INDEX: u32 = 0x8008_4540;
const STAGED_INDEX: u32 = 0x8007_B768;
const BUILD_MODE: u32 = 0x8007_B8C2;
const REENTRY: u32 = 0x8007_BA3C;
const PENDING_BITS: u32 = 0x1F80_0394;
const CLEARED: u32 = 0x8007_BA98;
const POOL_FULL: u32 = 0x8007_B828;
const LIST_HEAD: u32 = 0x8007_C34C;
const DESCRIPTOR: u32 = 0x8007_0734;
const ARG: u32 = 0x8013_0000;

fn cstr(m: &mut psx_retail_exec::Memory, at: u32) -> Vec<u8> {
    (0..64)
        .map(|i| m.rd8(at + i))
        .take_while(|b| *b != 0)
        .collect()
}

#[test]
fn the_named_scene_change_stages_what_retail_stages() {
    differential!(scus(PACKET));
    let (Some(mut cpu), Some(d)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let text = d.read_named_file("CDNAME.TXT").expect("CDNAME.TXT");
    let text = String::from_utf8_lossy(&text).into_owned();
    // The table as the loader leaves it (its own differential is
    // `format_leaves::the_cdname_table_is_retails`), zero past the last record.
    let (table, _) = legaia_prot::cdname::retail_name_table(&text);
    let mut image = table.clone();
    image.resize(0x96 * 0x10, 0);
    cpu.mem.load(TABLE, &image);
    let map = legaia_prot::cdname::parse_str(&text).expect("CDNAME.TXT parses");

    cpu.hook(STRCPY, |r, m| {
        let s = cstr(m, r.r[A1]);
        m.load(r.r[A0], &s);
        m.wr8(r.r[A0] + s.len() as u32, 0);
        r.r[V0] = r.r[A0];
    });
    cpu.hook(STRCMP, |r, m| {
        let (a, b) = (cstr(m, r.r[A0]), cstr(m, r.r[A1]));
        r.r[V0] = match a.cmp(&b) {
            std::cmp::Ordering::Less => -1i32 as u32,
            std::cmp::Ordering::Equal => 0,
            std::cmp::Ordering::Greater => 1,
        };
    });
    let printed = Rc::new(RefCell::new(0u32));
    let p = printed.clone();
    cpu.hook(PRINTF, move |_, _| *p.borrow_mut() += 1);
    let spawned = Rc::new(RefCell::new(Vec::<(u32, u32)>::new()));
    let answer = Rc::new(RefCell::new(0u32));
    let (s, a) = (spawned.clone(), answer.clone());
    cpu.hook(SPAWN, move |r, _| {
        s.borrow_mut().push((r.r[A0], r.r[A1]));
        r.r[V0] = *a.borrow();
    });

    let mut rng = Rng(0x5343_4E43);
    let (mut resolved, mut refused, mut misses) = (0, 0, 0);
    // Every define line's name, as the scripts write it (lower case and the
    // ISO 9660 upper case of roughly half the disc's `0x3F` operands), and
    // names no record holds.
    let mut names: Vec<String> = map.values().cloned().collect();
    names.extend(map.values().map(|n| n.to_ascii_uppercase()));
    names.extend(["nowhere", "TOWN99", "map0", "x"].map(String::from));
    for (round, name) in names.iter().enumerate() {
        let raw = name.as_bytes();
        let old_index = rng.next() & 0xFFFF;
        let old_active = format!("old{:03}", round % 1000);
        let list = 0x8014_0000 + rng.below(0x100) * 4;
        let full = rng.below(2) == 0;
        cpu.mem.wr16(BUILD_MODE, 1);
        cpu.mem.wr32(REENTRY, u32::from(round % 7 == 0));
        cpu.mem.wr32(INDEX, old_index);
        cpu.mem.load(ACTIVE, &[0; 0x10]);
        cpu.mem.load(ACTIVE, old_active.as_bytes());
        cpu.mem.load(SECOND, &[0; 0x10]);
        cpu.mem.load(DEV_PENDING, &[0; 0x10]);
        let pending_bits = rng.next() & !0x40;
        cpu.mem.wr32(PENDING_BITS, pending_bits);
        cpu.mem.wr32(CLEARED, rng.next() | 1);
        cpu.mem.wr32(POOL_FULL, 0x1234);
        cpu.mem.wr32(LIST_HEAD, list);
        cpu.mem.load(ARG, &[0; 0x20]);
        cpu.mem.load(ARG, raw);
        *answer.borrow_mut() = if full { 0 } else { 0x8015_0000 };
        spawned.borrow_mut().clear();
        *printed.borrow_mut() = 0;
        assert_eq!(cpu.call(PACKET, &[ARG]), Stop::Returned, "{name}");

        // What the resolver found: the record whose name the staged buffer
        // (lower-cased in its first eight bytes) equals, the last of them.
        let mut folded = raw.to_vec();
        for b in folded.iter_mut().take(8) {
            b.make_ascii_lowercase();
        }
        let label = format!("round {round} {name}");
        assert_eq!(cstr(&mut cpu.mem, STAGED), folded, "{label}");
        assert_eq!(cstr(&mut cpu.mem, ACTIVE), raw, "{label}: the active name");
        assert_eq!(
            cstr(&mut cpu.mem, SECOND),
            folded,
            "{label}: the second buffer is the resolver's copy of the new name"
        );
        assert_eq!(cstr(&mut cpu.mem, DEV_PENDING), b"", "{label}");
        assert_eq!(cpu.mem.rd32(INDEX), old_index, "{label}: restored");
        let staged = cpu.mem.rd16(STAGED_INDEX);
        assert_eq!(cpu.mem.rd32(PENDING_BITS), pending_bits | 0x40, "{label}");
        assert_eq!(cpu.mem.rd32(CLEARED), 0, "{label}");
        assert_eq!(*printed.borrow(), u32::from(round % 7 == 0), "{label}");
        assert_eq!(*spawned.borrow(), vec![(DESCRIPTOR, list)], "{label}");
        assert_eq!(
            cpu.mem.rd32(POOL_FULL),
            if full { 0x7FFF } else { 0x1234 },
            "{label}"
        );

        // The port: the op parks the request, the scene load resolves it.
        let (x, z, dir) = (
            rng.below(0x80) as u8,
            rng.below(0x80) as u8,
            rng.below(8) as u8,
        );
        let mut w = World::new();
        w.field_bytecode = [0x3F, 0, 0, raw.len() as u8]
            .into_iter()
            .chain(raw.iter().copied())
            .chain([x, z, dir])
            .collect();
        w.field_pc = 0;
        let _ = w.step_field();
        let parked = w.pending_named_scene_transition.clone();
        let hit = map
            .iter()
            .filter(|(_, n)| n.as_bytes() == folded.as_slice())
            .map(|(k, _)| *k)
            .last();
        match (&parked, hit) {
            (Some((n, px, pz, pd)), Some(define)) => {
                assert_eq!(n.as_bytes(), folded.as_slice(), "{label}");
                assert_eq!((*px, *pz, *pd), (x, z, dir), "{label}");
                assert_eq!(u32::from(staged), define & 0xFFFF, "{label}");
                let (start, _) =
                    legaia_prot::cdname::block_range_for_name(&map, n).expect("a define");
                assert_eq!(start, define, "{label}: the window the port loads");
                resolved += 1;
            }
            (Some((n, ..)), None) => {
                // No record: retail keeps the index it had, and the port's
                // load has no window to frame.
                assert_eq!(staged, old_index as u16, "{label}");
                assert!(
                    legaia_prot::cdname::block_range_for_name(&map, n).is_none(),
                    "{label}"
                );
                misses += 1;
            }
            (None, _) => {
                // The op's desync gate: every name it refuses carries a byte
                // outside `[A-Za-z0-9]` - the non-scene data blocks.
                assert!(
                    !raw.iter().all(u8::is_ascii_alphanumeric) || raw.len() > 12,
                    "{label}: the gate refused a scene name"
                );
                refused += 1;
            }
        }
    }
    assert!(resolved > 200, "{resolved}");
    assert!(misses >= 4 && refused > 0, "{misses} {refused}");
}
