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
use legaia_engine_dialog::dialog::OwnedDialogPanel;
use legaia_scene_formats::field_disasm::man_script_spans;
use psx_retail_exec::{A2, Stop};
use std::cell::Cell;
use std::collections::BTreeMap;
use std::rc::Rc;
use std::sync::Arc;

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

const POST_BOX: u32 = 0x8003_8050;
const LATCH: u32 = 0x8007_3F02;
const HOLD: u32 = 0x8007_B6B4;
const FEED: u32 = 0x801F_3538;

/// What one runner call left on the context, the control block and the
/// globals it writes.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct After {
    state: i16,
    pc: u16,
    flags: u32,
    heading: u16,
    depth: u16,
    pager: u32,
    latch: u8,
    hold: u32,
    feed: u32,
}

fn read_after(cpu: &mut psx_retail_exec::Cpu) -> After {
    After {
        state: cpu.mem.rd16(CTX + 0x9C) as i16,
        pc: cpu.mem.rd16(CTX + 0x9E),
        flags: cpu.mem.rd32(CTX + 0x10),
        heading: cpu.mem.rd16(CTX + 0x26),
        depth: cpu.mem.rd16(SCENE + 0xA),
        pager: cpu.mem.rd32(PAGER),
        latch: cpu.mem.rd8(LATCH),
        hold: cpu.mem.rd32(HOLD),
        feed: cpu.mem.rd32(FEED),
    }
}

