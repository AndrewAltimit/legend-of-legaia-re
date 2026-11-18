//! Disc-gated: the scene selector's **entry-beat table**
//! (`PICKER_ENTRY_BEATS` / `PICKER_ENTRY_BEATS_HELD` in
//! `scene_host/scene_entry/entry_beats.rs`) is the disc's answer, and a
//! selector visit to a sample of its scenes plays the beat.
//!
//! The sweep walks every scene the selector lists (the `town` / `field`
//! categories of the scene-name table; the overworlds keep their own seat).
//! For each, it takes the inbound arrivals in `ArrivalCatalog::ranked` order
//! and keeps those whose entry tile carries a gate-1 walk-on trigger on a
//! record that is not a door (no `0x3F`). Each candidate is seated as a fresh
//! selector visit (the free-roam staging, then `arm_picker_seat_rule` on the
//! arrival) and ticked with Cross tapped: the first candidate whose record
//! installs within [`DETECT_TICKS`], runs at least [`MIN_BEAT_TICKS`] and
//! hands control back in the scene is the row; the candidates ranked before
//! it, and every candidate of a scene with no row, are held with the reason
//! the disc gives. A beat behind a scripted arrival is held too: only an
//! overworld entrance or a walk-on door is a story entrance.
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
    HeldReason, PICKER_ENTRY_BEATS, PICKER_ENTRY_BEATS_HELD, PickerSeat, PickerSeatRule, SceneHost,
};
use legaia_engine_system::input::PadButton;

fn disc_path() -> Option<PathBuf> {
    let p = PathBuf::from(std::env::var_os("LEGAIA_DISC_BIN")?);
    p.is_file().then_some(p)
}

/// Ticks within which the arrival's trigger must install its record (the
/// first post-arrival frame's stale-tile compare fires it on tick one).
const DETECT_TICKS: u32 = 4;
/// The shortest run that counts as a cutscene: a record that ends sooner is
/// a flag latch or a stand-and-press check, not a beat.
const MIN_BEAT_TICKS: u32 = 20;
/// The longest a beat is followed, with Cross tapped to page its text.
const MAX_BEAT_TICKS: u32 = 9000;

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

/// What a seated selector visit did.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Outcome {
    /// No record installed within [`DETECT_TICKS`].
    NoInstall,
    /// It installed and ended after this many ticks, in the scene.
    Played(u32),
    /// It left for this scene.
    Left(String),
    /// It started a battle.
    Battle,
    /// Still running at [`MAX_BEAT_TICKS`].
    Running,
}

fn scene_name(h: &SceneHost) -> String {
    h.scene.as_ref().map(|s| s.name.clone()).unwrap_or_default()
}

/// Tick a just-entered visit and classify what its first frames start.
fn follow(h: &mut SceneHost, scene: &str) -> Outcome {
    let mut started = None;
    for t in 0..MAX_BEAT_TICKS {
        let pad = if started.is_some() && t % 2 == 0 {
            PadButton::Cross.mask()
        } else {
            0
        };
        h.world.set_pad(pad);
        if h.tick().is_err() {
            return Outcome::NoInstall;
        }
        if h.world.mode == SceneMode::Battle {
            return Outcome::Battle;
        }
        let now = scene_name(h);
        if !now.eq_ignore_ascii_case(scene) {
            return Outcome::Left(now);
        }
        let active = h.world.cutscene_timeline_active();
        match started {
            None if active => started = Some(t),
            None if t + 1 >= DETECT_TICKS => return Outcome::NoInstall,
            Some(t0) if !active => return Outcome::Played(t - t0),
            _ => {}
        }
    }
    Outcome::Running
}

/// One candidate arrival beat.
#[derive(Debug, Clone)]
struct Candidate {
    from: String,
    tile: (u8, u8),
    dir: u8,
    record: u8,
    c1: Vec<u16>,
    c2: Vec<u16>,
    class: ArrivalClass,
}

/// One scene's sweep result.
struct SceneResult {
    scene: String,
    old_seat: String,
    old_beat: bool,
    row: Option<(Candidate, u32)>,
    held: Vec<(Candidate, String, String)>,
}

