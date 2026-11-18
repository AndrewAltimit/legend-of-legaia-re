//! Disc-gated: the scene selector's **entry-beat table**
//! (`PICKER_ENTRY_BEATS` / `PICKER_ENTRY_BEATS_HELD` in
//! `scene_host/scene_entry/entry_beats.rs`) is the disc's answer, and a
//! selector visit to each of its scenes plays the beat.
//!
//! The sweep walks every scene the selector lists (the `town` / `field`
//! categories of the scene-name table; the overworlds keep their own seat).
//! For each, it takes the inbound arrivals in `ArrivalCatalog::ranked` order
//! and, per arrival, three kinds of candidate (`BeatTrigger`): the gate-1
//! walk-on trigger on the arrival tile, the entry script's spawn behind the
//! arriving door's hand-off flags (the `0x5x` sets its record makes before
//! the `0x3F`), and a gate-1 trigger within [`NEAR_TILES`] of the arrival
//! tile. A trigger record that carries a `0x3F` is a door, not a beat. Each
//! candidate is staged as a fresh selector visit (the free-roam staging, then
//! `arm_picker_beat` with the hand-off flags and the record's missing C2
//! flags) and ticked with Cross tapped: the first candidate whose record
//! engages the player within [`DETECT_TICKS`], runs at least
//! [`MIN_BEAT_TICKS`] (and speaks, or runs [`SILENT_BEAT_TICKS`]: a silent
//! short record is a door or lift animation) and hands control back in the
//! scene is the row (or
//! leaves it, for a scene in `PICKER_BEATS_THAT_LEAVE`). Trigger candidates
//! passed over on the way are held with the reason the disc gives.
//!
//! "Engages" is `World::script_context_engages_player`: a walk-on trigger
//! installs a modal timeline, an entry-script `0x44` spawn a helper context
//! that holds the player, and both are the beat.
//!
//! `LEGAIA_PICKER_BEATS_WRITE=1` rewrites the generated block from the sweep;
//! without it the sweep fails when the block and the disc disagree. Rows
//! carry scene labels, tiles, sectors, record indices and flag numbers only.
//!
//! Skips when `LEGAIA_DISC_BIN` is unset - CI runs without disc data.

use std::path::PathBuf;
use std::sync::Arc;

use legaia_engine_mode::mode::SceneMode;
use legaia_engine_scene::scene::is_world_map_scene;
use legaia_engine_scene::scene_arrivals::{ArrivalCatalog, ArrivalClass};
use legaia_engine_scene_host::scene_host::{
    BeatTrigger, HeldReason, PICKER_BEATS_THAT_LEAVE, PICKER_ENTRY_BEATS, PICKER_ENTRY_BEATS_HELD,
    PickerEntryBeat, PickerSeat, SceneHost,
};
use legaia_engine_system::input::PadButton;
use legaia_scene_formats::field_disasm::{self, FlagKind, InsnInfo};
use legaia_scene_formats::man_section::ManFile;

fn disc_path() -> Option<PathBuf> {
    let p = PathBuf::from(std::env::var_os("LEGAIA_DISC_BIN")?);
    p.is_file().then_some(p)
}

/// Ticks within which the beat must engage the player (the first
/// post-arrival frame's stale-tile compare and the entry script's first pass
/// both start it on tick one).
const DETECT_TICKS: u32 = 4;
/// The shortest run that counts as a cutscene: a record that ends sooner is
/// a flag latch or a stand-and-press check, not a beat.
const MIN_BEAT_TICKS: u32 = 20;
/// A beat that says nothing must run at least this long: a silent record
/// shorter than this is a door or lift animation, not a story beat.
const SILENT_BEAT_TICKS: u32 = 300;
/// The longest a beat is followed, with Cross tapped to page its text.
const MAX_BEAT_TICKS: u32 = 9000;
/// How far (Manhattan, whole tiles) past the arrival tile a
/// [`BeatTrigger::NearArrival`] trigger may lie.
const NEAR_TILES: i16 = 2;

