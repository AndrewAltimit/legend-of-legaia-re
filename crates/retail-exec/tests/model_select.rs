//! The placement spawner `FUN_8003A1E4`'s model-bank select against
//! `legaia_engine_scene::model_bank::resolve_model_id`: a synthetic scene
//! MAN whose placement record carries every model byte, the spawn
//! `FUN_80024C88` hooked to end the run where the select has stored its pool
//! index into the spawn template (`0x80073E5C`) and the translucent bit into
//! `s3`. The two bank-base globals hold the port's bases.

use crate::common::scus_cpu;
use legaia_engine_scene::model_bank::{PLAYER_BANK_BASE, SCENE_BANK_BASE, resolve_model_id};
use psx_retail_exec::{RA, RETURN_SENTINEL, Stop};
use std::cell::RefCell;
use std::rc::Rc;

const SPAWNER: u32 = 0x8003_A1E4;
const SPAWN: u32 = 0x8002_4C88;
const MAN_PTR: u32 = 0x8007_B898;
const SCENE_BASE: u32 = 0x8007_B6F8;
const PLAYER_BASE: u32 = 0x8007_B824;
const TEMPLATE_ID: u32 = 0x8007_3E5C;
const MAN: u32 = 0x8012_0000;
const S3: usize = 19;

#[test]
fn the_placement_spawner_bank_select_matches_retail() {
    differential!(scus(SPAWNER));
    let Some(mut cpu) = scus_cpu() else {
        return;
    };
    let seen = Rc::new(RefCell::new(None));
    let s = seen.clone();
    cpu.hook(SPAWN, move |r, m| {
        *s.borrow_mut() = Some((m.rd16(TEMPLATE_ID), r.r[S3]));
        r.r[RA] = RETURN_SENTINEL;
    });
    cpu.mem.wr32(MAN_PTR, MAN);
    cpu.mem.wr16(SCENE_BASE, SCENE_BANK_BASE);
    cpu.mem.wr16(PLAYER_BASE, PLAYER_BANK_BASE);
    // Partition counts `[0, 1, 0]`: placement 0 is the table's first record.
    let mut man = vec![0u8; 0x40];
    man[0x24] = 1;
    // One 3-byte offset, then the record: no skipped pairs, the model byte,
    // its cell.
    man[0x2B..0x2E].copy_from_slice(&[0, 0, 0]);
    cpu.mem.load(MAN, &man);
    let record = MAN + 0x2E;
    for id in 0..=255u8 {
        cpu.mem.load(record, &[0, id, 3, 4]);
        *seen.borrow_mut() = None;
        assert_eq!(cpu.call(SPAWNER, &[0, 0]), Stop::Returned);
        let (pool, s3) = seen.borrow().expect("the spawn was reached");
        let port = resolve_model_id(i16::from(id));
        assert_eq!(pool, port.pool_index, "model byte {id:#x}");
        assert_eq!(s3 == 0x0100_0000, port.translucent, "model byte {id:#x}");
    }
}

