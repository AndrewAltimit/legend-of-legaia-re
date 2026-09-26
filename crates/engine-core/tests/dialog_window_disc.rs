//! The field dialog box scrolls, carries rows across page turns and completes
//! a page on confirm the way the retail pager does.
//!
//! Retail reference: two PCSX-Redux traces of `town01` placement `P1[16]`'s
//! conversation (`scripts/pcsx-redux/autorun_dialog_typewriter_trace.lua`
//! from `s4_rimelm_door_transition`, `DAT_1F800393 = 2`), one row per vsync
//! of the pager state `_DAT_801F2734`, reveal counter, short-row hold,
//! `_DAT_801F3534`, scroll word `_DAT_801F2738` and the row table
//! `_DAT_801F3540[]` as record offsets of each row's `0x1F` lead. The probe
//! turns each page 40 vsyncs after it waits; the second trace also taps
//! confirm while pages type and scroll (`LEGAIA_SKIP_PAGES=0:10,1:33,2:4,3:60`).
//! Only states, counts, offsets and timings are pinned - no disc text.
//!
//! The engine drives the same conversation through the path both hosts share
//! (`World::step_inline_dialogue`), pressing confirm on the same vsyncs, and
//! must reproduce every row of both traces from the first typing call to
//! the last page's wait.
//!
//! Disc-gated: skip-passes when `LEGAIA_DISC_BIN` is unset or `extracted/`
//! is absent.

use std::path::PathBuf;

use legaia_engine_core::input::PadButton;
use legaia_engine_core::scene::SceneHost;

fn extracted_dir() -> Option<PathBuf> {
    ["extracted", "../extracted", "../../extracted"]
        .iter()
        .map(PathBuf::from)
        .find(|d| d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists())
}

fn gate() -> Option<PathBuf> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    let d = extracted_dir();
    if d.is_none() {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
    }
    d
}

const SLOT: u8 = 16;
/// Four letters, as the captured save's lead name. Not disc text.
const NAME: &str = "Zedd";
/// The probe turns a page this many vsyncs after it starts waiting.
const PAGE_WAIT: usize = 40;

/// One vsync: `(state, counter, hold, rows_on_page, scroll, row leads)`.
type Row = (u8, i32, i32, i32, i32, Vec<usize>);

/// Drive the conversation the way the probe does, from the first typing call
/// until the last page's wait (the page whose turn ends the talk). `skips`
/// are `(page, delay)` confirm taps `delay` vsyncs into a page's typing.
fn engine_trace(extracted: &std::path::Path, skips: &[(usize, usize)]) -> Vec<Row> {
    let mut host = SceneHost::open_extracted(extracted).expect("open SceneHost");
    host.world.toggles.use_vm_dialogue = true;
    host.enter_field_scene("town01", 0).expect("enter town01");
    host.world.party.party_names = vec![NAME.to_string()];
    host.world.trigger_field_interact(0, SLOT);

    let mut out = Vec::new();
    let mut started = false;
    let mut pad_next = 0u16;
    let (mut page, mut page_type_v, mut prev_st) = (0usize, None::<usize>, None::<u8>);
    let mut page_end_v = None::<usize>;
    let mut release_v = None::<usize>;
    let mut skip_done = vec![false; 16];
    for _ in 0..3000 {
        host.world.set_pad(pad_next);
        host.tick().expect("tick");
        let Some(panel) = host
            .world
            .dialog
            .inline
            .as_ref()
            .and_then(|r| r.panel.as_ref())
        else {
            if started {
                break;
            }
            continue;
        };
        let Some(w) = panel.window() else { continue };
        let st = w.phase.retail_state();
        if !started {
            if st != 0x0B || w.pacer.counter != 1 {
                continue;
            }
            started = true;
        }
        let v = out.len();
        out.push((
            st,
            w.pacer.counter,
            w.pacer.hold,
            w.rows_on_page,
            w.scroll,
            panel.window_row_leads(),
        ));
        // The probe's press schedule, applied to the next vsync.
        if release_v.is_some_and(|r| v >= r) {
            pad_next = 0;
            release_v = None;
        }
        if prev_st == Some(0x19) && st != 0x19 {
            page += 1;
            page_type_v = None;
        }
        prev_st = Some(st);
        // The probe's trace loop starts one vsync after the first typing
        // call, so page 0's typing clock does too.
        if st == 0x0B && page_type_v.is_none() && v >= 1 {
            page_type_v = Some(v);
        }
        if let Some(&(_, delay)) = skips.iter().find(|(p, _)| *p == page)
            && let Some(t) = page_type_v
            && !skip_done[page]
            && release_v.is_none()
            && v - t == delay
        {
            pad_next = PadButton::Cross.mask();
            release_v = Some(v + 4);
            skip_done[page] = true;
        }
        if st == 0x19 && release_v.is_none() {
            let end = *page_end_v.get_or_insert(v);
            if v - end == PAGE_WAIT {
                if panel.is_done() {
                    break;
                }
                pad_next = PadButton::Cross.mask();
                release_v = Some(v + 4);
                page_end_v = None;
            }
        } else if st != 0x19 {
            page_end_v = None;
        }
    }
    out
}