/// The selector's field scenes, in table order, deduplicated.
fn selector_scenes() -> Vec<String> {
    let db = legaia_gamedata::Database::load();
    let mut out: Vec<String> = Vec::new();
    for cat in ["town", "field"] {
        for s in db.scene_names().iter().filter(|s| s.category == cat) {
            if !is_world_map_scene(&s.id) && !out.contains(&s.id) {
                out.push(s.id.clone());
            }
        }
    }
    out
}

fn fresh(
    index: &Arc<legaia_engine_scene::scene::ProtIndex>,
    cat: &Arc<ArrivalCatalog>,
) -> SceneHost {
    let mut h = SceneHost::new(index.clone());
    h.set_arrival_catalog(cat.clone());
    h
}

/// What a staged selector visit did.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Outcome {
    /// Nothing engaged the player within [`DETECT_TICKS`].
    NoInstall,
    /// It engaged and let go after this many ticks, in the scene.
    Played(u32),
    /// It left for this scene after this many ticks.
    Left(String, u32),
    /// It started a battle.
    Battle,
    /// Still running at [`MAX_BEAT_TICKS`].
    Running,
}

fn scene_name(h: &SceneHost) -> String {
    h.scene.as_ref().map(|s| s.name.clone()).unwrap_or_default()
}

/// The partition-2 record whose body a running context executes.
fn running_record(h: &SceneHost, mf: &ManFile, man: &[u8]) -> Option<u8> {
    let w = &h.world;
    let body = w
        .cutscene
        .timeline
        .as_ref()
        .filter(|t| !t.is_done())
        .or_else(|| w.field_vm.helper_contexts.iter().find(|c| c.holds_player()))?
        .bytecode
        .clone();
    (0..=255u8).find(|&i| {
        legaia_engine_field::man_field_scripts::partition_record_span(mf, man, 2, i as usize)
            .is_some_and(|(start, _, len)| man.get(start..start + len) == Some(&body[..]))
    })
}

/// Tick a just-entered visit and classify what its first frames start; the
/// record that engaged, when one did.
fn follow(h: &mut SceneHost, scene: &str, mf: &ManFile, man: &[u8]) -> (Outcome, Option<u8>) {
    let mut started = None;
    let mut record = None;
    for t in 0..MAX_BEAT_TICKS {
        let pad = if started.is_some() && t % 2 == 0 {
            PadButton::Cross.mask()
        } else {
            0
        };
        h.world.set_pad(pad);
        if h.tick().is_err() {
            return (Outcome::NoInstall, record);
        }
        if h.world.mode == SceneMode::Battle {
            return (Outcome::Battle, record);
        }
        let now = scene_name(h);
        if !now.eq_ignore_ascii_case(scene) {
            let ran = started.map_or(0, |t0| t - t0);
            return (Outcome::Left(now, ran), record);
        }
        let active = h.world.script_context_engages_player();
        match started {
            None if active => {
                started = Some(t);
                record = running_record(h, mf, man);
            }
            None if t + 1 >= DETECT_TICKS => return (Outcome::NoInstall, record),
            Some(t0) if !active => return (Outcome::Played(t - t0), record),
            _ => {}
        }
    }
    (Outcome::Running, record)
}

/// One candidate beat.
#[derive(Debug, Clone)]
struct Candidate {
    from: String,
    tile: (u8, u8),
    step: (i8, i8),
    dir: u8,
    trigger: BeatTrigger,
    /// The trigger record (`None` for an entry-script candidate).
    record: Option<u8>,
    c1: Vec<u16>,
    c2: Vec<u16>,
    /// The arriving door's hand-off flags.
    handoff: Vec<u16>,
    class: ArrivalClass,
}

/// One scene's sweep result.
struct SceneResult {
    scene: String,
    old_seat: String,
    old_beat: bool,
    /// The row: the candidate, the record it ran, its flags, its run.
    row: Option<(Candidate, u8, Vec<u16>, String)>,
    held: Vec<(Candidate, String, String)>,
}

fn is_portal(mf: &ManFile, man: &[u8], record: usize) -> bool {
    let Some((start, pc0, len)) =
        legaia_engine_field::man_field_scripts::partition_record_span(mf, man, 2, record)
    else {
        return false;
    };
    let body = &man[start..start + len];
    let mut pc = pc0;
    while pc < body.len() {
        let Ok(insn) = field_disasm::decode(body, pc) else {
            return false;
        };
        if insn.size == 0 {
            return false;
        }
        if matches!(insn.info, InsnInfo::SceneChange { .. }) {
            return true;
        }
        pc += insn.size;
    }
    false
}