/// `FUN_80039B7C` over its three states, one call per round, with the
/// field-VM dispatcher hooked to a scripted step (as for the run-to-text
/// helper) and `FUN_80038050` to a drawn verdict; the bearing resolver
/// `FUN_80019B28` runs. Each arm is held against the port that carries it:
///
/// - state `0`'s op loop (`0x80039DD8..0x80039E5C`) against
///   `inline_dialogue::run_to_next_text` - the PCs stepped, the `+0x9E` it
///   parks and how it ends (text opens a line, an executed `0x21` tears the
///   context down, a stalled PC leaves it in state `0`);
/// - the face-the-player arm (`0x80039F48..0x80039F80`) against
///   `World::face_field_npc_at_player`;
/// - state `1`'s hand-off (`0x80039F9C..0x8003A000`): only an idle pager
///   (`1`, `4`, `7`) takes the line, a held `0x24` box (`4`) going on as the
///   same window (`HeldBox::next_line_panel`);
/// - state `2`'s walk on a byte that is not a line (`0x80039BF4`) against
///   `legaia_mes::dialog_box::shown_text_end`, and a busy pager leaving the
///   context alone;
/// - both teardowns (`0x80039C94..0x80039D60`, `0x80039E68..0x80039F34`):
///   the heading restore against `World::release_talk_facing`, and the
///   release at depth `0` against `World::tick_script_release_hold`.
#[test]
fn the_runner_states_step_as_retail() {
    differential!(scus(SM));
    use legaia_engine_core::world::World;
    use legaia_engine_field::field_warp_tile::SCRIPT_RELEASE_HOLD_FRAMES;
    use legaia_engine_field::inline_dialogue::{HeldBox, SliceEnd, run_to_next_text};
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    const LEN: usize = 0x100;
    let next: Rc<std::cell::RefCell<Vec<usize>>> = Rc::default();
    let stepped: Rc<std::cell::RefCell<Vec<usize>>> = Rc::default();
    let verdict = Rc::new(Cell::new(0u32));
    {
        let (next, stepped) = (next.clone(), stepped.clone());
        cpu.hook(FIELD_VM, move |r, _| {
            let pc = r.r[psx_retail_exec::A1] as i32 as usize;
            stepped.borrow_mut().push(pc);
            r.r[psx_retail_exec::V0] = next.borrow()[pc] as u32;
        });
        let v = verdict.clone();
        cpu.hook(POST_BOX, move |r, _| r.r[psx_retail_exec::V0] = v.get());
    }
    let mut rng = crate::common::Rng(0x8003_9B7C);
    let mut seen = BTreeMap::<String, u32>::new();
    for round in 0..6000 {
        let bytes: Vec<u8> = (0..LEN)
            .map(|_| match rng.below(16) {
                0 => rng.below(0x20) as u8 | (rng.below(2) as u8) << 7,
                1 => 0x21,
                2 => 0xA1,
                _ => 0x20 + rng.below(0xE0) as u8,
            })
            .collect();
        let table: Vec<usize> = (0..LEN)
            .map(|pc| match rng.below(16) {
                0 => pc,
                _ if pc + 9 >= LEN => pc,
                _ => pc + 1 + rng.below(8) as usize,
            })
            .collect();
        *next.borrow_mut() = table.clone();
        stepped.borrow_mut().clear();
        let state: i16 = [0, 0, 0, 1, 2][rng.below(5) as usize];
        let mut start = rng.below(LEN as u32 / 2) as usize;
        if state == 2 && rng.below(2) == 0 {
            // A line the walk crosses.
            start = (0..LEN / 2)
                .find(|&i| bytes[i] & 0x7F < 0x20)
                .unwrap_or(start);
        }
        let flags = 0x100
            | [0, 0x2_0000, 0x2_0000, 0x100_0000][rng.below(4) as usize]
            | if rng.below(3) == 0 { 0x40_0000 } else { 0 };
        let share = 1 + rng.below(2) as u16;
        let depth = share
            + if rng.below(2) == 0 {
                0
            } else {
                1 + rng.below(2) as u16
            };
        let pager = rng.below(11);
        let latch = rng.below(2) as u8;
        let saved = rng.next() as u16;
        let (nx, nz) = (rng.below(4000) as i16, rng.below(4000) as i16);
        let (px, pz) = (
            nx + rng.below(600) as i16 - 300,
            nz + rng.below(600) as i16 - 300,
        );
        let anim: u16 = if rng.below(4) == 0 { 0xFFFF } else { 3 };
        verdict.set(rng.below(2));

        cpu.mem.load(SCRIPT, &bytes);
        cpu.mem.load(CTX, &[0u8; 0x100]);
        cpu.mem.load(SCENE, &[0u8; 0x80]);
        cpu.mem.load(PLAYER, &[0u8; 0x100]);
        cpu.mem.wr16(CTX + 0x9C, state as u16);
        cpu.mem.wr32(CTX + 0x90, SCRIPT);
        cpu.mem.wr16(CTX + 0x9E, start as u16);
        cpu.mem.wr32(CTX + 0x10, flags);
        cpu.mem.wr16(CTX + 0x5C, anim);
        cpu.mem.wr16(CTX + 0x5A, saved);
        cpu.mem.wr16(CTX + 0x26, 0x123);
        cpu.mem.wr16(CTX + 0x2A, share);
        cpu.mem.wr16(CTX + 0x14, nx as u16);
        cpu.mem.wr16(CTX + 0x18, nz as u16);
        cpu.mem.wr16(PLAYER + 0x14, px as u16);
        cpu.mem.wr16(PLAYER + 0x18, pz as u16);
        cpu.mem.wr32(PLAYER + 0x10, 0x8_0000);
        cpu.mem.wr16(SCENE + 0xA, depth);
        cpu.mem.wr32(PAGER, pager);
        cpu.mem.wr8(LATCH, latch);
        cpu.mem.wr32(HOLD, 7);
        cpu.mem.wr32(FEED, 0);
        cpu.mem.wr32(SCENE_PTR, SCENE);
        cpu.mem.wr32(PLAYER_PTR, PLAYER);
        assert_eq!(cpu.call(SM, &[CTX]), Stop::Returned, "round {round}");
        let after = read_after(&mut cpu);
        let at = format!("round {round} state {state} start {start:#x} pager {pager}");
        let torn = after.flags & 0x100 == 0;

        // State 2: the walk, or nothing while the pager is busy.
        let mut ran_state0 = state == 0;
        let mut pc0 = start;
        // The walk steps the pager on as it takes the shown text (`0x80039C70`).
        let mut pager = pager;
        if state == 2 {
            if ![0, 3, 6, 9].contains(&pager) {
                assert_eq!(after.state, 2, "{at}: a busy pager holds the walk");
                assert_eq!(after.pc as usize, start, "{at}");
                *seen.entry("2 busy".into()).or_default() += 1;
                continue;
            }
            let walked = legaia_mes::dialog_box::shown_text_end(&bytes, start);
            if walked >= LEN {
                continue;
            }
            *seen
                .entry(format!("2 walk {}", walked == start))
                .or_default() += 1;
            pager += 1;
            if verdict.get() != 0 {
                // The talk goes on: state 0 runs in the same call.
                ran_state0 = true;
                pc0 = walked;
            } else {
                assert!(torn, "{at}: the talk ends");
                assert_eq!(after.pc as usize, walked, "{at}: the walk");
            }
        }

        // State 0: the op loop against the run-to-text port.
        let mut opened = false;
        if ran_state0 {
            let mut port_steps = Vec::new();
            let slice = run_to_next_text(
                &mut port_steps,
                pc0,
                usize::MAX,
                |_, pc| bytes.get(pc).copied(),
                |log, pc| {
                    log.push(pc);
                    Some(table[pc])
                },
            );
            assert_eq!(port_steps, *stepped.borrow(), "{at}: the PCs stepped");
            assert_eq!(after.pc as usize, slice.pc, "{at}: +0x9E");
            let ends = World::slice_ends_runner_pass(&slice);
            assert_eq!(
                torn, ends,
                "{at}: the pass ends ({:?} {:#x})",
                slice.end, slice.byte
            );
            if slice.end == SliceEnd::Text {
                assert!(after.state >= 1 && !torn, "{at}: a line opens");
                opened = true;
            } else {
                assert_eq!(after.state, 0, "{at}");
            }
            *seen.entry(format!("0 {:?} {ends}", slice.end)).or_default() += 1;
        }

        // The face arm, as a line opens.
        if opened && flags & 0x2_0000 != 0 {
            let mut w = World::new();
            w.install_field_player(0);
            w.actors[0].move_state.world_x = px;
            w.actors[0].move_state.world_z = pz;
            w.npcs.positions.insert(3, (nx, nz));
            w.npcs.headings.insert(3, 0x123);
            w.field_vm
                .channels
                .push(legaia_engine_field::field_channels::FieldChannel {
                    placement_index: 3,
                    ctx: legaia_engine_vm::field::FieldCtx {
                        flags,
                        ..Default::default()
                    },
                    record_offset: 0,
                    pc: 0,
                    done: false,
                    object_bind: false,
                });
            w.face_field_npc_at_player(3);
            let port = w.npcs.headings[&3] as u16 & 0xFFF;
            let faced = flags & 0x40_0000 == 0;
            if faced {
                // The engine's heading faces Z+ at 0, retail's `+0x26` Z-.
                assert_eq!((after.heading + 0x800) & 0xFFF, port, "{at}: the bearing");
            } else {
                assert_eq!(
                    (after.heading, port),
                    (0x123, 0x123),
                    "{at}: a static class keeps its heading"
                );
            }
            *seen.entry(format!("face {faced}")).or_default() += 1;
        }

        // State 1, entered or given: an idle pager takes the line.
        if state == 1 || opened {
            let idle = [1, 4, 7].contains(&pager);
            assert_eq!(after.state, if idle { 2 } else { 1 }, "{at}");
            if idle {
                assert_eq!(after.pager, pager + 1, "{at}");
                assert_eq!(after.feed, SCRIPT + after.pc as u32, "{at}: the line fed");
                assert_eq!(after.latch, 1, "{at}: the yielded latch");
                // The port's next panel: the held `0x24` box goes on as the
                // same window, anything else opens fresh.
                let body = Arc::new(vec![0x1F, 0x41, 0, 0x24, 0x1F, 0x42, 0, 0x25, 0]);
                let mut first = OwnedDialogPanel::at_segment(body.clone(), 0);
                for _ in 0..400 {
                    first.tick();
                }
                let held = HeldBox::after_press(&first, Some(if pager == 4 { 0x24 } else { 0x48 }));
                assert_eq!(held.pager_state() == Some(pager), pager == 4, "{at}");
                let next = HeldBox::next_line_panel(Some(held), &body, 4);
                assert_eq!(
                    next.window_row_leads().contains(&0),
                    pager == 4,
                    "{at}: rows kept"
                );
            } else {
                assert_eq!(
                    (after.pager, after.feed),
                    (pager, 0),
                    "{at}: a busy pager waits"
                );
            }
            *seen.entry(format!("1 idle {idle}")).or_default() += 1;
        }

        // A teardown: the heading restore and the release.
        if torn {
            let restored = after.heading == saved;
            let mut w = World::new();
            w.install_field_player(0);
            w.npcs.headings.insert(3, 0x123);
            if flags & 0x2_0000 != 0 {
                // Only a placement's talk saves a heading.
                w.npcs.facing_save = Some((3, saved as i16));
            }
            w.field_vm
                .channels
                .push(legaia_engine_field::field_channels::FieldChannel {
                    placement_index: 3,
                    ctx: legaia_engine_vm::field::FieldCtx {
                        flags,
                        ..Default::default()
                    },
                    record_offset: 0,
                    pc: 0,
                    done: false,
                    object_bind: false,
                });
            w.release_talk_facing();
            if flags & 0x0102_0000 != 0 && flags & 0x100_0000 == 0 {
                assert_eq!(
                    restored,
                    w.npcs.headings[&3] == saved as i16,
                    "{at}: the restore"
                );
            } else if flags & 0x0102_0000 == 0 {
                assert!(
                    !restored && w.npcs.headings[&3] == 0x123,
                    "{at}: no save, no restore"
                );
            }
            *seen.entry(format!("restore {restored}")).or_default() += 1;

            // The release: depth 0 lets the player go, and a raised latch
            // becomes the pad hold.
            let released = after.depth == 0;
            assert_eq!(after.depth, depth - share, "{at}");
            let mut w = World::new();
            w.dialog.script_engaged_prev = true;
            w.dialog.script_yield_latch = latch != 0;
            if !released {
                // Another context still counts in the depth.
                w.start_inline_dialogue(vec![0x41, 0x21]);
            }
            w.tick_script_release_hold();
            let port_hold = w.post_load_hold() == SCRIPT_RELEASE_HOLD_FRAMES;
            assert_eq!(after.hold == 0x10, port_hold, "{at}: the pad hold");
            assert_eq!(
                after.latch != 0,
                w.dialog.script_yield_latch,
                "{at}: the latch"
            );
            assert_eq!(
                cpu.mem.rd32(PLAYER + 0x10) & 0x8_0000 == 0,
                released,
                "{at}"
            );
            *seen
                .entry(format!("release {released} latch {latch}"))
                .or_default() += 1;
        }
    }
    for k in [
        "2 busy",
        "2 walk true",
        "2 walk false",
        "0 Text false",
        "0 FrameMark true",
        "0 Parked false",
        "0 Parked true",
        "face true",
        "face false",
        "1 idle true",
        "1 idle false",
        "restore true",
        "restore false",
        "release true latch 0",
        "release true latch 1",
        "release false latch 1",
    ] {
        assert!(seen.get(k).copied().unwrap_or(0) > 10, "{k}: {seen:?}");
    }
}
