//! Synthetic session-driver smoke commands (`battle`, `inventory`, `equip`, `title`, ...).
//!
//! Mechanical split from `commands.rs` (behavior-preserving).

use super::*;

/// Drive a synthetic [`BattleSession`] end-to-end. Reports per-frame
/// session events and the final phase. Intended as a smoke test for the
/// orchestrator wiring; engines that want a full UI use `play-window`
/// (which can host a `BattleSession` via the renderer's HUD draws).
pub(crate) fn cmd_battle(
    monsters: u8,
    monster_hp: u16,
    max_ticks: u64,
    script: &str,
) -> Result<()> {
    use legaia_art::Character;
    use legaia_engine_core::ap_gauge::ApGauge;
    use legaia_engine_core::battle_session::{
        BattlePhase, BattleSession, SessionInput, SessionSlotInfo,
    };
    use legaia_engine_core::battle_stats::StatRecord;
    use legaia_engine_core::world::{Actor, World};

    let mut session = BattleSession::new();
    session.set_party([Character::Vahn, Character::Noa, Character::Gala]);
    let names = ["Vahn", "Noa", "Gala"];
    for (i, name) in names.iter().enumerate() {
        session.set_slot_info(
            i as u8,
            SessionSlotInfo {
                name: (*name).into(),
                is_party: true,
                record: Some(StatRecord {
                    base_attack: 50,
                    base_udf: 30,
                    base_ldf: 25,
                    base_accuracy: 80,
                    base_evasion: 20,
                    ..Default::default()
                }),
                mp_max: 30,
            },
        );
    }
    let monster_count = monsters.min(5);
    for i in 0..monster_count {
        session.set_slot_info(
            3 + i,
            SessionSlotInfo {
                name: format!("Mon{i}"),
                is_party: false,
                record: Some(StatRecord {
                    base_attack: 30,
                    base_udf: 20,
                    base_ldf: 15,
                    base_accuracy: 70,
                    base_evasion: 10,
                    ..Default::default()
                }),
                mp_max: 0,
            },
        );
    }
    session.set_monster_count(monster_count);

    let mut world = World::new();
    while world.actors.len() < 8 {
        world.actors.push(Actor::default());
    }
    for i in 0..3 {
        world.actors[i].battle.hp = 100;
        world.actors[i].battle.max_hp = 100;
        world.actors[i].battle.mp = 30;
        world.ap_gauges[i] = ApGauge::with_base(8);
    }
    for i in 0..monster_count as usize {
        world.actors[3 + i].battle.hp = monster_hp;
        world.actors[3 + i].battle.max_hp = monster_hp;
    }

    session.begin_round(&mut world);
    println!(
        "battle: party=3 monsters={} phase={:?}",
        monster_count,
        session.phase()
    );

    let mut script_iter = script.chars();
    let mut total_events = 0usize;
    for tick in 0..max_ticks {
        let mut input = SessionInput::default();
        if let Some(c) = script_iter.next() {
            apply_script_char(c, &mut input);
        }
        let events = session.tick(&mut world, input);
        if !events.is_empty() {
            total_events += events.len();
            for ev in &events {
                println!("[t{tick}] {ev:?}");
            }
        }
        if session.is_done() {
            println!("battle ended at tick {tick}: {:?}", session.phase());
            break;
        }
        if matches!(session.phase(), BattlePhase::Idle) {
            break;
        }
    }
    println!(
        "battle: total_events={} final_phase={:?} hud_active_slots={}",
        total_events,
        session.phase(),
        session.hud.active_slots()
    );
    Ok(())
}

fn apply_script_char(c: char, input: &mut legaia_engine_core::battle_session::SessionInput) {
    use legaia_engine_core::battle_session::SessionInput as SI;
    let _: &SI = input;
    match c {
        'R' => input.right = true,
        'L' => input.left = true,
        'U' => input.up = true,
        'D' => input.down = true,
        'c' => input.cross = true,
        'o' => input.circle = true,
        't' => input.triangle = true,
        's' => input.square = true,
        'S' => input.start = true,
        _ => {}
    }
}