/// A scene's decompressed MAN (the bundle's, else the streamed form's).
fn scene_man(index: &legaia_engine_scene::scene::ProtIndex, name: &str) -> Option<Vec<u8>> {
    use legaia_engine_scene::scene::Scene;
    let scene = Scene::load_through_bundle(index, name).ok()?;
    match scene.field_man_payload(index).ok()? {
        Some(m) => Some(m),
        None => Scene::load(index, name)
            .ok()?
            .field_man_payload(index)
            .ok()
            .flatten(),
    }
}

/// The **hand-off flags** of the `0x3F` at `op_pc` in `from`'s MAN: the
/// system-flag sets (`0x5x`) its record makes on the straight walk from the
/// record's first op to the scene change, in order, deduplicated. Retail
/// arrives with them up; the destination's entry script reads them.
fn handoff_flags(
    index: &legaia_engine_scene::scene::ProtIndex,
    from: &str,
    op_pc: usize,
) -> Vec<u16> {
    let Some(man) = scene_man(index, from) else {
        return Vec::new();
    };
    let Ok(mf) = legaia_scene_formats::man_section::parse(&man) else {
        return Vec::new();
    };
    for part in 0..3 {
        for i in 0..256 {
            let Some((start, pc0, len)) =
                legaia_engine_field::man_field_scripts::partition_record_span(&mf, &man, part, i)
            else {
                break;
            };
            if !(start + pc0 <= op_pc && op_pc < start + len) {
                continue;
            }
            let body = &man[start..start + len];
            let mut pc = pc0;
            let mut out: Vec<u16> = Vec::new();
            while start + pc < op_pc && pc < body.len() {
                let Ok(insn) = field_disasm::decode(body, pc) else {
                    break;
                };
                if insn.size == 0 {
                    break;
                }
                if let InsnInfo::SystemFlag {
                    kind: FlagKind::Set,
                    idx,
                    ..
                } = insn.info
                    && !out.contains(&idx)
                {
                    out.push(idx);
                }
                pc += insn.size;
            }
            return out;
        }
    }
    Vec::new()
}

fn describe_seat(seat: Option<&PickerSeat>) -> String {
    let tile = |(x, z): (i16, i16)| format!("({:#04x},{:#04x})", x >> 7, z >> 7);
    match seat {
        Some(PickerSeat::Arrival { arrival, seat, .. }) => {
            format!("{} door from {}", tile(*seat), arrival.from_scene)
        }
        Some(PickerSeat::Override { seat, .. }) => format!("{} reviewed seat", tile(*seat)),
        Some(PickerSeat::EntryBeat { seat, .. }) => format!("{} entry beat", tile(*seat)),
        Some(PickerSeat::ColdFallback { seat, .. }) => format!("{} cold seat", tile(*seat)),
        None => "none".to_string(),
    }
}

/// How many text segments partition-2 record `record` carries.
fn speaks(mf: &ManFile, man: &[u8], record: u8) -> usize {
    let Some((start, pc0, len)) =
        legaia_engine_field::man_field_scripts::partition_record_span(mf, man, 2, record as usize)
    else {
        return 0;
    };
    let body = &man[start..start + len];
    let mut pc = pc0;
    let mut n = 0;
    while pc < body.len() {
        let Ok(insn) = field_disasm::decode(body, pc) else {
            break;
        };
        if insn.size == 0 {
            break;
        }
        if matches!(insn.info, InsnInfo::TextSegment { .. }) {
            n += 1;
        }
        pc += insn.size;
    }
    n
}

/// Whether a record that held the player `ticks` ticks is a story beat: it
/// ran [`MIN_BEAT_TICKS`], and it speaks or ran [`SILENT_BEAT_TICKS`].
fn is_beat(ticks: u32, mf: &ManFile, man: &[u8], record: u8) -> bool {
    ticks >= MIN_BEAT_TICKS && (ticks >= SILENT_BEAT_TICKS || speaks(mf, man, record) > 0)
}

