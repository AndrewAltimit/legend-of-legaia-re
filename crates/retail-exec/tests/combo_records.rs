//! The combo readout's fold into the battle records, `FUN_801E805C`
//! `0x801E88D4..0x801E89D8`, against `battle_value_readout::fold_combo_records`:
//! the acting member's Max Damage (record `+0xE8`) and Max Hits (`+0xEC`)
//! words. The run enters at the fold's head and stops where the readout's
//! draw begins, or where a shut cluster skips the rest of the pass.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_battle_vm::battle_value_readout::fold_combo_records;
use psx_retail_exec::{RETURN_SENTINEL, Stop};

const OVERLAY_BASE: u32 = 0x801C_E818;
const HEAD: u32 = 0x801E_88D4;
const DRAW: u32 = 0x801E_89D8;
const PASS_END: u32 = 0x801E_91B8;
const CTX_PTR: u32 = 0x8007_BD24;
const TOTAL: u32 = 0x8007_BD14;
const ACTORS: u32 = 0x801C_9370;
const SEATS: u32 = 0x8007_BD10;
const CHARS: u32 = 0x8008_4708;
const CHAR_STRIDE: u32 = 0x414;
const CTX: u32 = 0x8012_0000;
const ACTOR: u32 = 0x8013_0000;

#[test]
fn the_fold_keeps_what_retail_keeps() {
    differential!(prot(898, HEAD));
    let (Some(mut cpu), Some(disc)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem
        .load(OVERLAY_BASE, &disc.read_entry(898).expect("PROT 0898"));
    for stop in [DRAW, PASS_END] {
        cpu.hook(stop, |r, _| r.r[31] = RETURN_SENTINEL);
    }
    cpu.mem.wr32(CTX_PTR, CTX);
    for s in 0..8 {
        cpu.mem.wr32(ACTORS + s * 4, ACTOR + s * 0x400);
    }
    let mut rng = Rng(0x434F_4D42);
    let (mut folded, mut failures) = (0, Vec::new());
    for round in 0..20000 {
        let up = rng.below(4) != 0;
        let acting = rng.below(8) as u8;
        let category = [3, 3, 3, 2, 0, 1][rng.below(6) as usize];
        let total = match rng.below(4) {
            0 => rng.next(),
            1 => 0,
            _ => rng.below(2000),
        };
        let pushes = rng.below(256) as u8;
        let mut ids = [1u8, 2, 3];
        for i in (1..3).rev() {
            ids.swap(i, rng.below(i as u32 + 1) as usize);
        }
        let words: Vec<(u32, u32)> = (0..4)
            .map(|_| match rng.below(3) {
                0 => (rng.next(), rng.next()),
                _ => (rng.below(2000), rng.below(40)),
            })
            .collect();

        cpu.mem.wr8(CTX + 0x271, u8::from(up));
        cpu.mem.wr8(CTX + 0x13, acting);
        cpu.mem.wr8(CTX + 0x273, pushes);
        cpu.mem.wr32(TOTAL, total);
        cpu.mem
            .wr8(ACTOR + u32::from(acting) * 0x400 + 0x1DE, category);
        for (i, id) in ids.iter().enumerate() {
            cpu.mem.wr8(SEATS + i as u32, *id);
        }
        for (i, (d, h)) in words.iter().enumerate() {
            let rec = CHARS + i as u32 * CHAR_STRIDE;
            cpu.mem.wr32(rec + 0xE8, *d);
            cpu.mem.wr32(rec + 0xEC, *h);
        }
        assert_eq!(cpu.call(HEAD, &[]), Stop::Returned, "round {round}");
        let retail: Vec<(u32, u32)> = (0..4)
            .map(|i| {
                let rec = CHARS + i * CHAR_STRIDE;
                (cpu.mem.rd32(rec + 0xE8), cpu.mem.rd32(rec + 0xEC))
            })
            .collect();

        let mut port = words.clone();
        if acting < 3 {
            let r = usize::from(ids[usize::from(acting)] - 1);
            port[r] = fold_combo_records(up, acting, category, total, pushes, port[r]);
        }
        folded += u32::from(port != words);
        if retail != port && failures.len() < 8 {
            failures.push(format!(
                "round {round}: up {up} acting {acting} cat {category} total {total} pushes \
                 {pushes} ids {ids:?}\n retail {retail:?}\n port   {port:?}"
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert!(folded > 1000, "{folded}");
}