/// Drive a synthetic [`InventoryUseSession`] against a small world.
/// Reports cursor moves + the final outcome.
pub(crate) fn cmd_inventory(item: u8, party_size: u8, script: &str) -> Result<()> {
    use legaia_engine_core::inventory_use::{
        InventoryContext, InventoryUseInput, InventoryUseSession, TargetRow,
    };
    use legaia_engine_core::items::ItemCatalog;

    let catalog = ItemCatalog::vanilla();
    if catalog.get(item).is_none() {
        anyhow::bail!(
            "item id 0x{item:02X} not in vanilla catalog - pick from 0x10..0x41 or extend the catalog"
        );
    }
    let mut targets: Vec<TargetRow> = Vec::new();
    for i in 0..party_size {
        targets.push(TargetRow::new(i, format!("Slot{i}")).with_stats(50, 100, 10, 30));
    }

    let mut session =
        InventoryUseSession::new(catalog, vec![item], targets, InventoryContext::Field);
    println!("inventory: item=0x{item:02X} party_size={party_size}");
    for (idx, c) in script.chars().enumerate() {
        let input = match c {
            'U' => InventoryUseInput::Up,
            'D' => InventoryUseInput::Down,
            'c' => InventoryUseInput::Confirm,
            'o' => InventoryUseInput::Cancel,
            _ => continue,
        };
        session.input(input);
        let evs = session.drain_events();
        for ev in &evs {
            println!("[s{idx}={c}] {ev:?}");
        }
        if session.is_done() {
            break;
        }
    }
    println!("inventory: state={:?}", session.state);
    Ok(())
}

/// Run an equip session that confirms `item` into `slot`. Useful as a
/// smoke test for the SM and the BattleStats recompute path.
pub(crate) fn cmd_equip(slot: u8, item: u8) -> Result<()> {
    use legaia_engine_core::battle_stats::{
        EquipmentTable, ItemModifier, StatRecord, StatusModifiers,
    };
    use legaia_engine_core::equip_session::{EquipInput, EquipOutcome, EquipSession};
    use std::collections::HashMap;

    let record = StatRecord {
        base_attack: 50,
        base_udf: 30,
        base_ldf: 25,
        base_accuracy: 80,
        base_evasion: 20,
        base_spd: 35,
        base_int: 18,
        equip: [0; 8],
    };
    let mut inv = HashMap::new();
    // Re-encode the item id so its implied slot matches the requested
    // slot - the synthetic test catalog uses `id >> 5` as the slot bits.
    let encoded_id = (slot << 5) | (item & 0x1F);
    inv.insert(encoded_id, 1);
    let mut eq = EquipmentTable::new();
    eq.set(
        encoded_id,
        ItemModifier {
            atk: 10,
            ..Default::default()
        },
    );
    let mut session = EquipSession::new(record, inv, eq, StatusModifiers::default(), Vec::new());

    println!("equip: requested slot={slot} item=0x{item:02X} (encoded 0x{encoded_id:02X})");

    // Drive: down `slot` times to reach the slot, cross to enter picker,
    // cross to confirm item, cross to commit.
    let mut step_count = 0;
    for _ in 0..slot {
        session.input(EquipInput {
            down: true,
            ..Default::default()
        });
        step_count += 1;
    }
    session.input(EquipInput {
        cross: true,
        ..Default::default()
    });
    step_count += 1;
    session.input(EquipInput {
        cross: true,
        ..Default::default()
    });
    step_count += 1;
    session.input(EquipInput {
        cross: true,
        ..Default::default()
    });
    step_count += 1;

    println!(
        "equip: drove {step_count} inputs; outcome={:?}",
        session.outcome()
    );
    if let Some(EquipOutcome::Committed {
        added,
        slot: out_slot,
        removed,
    }) = session.outcome()
    {
        println!("equip: committed slot={out_slot} added=0x{added:02X} removed=0x{removed:02X}");
        println!(
            "equip: post-commit ATK={} (record.equip[{}]=0x{:02X})",
            session.preview_stats.atk,
            out_slot,
            session.record().equip[out_slot as usize]
        );
    }
    Ok(())
}