/// The same spawner's placement read up to the spawn: the record found
/// through partition 1's offset table past partition 0, its local pairs
/// skipped (`0x8003A2B0..0x8003A2CC`), and the spawn vector off the tile
/// bytes, the half-tile bit 7 of each (`0x8003A328..0x8003A384`), against
/// `ManFile::actor_placement`.
#[test]
fn the_placement_spawner_reads_the_record_the_port_reads() {
    use crate::common::Rng;
    use legaia_scene_formats::man_section;
    use psx_retail_exec::A0;
    const S1: usize = 17;
    differential!(scus(SPAWNER));
    let Some(mut cpu) = scus_cpu() else {
        return;
    };
    let seen = Rc::new(RefCell::new(None));
    let s = seen.clone();
    cpu.hook(SPAWN, move |r, m| {
        let v = r.r[A0];
        *s.borrow_mut() = Some((
            m.rd16(v) as i16,
            m.rd16(v + 2) as i16,
            m.rd16(v + 4) as i16,
            r.r[S1],
        ));
        r.r[RA] = RETURN_SENTINEL;
    });
    cpu.mem.wr32(MAN_PTR, MAN);
    cpu.mem.wr16(SCENE_BASE, SCENE_BANK_BASE);
    cpu.mem.wr16(PLAYER_BASE, PLAYER_BANK_BASE);
    let mut rng = Rng(0x504C_4143);
    let (mut halves, mut skipped) = (0, 0);
    for case in 0..3000 {
        let (n0, n1) = (rng.below(4) as usize, 2 + rng.below(5) as usize);
        let mut man = vec![0u8; 0x2B];
        man[0x22..0x24].copy_from_slice(&(n0 as i16).to_le_bytes());
        man[0x24..0x26].copy_from_slice(&(n1 as i16).to_le_bytes());
        // The records, then the offset table in front of them.
        let mut data = Vec::new();
        let mut offsets = Vec::new();
        for _ in 0..n0 + n1 {
            offsets.push(data.len() as u32);
            let locals = rng.below(4) as usize;
            data.push(locals as u8);
            data.extend((0..2 * locals).map(|_| rng.next() as u8));
            data.extend([
                rng.next() as u8,
                rng.next() as u8,
                rng.next() as u8,
                rng.next() as u8,
            ]);
            data.extend([0u8; 4]);
        }
        for o in &offsets {
            man.extend(&o.to_le_bytes()[..3]);
        }
        man[0x28..0x2B].copy_from_slice(&(data.len() as u32).to_le_bytes()[..3]);
        man.extend(&data);
        // The section chain behind the records: five one-byte sections and
        // the terminator.
        for _ in 0..5 {
            man.extend([1, 0, 0, 0]);
        }
        man.extend([0u8; 16]);
        cpu.mem.load(MAN, &man);
        let index = rng.below(n1 as u32) as usize;
        *seen.borrow_mut() = None;
        assert_eq!(cpu.call(SPAWNER, &[index as u32, 0]), Stop::Returned);
        let (x, y, z, header) = seen.borrow().expect("the spawn was reached");
        let file = man_section::parse(&man).expect("MAN");
        let p = file.actor_placement(&man, index).expect("placement");
        let what = format!("case {case}: n0 {n0} n1 {n1} index {index}");
        assert_eq!((x, y, z), (p.world_x, 0, p.world_z), "{what}");
        assert_eq!(
            header - MAN,
            (p.record_offset + 1 + 2 * p.local_count) as u32,
            "{what}: the header"
        );
        let hb = man[p.record_offset + 1 + 2 * p.local_count + 2];
        halves += usize::from(hb & 0x80 != 0);
        skipped += usize::from(p.local_count > 0);
    }
    assert!(halves > 500 && skipped > 1000, "{halves} {skipped}");
}

