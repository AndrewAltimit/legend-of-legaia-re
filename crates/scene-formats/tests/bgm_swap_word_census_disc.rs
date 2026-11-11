//! Can a field script change the BGM track word while a swap is reading?
//!
//! The swap poller's stage 6 latches the requested index of its own call
//! (`0x800247B4..0x800247C4`), so a second op-`0x35` sub-op `1` that changes
//! the word after stage 2's read plays the old score and reports the new one
//! loaded. Measured on retail the read runs from about `+6` to `+66` vsyncs
//! after the op (`docs/subsystems/audio.md`, "The swap takes a settle and a
//! read"). Sub-op `9` re-runs its own PC while a swap is in flight
//! (`0x801E0238`), so only a second sub-op `1` reaches the window.
//!
//! This walks every MAN script record from every sub-op `1` site along every
//! control-flow path (both arms of each conditional, the picker's table)
//! and adds up the explicit waits (`0x4A` `WAIT_FRAMES` targets and one per
//! yield), counting a frame as one vsync, the shortest it can be. Ops that
//! block on other state only lengthen a path, so the sum is a lower bound
//! and a pair that cannot reach the window here cannot reach it on retail.
//! A pair with no wait between them runs in one VM frame: the second store
//! lands before the poller's next call, and stage 2 reads it. Only a pair at
//! least one frame apart reaches the window. The walk does not see two
//! records each starting a track, which run as separate contexts.
//!
//! Skips and passes without `extracted/`.

use std::collections::{BTreeSet, VecDeque};
use std::path::PathBuf;

use legaia_scene_formats::field_disasm::{InsnInfo, InventoryCmpKind, decode, man_script_spans};

/// The poller's attach, past which a new word is a swap of its own.
const READ_WINDOW_VSYNCS: u32 = 66;
/// The park sentinel: not a track.
const PARK_WORD: u16 = 0x1000;

fn prot_dir() -> Option<PathBuf> {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let p = manifest.parent()?.parent()?.join("extracted").join("PROT");
    p.is_dir().then_some(p)
}

fn scene_man(buf: &[u8]) -> Option<Vec<u8>> {
    let table = legaia_scene_formats::scene_asset_table::detect(buf)?;
    let d = table.descriptors.iter().find(|d| d.type_byte == 0x03)?;
    let start = d.data_offset as usize;
    let (decoded, _) = psx_lzs::decompress_tracked(buf.get(start..)?, d.size as usize).ok()?;
    (decoded.len() == d.size as usize).then_some(decoded)
}

/// Where control can go after `info` at `next`, and the frames it waits.
fn successors(info: &InsnInfo, next: usize) -> (Vec<usize>, u32) {
    match info {
        InsnInfo::JmpRel { target, .. } => (vec![*target], 0),
        InsnInfo::CondJmp { target, .. } => (vec![next, *target], 0),
        InsnInfo::BBoxTest { skip_target, .. } => (vec![next, *skip_target], 0),
        InsnInfo::SystemFlag {
            target: Some(t), ..
        } => (vec![next, *t], 0),
        InsnInfo::InventoryCmp {
            kind:
                InventoryCmpKind::Compare { skip_target, .. }
                | InventoryCmpKind::PartyBank { skip_target, .. },
            ..
        } => (vec![next, *skip_target], 0),
        InsnInfo::Picker { count, targets, .. } => {
            let mut v = vec![next];
            v.extend(targets.iter().take(usize::from(*count)).copied());
            (v, 0)
        }
        InsnInfo::WaitFrames { target } => (vec![next], u32::from(*target)),
        InsnInfo::Yield { .. } => (vec![next], 1),
        _ => (vec![next], 0),
    }
}