fn is_portal(mf: &legaia_scene_formats::man_section::ManFile, man: &[u8], record: usize) -> bool {
    let Some((start, pc0, len)) =
        legaia_engine_field::man_field_scripts::partition_record_span(mf, man, 2, record)
    else {
        return false;
    };
    let body = &man[start..start + len];
    let mut pc = pc0;
    while pc < body.len() {
        let Ok(insn) = legaia_scene_formats::field_disasm::decode(body, pc) else {
            return false;
        };
        if insn.size == 0 {
            return false;
        }
        if matches!(
            insn.info,
            legaia_scene_formats::field_disasm::InsnInfo::SceneChange { .. }
        ) {
            return true;
        }
        pc += insn.size;
    }
    false
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

fn sweep_scene(
    index: &Arc<legaia_engine_scene::scene::ProtIndex>,
    cat: &Arc<ArrivalCatalog>,
    scene: &str,
) -> Option<SceneResult> {
    // The default seat, and whether an entry-script beat plays from it.
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
    let old_beat = matches!(follow(&mut h, scene), Outcome::Played(n) if n >= MIN_BEAT_TICKS);

    let mut seen: Vec<(u8, u8)> = Vec::new();
    let mut cands: Vec<Candidate> = Vec::new();
    for a in cat.ranked(scene) {
        if a.class == ArrivalClass::OwnSeat {
            continue;
        }
        let tile = (a.entry_x & 0x7F, a.entry_z & 0x7F);
        if seen.contains(&tile) {
            continue;
        }
        seen.push(tile);
        let Some(t) =
            legaia_engine_field::field_regions::lookup_tile_trigger(&p, &f, tile.0, tile.1)
                .filter(|t| t.gate == 1)
        else {
            continue;
        };
        if is_portal(&mf, &man, t.record as usize) {
            continue;
        }
        let Some((c1, c2)) = legaia_engine_field::man_field_scripts::partition2_record_gates(
            &mf,
            &man,
            t.record as usize,
        ) else {
            continue;
        };
        cands.push(Candidate {
            from: a.from_scene.clone(),
            tile,
            dir: a.dir,
            record: t.record,
            c1,
            c2,
            class: a.class,
        });
    }

    let mut held = Vec::new();
    let mut row = None;
    for c in cands {
        let missing: Vec<u16> =
            c.c2.iter()
                .copied()
                .filter(|fl| !staged.contains(fl))
                .collect();
        let blocking: Vec<u16> =
            c.c1.iter()
                .copied()
                .filter(|fl| staged.contains(fl))
                .collect();
        let reason = if !missing.is_empty() {
            Some(format!("HeldReason::NeedsFlags(&{})", hex_list(&missing)))
        } else if !blocking.is_empty() {
            Some(format!(
                "HeldReason::BlockedByFlags(&{})",
                hex_list(&blocking)
            ))
        } else {
            let mut h = fresh(index, cat);
            h.world.stage_picker_entry(scene, false);
            let rule = PickerSeatRule::Arrival {
                from: leak(&c.from),
                tile: c.tile,
                step: (0, 0),
            };
            if !h.arm_picker_seat_rule(scene, rule) || h.enter_field_scene(scene, 0).is_err() {
                Some("HeldReason::DoesNotPlay".to_string())
            } else {
                match follow(&mut h, scene) {
                    Outcome::Played(n) if n >= MIN_BEAT_TICKS => {
                        if c.class == ArrivalClass::Scripted {
                            Some("HeldReason::ScriptedArrival".to_string())
                        } else {
                            row = Some((c.clone(), n));
                            None
                        }
                    }
                    Outcome::Running => Some("HeldReason::DoesNotEnd".to_string()),
                    Outcome::Played(_) | Outcome::NoInstall => {
                        Some("HeldReason::DoesNotPlay".to_string())
                    }
                    Outcome::Left(to) => Some(format!("HeldReason::LeavesScene({to:?})")),
                    Outcome::Battle => Some("HeldReason::StartsBattle".to_string()),
                }
            }
        };
        match reason {
            Some(r) => {
                let gates = format!("C1 {} C2 {}", hex_list(&c.c1), hex_list(&c.c2));
                held.push((c, r, gates));
            }
            None => break,
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
    s.push_str("/// Selector scenes seated on their arrival beat.\n");
    s.push_str("pub const PICKER_ENTRY_BEATS: &[PickerEntryBeat] = &[\n");
    for r in results {
        if let Some((c, _)) = &r.row {
            s.push_str(&format!(
                "    PickerEntryBeat {{\n        scene: {:?},\n        from: {:?},\n        tile: (0x{:02X}, 0x{:02X}),\n        dir: {},\n        record: {},\n    }},\n",
                r.scene, c.from, c.tile.0, c.tile.1, c.dir, c.record
            ));
        }
    }
    s.push_str("];\n\n");
    s.push_str("/// Arrival beats the selector holds back.\n");
    s.push_str("pub const PICKER_ENTRY_BEATS_HELD: &[PickerEntryBeatHeld] = &[\n");
    for r in results {
        for (c, reason, _) in &r.held {
            s.push_str(&format!(
                "    PickerEntryBeatHeld {{\n        scene: {:?},\n        from: {:?},\n        tile: (0x{:02X}, 0x{:02X}),\n        record: {},\n        reason: {},\n    }},\n",
                r.scene, c.from, c.tile.0, c.tile.1, c.record, reason
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
            Some((c, n)) => format!(
                "({:#04x},{:#04x}) dir {} from {} ({:?}) P2[{}] C1 {} C2 {} plays {n} ticks",
                c.tile.0,
                c.tile.1,
                c.dir,
                c.from,
                c.class,
                c.record,
                hex_list(&c.c1),
                hex_list(&c.c2)
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
                "HELD\t{}\t({:#04x},{:#04x}) from {} P2[{}] {gates}\t{reason}",
                r.scene, c.tile.0, c.tile.1, c.from, c.record
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

/// A selector visit through the hosts' arm plays a sample of the table's
/// beats within [`DETECT_TICKS`], seated on the row's arrival.
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
    // Every few rows, plus the first and last.
    let step = (PICKER_ENTRY_BEATS.len() / 8).max(1);
    let mut sample: Vec<_> = PICKER_ENTRY_BEATS.iter().step_by(step).collect();
    if let Some(last) = PICKER_ENTRY_BEATS.last()
        && !sample.contains(&last)
    {
        sample.push(last);
    }
    for beat in sample {
        let mut h = fresh(&index, &cat);
        h.world.stage_picker_entry(beat.scene, false);
        assert!(
            h.arm_picker_story_entry(beat.scene),
            "[{}] arm refused",
            beat.scene
        );
        h.enter_field_scene(beat.scene, 0).expect("enter");
        let want = (
            (i16::from(beat.tile.0) << 7) + 0x40,
            (i16::from(beat.tile.1) << 7) + 0x40,
        );
        match h.picker_seat() {
            Some(PickerSeat::EntryBeat { seat, record, .. }) => {
                assert_eq!(*seat, want, "[{}] seat", beat.scene);
                assert_eq!(*record, beat.record, "[{}] record", beat.scene);
            }
            other => panic!("[{}] not seated on its beat: {other:?}", beat.scene),
        }
        let mut fired = None;
        for t in 0..DETECT_TICKS {
            h.world.set_pad(0);
            h.tick().expect("tick");
            if h.world.cutscene_timeline_active() {
                fired = Some(t);
                break;
            }
        }
        assert!(
            fired.is_some(),
            "[{}] P2[{}] did not start within {DETECT_TICKS} ticks",
            beat.scene,
            beat.record
        );
        println!(
            "[{}] P2[{}] starts on tick {:?}",
            beat.scene, beat.record, fired
        );
    }
}

/// Every held row names a scene the selector lists, and a scene with a row
/// is never also held at the row's own arrival.
#[test]
fn held_rows_are_consistent() {
    for h in PICKER_ENTRY_BEATS_HELD {
        assert!(
            !PICKER_ENTRY_BEATS
                .iter()
                .any(|b| b.scene == h.scene && b.tile == h.tile),
            "{} held and seated at the same arrival",
            h.scene
        );
        if let HeldReason::NeedsFlags(f) | HeldReason::BlockedByFlags(f) = h.reason {
            assert!(!f.is_empty(), "{}: empty flag list", h.scene);
        }
    }
}