/// The spawner run whole, the way the scene init `FUN_8003AEB0` runs it,
/// over every scene on the disc and over generated scenes whose prologues aim
/// cross-context ops at earlier placements, later ones and ids nothing
/// carries: each placement in order (`a1` the partition-0 count, so `+0x50`
/// is the flat id), the real spawn `FUN_80024C88` / `FUN_80020DE0` taking its
/// node off the pool `FUN_800203EC` seeds onto the list the context resolver
/// `FUN_8003C83C` walks, the clip bind `FUN_800204F8` and `rand()` hooked, and the field VM
/// `FUN_801DE840` (PROT 0897) the real one, pre-running each `0x24` / `0x25`
/// prologue on a fresh game's flags under the spawn bracket the scene init
/// raises. Once the scene's placements are all in, each actor's seat
/// `+0x14` / `+0x18`, heading `+0x26`, retire bit `+0x10 & 8` and the pc
/// `+0x9E` its pre-run stopped on, against the World's own pre-run
/// (`World::pre_run_field_channel_prologues`) over the channels
/// `World::seed_field_channels` installs, and the record base `+0x90` and
/// context id `+0x50` against `ManFile::actor_placement`.
#[test]
fn every_placement_spawns_where_the_port_seats_it() {
    use crate::common::{Rng, disc};
    use legaia_engine_core::world::World;
    use legaia_scene_formats::man_section;
    use psx_retail_exec::{A0, V0};
    const FIELD_VM: u32 = 0x801D_E840;
    const CLIP_BIND: u32 = 0x8002_04F8;
    const RAND: u32 = 0x8005_6798;
    const CLIP: u32 = 0x8016_F000;
    // The three actor lists (`_DAT_8007C34C` / `C350` / `C354`), each a
    // header whose `+0x0C` is the tail link; the spawn `FUN_80020DE0` takes
    // a node off the pool `FUN_800203EC` seeds and appends it to the list it
    // is handed, and the context resolver `FUN_8003C83C` walks `C354`. A
    // scene's placements spawn in order, so a prologue's cross-context op
    // finds the placements before it and not those after.
    const LISTS: [u32; 3] = [0x8007_C34C, 0x8007_C350, 0x8007_C354];
    const HEADERS: u32 = 0x8016_FF80;
    const POOL_INIT: u32 = 0x8002_03EC;
    differential!(scus(SPAWNER), prot(897, FIELD_VM));
    let (Some(mut cpu), Some(d)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem
        .load(0x801C_E818, &d.read_entry(897).expect("PROT 0897"));
    // The pre-run reaches the field VM by `jal`, so the ledger watches it.
    cpu.watched.insert(FIELD_VM);
    cpu.mem.wr8(0x1F80_0393, 1);
    cpu.mem.wr32(MAN_PTR, MAN);
    cpu.mem.wr16(SCENE_BASE, SCENE_BANK_BASE);
    cpu.mem.wr16(PLAYER_BASE, PLAYER_BANK_BASE);
    // The spawn bracket the scene init raises around its spawn loops
    // (`sh 1,8(*_DAT_801C6EA4)` at `0x8003B73C`), which lifts the
    // dispatcher's halted-target refusal (`0x801DE928`).
    const BRACKET: u32 = 0x8016_FF00;
    cpu.mem.wr32(0x801C_6EA4, BRACKET);
    cpu.mem.wr16(BRACKET + 8, 1);
    // The debug print, which goes to the BIOS, and `VSync` and the
    // interrupt-mask swap, which reach the GPU status and interrupt ports
    // the interpreter has no device behind.
    cpu.stub(0x8005_67A8, 0);
    cpu.stub(0x8005_FB84, 0);
    cpu.stub(0x8005_FF04, 0);
    // And the libgpu image transfer `FUN_80058490` a prologue's VRAM op
    // queues, which waits on the GPU.
    cpu.stub(0x8005_8490, 0);
    // And the move VM `FUN_80023070`, whose program the scene's move table
    // (not loaded here) supplies: `izumi` P1[1]'s prologue starts one.
    cpu.stub(0x8002_3070, 0);
    cpu.hook(CLIP_BIND, move |r, m| {
        m.wr32(r.r[A0] + 0x4C, CLIP);
        m.wr16(CLIP + 2, 8);
    });
    let mut rng = Rng(0x5350_574E);
    cpu.hook(RAND, move |r, _| r.r[V0] = rng.below(0x8000));
    // Every scene pass starts from the same RAM: a prologue's writes (and
    // the scene tables it reads, which the rig does not load) stay its own.
    let ram = cpu.mem.read(0x8000_0000, 0x20_0000);
    let index = crate::walk_on::index();
    let mut flags_rng = Rng(0x464C_4147);
    let mut seen = std::collections::BTreeMap::<&str, usize>::new();
    let mut failures = Vec::new();
    // Each scene twice: on a fresh game's flags, then on a random bank, so
    // the prologues' story arms past their fresh-game branches run too.
    let mut scenes: Vec<(String, Vec<u8>, bool)> = Vec::new();
    for name in index.cdname_scene_names() {
        let Ok(scene) = legaia_engine_scene::scene::Scene::load(&index, &name) else {
            continue;
        };
        let Ok(Some(man)) = scene.field_man_payload(&index) else {
            continue;
        };
        scenes.push((name.clone(), man.clone(), false));
        scenes.push((name, man, true));
    }
    // Then generated scenes whose prologues aim cross-context ops at the
    // placements before them, the ones after, and ids no context answers.
    let mut made = Rng(0x5843_5458);
    for k in 0..400 {
        scenes.push((
            format!("generated {k}"),
            cross_context_man(&mut made),
            false,
        ));
    }
    for (name, man, random_flags) in scenes {
        let Ok(file) = man_section::parse(&man) else {
            continue;
        };
        let bank: Vec<u8> = (0..0x200)
            .map(|_| {
                if random_flags {
                    flags_rng.next() as u8
                } else {
                    0
                }
            })
            .collect();
        let n0 = file.header.partition_counts[0].max(0) as u32;
        cpu.mem.load(0x8000_0000, &ram);
        cpu.mem.load(MAN, &man);
        cpu.mem.load(0x8008_5758, &bank);
        assert_eq!(cpu.call(POOL_INIT, &[]), Stop::Returned);
        for (k, l) in LISTS.iter().enumerate() {
            let h = HEADERS + 0x10 * k as u32;
            cpu.mem.load(h, &[0u8; 0x10]);
            cpu.mem.wr32(h + 0xC, h);
            cpu.mem.wr32(*l, h);
        }
        let mut actors = Vec::new();
        for p in file.actor_placements(&man) {
            let stop = cpu.call(SPAWNER, &[p.index as u32, n0]);
            if stop != Stop::Returned {
                failures.push(format!("{name} #{}: {stop:?}", p.index));
                continue;
            }
            // The placement's node: the one on the context list carrying
            // its id and record.
            let mut node = cpu.mem.rd32(HEADERS + 0x20);
            let mut own = None;
            while node != 0 {
                if u32::from(cpu.mem.rd16(node + 0x50)) == n0 + p.index as u32
                    && cpu.mem.rd32(node + 0x90) == MAN + p.record_offset as u32
                {
                    own = Some(node);
                }
                node = cpu.mem.rd32(node);
            }
            let Some(own) = own else {
                failures.push(format!("{name} #{}: no node on the list", p.index));
                continue;
            };
            actors.push((p, own));
        }
        let mut world = World::default();
        for (b, &v) in bank.iter().enumerate() {
            for k in 0..8u16 {
                if v & (0x80 >> k) != 0 {
                    world.system_flag_set(b as u16 * 8 + k);
                }
            }
        }
        world.seed_field_channels(&file, &man);
        world.pre_run_field_channel_prologues();
        *seen.entry("scenes").or_default() += 1;
        for (p, own) in actors {
            *seen.entry("placements").or_default() += 1;
            let at = format!("{name} #{} (random flags {random_flags})", p.index);
            let slot = p.index as u8;
            let block = cpu.mem.read(own, 0x100);
            let rd16 = |o: u32| u16::from_le_bytes([block[o as usize], block[o as usize + 1]]);
            let rd32 =
                |o: u32| u32::from_le_bytes(block[o as usize..o as usize + 4].try_into().unwrap());
            let mut out: Vec<(&str, i64, i64)> = vec![
                (
                    "+0x90",
                    i64::from(rd32(0x90)),
                    i64::from(MAN + p.record_offset as u32),
                ),
                (
                    "+0x50",
                    i64::from(rd16(0x50)),
                    i64::from(n0 + p.index as u32),
                ),
            ];
            // A prologue that raises its own retire bit (`31 03`) leaves an
            // actor the next frame's sweep unlinks; its seat never shows.
            let retired = rd32(0x10) & 0x8 != 0;
            out.push((
                "retired",
                i64::from(retired),
                i64::from(world.field_placement_retired(slot)),
            ));
            if retired {
                *seen.entry("retired").or_default() += 1;
                for (what, got, want) in out {
                    if got != want {
                        failures.push(format!("{at}: {what} {got:#x}, port {want:#x}"));
                    }
                }
                continue;
            }
            let (x, z) = world
                .npcs
                .positions
                .get(&slot)
                .copied()
                .unwrap_or((p.world_x, p.world_z));
            if (rd16(0x14) as i16, rd16(0x18) as i16) != (p.world_x, p.world_z) {
                *seen.entry("reseated").or_default() += 1;
            }
            if rd16(0x26) != 0 {
                *seen.entry("turned").or_default() += 1;
            }
            out.push(("x", i64::from(rd16(0x14) as i16), i64::from(x)));
            out.push(("z", i64::from(rd16(0x18) as i16), i64::from(z)));
            // Engine headings are retail's turned a half-turn.
            let heading = world.npcs.heading(slot);
            out.push((
                "heading",
                i64::from((rd16(0x26).wrapping_add(0x800)) & 0xFFF),
                i64::from(heading as u16 & 0xFFF),
            ));
            if let Some(c) = world
                .field_vm
                .channels
                .iter()
                .find(|c| !c.object_bind && c.placement_index == p.index)
            {
                out.push(("pc", i64::from(rd16(0x9E)), c.pc as i64));
            }
            for (what, got, want) in out {
                if got != want {
                    failures.push(format!("{at}: {what} {got:#x}, port {want:#x}"));
                }
            }
        }
    }
    eprintln!("{seen:?}");
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    let n = |k: &str| seen.get(k).copied().unwrap_or(0);
    assert!(
        n("scenes") > 80 && n("placements") > 2000 && n("reseated") > 500 && n("turned") > 500,
        "{seen:?}"
    );
}

/// A MAN with no partition 0 and `2..8` placements, each record a header on
/// its own tile and a `0x25` prologue of moves, compass turns and context
/// flag sets ending on `0x21`. Each op is own-context or aimed (`op | 0x80`,
/// then the id) at an earlier placement, a later one or an id no context
/// carries.
fn cross_context_man(rng: &mut crate::common::Rng) -> Vec<u8> {
    let n1 = 2 + rng.below(7) as usize;
    let mut data = Vec::new();
    let mut offsets = Vec::new();
    for i in 0..n1 {
        offsets.push(data.len() as u32);
        data.extend([0, 0, 0, rng.below(0x40) as u8, rng.below(0x40) as u8]);
        if i == 0 {
            data.push(0x21);
            continue;
        }
        data.push(0x25);
        for _ in 0..rng.below(5) {
            let target = match rng.below(4) {
                0 => None,
                1 => Some(1 + rng.below(i as u32) as u8),
                2 => Some((i + 1 + rng.below((n1 - i) as u32) as usize) as u8),
                _ => Some(0x60 + rng.below(0x10) as u8),
            };
            let (op, args): (u8, Vec<u8>) = match rng.below(3) {
                0 => (0x23, vec![rng.below(0x40) as u8, rng.below(0x40) as u8]),
                1 => (0x38, vec![rng.below(8) as u8, 0]),
                // A context flag bit, the retire bit `3` left out.
                _ => (0x31, vec![[0, 1, 2, 5, 6, 7][rng.below(6) as usize]]),
            };
            match target {
                Some(t) => data.extend([op | 0x80, t]),
                None => data.push(op),
            }
            data.extend(args);
        }
        data.push(0x21);
    }
    let mut man = vec![0u8; 0x2B];
    man[0x24..0x26].copy_from_slice(&(n1 as i16).to_le_bytes());
    for o in &offsets {
        man.extend(&o.to_le_bytes()[..3]);
    }
    man[0x28..0x2B].copy_from_slice(&(data.len() as u32).to_le_bytes()[..3]);
    man.extend(&data);
    for _ in 0..5 {
        man.extend([1, 0, 0, 0]);
    }
    man.extend([0u8; 16]);
    man
}
