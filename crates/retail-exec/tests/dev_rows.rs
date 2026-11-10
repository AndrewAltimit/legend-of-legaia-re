//! Four overlay rows the differential does not run, each settled by what the
//! disc image shows about it: who reaches the routine and what it calls.
//!
//! - `FUN_801D5C2C` (PROT 0972) has no reference anywhere on the disc.
//! - `FUN_801D553C` (PROT 0976) hands its text to a stub retail ships empty.
//! - `FUN_801D4FC8` (PROT 0976) is a template tick spawned by one cabinet arm.
//! - `FUN_801D2050` (PROT 0972) is the lead angler's template tick, a
//!   dispatcher over callees that run in their own differentials.

use crate::common::disc;
use legaia_minigame_formats::fishing_species as fs;

const BAKA_BASE: u32 = 0x801C_E818;

fn words(image: &[u8], base: u32) -> impl Iterator<Item = (u32, u32)> + '_ {
    image
        .as_chunks::<4>()
        .0
        .iter()
        .enumerate()
        .map(move |(i, w)| (base + 4 * i as u32, u32::from_le_bytes(*w)))
}

fn jal_target(w: u32) -> Option<u32> {
    (w >> 26 == 3 || w >> 26 == 2).then_some(((w & 0x03FF_FFFF) << 2) | 0x8000_0000)
}

/// Every `jal` / `j` target in `[from, to)`, in order.
fn calls_in(image: &[u8], base: u32, from: u32, to: u32) -> Vec<u32> {
    words(image, base)
        .filter(|&(va, _)| (from..to).contains(&va))
        .filter(|&(_, w)| w >> 26 == 3)
        .filter_map(|(_, w)| jal_target(w))
        .collect()
}

/// Whether anything in `image` names `target`: a literal word, a `jal` / `j`
/// to it, or a `lui` of its high half followed within four instructions by an
/// `addiu` / `ori` of its low half.
fn referenced(image: &[u8], base: u32, target: u32) -> Vec<u32> {
    let w: Vec<(u32, u32)> = words(image, base).collect();
    let hi = (target.wrapping_add(0x8000) >> 16) as u16;
    let lo = target as u16;
    let mut out = Vec::new();
    for (i, &(va, word)) in w.iter().enumerate() {
        if word == target || jal_target(word) == Some(target) {
            out.push(va);
        }
        if word >> 26 == 0x0F && word as u16 == hi {
            let rt = (word >> 16) & 0x1F;
            let pair = w[i + 1..w.len().min(i + 5)].iter().any(|&(_, n)| {
                matches!(n >> 26, 0x09 | 0x0D) && (n >> 21) & 0x1F == rt && n as u16 == lo
            });
            if pair {
                out.push(va);
            }
        }
    }
    out
}

#[test]
fn the_segment_clip_is_reached_by_nothing_on_the_disc() {
    not_a_differential!(
        "FUN_801D5C2C has no word, jump or lui pair naming it in any code image; retail never runs it"
    );
    let Some(d) = disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let overlay = d
        .read_entry(fs::FISHING_OVERLAY_PROT_INDEX)
        .expect("PROT 0972");
    let base = fs::FISHING_OVERLAY_BASE_VA;
    assert_eq!(
        referenced(&overlay, base, 0x801D_5C2C),
        Vec::<u32>::new(),
        "the fishing overlay names the clip"
    );
    let scus = crate::common::scus().expect("SCUS");
    assert_eq!(
        referenced(&scus[0x800..], 0x8001_0000, 0x801D_5C2C),
        Vec::<u32>::new(),
        "SCUS names the clip"
    );
}

#[test]
fn the_action_dump_hands_its_text_to_an_emptied_stub() {
    not_a_differential!(
        "FUN_801D553C formats the action tables into a malloc'd buffer and hands it to FUN_8003E7F0, a stub retail ships empty, then frees it: nothing in retail is changed; the port returns its own text"
    );
    let Some(d) = disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let overlay = d.read_entry(976).expect("PROT 0976");
    let calls = calls_in(&overlay, BAKA_BASE, 0x801D_553C, 0x801D_553C + 640);
    let mut kinds: Vec<u32> = calls.clone();
    kinds.sort_unstable();
    kinds.dedup();
    // DrawSync, malloc, strcpy, strlen, the formatter, the stub, free.
    assert_eq!(
        kinds,
        vec![
            0x8001_7888,
            0x8001_7B94,
            0x8003_E7F0,
            0x8005_6758,
            0x8005_6768,
            0x8005_67B8,
            0x8005_8104
        ],
        "{calls:x?}"
    );
    // The write: `addiu sp,-0x90; addiu sp,0x90; jr ra`.
    let scus = crate::common::scus().expect("SCUS");
    let o = (0x8003_E7F0u32 - 0x8001_0000 + 0x800) as usize;
    let body: Vec<u32> = (0..3)
        .map(|i| u32::from_le_bytes(scus[o + 4 * i..o + 4 * i + 4].try_into().unwrap()))
        .collect();
    assert_eq!(body, vec![0x27BD_FF70, 0x27BD_0090, 0x03E0_0008]);
    // Its one caller is the cabinet's pad-edge arm (`_DAT_8007B874 & 4`).
    assert_eq!(
        referenced(&overlay, BAKA_BASE, 0x801D_553C),
        vec![0x801D_1E90]
    );
}

#[test]
fn the_keyframe_editor_is_one_cabinet_arms_template_tick() {
    not_a_differential!(
        "FUN_801D4FC8 is the tick word of actor template 0x801D766C, which the cabinet spawns at 0x801D1D90 in its developer editor arm; the port keeps the editor's pad steps, and no shipped play enters that arm"
    );
    let Some(d) = disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let overlay = d.read_entry(976).expect("PROT 0976");
    assert_eq!(
        referenced(&overlay, BAKA_BASE, 0x801D_4FC8),
        vec![0x801D_7674],
        "the template's tick word is the editor's only reference"
    );
    // The template is named once, by the arm that spawns it.
    assert_eq!(
        referenced(&overlay, BAKA_BASE, 0x801D_766C),
        vec![0x801D_1D68]
    );
    assert_eq!(
        calls_in(&overlay, BAKA_BASE, 0x801D_1D68, 0x801D_1D98),
        vec![0x8002_0DE0]
    );
}

#[test]
fn the_lead_anglers_tick_dispatches_to_differential_run_callees() {
    not_a_differential!(
        "FUN_801D2050 is the lead angler's template tick: it seats the actor once, runs the wander FUN_801D2278 and the sky strip FUN_801D24EC (each with its own differential), the clip driver, the showcase spawn and a debug readout"
    );
    let Some(d) = disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let overlay = d
        .read_entry(fs::FISHING_OVERLAY_PROT_INDEX)
        .expect("PROT 0972");
    let base = fs::FISHING_OVERLAY_BASE_VA;
    assert_eq!(
        referenced(&overlay, base, 0x801D_2050),
        vec![0x801D_8FB4],
        "the lead's template tick word"
    );
    assert_eq!(
        calls_in(&overlay, base, 0x801D_2050, 0x801D_2278),
        vec![
            0x801D_2278,
            0x801D_6028,
            0x8002_04F8,
            0x8002_4C88,
            0x8001_A068,
            0x8001_A068,
            0x801D_24EC
        ]
    );
}