/// Expand a run-length list `(count, state, counter, hold, rows_on_page,
/// scroll, row leads)` into per-vsync rows.
fn expand(runs: &[Run]) -> Vec<Row> {
    let mut v = Vec::new();
    for &(n, st, c, h, rop, sc, leads) in runs {
        for _ in 0..n {
            v.push((st, c, h, rop, sc, leads.to_vec()));
        }
    }
    v
}

fn dump(rows: &[Row]) {
    if std::env::var_os("LEGAIA_WINDOW_TRACE_DUMP").is_some() {
        for (i, r) in rows.iter().enumerate() {
            eprintln!("ROW,{i},{},{},{},{},{},{:?}", r.0, r.1, r.2, r.3, r.4, r.5);
        }
    }
}

/// A run of identical vsyncs: `(vsyncs, state, counter, hold, rows_on_page,
/// scroll, row leads as record offsets)`.
type Run = (usize, u8, i32, i32, i32, i32, &'static [usize]);

/// The plain trace, from the first typing call to the last page's turn.
#[rustfmt::skip]
const RETAIL_PLAIN: &[Run] = &[
    (2, 0x0B, 1, 0, 1, 0, &[76]),
    (2, 0x0B, 3, 0, 1, 0, &[76]),
    (2, 0x0B, 5, 0, 1, 0, &[76]),
    (2, 0x0B, 7, 0, 1, 0, &[76]),
    (2, 0x0B, 9, 0, 1, 0, &[76]),
    (2, 0x0B, 11, 0, 1, 0, &[76]),
    (2, 0x0B, 13, 0, 1, 0, &[76]),
    (2, 0x0B, 15, 0, 1, 0, &[76]),
    (2, 0x0B, 17, 0, 1, 0, &[76]),
    (2, 0x0B, 19, 0, 1, 0, &[76]),
    (2, 0x0B, 21, 0, 1, 0, &[76]),
    (2, 0x0B, 23, 0, 1, 0, &[76]),
    (2, 0x0B, 25, 0, 1, 0, &[76]),
    (2, 0x0B, 0, 36, 2, 0, &[76, 99]),
    (2, 0x0B, 0, 0, 2, 0, &[76, 99]),
    (2, 0x0B, 2, 0, 2, 0, &[76, 99]),
    (2, 0x0B, 4, 0, 2, 0, &[76, 99]),
    (2, 0x0B, 6, 0, 2, 0, &[76, 99]),
    (2, 0x0B, 8, 0, 2, 0, &[76, 99]),
    (2, 0x0B, 10, 0, 2, 0, &[76, 99]),
    (2, 0x0B, 12, 0, 2, 0, &[76, 99]),
    (2, 0x0B, 14, 0, 2, 0, &[76, 99]),
    (2, 0x0B, 16, 0, 2, 0, &[76, 99]),
    (2, 0x0B, 18, 0, 2, 0, &[76, 99]),
    (2, 0x0B, 20, 0, 2, 0, &[76, 99]),
    (2, 0x0B, 22, 0, 2, 0, &[76, 99]),
    (2, 0x0B, 24, 0, 2, 0, &[76, 99]),
    (2, 0x0B, 26, 0, 2, 0, &[76, 99]),
    (2, 0x0B, 28, 0, 2, 0, &[76, 99]),
    (2, 0x0B, 30, 0, 2, 0, &[76, 99]),
    (2, 0x19, 0, 16, 3, 0, &[76, 99]),
    (40, 0x19, 0, 0, 3, 0, &[76, 99]),
    (2, 0x05, 0, 0, 0, 0, &[76, 99]),
    (2, 0x0B, 1, 0, 1, 0, &[76, 99, 131]),
    (2, 0x0B, 3, 0, 1, 0, &[76, 99, 131]),
    (2, 0x0B, 5, 0, 1, 0, &[76, 99, 131]),
    (2, 0x0B, 7, 0, 1, 0, &[76, 99, 131]),
    (2, 0x0B, 9, 0, 1, 0, &[76, 99, 131]),
    (2, 0x0B, 11, 0, 1, 0, &[76, 99, 131]),
    (2, 0x0B, 13, 0, 1, 0, &[76, 99, 131]),
    (2, 0x0B, 15, 0, 1, 0, &[76, 99, 131]),
    (2, 0x0B, 17, 0, 1, 0, &[76, 99, 131]),
    (2, 0x0B, 19, 0, 1, 0, &[76, 99, 131]),
    (2, 0x0B, 21, 0, 1, 0, &[76, 99, 131]),
    (2, 0x0B, 23, 0, 1, 0, &[76, 99, 131]),
    (2, 0x0B, 25, 0, 1, 0, &[76, 99, 131]),
    (2, 0x0B, 27, 0, 1, 0, &[76, 99, 131]),
    (2, 0x0B, 29, 0, 1, 0, &[76, 99, 131]),
    (2, 0x0B, 31, 0, 1, 0, &[76, 99, 131]),
    (2, 0x0C, 0, 8, 1, 0, &[76, 99, 131]),
    (2, 0x0C, 0, 0, 1, 0, &[76, 99, 131]),
    (2, 0x0C, 0, 0, 1, -72, &[76, 99, 131]),
    (2, 0x0C, 0, 0, 1, -144, &[76, 99, 131]),
    (2, 0x0C, 0, 0, 1, -216, &[76, 99, 131]),
    (2, 0x0B, 0, 0, 2, 0, &[99, 131, 164]),
    (2, 0x0B, 2, 0, 2, 0, &[99, 131, 164]),
    (2, 0x0B, 4, 0, 2, 0, &[99, 131, 164]),
    (2, 0x0B, 6, 0, 2, 0, &[99, 131, 164]),
    (2, 0x0B, 8, 0, 2, 0, &[99, 131, 164]),
    (2, 0x0B, 10, 0, 2, 0, &[99, 131, 164]),
    (2, 0x0B, 12, 0, 2, 0, &[99, 131, 164]),
    (2, 0x0B, 14, 0, 2, 0, &[99, 131, 164]),
    (2, 0x0B, 16, 0, 2, 0, &[99, 131, 164]),
    (2, 0x0B, 18, 0, 2, 0, &[99, 131, 164]),
    (2, 0x0B, 20, 0, 2, 0, &[99, 131, 164]),
    (2, 0x0B, 22, 0, 2, 0, &[99, 131, 164]),
    (2, 0x0B, 24, 0, 2, 0, &[99, 131, 164]),
    (2, 0x0F, 0, 40, 2, 0, &[99, 131, 164]),
    (2, 0x0F, 0, 0, 2, 0, &[99, 131, 164]),
    (2, 0x0F, 0, 0, 2, -54, &[99, 131, 164]),
    (2, 0x0F, 0, 0, 2, -108, &[99, 131, 164]),
    (2, 0x0F, 0, 0, 2, -162, &[99, 131, 164]),
    (2, 0x0F, 0, 0, 2, -216, &[99, 131, 164]),
    (42, 0x19, 0, 0, 3, 0, &[131, 164]),
    (2, 0x05, 0, 0, 0, 0, &[131, 164]),
    (2, 0x0B, 1, 0, 1, 0, &[131, 164, 190]),
    (2, 0x0B, 3, 0, 1, 0, &[131, 164, 190]),
    (2, 0x0B, 5, 0, 1, 0, &[131, 164, 190]),
    (2, 0x0B, 7, 0, 1, 0, &[131, 164, 190]),
    (2, 0x0B, 9, 0, 1, 0, &[131, 164, 190]),
    (2, 0x0B, 11, 0, 1, 0, &[131, 164, 190]),
    (2, 0x0F, 0, 92, 1, 0, &[131, 164, 190]),
    (2, 0x0F, 0, 28, 1, 0, &[131, 164, 190]),
    (2, 0x0F, 0, 0, 1, 0, &[131, 164, 190]),
    (2, 0x0F, 0, 0, 1, -54, &[131, 164, 190]),
    (2, 0x0F, 0, 0, 1, -108, &[131, 164, 190]),
    (2, 0x0F, 0, 0, 1, -162, &[131, 164, 190]),
    (2, 0x0F, 0, 0, 1, -216, &[131, 164, 190]),
    (2, 0x0F, 0, 0, 2, -30, &[164, 190]),
    (2, 0x0F, 0, 0, 2, -84, &[164, 190]),
    (2, 0x0F, 0, 0, 2, -138, &[164, 190]),
    (2, 0x0F, 0, 0, 2, -192, &[164, 190]),
    (42, 0x19, 0, 0, 3, 0, &[190]),
    (2, 0x05, 0, 0, 0, 0, &[190]),
    (2, 0x0B, 1, 0, 1, 0, &[190, 203]),
    (2, 0x0B, 3, 0, 1, 0, &[190, 203]),
    (2, 0x0B, 5, 0, 1, 0, &[190, 203]),
    (2, 0x0B, 7, 0, 1, 0, &[190, 203]),
    (2, 0x0B, 9, 0, 1, 0, &[190, 203]),
    (2, 0x0B, 11, 0, 1, 0, &[190, 203]),
    (2, 0x0B, 13, 0, 1, 0, &[190, 203]),
    (2, 0x0B, 15, 0, 1, 0, &[190, 203]),
    (2, 0x0B, 0, 72, 2, 0, &[190, 203, 217]),
    (2, 0x0B, 0, 8, 2, 0, &[190, 203, 217]),
    (2, 0x0B, 0, 0, 2, 0, &[190, 203, 217]),
    (2, 0x0B, 2, 0, 2, 0, &[190, 203, 217]),
    (2, 0x0B, 4, 0, 2, 0, &[190, 203, 217]),
    (2, 0x0B, 6, 0, 2, 0, &[190, 203, 217]),
    (2, 0x0B, 8, 0, 2, 0, &[190, 203, 217]),
    (2, 0x0B, 10, 0, 2, 0, &[190, 203, 217]),
    (2, 0x0B, 12, 0, 2, 0, &[190, 203, 217]),
    (2, 0x0B, 14, 0, 2, 0, &[190, 203, 217]),
    (2, 0x0B, 16, 0, 2, 0, &[190, 203, 217]),
    (2, 0x0B, 18, 0, 2, 0, &[190, 203, 217]),
    (2, 0x0B, 20, 0, 2, 0, &[190, 203, 217]),
    (2, 0x0B, 22, 0, 2, 0, &[190, 203, 217]),
    (2, 0x0B, 24, 0, 2, 0, &[190, 203, 217]),
    (2, 0x0B, 26, 0, 2, 0, &[190, 203, 217]),
    (2, 0x0B, 28, 0, 2, 0, &[190, 203, 217]),
    (2, 0x0B, 30, 0, 2, 0, &[190, 203, 217]),
    (2, 0x0F, 0, 12, 2, 0, &[190, 203, 217]),
    (2, 0x0F, 0, 0, 2, 0, &[190, 203, 217]),
    (2, 0x0F, 0, 0, 2, -54, &[190, 203, 217]),
    (2, 0x0F, 0, 0, 2, -108, &[190, 203, 217]),
    (2, 0x0F, 0, 0, 2, -162, &[190, 203, 217]),
    (2, 0x0F, 0, 0, 2, -216, &[190, 203, 217]),
    (41, 0x19, 0, 0, 3, 0, &[203, 217]),
];

/// The confirm-tap trace (`0:10,1:33,2:4,3:60`), same span.
#[rustfmt::skip]
const RETAIL_TAPS: &[Run] = &[
    (2, 0x0B, 1, 0, 1, 0, &[76]),
    (2, 0x0B, 3, 0, 1, 0, &[76]),
    (2, 0x0B, 5, 0, 1, 0, &[76]),
    (2, 0x0B, 7, 0, 1, 0, &[76]),
    (2, 0x0B, 9, 0, 1, 0, &[76]),
    (2, 0x0B, 11, 0, 1, 0, &[76]),
    (2, 0x0B, 13, 0, 1, 0, &[76]),
    (2, 0x0D, 13, 0, 1, 0, &[76]),
    (2, 0x0D, 0, 0, 2, 0, &[76, 99]),
    (42, 0x19, 0, 0, 3, 0, &[76, 99]),
    (2, 0x05, 0, 0, 0, 0, &[76, 99]),
    (2, 0x0B, 1, 0, 1, 0, &[76, 99, 131]),
    (2, 0x0B, 3, 0, 1, 0, &[76, 99, 131]),
    (2, 0x0B, 5, 0, 1, 0, &[76, 99, 131]),
    (2, 0x0B, 7, 0, 1, 0, &[76, 99, 131]),
    (2, 0x0B, 9, 0, 1, 0, &[76, 99, 131]),
    (2, 0x0B, 11, 0, 1, 0, &[76, 99, 131]),
    (2, 0x0B, 13, 0, 1, 0, &[76, 99, 131]),
    (2, 0x0B, 15, 0, 1, 0, &[76, 99, 131]),
    (2, 0x0B, 17, 0, 1, 0, &[76, 99, 131]),
    (2, 0x0B, 19, 0, 1, 0, &[76, 99, 131]),
    (2, 0x0B, 21, 0, 1, 0, &[76, 99, 131]),
    (2, 0x0B, 23, 0, 1, 0, &[76, 99, 131]),
    (2, 0x0B, 25, 0, 1, 0, &[76, 99, 131]),
    (2, 0x0B, 27, 0, 1, 0, &[76, 99, 131]),
    (2, 0x0B, 29, 0, 1, 0, &[76, 99, 131]),
    (2, 0x0B, 31, 0, 1, 0, &[76, 99, 131]),
    (2, 0x0C, 0, 8, 1, 0, &[76, 99, 131]),
    (2, 0x0C, 0, 0, 1, 0, &[76, 99, 131]),
    (2, 0x0C, 0, 0, 1, -74, &[76, 99, 131]),
    (2, 0x0C, 0, 0, 1, -148, &[76, 99, 131]),
    (2, 0x0C, 0, 0, 1, -222, &[76, 99, 131]),
    (2, 0x0D, 0, 0, 2, 0, &[99, 131, 164]),
    (2, 0x0F, 0, 0, 2, 0, &[99, 131, 164]),
    (2, 0x0F, 0, 0, 2, -56, &[99, 131, 164]),
    (2, 0x0F, 0, 0, 2, -112, &[99, 131, 164]),
    (2, 0x0F, 0, 0, 2, -168, &[99, 131, 164]),
    (2, 0x0F, 0, 0, 2, -224, &[99, 131, 164]),
    (42, 0x19, 0, 0, 3, 0, &[131, 164]),
    (2, 0x05, 0, 0, 0, 0, &[131, 164]),
    (2, 0x0B, 1, 0, 1, 0, &[131, 164, 190]),
    (2, 0x0B, 3, 0, 1, 0, &[131, 164, 190]),
    (2, 0x0B, 5, 0, 1, 0, &[131, 164, 190]),
    (2, 0x0D, 5, 0, 1, 0, &[131, 164, 190]),
    (2, 0x0F, 0, 0, 1, 0, &[131, 164, 190]),
    (2, 0x0F, 0, 0, 1, -56, &[131, 164, 190]),
    (2, 0x0F, 0, 0, 1, -112, &[131, 164, 190]),
    (2, 0x0F, 0, 0, 1, -168, &[131, 164, 190]),
    (2, 0x0F, 0, 0, 1, -224, &[131, 164, 190]),
    (2, 0x0F, 0, 0, 2, -40, &[164, 190]),
    (2, 0x0F, 0, 0, 2, -96, &[164, 190]),
    (2, 0x0F, 0, 0, 2, -152, &[164, 190]),
    (2, 0x0F, 0, 0, 2, -208, &[164, 190]),
    (42, 0x19, 0, 0, 3, 0, &[190]),
    (2, 0x05, 0, 0, 0, 0, &[190]),
    (2, 0x0B, 1, 0, 1, 0, &[190, 203]),
    (2, 0x0B, 3, 0, 1, 0, &[190, 203]),
    (2, 0x0B, 5, 0, 1, 0, &[190, 203]),
    (2, 0x0B, 7, 0, 1, 0, &[190, 203]),
    (2, 0x0B, 9, 0, 1, 0, &[190, 203]),
    (2, 0x0B, 11, 0, 1, 0, &[190, 203]),
    (2, 0x0B, 13, 0, 1, 0, &[190, 203]),
    (2, 0x0B, 15, 0, 1, 0, &[190, 203]),
    (2, 0x0B, 0, 72, 2, 0, &[190, 203, 217]),
    (2, 0x0B, 0, 8, 2, 0, &[190, 203, 217]),
    (2, 0x0B, 0, 0, 2, 0, &[190, 203, 217]),
    (2, 0x0B, 2, 0, 2, 0, &[190, 203, 217]),
    (2, 0x0B, 4, 0, 2, 0, &[190, 203, 217]),
    (2, 0x0B, 6, 0, 2, 0, &[190, 203, 217]),
    (2, 0x0B, 8, 0, 2, 0, &[190, 203, 217]),
    (2, 0x0B, 10, 0, 2, 0, &[190, 203, 217]),
    (2, 0x0B, 12, 0, 2, 0, &[190, 203, 217]),
    (2, 0x0B, 14, 0, 2, 0, &[190, 203, 217]),
    (2, 0x0B, 16, 0, 2, 0, &[190, 203, 217]),
    (2, 0x0B, 18, 0, 2, 0, &[190, 203, 217]),
    (2, 0x0B, 20, 0, 2, 0, &[190, 203, 217]),
    (2, 0x0B, 22, 0, 2, 0, &[190, 203, 217]),
    (2, 0x0B, 24, 0, 2, 0, &[190, 203, 217]),
    (2, 0x0B, 26, 0, 2, 0, &[190, 203, 217]),
    (2, 0x0B, 28, 0, 2, 0, &[190, 203, 217]),
    (2, 0x0B, 30, 0, 2, 0, &[190, 203, 217]),
    (2, 0x0F, 0, 12, 2, 0, &[190, 203, 217]),
    (2, 0x0F, 0, 0, 2, 0, &[190, 203, 217]),
    (2, 0x0F, 0, 0, 2, -54, &[190, 203, 217]),
    (2, 0x0F, 0, 0, 2, -108, &[190, 203, 217]),
    (2, 0x0F, 0, 0, 2, -162, &[190, 203, 217]),
    (2, 0x0F, 0, 0, 2, -218, &[190, 203, 217]),
    (41, 0x19, 0, 0, 3, 0, &[203, 217]),
];

fn first_divergence(got: &[Row], want: &[Row]) -> String {
    let i = got
        .iter()
        .zip(want)
        .position(|(g, w)| g != w)
        .unwrap_or(got.len().min(want.len()));
    format!(
        "{} engine vsyncs vs {} retail; first divergence at vsync {i}: engine {:?} retail {:?}",
        got.len(),
        want.len(),
        got.get(i),
        want.get(i)
    )
}

#[test]
fn town01_npc16_pages_scroll_like_the_retail_pager() {
    let Some(extracted) = gate() else { return };
    let got = engine_trace(&extracted, &[]);
    dump(&got);
    let want = expand(RETAIL_PLAIN);
    assert!(got == want, "{}", first_divergence(&got, &want));
    eprintln!(
        "[ok] town01 P1[16]: {} vsyncs of pager state / counter / hold / rows / scroll / row table match retail",
        got.len()
    );
}

#[test]
fn town01_npc16_confirm_completes_pages_like_the_retail_pager() {
    let Some(extracted) = gate() else { return };
    let got = engine_trace(&extracted, &[(0, 10), (1, 33), (2, 4), (3, 60)]);
    dump(&got);
    let want = expand(RETAIL_TAPS);
    assert!(got == want, "{}", first_divergence(&got, &want));
    eprintln!(
        "[ok] town01 P1[16] with confirm taps: {} vsyncs match retail (skip latch, state 0x0D, faster scrolls)",
        got.len()
    );
}