/// A tile that is a placeholder, not a place: the off-map hide box, the
/// map-centre stand-in a later script moves the party from, the origin.
fn placeholder(tile: (u8, u8)) -> bool {
    matches!(tile, (0x7F, 0x7F) | (0x40, 0x40) | (0, 0))
}

/// The flags a candidate raises: the hand-off flags, then the C2 flags the
/// staging and the hand-off leave clear.
fn seeds(c: &Candidate, staged: &[u16]) -> Vec<u16> {
    let mut out = c.handoff.clone();
    for &f in &c.c2 {
        if !staged.contains(&f) && !out.contains(&f) {
            out.push(f);
        }
    }
    out
}

fn sweep_scene(
    index: &Arc<legaia_engine_scene::scene::ProtIndex>,
    cat: &Arc<ArrivalCatalog>,
    scene: &str,
) -> Option<SceneResult> {
    // The default seat, and whether a beat plays from it.
    let mut h = fresh(index, cat);
    h.world.stage_picker_entry(scene, false);
    h.arm_picker_entry_seat(scene);
    if h.enter_field_scene(scene, 0).is_err() {
        return None;
    }
    let old_seat = describe_seat(h.picker_seat());
    let sc = h.scene.as_ref()?;
    let (p, f) = sc.field_tile_triggers(&h.index).ok()?;
    let man = sc.field_man_payload(&h.index).ok().flatten()?;
    let mf = legaia_scene_formats::man_section::parse(&man).ok()?;
    let staged: Vec<u16> = (0..0x800u16)
        .filter(|&fl| h.world.system_flag_test(fl))
        .collect();
    let old_beat = matches!(
        follow(&mut h, scene, &mf, &man),
        (Outcome::Played(n), Some(r)) if is_beat(n, &mf, &man, r)
    );

    let trigger_at = |tile: (u8, u8)| {
        legaia_engine_field::field_regions::lookup_tile_trigger(&p, &f, tile.0, tile.1)
            .filter(|t| t.gate == 1 && !is_portal(&mf, &man, t.record as usize))
    };
    let gates = |record: u8| {
        legaia_engine_field::man_field_scripts::partition2_record_gates(&mf, &man, record as usize)
    };

    let mut seen: Vec<((u8, u8), Vec<u16>)> = Vec::new();
    // Per trigger kind, in `BeatTrigger` order: every arrival's tile
    // trigger before any entry-script candidate, and those before any near
    // trigger.
    let mut on_tile: Vec<Candidate> = Vec::new();
    let mut scripted: Vec<Candidate> = Vec::new();
    let mut near_by: Vec<Candidate> = Vec::new();
    for a in cat.ranked(scene) {
        if a.class == ArrivalClass::OwnSeat {
            continue;
        }
        let tile = (a.entry_x & 0x7F, a.entry_z & 0x7F);
        let handoff = handoff_flags(index, &a.from_scene, a.op_pc);
        if seen.contains(&(tile, handoff.clone())) {
            continue;
        }
        seen.push((tile, handoff.clone()));
        let base = Candidate {
            from: a.from_scene.clone(),
            tile,
            step: (0, 0),
            dir: a.dir,
            trigger: BeatTrigger::ArrivalTile,
            record: None,
            c1: Vec::new(),
            c2: Vec::new(),
            handoff: handoff.clone(),
            class: a.class,
        };
        // The trigger on the arrival tile.
        if let Some(t) = trigger_at(tile)
            && let Some((c1, c2)) = gates(t.record)
        {
            on_tile.push(Candidate {
                record: Some(t.record),
                c1,
                c2,
                ..base.clone()
            });
        }
        // The entry script's spawn behind the hand-off flags.
        if !old_beat && handoff.iter().any(|f| !staged.contains(f)) {
            scripted.push(Candidate {
                trigger: BeatTrigger::EntryScript,
                ..base.clone()
            });
        }
        // A trigger on the party's first steps past the arrival tile.
        if !placeholder(tile) {
            let mut near: Vec<(i16, (u8, u8), u8)> = Vec::new();
            for t in p.iter().chain(f.iter()) {
                let (dx, dz) = (
                    i16::from(t.tile_x) - i16::from(tile.0),
                    i16::from(t.tile_z) - i16::from(tile.1),
                );
                let d = dx.abs() + dz.abs();
                if d == 0 || d > NEAR_TILES {
                    continue;
                }
                let Some(hit) = trigger_at((t.tile_x, t.tile_z)) else {
                    continue;
                };
                if hit != *t || near.iter().any(|n| n.2 == t.record) {
                    continue;
                }
                near.push((d, (t.tile_x, t.tile_z), t.record));
            }
            near.sort();
            for (_, at, record) in near {
                let Some((c1, c2)) = gates(record) else {
                    continue;
                };
                near_by.push(Candidate {
                    trigger: BeatTrigger::NearArrival,
                    step: (
                        (i16::from(at.0) - i16::from(tile.0)) as i8,
                        (i16::from(at.1) - i16::from(tile.1)) as i8,
                    ),
                    record: Some(record),
                    c1,
                    c2,
                    ..base.clone()
                });
            }
        }
    }

    let may_leave = PICKER_BEATS_THAT_LEAVE.contains(&scene);
    let mut held = Vec::new();
    let mut row = None;
    for c in on_tile.into_iter().chain(scripted).chain(near_by) {
        let flags = seeds(&c, &staged);
        let blocking: Vec<u16> =
            c.c1.iter()
                .copied()
                .filter(|fl| staged.contains(fl) || flags.contains(fl))
                .collect();
        let (reason, ran) = if !blocking.is_empty() {
            (
                Some(format!(
                    "HeldReason::BlockedByFlags(&{})",
                    hex_list(&blocking)
                )),
                None,
            )
        } else {
            let mut h = fresh(index, cat);
            h.world.stage_picker_entry(scene, false);
            let beat = PickerEntryBeat {
                scene: leak(scene),
                from: leak(&c.from),
                tile: c.tile,
                step: c.step,
                dir: c.dir,
                record: c.record.unwrap_or(0xFF),
                trigger: c.trigger,
                flags: Box::leak(flags.clone().into_boxed_slice()),
            };
            if !h.arm_picker_beat(&beat) || h.enter_field_scene(scene, 0).is_err() {
                (Some("HeldReason::DoesNotPlay".to_string()), None)
            } else {
                let (out, ran) = follow(&mut h, scene, &mf, &man);
                // A trigger candidate must run its own record.
                let ran = ran.filter(|r| c.record.is_none_or(|want| want == *r));
                match (out, ran) {
                    (Outcome::Played(n), Some(r)) if is_beat(n, &mf, &man, r) => (
                        None,
                        Some((
                            r,
                            format!("plays {n} ticks, {} lines", speaks(&mf, &man, r)),
                        )),
                    ),
                    (Outcome::Left(to, n), Some(r)) if may_leave && is_beat(n, &mf, &man, r) => (
                        None,
                        Some((
                            r,
                            format!(
                                "plays {n} ticks, {} lines, leaves for {to}",
                                speaks(&mf, &man, r)
                            ),
                        )),
                    ),
                    (Outcome::Running, _) => (Some("HeldReason::DoesNotEnd".to_string()), None),
                    (Outcome::Left(to, _), _) => {
                        (Some(format!("HeldReason::LeavesScene({to:?})")), None)
                    }
                    (Outcome::Battle, _) => (Some("HeldReason::StartsBattle".to_string()), None),
                    _ => (Some("HeldReason::DoesNotPlay".to_string()), None),
                }
            }
        };
        match (reason, ran) {
            (None, Some((record, run))) => {
                row = Some((c, record, flags, run));
                break;
            }
            (Some(r), _) => {
                // An entry-script or near candidate that started nothing is
                // no beat at all, not a held one.
                let quiet = c.trigger != BeatTrigger::ArrivalTile && r == "HeldReason::DoesNotPlay";
                if !quiet {
                    let gates = format!("C1 {} C2 {}", hex_list(&c.c1), hex_list(&c.c2));
                    held.push((c, r, gates));
                }
            }
            (None, None) => {}
        }
    }
    Some(SceneResult {
        scene: scene.to_string(),
        old_seat,
        old_beat,
        row,
        held,
    })
}

