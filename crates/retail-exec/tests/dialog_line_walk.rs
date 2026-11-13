//! The per-actor dialog SM `FUN_80039B7C`'s state-`2` arm
//! (`0x80039BD4..0x80039C68`): with the pager idle it walks the cursor past
//! the text it has shown - every line from the lead to its terminator,
//! skipping the `0xC?` two-byte escapes, while the byte after a line is
//! another yield byte - and parks the context's `+0x9E` there. Held over every
//! `0x1F` lead in the disc's interaction records against
//! `legaia_mes::dialog_box::shown_text_end`, which the inline runner resumes
//! the record from once a box is dismissed.
//!
//! Every call the arm and the teardown after it make is stubbed; the cursor
//! is read at its store.

use crate::common::{disc, scus_cpu};
use legaia_scene_formats::field_disasm::man_script_spans;
use psx_retail_exec::{A2, Stop};
use std::cell::Cell;
use std::collections::BTreeMap;
use std::rc::Rc;

const SM: u32 = 0x8003_9B7C;
const SM_END: u32 = 0x8003_A024;
const CURSOR_STORE: u32 = 0x8003_9C68;
const PAGER: u32 = 0x801F_2734;
const SCENE_PTR: u32 = 0x801C_6EA4;
const PLAYER_PTR: u32 = 0x8007_C364;
const SCRIPT: u32 = 0x8011_0000;
const CTX: u32 = 0x8012_0000;
const SCENE: u32 = 0x8013_0000;
const PLAYER: u32 = 0x8013_1000;
const RUN_TO_TEXT: u32 = 0x8003_CF7C;
const FIELD_VM: u32 = 0x801D_E840;