/// Load a JSON Cop2Trace and replay it through a fresh emulator. Reports
/// any per-step register divergence; exits 0 on clean replay.
pub(crate) fn cmd_gte_replay(trace_path: &Path, verbose: bool) -> Result<()> {
    use legaia_engine_render::gte_trace::Cop2Trace;
    let bytes = std::fs::read(trace_path)
        .with_context(|| format!("read trace file {}", trace_path.display()))?;
    let json = std::str::from_utf8(&bytes).context("trace file is not valid UTF-8")?;
    let trace = Cop2Trace::read_json(json).context("parse trace JSON")?;
    println!(
        "gte-replay: loaded {} steps (label={})",
        trace.len(),
        trace.label.as_deref().unwrap_or("<none>")
    );
    let mismatches = trace.replay();
    if mismatches.is_empty() {
        println!("gte-replay: clean - every step replayed bit-exact");
        if verbose {
            println!("gte-replay: trace label = {:?}", trace.label);
        }
        return Ok(());
    }
    eprintln!(
        "gte-replay: {} step(s) diverged from the recorded snapshot",
        mismatches.len()
    );
    for m in &mismatches {
        eprintln!("  step {} ({}):", m.step, m.op);
        for f in &m.fields {
            eprintln!(
                "    {} expected={} actual={}",
                f.field, f.expected, f.actual
            );
        }
    }
    anyhow::bail!("trace replay produced mismatches");
}

/// Map an input letter to a [`legaia_engine_core::title::TitleInput`] mask.
fn title_input_for(c: char) -> legaia_engine_core::title::TitleInput {
    use legaia_engine_core::title::TitleInput;
    let mut i = TitleInput::default();
    match c {
        's' => i.start = true,
        'c' => i.cross = true,
        'o' => i.circle = true,
        'U' => i.up = true,
        'D' => i.down = true,
        _ => {}
    }
    i
}

pub(crate) fn cmd_title(script: &str, no_save: bool, fade_frames: u16) -> Result<()> {
    use legaia_engine_core::title::{TitleEvent, TitleSession};
    let mut s = if no_save {
        TitleSession::without_save_data()
    } else {
        TitleSession::new()
    };
    s.fade_in_frames = fade_frames;
    s.skip_fade_in();
    println!("title: starting (no_save={no_save})");
    for (i, ch) in script.chars().enumerate() {
        if s.is_done() {
            break;
        }
        let evs = s.tick(title_input_for(ch));
        for e in evs {
            match e {
                TitleEvent::CursorMoved { row } => println!("  tick {i}: cursor → {row}"),
                TitleEvent::StartPressed => println!("  tick {i}: start pressed"),
                TitleEvent::MenuConfirmed { row } => println!("  tick {i}: confirmed row {row}"),
                TitleEvent::NewGameSelected => println!("  tick {i}: NewGame"),
                TitleEvent::ContinueSelected => println!("  tick {i}: Continue"),
                TitleEvent::OptionsSelected => println!("  tick {i}: Options"),
                TitleEvent::FadeInDone => println!("  tick {i}: fade-in done"),
            }
        }
    }
    println!("title: outcome = {:?}", s.outcome());
    Ok(())
}

fn select_input_for(c: char) -> legaia_engine_core::save_select::SelectInput {
    use legaia_engine_core::save_select::SelectInput;
    let mut i = SelectInput::default();
    match c {
        'c' => i.cross = true,
        'o' => i.circle = true,
        't' => i.triangle = true,
        'U' => i.up = true,
        'D' => i.down = true,
        'L' => i.left = true,
        'R' => i.right = true,
        _ => {}
    }
    i
}

