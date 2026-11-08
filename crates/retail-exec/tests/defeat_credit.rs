//! The monster death arm's credit to the records page, `FUN_8004AD80`
//! `0x8004B184..0x8004B29C`, against `battle_steal::defeat_credit`: which
//! character's Monsters Defeated word (record `+0x9C`) a death bumps. The run
//! enters at the credit's head with `s1` the dying actor and stops where the
//! death-spoils arm begins (compared in `death_spoils`).

use crate::common::{Rng, scus_cpu};
use legaia_engine_battle::battle_steal::defeat_credit;
use psx_retail_exec::{RETURN_SENTINEL, Stop};

const HEAD: u32 = 0x8004_B184;
const SPOILS_ARM: u32 = 0x8004_B29C;
const CTX_PTR: u32 = 0x8007_BD24;
const SEATS: u32 = 0x8007_BD10;
const CHARS: u32 = 0x8008_4708;
const CHAR_STRIDE: u32 = 0x414;
const CTX: u32 = 0x8012_0000;
const VICTIM: u32 = 0x8013_0000;
const S1: usize = 17;

#[test]
fn a_death_credits_who_retail_credits() {
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.hook(SPOILS_ARM, |r, _| r.r[31] = RETURN_SENTINEL);
    cpu.mem.wr32(CTX_PTR, CTX);
    let mut rng = Rng(0x4445_4645);
    let (mut credited, mut failures) = (0, Vec::new());
    for round in 0..20000 {
        let acting = rng.below(8) as u8;
        let flags = rng.below(0x100) as u8;
        // A seat's roster id: the record it names, not its seat number.
        let mut ids = [1u8, 2, 3];
        for i in (1..3).rev() {
            ids.swap(i, rng.below(i as u32 + 1) as usize);
        }
        let before: Vec<u32> = (0..4).map(|_| rng.below(1000)).collect();
        for b in 0..0x300 {
            cpu.mem.wr8(CTX + b, 0);
        }
        cpu.mem.wr8(CTX + 0x13, acting);
        for (i, id) in ids.iter().enumerate() {
            cpu.mem.wr8(SEATS + i as u32, *id);
        }
        for (i, v) in before.iter().enumerate() {
            cpu.mem.wr32(CHARS + i as u32 * CHAR_STRIDE + 0x9C, *v);
        }
        cpu.mem.wr8(VICTIM + 0x1DC, flags);
        cpu.regs.r[S1] = VICTIM;
        assert_eq!(cpu.call(HEAD, &[]), Stop::Returned, "round {round}");
        let retail: Vec<u32> = (0..4)
            .map(|i| cpu.mem.rd32(CHARS + i * CHAR_STRIDE + 0x9C))
            .collect();

        let mut port = before.clone();
        if let Some(seat) = defeat_credit(acting, flags) {
            let rec = usize::from(ids[usize::from(seat)] - 1);
            port[rec] += 1;
            credited += 1;
        }
        if retail != port && failures.len() < 8 {
            failures.push(format!(
                "round {round}: acting {acting} flags {flags:#x} ids {ids:?}\n retail {retail:?}\n port   {port:?}"
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert!(credited > 3000, "{credited}");
}