fn leak(s: &str) -> &'static str {
    Box::leak(s.to_string().into_boxed_str())
}

fn hex_list(v: &[u16]) -> String {
    let items: Vec<String> = v.iter().map(|f| format!("0x{f:03X}")).collect();
    format!("[{}]", items.join(", "))
}

/// The generated block for `results`, in the module's own syntax.
fn render(results: &[SceneResult]) -> String {
    let mut s = String::new();
    s.push_str("// BEGIN GENERATED by picker_entry_beats_disc (LEGAIA_PICKER_BEATS_WRITE=1)\n");
    s.push_str("/// Selector scenes opened on their story beat.\n");
    s.push_str("pub const PICKER_ENTRY_BEATS: &[PickerEntryBeat] = &[\n");
    for r in results {
        if let Some((c, record, flags, _)) = &r.row {
            s.push_str(&format!(
                "    PickerEntryBeat {{\n        scene: {:?},\n        from: {:?},\n        tile: (0x{:02X}, 0x{:02X}),\n        step: ({}, {}),\n        dir: {},\n        record: {},\n        trigger: BeatTrigger::{:?},\n        flags: &{},\n    }},\n",
                r.scene, c.from, c.tile.0, c.tile.1, c.step.0, c.step.1, c.dir, record, c.trigger, hex_list(flags)
            ));
        }
    }
    s.push_str("];\n\n");
    s.push_str("/// Beats the selector holds back.\n");
    s.push_str("pub const PICKER_ENTRY_BEATS_HELD: &[PickerEntryBeatHeld] = &[\n");
    for r in results {
        for (c, reason, _) in &r.held {
            s.push_str(&format!(
                "    PickerEntryBeatHeld {{\n        scene: {:?},\n        from: {:?},\n        tile: (0x{:02X}, 0x{:02X}),\n        trigger: BeatTrigger::{:?},\n        record: {},\n        reason: {},\n    }},\n",
                r.scene, c.from, c.tile.0, c.tile.1, c.trigger, c.record.unwrap_or(0xFF), reason
            ));
        }
    }
    s.push_str("];\n// END GENERATED\n");
    s
}