pub(crate) fn cmd_save_select(mode: &str, slots: &str, script: &str) -> Result<()> {
    use legaia_engine_core::save_select::{
        SaveSelectMode, SaveSelectSession, SelectEvent, SlotContent, SlotSnapshot,
    };
    let mode = match mode.to_ascii_lowercase().as_str() {
        "load" => SaveSelectMode::Load,
        "save" => SaveSelectMode::Save,
        other => anyhow::bail!("unknown save-select mode: {other}"),
    };
    let snapshots: Vec<SlotSnapshot> = slots
        .split(',')
        .enumerate()
        .map(|(i, p)| {
            let present = p.trim() == "1";
            if present {
                SlotSnapshot {
                    slot: i as u8,
                    present: true,
                    content: SlotContent::LegaiaSave,
                    label: format!("Slot {i}: Vahn  Lv 5"),
                    play_time_seconds: 1234,
                    party_lv: 5,
                    location: "Town01".into(),
                    money: 100,
                    leader_char_id: 0,
                    leader_name: "Vahn".into(),
                    leader_hp: (100, 100),
                    leader_mp: (20, 20),
                }
            } else {
                SlotSnapshot::empty(i as u8)
            }
        })
        .collect();
    let mut s = SaveSelectSession::new(mode, snapshots);
    println!(
        "save-select: mode={:?}, {} slot(s)",
        s.mode(),
        s.slots().len()
    );
    for (i, ch) in script.chars().enumerate() {
        if s.is_done() {
            break;
        }
        let evs = s.tick(select_input_for(ch));
        for e in evs {
            match e {
                SelectEvent::CursorMoved { slot } => {
                    println!("  tick {i}: cursor → slot {slot}")
                }
                SelectEvent::EnteredConfirm { slot, kind } => {
                    println!("  tick {i}: entered {:?} confirm on slot {slot}", kind)
                }
                SelectEvent::Confirmed { slot, kind } => {
                    println!("  tick {i}: confirmed {:?} on slot {slot}", kind)
                }
                SelectEvent::ConfirmCancelled { slot, kind } => {
                    println!("  tick {i}: cancelled {:?} on slot {slot}", kind)
                }
                SelectEvent::InvalidConfirm => println!("  tick {i}: invalid confirm"),
                SelectEvent::EnteredNowChecking { slot } => {
                    println!("  tick {i}: entered NowChecking on slot {slot}")
                }
                SelectEvent::EnteredSlotPreview { slot } => {
                    println!("  tick {i}: entered SlotPreview on slot {slot}")
                }
                SelectEvent::LoadConfirmed { slot } => {
                    println!("  tick {i}: load confirmed on slot {slot}")
                }
                SelectEvent::SlotPreviewCancelled { slot } => {
                    println!("  tick {i}: slot preview cancelled on slot {slot}")
                }
                SelectEvent::Cancelled => println!("  tick {i}: cancelled"),
            }
        }
    }
    println!("save-select: outcome = {:?}", s.outcome());
    Ok(())
}

pub(crate) fn cmd_encounter(rate: u8, steps: u32, seed: u32) -> Result<()> {
    use legaia_engine_core::encounter::{
        EncounterEntry, EncounterSession, EncounterTable, EncounterTracker,
    };
    let mut table = EncounterTable::new("test_scene");
    table.set_trigger_rate(rate);
    table.push(EncounterEntry::new(1, 50));
    table.push(EncounterEntry::new(2, 30));
    table.push(EncounterEntry::new(3, 20));
    let mut session = EncounterSession::new(EncounterTracker::new(table));
    let mut rng = seed;
    let mut hit_step = None;
    for step in 0..steps {
        // xorshift32
        rng ^= rng << 13;
        rng ^= rng >> 17;
        rng ^= rng << 5;
        if session.on_step(rng) {
            hit_step = Some(step);
            break;
        }
    }
    if let Some(s) = hit_step {
        // Drain through transition.
        for _ in 0..session.transition_frames + 1 {
            session.tick_frame();
        }
        if let Some(roll) = session.drain_triggered() {
            println!(
                "encounter: triggered at step {s} → formation {} (roll q8={})",
                roll.formation_id, roll.roll_q8
            );
        } else {
            println!("encounter: triggered at step {s} but transition lost");
        }
    } else {
        println!("encounter: no trigger after {steps} step(s)");
    }
    println!(
        "encounter: total_steps={} steps_since_last={}",
        session.tracker().total_steps(),
        session.tracker().steps_since_last_battle()
    );
    Ok(())
}

fn picker_input_for(c: char) -> legaia_engine_core::target_picker::PickerInput {
    use legaia_engine_core::target_picker::PickerInput;
    let mut i = PickerInput::default();
    match c {
        'c' => i.cross = true,
        'o' => i.circle = true,
        'L' => i.left = true,
        'R' => i.right = true,
        'U' => i.up = true,
        'D' => i.down = true,
        _ => {}
    }
    i
}

