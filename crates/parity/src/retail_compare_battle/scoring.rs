//! Scoring the battle channels: per-combatant scores and
//! [`compare_battle`].

use super::*;

/// Fraction of equal `(hp, hp_max, mp, mp_max)` fields over the retail
/// combatants; `mp_max` is left out where the engine has none (`0`), and so
/// is any field the manifest names as written by the capture probe after
/// battle init (`injected`, e.g. `p0.mp_max`).
pub(super) fn combatant_score(
    retail: &[Option<Combatant>],
    engine: &[Combatant],
    tag: &str,
    injected: &[String],
) -> (f64, String) {
    let mut total = 0usize;
    let mut skipped = Vec::new();
    let mut equal = 0usize;
    let mut diffs = Vec::new();
    for (i, r) in retail.iter().enumerate() {
        let Some(r) = r else { continue };
        let e = engine.get(i);
        let fields = [
            ("hp", r.hp, e.map(|e| e.hp)),
            ("hp_max", r.hp_max, e.map(|e| e.hp_max)),
            ("mp", r.mp, e.map(|e| e.mp)),
            ("mp_max", r.mp_max, e.map(|e| e.mp_max)),
        ];
        for (name, want, got) in fields {
            if name == "mp_max" && got == Some(0) {
                continue;
            }
            let key = format!("{tag}{i}.{name}");
            if injected.contains(&key) {
                skipped.push(format!("{key} retail={want} engine={got:?}"));
                continue;
            }
            total += 1;
            if got == Some(want) {
                equal += 1;
            } else {
                diffs.push(format!("{tag}{i}.{name} retail={want} engine={got:?}"));
            }
        }
    }
    let score = if total == 0 {
        1.0
    } else {
        equal as f64 / total as f64
    };
    let mut d = format!("{equal}/{total} fields");
    if !diffs.is_empty() {
        d.push_str("; ");
        d.push_str(&diffs.iter().take(4).cloned().collect::<Vec<_>>().join(", "));
    }
    if !skipped.is_empty() {
        d.push_str(&format!(
            "; not scored, written by the capture probe or a resident patch: {}",
            skipped.join(", ")
        ));
    }
    (score, d)
}

/// Score one seeded battle state.
pub fn compare_battle(
    retail: &RetailObs,
    battle: &RetailBattle,
    engine: &EngineBattle,
    injected: &[String],
) -> (BTreeMap<String, f64>, BTreeMap<String, String>) {
    use crate::retail_compare::{camera_score, flags_score, inventory_score, round3};
    let mut ch = BTreeMap::new();
    let mut det = BTreeMap::new();
    let mut put = |name: &str, score: f64, detail: String| {
        ch.insert(name.to_string(), round3(score));
        det.insert(name.to_string(), detail);
    };
    let scene_ok = engine.scene.as_deref() == Some(retail.scene.as_str());
    put(
        "scene",
        f64::from(u8::from(scene_ok)),
        format!("retail={} engine={:?}", retail.scene, engine.scene),
    );
    put(
        "mode",
        f64::from(u8::from(engine.mode == SceneMode::Battle)),
        format!("retail=0x{:02X} engine={:?}", retail.game_mode, engine.mode),
    );
    let n = battle
        .monster_ids
        .len()
        .max(engine.monster_ids.len())
        .max(1);
    let same = battle
        .monster_ids
        .iter()
        .zip(&engine.monster_ids)
        .filter(|(r, e)| **e == Some(u16::from(**r)))
        .count();
    put(
        "enemies",
        same as f64 / n as f64,
        format!(
            "retail={:02X?} engine={:?} ({})",
            battle.monster_ids, engine.monster_ids, engine.formation_source
        ),
    );
    let (s, d) = combatant_score(&battle.monsters, &engine.monsters, "m", injected);
    put("enemy_hp", s, d);
    let (s, mut d) = combatant_score(&battle.party, &engine.party, "p", injected);
    if battle.party.len() != engine.party.len() {
        d.push_str(&format!(
            "; retail seats {:?} vs engine party of {}",
            battle.seat_chars,
            engine.party.len()
        ));
    }
    put("battle_party", s, d);
    let want = BattleFlowState::from_raw(battle.flow);
    // A replayed cast or a driven action is scored on the action-SM state as
    // well (the flow byte reads `Idle` for every in-flight action alike), a
    // driven menu on the member it is open for.
    let plan = battle.seed_plan();
    let replayed = engine.inflight.is_some()
        || matches!(plan, SeedPlan::Action { .. }) && engine.driven.is_some();
    let seat_ok = match plan {
        SeedPlan::Menu { flow, seat } if engine.driven.is_some() => {
            !menu_seat_matters(flow) || engine.active_actor == seat
        }
        SeedPlan::Action { seat, .. } if engine.driven.is_some() => {
            engine.active_actor == engine_seat(seat, engine.party.len() as u8)
        }
        _ => true,
    };
    // Past the end signal the action SM is parked and the drive's own gate
    // ([`SpanGate`]: the sequencer phase on the pose actor) is the phase.
    let end_held = battle.span_gate.is_end() && matches!(engine.driven, Some(Some(_)));
    let phase_ok = end_held
        || engine.flow == want
            && (!replayed || engine.action_state == battle.action_state)
            && seat_ok;
    let inflight = match engine.inflight {
        None => String::new(),
        Some(Some(t)) => format!("; cast replayed, phase reached at +{t}"),
        Some(None) => "; cast replayed, phase never reached".to_string(),
    };
    let driven = match (plan, engine.driven) {
        (_, None) if plan == SeedPlan::Opening => {
            "; sampled at the entry-sweep counter".to_string()
        }
        (_, None) => String::new(),
        (_, Some(Some(t))) if battle.span_gate != SpanGate::None => {
            format!("; driven by pad, {:?} reached at +{t}", battle.span_gate)
        }
        (_, Some(Some(t))) => format!("; driven by pad, reached at +{t}"),
        (_, Some(None)) => "; driven by pad, never reached".to_string(),
    };
    put(
        "phase",
        f64::from(u8::from(phase_ok)),
        format!(
            "retail flow=0x{:02X} ({want:?}) action=0x{:02X} run=0x{:02X} seat={} cat={} queued=0x{:02X} clip=0x{:02X} target={}; engine {:?} action=0x{:02X} seat={} target={} (first prompt at +{:?}){inflight}{driven}",
            battle.flow,
            battle.action_state,
            battle.run_state,
            battle.active_actor,
            battle.queued_category,
            battle.queued_action,
            battle.caster_clip,
            battle.target_code,
            engine.flow,
            engine.action_state,
            engine.active_actor,
            engine.active_target,
            engine.prompt_tick
        ),
    );
    let (s, mut d) = camera_score(&retail.camera, &engine.camera);
    if let Some(t) = &engine.cam_trace {
        d.push_str(&format!("; [{t}]"));
    }
    put("camera", s, d);
    put(
        "bgm",
        f64::from(u8::from(engine.bgm_id == Some(retail.bgm_id))),
        format!(
            "retail word={} engine field track={:?} (engine battle track {:?}; pre-battle word {:?}, current {:?})",
            retail.bgm_id,
            engine.bgm_id,
            engine.battle_bgm,
            engine.field_word,
            engine.field_current
        ),
    );
    if let Some(rs) = &retail.save {
        let (s, d) = flags_score(&rs.ext.story_flag_bits, &engine.save.ext.story_flag_bits);
        put("flags", s, d);
        let (s, d) = inventory_score(rs, &engine.save);
        put("inventory", s, d);
    }
    (ch, det)
}