fn table_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/scene_host/scene_entry/entry_beats.rs")
}

#[test]
fn picker_entry_beat_table_matches_the_disc() {
    let Some(disc) = disc_path() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset or not a file");
        return;
    };
    let mut base = SceneHost::open_disc(&disc).expect("open disc host");
    let index = base.index.clone();
    let cat = base.arrival_catalog();
    let mut results = Vec::new();
    for scene in selector_scenes() {
        let Some(r) = sweep_scene(&index, &cat, &scene) else {
            eprintln!("[{scene}] does not enter - skipped");
            continue;
        };
        let new = match &r.row {
            Some((c, record, flags, run)) => format!(
                "{:?} ({:#04x},{:#04x})+{:?} dir {} from {} ({:?}) P2[{record}] C1 {} C2 {} flags {} {run}",
                c.trigger,
                c.tile.0,
                c.tile.1,
                c.step,
                c.dir,
                c.from,
                c.class,
                hex_list(&c.c1),
                hex_list(&c.c2),
                hex_list(flags),
            ),
            None => "unchanged".to_string(),
        };
        println!(
            "ROW\t{}\told: {}{}\tnew: {}",
            r.scene,
            r.old_seat,
            if r.old_beat {
                " (entry beat plays)"
            } else {
                ""
            },
            new
        );
        for (c, reason, gates) in &r.held {
            println!(
                "HELD\t{}\t{:?} ({:#04x},{:#04x})+{:?} from {} P2[{:?}] {gates} hand-off {}\t{reason}",
                r.scene,
                c.trigger,
                c.tile.0,
                c.tile.1,
                c.step,
                c.from,
                c.record,
                hex_list(&c.handoff)
            );
        }
        results.push(r);
    }
    let block = render(&results);
    let path = table_path();
    let src = std::fs::read_to_string(&path).expect("read entry_beats.rs");
    let begin = src
        .find("// BEGIN GENERATED")
        .expect("generated block start");
    let end =
        src.find("// END GENERATED\n").expect("generated block end") + "// END GENERATED\n".len();
    let current = &src[begin..end];
    if std::env::var_os("LEGAIA_PICKER_BEATS_WRITE").is_some() {
        let out = format!("{}{}{}", &src[..begin], block, &src[end..]);
        std::fs::write(&path, out).expect("write entry_beats.rs");
        println!("rewrote {}", path.display());
        return;
    }
    assert_eq!(
        current, block,
        "the entry-beat table disagrees with the disc; rerun with LEGAIA_PICKER_BEATS_WRITE=1"
    );
}

