//! The dialog SM's post-box byte classification `FUN_80038050` against
//! `talk_dispatch`, over every byte value with generated successors, and its
//! option-jump arm against `Picker::jump_target` over every option picker in
//! the disc's scene MANs, each option chosen in turn.
//!
//! The `4C FE xx` arm also stores `xx` to `_DAT_801F273C` (the pager's
//! box-gone count, `0x80038144..0x8003814C`); the port leaves that store out,
//! and the field-op census finds no `4C FE` on the disc, so the test holds the
//! cursor and the verdict there and counts the store as latent.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_field::inline_dialogue::{TalkDispatch, talk_dispatch};
use legaia_mes::picker::parse_picker_at;
use psx_retail_exec::{Cpu, Stop, V0};

const CLASSIFY: u32 = 0x8003_8050;
const SCENE_PTR: u32 = 0x801C_6EA4;
const SCENE: u32 = 0x8014_0000;
const CTX: u32 = 0x8013_0000;
const SCRIPT: u32 = 0x8011_0000;

fn run(cpu: &mut Cpu, base: u32, pc: u16) -> (u32, u16) {
    cpu.mem.wr32(CTX + 0x90, base);
    cpu.mem.wr16(CTX + 0x9E, pc);
    cpu.regs.r[V0] = 0xDEAD_BEEF;
    assert_eq!(cpu.call(CLASSIFY, &[CTX]), Stop::Returned);
    (cpu.regs.r[V0], cpu.mem.rd16(CTX + 0x9E))
}

#[test]
fn the_post_box_byte_is_classified_as_retail_classifies_it() {
    differential!(scus(CLASSIFY));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem.wr32(SCENE_PTR, SCENE);
    let mut rng = Rng(0x5441_4C4B);
    let mut seen = [0u32; 3];
    for _ in 0..40_000 {
        let pc = rng.below(0x40) as usize;
        let mut bytes: Vec<u8> = (0..pc + 4).map(|_| rng.next() as u8).collect();
        bytes[pc] = match rng.below(4) {
            0 => rng.next() as u8,
            _ => [0x21u8, 0x24, 0x25, 0x48, 0x4C, 0x26, 0x00, 0xA4, 0xCC][rng.below(9) as usize],
        };
        if bytes[pc] == 0x4C {
            bytes[pc + 1] = [0xFFu8, 0xFE, rng.next() as u8][rng.below(3) as usize];
        }
        if (0x27..=0x2A).contains(&bytes[pc]) {
            continue;
        }
        cpu.mem.load(SCRIPT, &bytes);
        let (verdict, cursor) = run(&mut cpu, SCRIPT, pc as u16);
        let (keep, next) = match talk_dispatch(&bytes, pc) {
            TalkDispatch::EndAfter(n) => {
                seen[0] += 1;
                (0, n)
            }
            TalkDispatch::Continue(n) => {
                seen[1] += 1;
                (1, n)
            }
            TalkDispatch::EndParked(n) => {
                seen[2] += 1;
                (0, n)
            }
        };
        assert_eq!(
            (verdict, usize::from(cursor)),
            (keep, next),
            "{:02x?} at {pc}",
            &bytes[pc..]
        );
    }
    assert!(seen.iter().all(|n| *n > 2000), "{seen:?}");
}

fn man_payloads(buf: &[u8]) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    if let Some(table) = legaia_scene_formats::scene_asset_table::detect(buf) {
        for d in table.descriptors.iter().filter(|d| d.type_byte == 0x03) {
            let start = d.data_offset as usize;
            if start < buf.len()
                && let Ok((decoded, _)) =
                    psx_lzs::decompress_tracked(&buf[start..], d.size as usize)
            {
                out.push(decoded);
            }
        }
    }
    out
}

#[test]
fn every_disc_picker_jumps_where_retail_jumps() {
    differential!(scus(CLASSIFY));
    let (Some(mut cpu), Some(disc)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem.wr32(SCENE_PTR, SCENE);
    let (mut pickers, mut jumps) = (0, 0);
    for entry in 0..863 {
        let Ok(buf) = disc.read_entry(entry) else {
            continue;
        };
        for man in man_payloads(&buf) {
            let mut loaded = false;
            for open in 0..man.len() {
                if !(0x27..=0x2A).contains(&man[open]) {
                    continue;
                }
                let Some(picker) = parse_picker_at(&man, open) else {
                    continue;
                };
                // A byte run that only parses as a picker jumps off the MAN;
                // a real one lands inside it.
                if (0..picker.n).any(|i| picker.jump_target(i).is_none_or(|t| t >= man.len())) {
                    continue;
                }
                if !loaded {
                    cpu.mem.load(SCRIPT, &man);
                    loaded = true;
                }
                pickers += 1;
                // The context's base sits a little behind the open byte, so the
                // signed cursor `+0x9E` stays small as in a record.
                let back = open.min(0x100);
                let base = SCRIPT + (open - back) as u32;
                for idx in 0..picker.n {
                    cpu.mem.wr16(SCENE + 0xC, idx as u16);
                    let (verdict, cursor) = run(&mut cpu, base, back as u16);
                    let target = picker.jump_target(idx).expect("in-buffer jump") as i64
                        - (open - back) as i64;
                    assert_eq!(
                        (verdict, cursor),
                        (1, target as u16),
                        "entry {entry} picker at {open:#x} option {idx}"
                    );
                    jumps += 1;
                }
            }
        }
    }
    assert!(pickers > 50 && jumps > 100, "{pickers} {jumps}");
}
