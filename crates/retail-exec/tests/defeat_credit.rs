//! The monster death arm's credit to the records page, `FUN_8004AD80`
//! `0x8004B184..0x8004B29C`, against `battle_steal::defeat_credit`: which
//! character's Monsters Defeated word (record `+0x9C`) a death bumps. The run
//! enters at the credit's head with `s1` the dying actor and `s3` its seat,
//! and stops where the death-spoils arm begins (compared in `death_spoils`).
//! The defeat cue between them (`0x8004B1F8..0x8004B298`, against
//! `battle_steal::defeat_cue_due`) is compared on the same run: the ring slot
//! the cursor `ctx[+9]` names and the cursor's wrap.

use crate::common::{Rng, scus_cpu};
use legaia_engine_battle::battle_steal::{defeat_credit, defeat_cue_due};
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
const S3: usize = 19;
const CUE_RING: u32 = 0x8007_B6D8;
/// The formation's second cell, `gp+0x9F5`.
const SECOND_CELL: u32 = 0x8007_BD0D;

#[test]
fn a_death_credits_who_retail_credits() {
    differential!(scus(HEAD));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.hook(SPOILS_ARM, |r, _| r.r[31] = RETURN_SENTINEL);
    cpu.mem.wr32(CTX_PTR, CTX);
    let mut rng = Rng(0x4445_4645);
    let (mut credited, mut failures) = (0, Vec::new());
    let (mut cued, mut by_seat, mut held) = (0, 0, [0u32; 2]);
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
        // The cue's gates: a staged Seru absorb, a scripted fight, the
        // second cell and the dying seat (the arm runs for monster seats; a
        // party seat is driven so the last term's arm is compared too).
        let seru = rng.below(4) == 0;
        let scripted = rng.below(2) == 0;
        let second = [0u8, 0x12][rng.below(2) as usize];
        let seat = rng.below(8) as u8;
        let cursor = rng.below(4) as u8;
        cpu.mem.wr8(CTX + 9, cursor);
        cpu.mem.wr8(CTX + 0x269, u8::from(seru));
        cpu.mem.wr8(CTX + 0x287, u8::from(scripted));
        cpu.mem.wr8(SECOND_CELL, second);
        for k in 0..4 {
            cpu.mem.wr16(CUE_RING + 2 * k, 0);
        }
        cpu.mem.wr8(VICTIM + 0x1DC, flags);
        cpu.regs.r[S1] = VICTIM;
        cpu.regs.r[S3] = u32::from(seat);
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
        let due = defeat_cue_due(flags, seru, scripted, second, seat);
        let retail_cue = (
            cpu.mem.rd8(CTX + 9),
            (0..4)
                .map(|k| cpu.mem.rd16(CUE_RING + 2 * k))
                .collect::<Vec<_>>(),
        );
        let mut ring = vec![0u16; 4];
        let next = if due {
            ring[usize::from(cursor)] = 0x0D;
            if cursor + 1 >= 4 { 0 } else { cursor + 1 }
        } else {
            cursor
        };
        if due {
            cued += 1;
            if scripted && second == 0 {
                by_seat += 1;
            }
        } else if flags & 8 == 0 {
            // Held by the Seru absorb, or by the scripted fight's gate.
            held[usize::from(!seru)] += 1;
        }
        if retail_cue != (next, ring) && failures.len() < 8 {
            failures.push(format!(
                "round {round}: cue: flags {flags:#x} seru {seru} scripted {scripted} second {second:#x} seat {seat} cursor {cursor}\n retail {retail_cue:?}"
            ));
        }
        if retail != port && failures.len() < 8 {
            failures.push(format!(
                "round {round}: acting {acting} flags {flags:#x} ids {ids:?}\n retail {retail:?}\n port   {port:?}"
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert!(credited > 3000, "{credited}");
    assert!(
        cued > 3000 && by_seat > 300,
        "cued {cued}, by seat {by_seat}"
    );
    assert!(held.iter().all(|&n| n > 300), "held {held:?}");
}