/// Every row, staged through the hosts' arm, engages its record within
/// [`DETECT_TICKS`]: a seated row on its trigger tile, every row with its
/// flags raised.
#[test]
fn a_selector_visit_plays_the_entry_beat() {
    let Some(disc) = disc_path() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset or not a file");
        return;
    };
    assert!(!PICKER_ENTRY_BEATS.is_empty(), "the table has rows");
    let mut base = SceneHost::open_disc(&disc).expect("open disc host");
    let index = base.index.clone();
    let cat = base.arrival_catalog();
    for beat in PICKER_ENTRY_BEATS {
        let mut h = fresh(&index, &cat);
        h.world.stage_picker_entry(beat.scene, false);
        assert!(
            h.arm_picker_story_entry(beat.scene),
            "[{}] arm refused",
            beat.scene
        );
        for &f in beat.flags {
            assert!(
                h.world.system_flag_test(f),
                "[{}] flag {f:#x} not raised",
                beat.scene
            );
        }
        h.enter_field_scene(beat.scene, 0).expect("enter");
        if beat.trigger != BeatTrigger::EntryScript {
            let want = (
                ((i16::from(beat.tile.0) + i16::from(beat.step.0)) << 7) + 0x40,
                ((i16::from(beat.tile.1) + i16::from(beat.step.1)) << 7) + 0x40,
            );
            match h.picker_seat() {
                Some(PickerSeat::EntryBeat { seat, record, .. }) => {
                    assert_eq!(*seat, want, "[{}] seat", beat.scene);
                    assert_eq!(*record, beat.record, "[{}] record", beat.scene);
                }
                other => panic!("[{}] not seated on its beat: {other:?}", beat.scene),
            }
        }
        let sc = h.scene.as_ref().expect("scene");
        let man = sc
            .field_man_payload(&h.index)
            .expect("man")
            .expect("man payload");
        let mf = legaia_scene_formats::man_section::parse(&man).expect("parse man");
        let mut fired = None;
        for t in 0..DETECT_TICKS {
            h.world.set_pad(0);
            h.tick().expect("tick");
            if h.world.script_context_engages_player() {
                fired = Some((t, running_record(&h, &mf, &man)));
                break;
            }
        }
        let Some((t, record)) = fired else {
            panic!(
                "[{}] P2[{}] did not start within {DETECT_TICKS} ticks",
                beat.scene, beat.record
            );
        };
        assert_eq!(
            record,
            Some(beat.record),
            "[{}] the record that runs",
            beat.scene
        );
        println!(
            "[{}] {:?} P2[{}] starts on tick {t}",
            beat.scene, beat.trigger, beat.record
        );
    }
}

/// Every held row names a scene the selector lists, and a scene with a row
/// is never also held at the row's own arrival and trigger.
#[test]
fn held_rows_are_consistent() {
    for h in PICKER_ENTRY_BEATS_HELD {
        assert!(
            !PICKER_ENTRY_BEATS.iter().any(|b| b.scene == h.scene
                && b.tile == h.tile
                && b.trigger == h.trigger
                && b.record == h.record),
            "{} held and seated at the same arrival",
            h.scene
        );
        if let HeldReason::BlockedByFlags(f) = h.reason {
            assert!(!f.is_empty(), "{}: empty flag list", h.scene);
        }
    }
    for b in PICKER_ENTRY_BEATS {
        if b.trigger != BeatTrigger::NearArrival {
            assert_eq!(
                b.step,
                (0, 0),
                "{}: only a near trigger is stepped",
                b.scene
            );
        }
    }
}
