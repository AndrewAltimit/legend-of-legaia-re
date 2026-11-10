//! Two leaves of the queue builder `FUN_801EED1C`, run on their own:
//!
//! - `FUN_801EF9E4`, the Super Art tail replace, against
//!   `apply_super_tail_replace` over every character's five resident rows in
//!   PROT 0898: queues ending in each row's find string, queues that end in
//!   part of one, random queues, and the empty queue. The written queue, the
//!   starter marks at `0x801F6990` and the trigger latch `0x801F696C` are
//!   compared whole.
//! - `FUN_801EFBFC`, the learned-art check, against `check_and_learn_art`
//!   with the gate `World::learn_art_on_record` rolls (the seat's Auto flag
//!   `ctx[+0x266 + seat]`, one `rand() & 0x1FF`, Tetsu's spar
//!   `DAT_8007BD0C == 0x4F`) over lists of every length, ids either side of
//!   the innate cap and the zero id. The record's list at `+0x185` and the
//!   draws are compared.

use crate::common::{Rng, diff_offsets, disc, scus_cpu};
use legaia_engine_battle_vm::battle_action::{
    ACTION_QUEUE_CAP, ArtUseCheck, SUPER_FIND_STRIDE, SUPER_REPLACE_STRIDE, SUPER_ROWS,
    apply_super_tail_replace, check_and_learn_art,
};
use psx_retail_exec::{Stop, V0};
use std::cell::Cell;
use std::rc::Rc;

const SUPER: u32 = 0x801E_F9E4;
const LEARN: u32 = 0x801E_FBFC;
const RAND: u32 = 0x8005_6798;
const OVERLAY_BASE: u32 = 0x801C_E818;
const CTX_PTR: u32 = 0x8007_BD24;
const CTX: u32 = 0x8012_0000;
const ACTORS: u32 = 0x801C_9370;
const SEATS: u32 = 0x8007_BD10;
const FIRST_MONSTER: u32 = 0x8007_BD0C;
const RECORDS: u32 = 0x8008_4708;
const STRIDE: u32 = 0x414;
const ACTOR: u32 = 0x8013_0000;
const FIND_ROWS: u32 = 0x801F_6524;
const REPLACE_ROWS: u32 = 0x801F_65E8;
const MARKS: u32 = 0x801F_6990;
const LATCH: u32 = 0x801F_696C;
const CAPS: u32 = 0x801F_686C;

