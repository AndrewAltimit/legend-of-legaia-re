//! The auto-command string pair against `World`: the preseed `FUN_801DA34C`
//! (`preseed_auto_command_string`) and the write-back `FUN_801DA59C`
//! (`save_auto_command_string`), over every party seat and roster order, the
//! gauge pair `+0x154` / `+0x156` either side, each band's string empty or
//! not, and for the write-back a seat alive or down on any action category.
//! Compared: the sixteen bytes of the `+0x1DF` window and both record strings
//! (`+0x76F` / `+0x77F` off `0x80084140`). Retail's staging gate
//! `DAT_8007BD04` stands raised, the one case the port calls it in.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_core::world::World;
use legaia_save::{AutoCommandBand, Party};
use psx_retail_exec::Stop;

const PRESEED: u32 = 0x801D_A34C;
const SAVE: u32 = 0x801D_A59C;
const OVERLAY_BASE: u32 = 0x801C_E818;
const CTX_PTR: u32 = 0x8007_BD24;
const CTX: u32 = 0x8012_0000;
const ACTORS: u32 = 0x801C_9370;
const CHAR_IDS: u32 = 0x8007_BD10;
const STAGED: u32 = 0x8007_BD04;
const RECORDS: u32 = 0x8008_4140;
const STRIDE: u32 = 0x414;

fn actor(seat: u32) -> u32 {
    0x8013_0000 + seat * 0x400
}

fn string(rng: &mut Rng) -> [u8; 16] {
    let mut s = [0u8; 16];
    if rng.below(3) != 0 {
        let n = 1 + rng.below(16) as usize;
        for b in s.iter_mut().take(n) {
            *b = 1 + rng.below(0x40) as u8;
        }
    }
    s
}

#[test]
fn the_auto_command_strings_are_retails() {
    let (Some(mut cpu), Some(disc)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem
        .load(OVERLAY_BASE, &disc.read_entry(898).expect("PROT 0898"));
    cpu.mem.wr32(CTX_PTR, CTX);
    cpu.mem.wr8(STAGED, 1);
    for seat in 0..8 {
        cpu.mem.wr32(ACTORS + seat * 4, actor(seat));
    }
    let mut rng = Rng(0x4155_544F);
    let (mut failures, mut wrote, mut refused) = (Vec::new(), 0, 0);
    for round in 0..20_000 {
        let party = 1 + rng.below(3) as u8;
        let mut w = World::new();
        w.load_party(Party::zeroed(3));
        w.enter_battle(party, 2);
        let mut order = vec![0u8, 1, 2];
        for i in (1..3).rev() {
            order.swap(i, rng.below(i as u32 + 1) as usize);
        }
        order.truncate(usize::from(party));
        w.party.active_party = order.clone();
        let seat = rng.below(u32::from(party)) as u8;
        for (s, &id) in order.iter().enumerate() {
            cpu.mem.wr8(CHAR_IDS + s as u32, id + 1);
        }
        for id in 0..3u32 {
            let (a, b) = (string(&mut rng), string(&mut rng));
            cpu.mem.load(RECORDS + id * STRIDE + 0x76F, &a);
            cpu.mem.load(RECORDS + id * STRIDE + 0x77F, &b);
            let rec = &mut w.party.roster.members[id as usize];
            rec.set_auto_command_string(AutoCommandBand::Primary, a);
            rec.set_auto_command_string(AutoCommandBand::Secondary, b);
        }
        let g = [0u16, 1, 50, 99, 100][rng.below(5) as usize];
        let base = [0u16, 1, 50, 99, 100][rng.below(5) as usize];
        let window: Vec<u8> = (0..16).map(|_| rng.next() as u8).collect();
        let hp = [0u16, 1, 300][rng.below(3) as usize];
        let category = rng.below(6) as u8;
        let a = actor(u32::from(seat));
        cpu.mem.wr16(a + 0x154, g);
        cpu.mem.wr16(a + 0x156, base);
        cpu.mem.load(a + 0x1DF, &window);
        cpu.mem.wr16(a + 0x14C, hp);
        cpu.mem.wr8(a + 0x1DE, category);
        cpu.mem.wr8(CTX + 0x13, seat);
        let b = &mut w.actors[usize::from(seat)].battle;
        b.agl = g;
        b.agl_base = base;
        b.params[..16].copy_from_slice(&window);
        b.hp = hp;
        b.liveness = hp;
        b.action_category = category;

        let save = rng.below(2) == 0;
        let stop = if save {
            cpu.call(SAVE, &[u32::from(seat)])
        } else {
            cpu.call(PRESEED, &[])
        };
        assert_eq!(stop, Stop::Returned, "round {round}");
        if save {
            if w.save_auto_command_string(seat) {
                wrote += 1;
            } else {
                refused += 1;
            }
        } else {
            w.preseed_auto_command_string(seat);
        }
        let mut retail = cpu.mem.read(a + 0x1DF, 16);
        let mut port = w.actors[usize::from(seat)].battle.params[..16].to_vec();
        for id in 0..3u32 {
            retail.extend(cpu.mem.read(RECORDS + id * STRIDE + 0x76F, 32));
            let rec = &w.party.roster.members[id as usize];
            port.extend(rec.auto_command_string(AutoCommandBand::Primary));
            port.extend(rec.auto_command_string(AutoCommandBand::Secondary));
        }
        if retail != port && failures.len() < 8 {
            failures.push(format!(
                "round {round} {} seat {seat} order {order:?} gauge {g}/{base} hp {hp} category {category}: {:?}",
                if save { "save" } else { "preseed" },
                crate::common::diff_offsets(&retail, &port)
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert!(wrote > 1_000 && refused > 1_000, "{wrote} {refused}");
}