#[test]
fn the_shown_text_walk_parks_where_retail_parks() {
    differential!(scus(SM));
    let (Some(mut cpu), Some(d)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.stub_outside = Some((SM, SM_END));
    let parked = Rc::new(Cell::new(None::<u32>));
    let p = parked.clone();
    cpu.probe(CURSOR_STORE, move |r, _, _| p.set(Some(r.r[A2] & 0xFFFF)));
    let mut seen = std::collections::BTreeSet::new();
    let (mut leads, mut long_pages) = (0usize, 0usize);
    let mut differ: BTreeMap<String, (usize, String)> = BTreeMap::new();
    for entry in 0..863 {
        let Ok(buf) = d.read_entry(entry) else {
            continue;
        };
        let Some(table) = legaia_scene_formats::scene_asset_table::detect(&buf) else {
            continue;
        };
        for desc in table.descriptors.iter().filter(|d| d.type_byte == 0x03) {
            let start = desc.data_offset as usize;
            let Some(Ok((man, _))) = buf
                .get(start..)
                .map(|b| psx_lzs::decompress_tracked(b, desc.size as usize))
            else {
                continue;
            };
            let Ok(man_file) = legaia_scene_formats::man_section::parse(&man) else {
                continue;
            };
            for (_, _, s, _, len) in man_script_spans(&man_file, &man) {
                let body = &man[s..s + len];
                if !seen.insert(body.to_vec()) || body.len() > 0x8000 {
                    continue;
                }
                cpu.mem.load(SCRIPT, body);
                cpu.mem.wr8(SCRIPT + body.len() as u32, 0x25);
                for pc in (0..body.len()).filter(|&i| body[i] == 0x1F) {
                    if pc > 0 && body[pc - 1] >= 0x20 && body[pc - 1] & 0x80 == 0 {
                        // Inside a line, not a lead.
                        continue;
                    }
                    cpu.mem.load(CTX, &[0u8; 0x100]);
                    cpu.mem.wr16(CTX + 0x9C, 2);
                    cpu.mem.wr32(CTX + 0x90, SCRIPT);
                    cpu.mem.wr16(CTX + 0x9E, pc as u16);
                    cpu.mem.wr16(CTX + 0x5C, 0xFFFF);
                    cpu.mem.wr32(PAGER, [0, 3, 6, 9][leads % 4]);
                    cpu.mem.wr32(SCENE_PTR, SCENE);
                    cpu.mem.wr32(PLAYER_PTR, PLAYER);
                    parked.set(None);
                    assert_eq!(cpu.call(SM, &[CTX]), Stop::Returned);
                    let Some(retail) = parked.get() else {
                        panic!("entry {entry} pc {pc:#x}: the arm did not run");
                    };
                    let retail = retail as usize;
                    leads += 1;
                    long_pages += usize::from(
                        legaia_mes::dialog_box::pack_page(body, pc)
                            .unwrap()
                            .lines
                            .len()
                            > 3,
                    );
                    let port = legaia_mes::dialog_box::shown_text_end(body, pc);
                    if port >= body.len() {
                        // The walk ran off the record: the bytes past it are
                        // the MAN's next record, not this harness's.
                        continue;
                    }
                    if port != retail {
                        let e = differ.entry(format!("{:02X}", body[pc + 1])).or_default();
                        e.0 += 1;
                        if e.0 == 1 {
                            e.1 = format!(
                                "entry {entry} pc {pc:#x}: retail {retail:#x} port {port:#x}"
                            );
                        }
                    }
                }
            }
        }
    }
    let report: Vec<String> = differ
        .iter()
        .map(|(k, (n, ex))| format!("{k}: {n} e.g. {ex}"))
        .collect();
    assert!(report.is_empty(), "{}", report.join("\n"));
    // Pages past the window's three rows, and every lead the disc's records
    // carry, ran.
    assert!(leads > 20_000 && long_pages > 400, "{leads} {long_pages}");
}

/// `FUN_8003CF7C`, the run-to-next-text helper the SM's other states call,
/// against `inline_dialogue::run_to_next_text`. The field-VM dispatcher it
/// calls is hooked to a scripted step: each PC's next PC is drawn once (a
/// stay, a hop forward), and both sides read the same bytes, so the returned
/// byte, the `+0x9E` the context is left on and the PCs stepped are compared.
/// Every end is reached: text before the first step, text after one, the
/// `0x21` frame mark and the park.
#[test]
fn the_run_to_text_helper_matches_retail() {
    differential!(scus(RUN_TO_TEXT));
    use legaia_engine_field::inline_dialogue::{SliceEnd, run_to_next_text};
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    const LEN: usize = 0x200;
    let next: Rc<std::cell::RefCell<Vec<usize>>> = Rc::default();
    let stepped: Rc<std::cell::RefCell<Vec<usize>>> = Rc::default();
    {
        let (next, stepped) = (next.clone(), stepped.clone());
        cpu.hook(FIELD_VM, move |r, m| {
            let pc = r.r[psx_retail_exec::A1] as i32 as usize;
            assert_eq!(r.r[A2], CTX, "the context rides in a2");
            assert_eq!(r.r[psx_retail_exec::A0], m.rd32(CTX + 0x90));
            stepped.borrow_mut().push(pc);
            r.r[psx_retail_exec::V0] = next.borrow()[pc] as u32;
        });
    }
    let mut rng = crate::common::Rng(0x8003_CF7C);
    let mut ends = std::collections::BTreeMap::<String, u32>::new();
    for _ in 0..4000 {
        // Mostly op bytes, with text and `0x21` mixed in.
        let bytes: Vec<u8> = (0..LEN)
            .map(|_| match rng.below(16) {
                0 => rng.below(0x20) as u8 | (rng.below(2) as u8) << 7,
                1 => 0x21,
                _ => 0x20 + rng.below(0xE0) as u8,
            })
            .collect();
        // A PC goes forward or stays; the last few always stay.
        let table: Vec<usize> = (0..LEN)
            .map(|pc| match rng.below(24) {
                0 => pc,
                _ if pc + 9 >= LEN => pc,
                _ => pc + 1 + rng.below(8) as usize,
            })
            .collect();
        let start = rng.below(LEN as u32 / 2) as usize;
        *next.borrow_mut() = table.clone();
        stepped.borrow_mut().clear();
        cpu.mem.load(SCRIPT, &bytes);
        cpu.mem.wr32(CTX + 0x90, SCRIPT);
        cpu.mem.wr16(CTX + 0x9E, start as u16);
        assert_eq!(cpu.call(RUN_TO_TEXT, &[CTX]), Stop::Returned);
        let byte = cpu.regs.r[psx_retail_exec::V0] as u8;
        let pc = cpu.mem.rd16(CTX + 0x9E) as usize;

        let mut port_steps = Vec::new();
        let slice = run_to_next_text(
            &mut port_steps,
            start,
            usize::MAX,
            |_, pc| bytes.get(pc).copied(),
            |log, pc| {
                log.push(pc);
                Some(table[pc])
            },
        );
        let at = format!("start {start:#x}");
        assert_ne!(slice.end, SliceEnd::Stopped, "{at}");
        assert_eq!(slice.byte, byte, "returned byte, {at}");
        assert_eq!(slice.pc, pc, "+0x9E, {at}");
        assert_eq!(port_steps, *stepped.borrow(), "PCs stepped, {at}");
        let kind = match (slice.end, slice.steps) {
            (SliceEnd::Text, 0) => "text at entry".to_string(),
            (e, _) => format!("{e:?}"),
        };
        *ends.entry(kind).or_default() += 1;
    }
    for e in ["text at entry", "Text", "FrameMark", "Parked"] {
        assert!(ends.get(e).copied().unwrap_or(0) > 50, "{e}: {ends:?}");
    }
}