#[test]
fn the_super_tail_replace_is_retails() {
    differential!(prot(898, SUPER));
    let (Some(mut cpu), Some(disc)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem
        .load(OVERLAY_BASE, &disc.read_entry(898).expect("PROT 0898"));
    for s in 0..8 {
        cpu.mem.wr32(ACTORS + s * 4, ACTOR + s * 0x400);
    }
    let mut rng = Rng(0x5355_5052);
    let (mut matched, mut missed, mut starters) = (0, 0, 0);
    for n in 0..30_000 {
        // Vahn, Noa and Gala; the bytes past their rows are other tables.
        let ch = rng.below(3);
        let seat = rng.below(3);
        let mut find_rows = [[0u8; SUPER_FIND_STRIDE]; SUPER_ROWS];
        let mut replace_rows = [[0u8; SUPER_REPLACE_STRIDE]; SUPER_ROWS];
        for row in 0..SUPER_ROWS {
            let f = cpu.mem.read(
                FIND_ROWS + (row * SUPER_FIND_STRIDE) as u32 + ch * 65,
                SUPER_FIND_STRIDE,
            );
            let r = cpu.mem.read(
                REPLACE_ROWS + (row * SUPER_REPLACE_STRIDE) as u32 + ch * 80,
                SUPER_REPLACE_STRIDE,
            );
            find_rows[row].copy_from_slice(&f);
            replace_rows[row].copy_from_slice(&r);
        }
        // The queue: a prefix of arrow tokens, then most often one row's
        // find string (whole, or cut short), then the terminator.
        let mut queue = [0u8; ACTION_QUEUE_CAP];
        let mut len = rng.below(8) as usize;
        for b in queue.iter_mut().take(len) {
            *b = 1 + rng.below(6) as u8;
        }
        if rng.below(4) != 0 {
            let row = &find_rows[rng.below(SUPER_ROWS as u32) as usize];
            let mut flen = usize::from(row[0]).min(SUPER_FIND_STRIDE - 1);
            if rng.below(4) == 0 {
                flen = flen.saturating_sub(1);
            }
            for j in 0..flen {
                if len < 16 {
                    queue[len] = row[1 + j];
                    len += 1;
                }
            }
        }
        if n % 50 == 0 {
            len = 0;
        }
        for b in queue.iter_mut().skip(len) {
            *b = 0;
        }
        if len >= 16 && rng.below(2) == 0 {
            // No terminator inside the scan window.
            queue[16..]
                .iter_mut()
                .for_each(|b| *b = 1 + rng.below(6) as u8);
        }
        let mut marks = [0u32; ACTION_QUEUE_CAP];
        for m in marks.iter_mut() {
            *m = rng.below(3);
        }
        let actor = ACTOR + seat * 0x400;
        cpu.mem.load(actor + 0x1DF, &queue);
        cpu.mem
            .load(actor + 0x1DF + ACTION_QUEUE_CAP as u32, &[0xEE; 0x20]);
        for (i, m) in marks.iter().enumerate() {
            cpu.mem.wr32(MARKS + i as u32 * 4, *m);
        }
        for i in ACTION_QUEUE_CAP..0x20 {
            cpu.mem.wr32(MARKS + i as u32 * 4, 0xEEEE_EEEE);
        }
        cpu.mem.wr32(LATCH, 0);
        assert_eq!(cpu.call(SUPER, &[seat, ch]), Stop::Returned);

        let verdict = apply_super_tail_replace(&mut queue, &mut marks, &find_rows, &replace_rows);
        let retail_q = cpu.mem.read(actor + 0x1DF, ACTION_QUEUE_CAP);
        assert!(
            retail_q == queue,
            "n {n} char {ch}: {:?}",
            diff_offsets(&retail_q, &queue)
        );
        assert_eq!(
            cpu.mem.read(actor + 0x1DF + ACTION_QUEUE_CAP as u32, 0x20),
            vec![0xEE; 0x20],
            "n {n}: retail wrote past the queue"
        );
        for (i, m) in marks.iter().enumerate() {
            assert_eq!(cpu.mem.rd32(MARKS + i as u32 * 4), *m, "n {n} mark {i}");
        }
        assert_eq!(cpu.mem.rd32(LATCH) == 1, verdict.is_some(), "n {n}");
        match verdict {
            Some(v) => {
                matched += 1;
                if queue[v.start..v.start + v.written].contains(&0x1A) {
                    starters += 1;
                }
            }
            None => missed += 1,
        }
    }
    assert!(
        matched > 3000 && missed > 3000 && starters > 500,
        "{matched} {missed} {starters}"
    );
}

#[test]
fn the_learned_art_check_is_retails() {
    differential!(prot(898, LEARN));
    let (Some(mut cpu), Some(disc)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem
        .load(OVERLAY_BASE, &disc.read_entry(898).expect("PROT 0898"));
    cpu.mem.wr32(CTX_PTR, CTX);
    let draws = Rc::new(Cell::new(0u32));
    let next = Rc::new(Cell::new(0u32));
    {
        let (draws, next) = (draws.clone(), next.clone());
        cpu.hook(RAND, move |r, _| {
            draws.set(draws.get() + 1);
            r.r[V0] = next.get();
        });
    }
    let caps: Vec<u8> = cpu.mem.read(CAPS, 4);
    let mut rng = Rng(0x4C52_4E41);
    let mut seen = [0u32; 3];
    for n in 0..40_000 {
        let seat = rng.below(3);
        let roster = 1 + rng.below(4);
        cpu.mem.wr8(SEATS + seat, roster as u8);
        let auto = rng.below(2) == 0;
        cpu.mem.wr8(
            CTX + 0x266 + seat,
            if auto { 1 + rng.below(3) as u8 } else { 0 },
        );
        let spar = rng.below(10) == 0;
        cpu.mem.wr8(
            FIRST_MONSTER,
            if spar { 0x4F } else { rng.below(0x4F) as u8 },
        );
        let roll = if rng.below(4) == 0 {
            0
        } else {
            rng.below(0x8000)
        };
        next.set(roll);
        draws.set(0);

        // An ascending list of distinct ids, of every length to fifteen.
        let count = rng.below(16) as u8;
        let mut ids = [0u8; 16];
        let mut pool: Vec<u8> = (1..0x40).collect();
        for i in (1..pool.len()).rev() {
            pool.swap(i, rng.below(i as u32 + 1) as usize);
        }
        let mut list: Vec<u8> = pool[..usize::from(count)].to_vec();
        list.sort_unstable();
        ids[..list.len()].copy_from_slice(&list);
        for b in ids.iter_mut().skip(list.len()) {
            *b = rng.next() as u8;
        }
        let art = match rng.below(5) {
            0 if count > 0 => list[rng.below(u32::from(count)) as usize],
            1 => 0,
            _ => rng.below(0x40) as u8,
        };
        let rec = RECORDS + (roster - 1) * STRIDE;
        cpu.mem.wr8(rec + 0x185, count);
        cpu.mem.load(rec + 0x186, &ids);
        cpu.mem.wr8(rec + 0x196, 0xEE);
        assert_eq!(cpu.call(LEARN, &[seat, u32::from(art)]), Stop::Returned);
        let retail = cpu.regs.r[V0];

        let mut port_draws = 0;
        let learn_gate_open = !auto
            || {
                port_draws += 1;
                roll & 0x1FF == 0
            }
            || spar;
        let mut port_count = count;
        let mut port_ids = ids;
        let verdict = check_and_learn_art(
            &mut port_count,
            &mut port_ids,
            art,
            learn_gate_open,
            caps[(roster - 1) as usize],
        );
        let port = match verdict {
            ArtUseCheck::Unknown => 0,
            ArtUseCheck::Known => 1,
            ArtUseCheck::Learned => 2,
        };
        assert_eq!(retail, port, "n {n} art {art} count {count} {ids:?}");
        assert_eq!(draws.get(), port_draws, "n {n}");
        let mut want = vec![port_count];
        want.extend(port_ids);
        want.push(0xEE);
        let got = cpu.mem.read(rec + 0x185, 18);
        assert!(got == want, "n {n}: {:?}", diff_offsets(&got, &want));
        seen[port as usize] += 1;
    }
    assert!(seen.iter().all(|&s| s > 3000), "{seen:?}");
}