pub(crate) fn cmd_target_pick(kind: &str, actor: u8, script: &str) -> Result<()> {
    use legaia_engine_core::target_picker::{
        PickerEvent, SlotState, TargetKind, TargetPickerSession,
    };
    let kind = match kind.to_ascii_lowercase().as_str() {
        "enemy" => TargetKind::SingleEnemy,
        "ally" => TargetKind::SingleAlly,
        "ally-or-self" => TargetKind::SingleAllyOrSelf,
        "dead-ally" => TargetKind::DeadAlly,
        "any-ally" => TargetKind::AnyAlly,
        "all-enemies" => TargetKind::AllEnemies,
        "all-allies" => TargetKind::AllAllies,
        "self" => TargetKind::Self_,
        other => anyhow::bail!("unknown target kind: {other}"),
    };
    let party = [SlotState::alive(true, true); 3];
    let monsters = [SlotState::alive(true, true); 5];
    let mut s = TargetPickerSession::new(kind, actor, party, monsters);
    println!("target-pick: kind={:?} actor={actor}", s.kind());
    for ch in script.chars() {
        if s.is_done() {
            break;
        }
        s.input(picker_input_for(ch));
        for e in s.drain_events() {
            match e {
                PickerEvent::CursorMoved { row, slot } => {
                    println!("  cursor → {:?} slot {slot}", row)
                }
                PickerEvent::RowSwitched { row, slot } => {
                    println!("  row switched → {:?} slot {slot}", row)
                }
                PickerEvent::Confirmed { row, slot } => {
                    println!("  confirmed {:?} slot {slot}", row)
                }
                PickerEvent::SweepConfirmed { row } => {
                    println!("  sweep confirmed {:?}", row)
                }
                PickerEvent::Cancelled => println!("  cancelled"),
                PickerEvent::InvalidConfirm => println!("  invalid confirm"),
            }
        }
    }
    println!("target-pick: outcome = {:?}", s.outcome());
    Ok(())
}

fn editor_input_for(c: char) -> legaia_engine_core::tactical_arts_editor::EditInput {
    use legaia_engine_core::tactical_arts_editor::EditInput;
    let mut i = EditInput::default();
    match c {
        'L' => i.left = true,
        'R' => i.right = true,
        'U' => i.up = true,
        'D' => i.down = true,
        'c' => i.cross = true,
        'o' => i.circle = true,
        't' => i.triangle = true,
        'n' => i.name_next = true,
        _ => {}
    }
    i
}

pub(crate) fn cmd_chain_editor(char_slot: u8, script: &str) -> Result<()> {
    use legaia_engine_core::tactical_arts_editor::{ChainEditor, ChainLibrary, EditEvent};
    let lib = ChainLibrary::new();
    let mut ed = ChainEditor::new(char_slot, &lib);
    println!("chain-editor: char_slot={char_slot}");
    for ch in script.chars() {
        if ed.is_done() {
            break;
        }
        for e in ed.tick(editor_input_for(ch)) {
            match e {
                EditEvent::BrowseCursorMoved { row } => println!("  cursor → row {row}"),
                EditEvent::EnteredEdit { editing_slot } => {
                    println!("  entered edit slot={:?}", editing_slot)
                }
                EditEvent::SequenceAppended { command, len } => {
                    println!("  appended {:?} (len={len})", command)
                }
                EditEvent::SequencePopped { len } => println!("  popped (len={len})"),
                EditEvent::InvalidCommit { len } => println!("  invalid commit at len {len}"),
                EditEvent::EnteredNaming => println!("  entered naming"),
                EditEvent::Saved { slot } => println!("  saved slot {slot}"),
                EditEvent::Replaced { slot } => println!("  replaced slot {slot}"),
                EditEvent::Deleted { slot } => println!("  deleted slot {slot}"),
                EditEvent::Cancelled => println!("  cancelled"),
            }
        }
    }
    println!("chain-editor: outcome = {:?}", ed.outcome());
    Ok(())
}

pub(crate) fn cmd_seru_capture(seru: u16, count: u32, party: &str) -> Result<()> {
    use legaia_engine_core::seru_learning::{SeruCaptureLog, SeruRegistry, record_capture};
    let registry = SeruRegistry::retail();
    let party: Vec<u8> = party
        .split(',')
        .filter_map(|s| s.trim().parse::<u8>().ok())
        .collect();
    let mut log = SeruCaptureLog::new();
    println!("seru-capture: seru={seru} count={count} party={:?}", party);
    for i in 0..count {
        let out = record_capture(&registry, &mut log, seru, &party);
        if !out.accepted {
            println!("  capture {i}: rejected (unknown seru)");
            return Ok(());
        }
        if !out.learns.is_empty() {
            for ev in &out.learns {
                println!(
                    "  capture {i}: char {} learned spell {:#04x} from seru {}",
                    ev.char_slot, ev.spell_id, ev.seru_id
                );
            }
        }
    }
    println!(
        "seru-capture: final per-char totals: {:?}",
        party
            .iter()
            .map(|c| (*c, log.total_points(*c)))
            .collect::<Vec<_>>()
    );
    for c in &party {
        println!("  char {c} learned spells: {:?}", log.learned_spells(*c));
    }
    Ok(())
}