/// Every sub-op `1` naming another word that some path from `from` reaches
/// with less than the read window waited, as `(pc, frames, word)`: the
/// shortest wait per site, and separately the shortest of at least a frame.
/// The search is over `(pc, frames waited)`, so a longer path to a site a
/// shorter one also reaches is still walked.
fn changes_within_window(
    body: &[u8],
    pc0: usize,
    from: usize,
    word: u16,
) -> (bool, Option<(usize, u32, u16)>) {
    let mut seen: BTreeSet<(usize, u32)> = BTreeSet::new();
    let mut queue = VecDeque::from([(from, 0u32)]);
    let (mut same_frame, mut later): (bool, Option<(usize, u32, u16)>) = (false, None);
    while let Some((pc, waited)) = queue.pop_front() {
        if waited >= READ_WINDOW_VSYNCS
            || pc < pc0
            || pc >= body.len()
            || !seen.insert((pc, waited))
        {
            continue;
        }
        let Ok(insn) = decode(body, pc) else {
            continue;
        };
        if let InsnInfo::Bgm { text_id, sub_op: 1 } = insn.info
            && pc != from
            && insn.extended.is_none()
            && text_id != word
            && text_id != PARK_WORD
        {
            if waited == 0 {
                same_frame = true;
            } else if later.is_none_or(|f| waited < f.1) {
                later = Some((pc, waited, text_id));
            }
            continue;
        }
        let (next, wait) = successors(&insn.info, pc + insn.size);
        for n in next {
            queue.push_back((n, waited + wait));
        }
    }
    (same_frame, later)
}

#[test]
fn no_script_changes_the_track_word_inside_a_swaps_read() {
    let Some(dir) = prot_dir() else {
        eprintln!("[skip] extracted/PROT missing");
        return;
    };
    let mut entries: Vec<PathBuf> = std::fs::read_dir(&dir)
        .expect("read extracted/PROT")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_file())
        .collect();
    entries.sort();
    let (mut sites, mut scenes) = (0usize, BTreeSet::new());
    let (mut reached, mut same_frame) = (Vec::new(), 0usize);
    for path in entries {
        let Ok(buf) = std::fs::read(&path) else {
            continue;
        };
        let Some(man) = scene_man(&buf) else {
            continue;
        };
        let Ok(mf) = legaia_scene_formats::man_section::parse(&man) else {
            continue;
        };
        let name = path.file_stem().unwrap().to_string_lossy().into_owned();
        for (part, record, start, pc0, len) in man_script_spans(&mf, &man) {
            let body = &man[start..start + len];
            let mut pc = pc0;
            while pc < body.len() {
                let Ok(insn) = decode(body, pc) else {
                    pc += 1;
                    continue;
                };
                if let InsnInfo::Bgm { text_id, sub_op: 1 } = insn.info
                    && insn.extended.is_none()
                    && text_id != PARK_WORD
                {
                    sites += 1;
                    scenes.insert(name.clone());
                    let (same, later) = changes_within_window(body, pc0, pc, text_id);
                    same_frame += usize::from(same);
                    if let Some((at, waited, word)) = later {
                        reached.push(format!(
                            "{name} P{part}[{record}] {pc:#06x} {text_id} -> {at:#06x} {word} after {waited} frames"
                        ));
                    }
                }
                pc += insn.size.max(1);
            }
        }
    }
    eprintln!(
        "[bgm-swap] {sites} sub-op 1 sites in {} scenes; {same_frame} reach another word in the same frame, {} a frame or more on and inside {READ_WINDOW_VSYNCS} vsyncs",
        scenes.len(),
        reached.len()
    );
    for r in &reached {
        eprintln!("  {r}");
    }
    assert!(
        sites > 100,
        "the walk found the corpus's track starts: {sites}"
    );
    assert!(reached.is_empty(), "{reached:#?}");
}

/// The search sees a pair a wait apart, and a pair with none, apart.
#[test]
fn the_search_finds_a_word_change_behind_a_wait() {
    // `35 E0 07 01` (2016), `4A 05 00` (wait 5), `35 DF 07 01` (2015).
    let waited = [
        0x35u8, 0xE0, 0x07, 0x01, 0x4A, 0x05, 0x00, 0x35, 0xDF, 0x07, 0x01,
    ];
    assert_eq!(
        changes_within_window(&waited, 0, 0, 2016),
        (false, Some((7, 5, 2015)))
    );
    let same = [0x35u8, 0xE0, 0x07, 0x01, 0x35, 0xDF, 0x07, 0x01];
    assert_eq!(changes_within_window(&same, 0, 0, 2016), (true, None));
}
