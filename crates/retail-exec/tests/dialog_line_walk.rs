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
/// is not compared: the port has no kernel of its own for it.
/// `World::step_inline_dialogue` runs the field VM in one loop with the
/// SM's box, park and pass-end decisions, so where the helper returns a raw
/// `0x21` and leaves the next text to the SM's next frame, the runner opens
/// that box in the same call. What is checked is the helper's shape: one
/// callee, the field VM, and the two ends the runner maps (the `0x21` compare
/// and the stalled-cursor compare).
#[test]
fn the_run_to_text_helper_calls_only_the_field_vm() {
    not_a_differential!(
        "FUN_8003CF7C's loop is fused into World::step_inline_dialogue with the dialog SM's decisions; checks the helper calls only the field VM and carries the 0x21 and stalled-cursor ends"
    );
    let Some(scus) = crate::common::scus() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let at = |va: u32| {
        let o = (va - 0x8001_0000) as usize + 0x800;
        u32::from_le_bytes(scus[o..o + 4].try_into().unwrap())
    };
    let body: Vec<u32> = (0x8003_CF7Cu32..0x8003_D038).step_by(4).map(at).collect();
    let calls: Vec<u32> = body
        .iter()
        .filter(|w| *w >> 26 == 3)
        .map(|w| 0x8000_0000 | (w & 0x03FF_FFFF) << 2)
        .collect();
    assert_eq!(calls, vec![0x801D_E840]);
    // `li s4,0x21` and the `beq s0,s4` it feeds; `beq s1,v0` on the cursor.
    assert!(body.contains(&0x2414_0021));
    assert_eq!(at(0x8003_CFE0) >> 16, 0x1214);
    assert_eq!(at(0x8003_CFF0) >> 16, 0x1222);
}
