//! Disc-gated sweep: every monster's special attacks animate.
//!
//! One synthetic fight per (monster, castable spell): a sturdy three-member
//! party against a lone copy of the monster with its real record and real
//! action clips, the monster's next turn seeded to cast the spell
//! (`BattleState::forced_monster_cast`). The oracle per cast: while the
//! monster is the acting actor, the clips it commits include at least one
//! non-idle, non-walk entry whose keyframes actually move, and the player
//! advances through it.
//!
//! Skip-passes without `LEGAIA_DISC_BIN` or the extracted archive.

use std::path::PathBuf;

use legaia_asset::monster_archive::{self, MonsterAnimation};
use legaia_engine_core::monster_catalog::{
    FormationDef, FormationSlot, FormationTable, catalog_from_monster_archive,
};
use legaia_engine_core::world::{Actor, CASTER_STAGE_TICK_LIMIT, SceneMode, World};

fn archive() -> Option<Vec<u8>> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return None;
    }
    let mut dirs = Vec::new();
    if let Some(d) = std::env::var_os("LEGAIA_EXTRACTED_DIR") {
        dirs.push(PathBuf::from(d).join("PROT"));
    }
    dirs.push(PathBuf::from("../../extracted/PROT"));
    dirs.push(PathBuf::from("extracted/PROT"));
    for d in dirs {
        if let Ok(bytes) = std::fs::read(d.join("0867_battle_data.BIN")) {
            return Some(bytes);
        }
    }
    eprintln!("[skip] extracted PROT 0867 missing");
    None
}

/// How long the capture band may hold before the sweep reads it as the
/// stage guard rather than a choreography. PROT 0954's body is longer than
/// the guard even with the wheel stopped at once: its arms count down
/// `0x40 + 0x80 + 0x40 + 0x80` ticks before the spin, the deceleration
/// alone drains `scalar << 9` at `2 * scalar` a tick, and the landing,
/// banner and outcome arms add `0x20 + 0x40 + 0x80` more.
fn band_limit(t: &CastTrace) -> u32 {
    if t.owns_band {
        3 * u32::from(CASTER_STAGE_TICK_LIMIT)
    } else {
        u32::from(CASTER_STAGE_TICK_LIMIT)
    }
}

/// A clip whose keyframes move: some part differs from frame 0.
fn clip_moves(c: &MonsterAnimation) -> bool {
    c.frames.len() > 1 && c.frames.iter().skip(1).any(|f| f != &c.frames[0])
}

/// What one cast did on the caster's seat.
#[derive(Debug, Default)]
struct CastTrace {
    /// The clip byte the cast staged (`+0x1E0`), as armed.
    staged_clip: Option<u8>,
    /// Every anim id committed on the seat while it was acting, in order.
    committed: Vec<u8>,
    /// The highest frame the player reached on a moving special clip.
    special_frames: i16,
    /// Whether the monster acted at all.
    acted: bool,
    /// The action states the cast walked, deduplicated.
    states: Vec<u8>,
    /// Ticks the monster spent acting.
    ticks: u32,
    /// The longest unbroken stretch in the capture band's module hold
    /// (`0x70`), and the stretch in progress.
    band_ticks: u32,
    band_run: u32,
    /// The cast runs a capture body ported whole and paced by its own
    /// countdowns (PROT 0954's roulette), whose band is the choreography
    /// rather than the stage guard.
    owns_band: bool,
    /// The cast runs a capture-class body that stages no caster clip in
    /// retail either (`capture_body_idles_caster`).
    idles_in_retail: bool,
    /// The cast's capture-class body has a damage site whose power the
    /// engine seeds the fold with (`capture_site_power`).
    deals_damage: bool,
    /// Party HP lost while the monster acted.
    party_hp_lost: u32,
    /// The cast's capture-class body walks its caster into reach first
    /// (`capture_body_approaches`).
    approaches: bool,
    /// Farthest the caster got from where it stood when it began acting.
    moved: i32,
    /// The body walks its caster in and stages nothing else on it.
    walk_only: bool,
}

/// Every `(monster id, spell id)` the scripted per-monster AI switch can
/// queue (`legaia_engine_core::monster_ai::decide`, the port of
/// `FUN_801E9FD4`'s switch): the boss phases and the scripted casts that do
/// not come out of a record's magic slots. Transcribed from the switch's
/// `cast_*` literals; the generic slot casts are the archive's own.
const SCRIPTED_AI_CASTS: &[(u16, u8)] = &[
    (0x04, 0x51),
    (0x05, 0x51),
    (0x06, 0x51),
    (0x06, 0x52),
    (0x07, 0x50),
    (0x08, 0x50),
    (0x09, 0x50),
    (0x09, 0x6F),
    (0x0F, 0x53),
    (0x39, 0x71),
    (0x43, 0x51),
    (0x44, 0x51),
    (0x45, 0x51),
    (0x47, 0x52),
    (0x48, 0x52),
    (0x4B, 0x55),
    (0x4B, 0x56),
    (0x4D, 0xB9),
    (0x54, 0x60),
    (0x55, 0x60),
    (0x59, 0x60),
    (0x59, 0x73),
    (0x5A, 0x60),
    (0x5A, 0x73),
    (0x5B, 0x60),
    (0x5B, 0x73),
    (0x62, 0x5A),
    (0x63, 0x5A),
    (0x64, 0x5A),
    (0x68, 0x52),
    (0x69, 0x52),
    (0x6A, 0x52),
    (0x6B, 0x72),
    (0x6C, 0x72),
    (0x6D, 0x72),
    (0x6F, 0x60),
    (0x70, 0x60),
    (0x89, 0xBA),
    (0x8A, 0x4E),
    (0x8B, 0x5D),
    (0x8B, 0x5E),
    (0x92, 0x51),
    (0x93, 0x60),
    (0x94, 0x60),
    (0x95, 0x60),
    (0x99, 0x75),
    (0x9A, 0x75),
    (0x9B, 0x75),
    (0x9C, 0x76),
    (0x9D, 0x76),
    (0x9E, 0x76),
    (0x9F, 0x77),
    (0xA0, 0x77),
    (0xA1, 0x77),
    (0xA6, 0xA6),
    (0xA7, 0xB5),
    (0xA8, 0xAF),
    (0xA9, 0xAE),
    (0xAA, 0xAE),
    (0xAD, 0xB9),
    (0xAE, 0xB9),
    (0xB3, 0xB3),
    (0xB4, 0xAC),
    (0xB4, 0xAD),
    (0xB4, 0xB7),
    (0xB5, 0xA5),
    (0xB5, 0xB6),
    (0xB6, 0xA1),
];

fn scus() -> Option<Vec<u8>> {
    use legaia_engine_core::Vfs;
    let path = PathBuf::from(std::env::var_os("LEGAIA_DISC_BIN")?);
    legaia_engine_core::DiscVfs::open(&path)
        .ok()?
        .read("SCUS_942.54")
        .ok()
}

fn setup(
    entry: &[u8],
    scus: &[u8],
    id: u16,
) -> Option<(World, usize, Vec<Option<MonsterAnimation>>)> {
    let cat = catalog_from_monster_archive(entry, &[id]);
    cat.get(id)?;
    let clips = monster_archive::animations_by_entry(entry, id)
        .ok()
        .flatten()?;
    if !clips.iter().any(Option::is_some) {
        return None;
    }
    let mut w = World::new();
    w.set_spell_catalog(legaia_engine_core::retail_magic::seru_magic_catalog_from_scus(scus)?);
    w.install_menu_text(scus);
    while w.actors.len() < 8 {
        w.actors.push(Actor::default());
    }
    w.party.party_count = 3;
    w.load_party(legaia_save::Party::zeroed(3));
    let mut party = w.party.roster.clone();
    for rec in party.members.iter_mut() {
        let mut hms = rec.hp_mp_sp();
        hms.hp_cur = 9999;
        hms.hp_max = 9999;
        rec.set_hp_mp_sp(hms);
    }
    w.load_party(party);
    for i in 0..3 {
        w.actors[i].active = true;
        w.actors[i].battle.hp = 9999;
        w.actors[i].battle.max_hp = 9999;
        w.actors[i].battle.liveness = 1;
        w.set_battle_attack(i as u8, 1);
        w.set_battle_defense(i as u8, 20);
    }
    let mut table = FormationTable::new();
    table.insert(FormationDef::new(1, vec![FormationSlot::new(id)]));
    w.set_formation_table(table, cat);
    w.mode = SceneMode::Field;
    if !w.trigger_scripted_battle(1) {
        return None;
    }
    for _ in 0..300 {
        if w.mode == SceneMode::Battle {
            break;
        }
        w.tick();
    }
    if w.mode != SceneMode::Battle {
        return None;
    }
    let arc = std::sync::Arc::new(clips.clone());
    let mut seat = None;
    for slot in 0..w.actors.len() {
        if w.actors[slot].battle_monster_id == Some(id) {
            w.set_actor_battle_action_clips(slot, arc.clone());
            seat.get_or_insert(slot);
        }
    }
    Some((w, seat?, clips))
}

fn cast(
    entry: &[u8],
    scus: &[u8],
    id: u16,
    spell: u8,
) -> Option<(CastTrace, Vec<Option<MonsterAnimation>>)> {
    let (mut w, seat, clips) = setup(entry, scus, id)?;
    w.battle.forced_monster_cast = Some((seat as u8, spell));
    let mut t = CastTrace {
        idles_in_retail: w.cast_module_for(spell).is_some_and(|entry| {
            legaia_engine_vm::cast_module_ticks::capture_body_idles_caster(entry, spell)
        }),
        ..CastTrace::default()
    };
    let mut last = w.actors[seat].battle.current_anim;
    // A synthetic fight skips the battle intro, so the monster can already be
    // mid-action (armed before its clips were installed) when the sweep takes
    // over; that action is not the seeded one. Only an action that *starts*
    // under observation counts.
    t.approaches = w.cast_module_for(spell).is_some_and(|entry| {
        legaia_engine_vm::cast_module_ticks::capture_body_approaches(entry, spell)
    });
    t.walk_only = t.approaches
        && w.cast_module_for(spell).is_some_and(|entry| {
            legaia_engine_vm::cast_module_ticks::capture_caster_stages(entry, spell, 0, 0).is_none()
        });
    t.owns_band = w.cast_module_for(spell)
        == Some(legaia_engine_vm::cast_fatal_decision::FATAL_DECISION_ENTRY);
    let mut start = (0i32, 0i32);
    t.deals_damage = w.cast_module_for(spell).is_some_and(|entry| {
        legaia_engine_vm::cast_module_ticks::capture_site_power(entry, spell).is_some()
    });
    let party_hp = |w: &World| -> u32 { (0..3).map(|i| u32::from(w.actors[i].battle.hp)).sum() };
    let mut hp_at_start = party_hp(&w);
    let mut acting = usize::from(w.battle_ctx.active_actor) == seat;
    for _ in 0..8000 {
        // The player's half of the band: PROT 0954's wheel spins until the
        // confirm button stops it, so the sweep taps it through `0x70` (no
        // other module reads the pad there).
        let tap = w.battle_ctx.action_state == 0x70 && t.ticks.is_multiple_of(2);
        w.set_pad(if tap {
            legaia_engine_core::input::PadButton::Cross.mask()
        } else {
            0
        });
        w.tick();
        if w.mode != SceneMode::Battle {
            break;
        }
        let a = &w.actors[seat];
        let now_acting =
            usize::from(w.battle_ctx.active_actor) == seat && w.battle_ctx.queued_action == 2;
        if now_acting && !acting {
            hp_at_start = party_hp(&w);
            start = (
                i32::from(a.move_state.world_x),
                i32::from(a.move_state.world_z),
            );
            t.acted = true;
            t.staged_clip = Some(a.battle.params[1]);
        }
        if !now_acting && t.acted {
            t.party_hp_lost = hp_at_start.saturating_sub(party_hp(&w));
            break;
        }
        acting = now_acting;
        if !acting {
            continue;
        }
        t.ticks += 1;
        let d = (i32::from(a.move_state.world_x) - start.0).abs()
            + (i32::from(a.move_state.world_z) - start.1).abs();
        t.moved = t.moved.max(d);
        if w.battle_ctx.action_state == 0x70 {
            t.band_run += 1;
            t.band_ticks = t.band_ticks.max(t.band_run);
        } else {
            t.band_run = 0;
        }
        if t.states.last() != Some(&w.battle_ctx.action_state) {
            t.states.push(w.battle_ctx.action_state);
        }
        let cur = a.battle.current_anim;
        if cur != last {
            t.committed.push(cur);
            last = cur;
        }
        let special = clips
            .get(usize::from(cur))
            .and_then(|c| c.as_ref())
            .is_some_and(|c| cur != 0 && c.action_id != 1 && clip_moves(c));
        if special && let Some(p) = a.battle_animation.as_ref() {
            t.special_frames = t.special_frames.max(p.current_frame());
        }
    }
    Some((t, clips))
}

#[test]
fn every_monster_special_attack_animates() {
    let Some(entry) = archive() else {
        return;
    };
    let Some(scus) = scus() else {
        eprintln!("[skip] SCUS_942.54 unreadable");
        return;
    };
    let n = monster_archive::slot_count(&entry) as u16;
    let (mut total, mut animated, mut retail_idle) = (0usize, 0usize, Vec::new());
    let mut damaged = 0usize;
    let (mut approached, mut walked) = (0usize, 0usize);
    let mut broken = Vec::new();
    for id in 1..=n {
        if !matches!(monster_archive::record(&entry, id), Ok(Some(_))) {
            continue;
        }
        let cat = catalog_from_monster_archive(&entry, &[id]);
        let Some(def) = cat.get(id) else { continue };
        let mut spells = def.magic_attacks.clone();
        for &(m, sp) in SCRIPTED_AI_CASTS {
            if m == id && !spells.contains(&sp) {
                spells.push(sp);
            }
        }
        for spell in spells {
            // A scripted cast's target is the switch's class, which the
            // forced-cast seam does not replay (it resolves the spell
            // record's shape, as the generic picker does), so its hit is not
            // asserted - Genocidal Cannon's record reads ally-side while the
            // switch aims it at the party.
            let scripted = !def.magic_attacks.contains(&spell);
            let r = std::panic::catch_unwind(|| cast(&entry, &scus, id, spell));
            total += 1;
            let line = match r {
                Err(_) => Some("panicked".to_string()),
                Ok(None) => Some("no fight".to_string()),
                Ok(Some((t, clips))) => {
                    let tags: Vec<u8> = clips
                        .iter()
                        .map(|c| c.as_ref().map_or(0xEE, |c| c.action_id))
                        .collect();
                    if t.deals_damage && t.party_hp_lost > 0 {
                        damaged += 1;
                    }
                    if t.approaches && t.states.contains(&0x6E) {
                        approached += 1;
                        if t.moved > 0 {
                            walked += 1;
                        }
                    }
                    if !t.acted {
                        Some(format!("never cast; tags {tags:02x?}"))
                    } else if t.band_ticks >= band_limit(&t) {
                        Some(format!(
                            "the capture band held {} ticks (the stage guard, not a choreography)",
                            t.band_ticks
                        ))
                    } else if t.deals_damage && !scripted && t.party_hp_lost == 0 {
                        Some("a damaging capture special dealt nothing".to_string())
                    } else if t.special_frames > 0 {
                        animated += 1;
                        None
                    } else if t.walk_only {
                        // A body whose only caster stage is its walk (Steal):
                        // it moves when it starts out of reach, and holds
                        // its idle like retail when it does not.
                        animated += usize::from(t.moved > 0);
                        None
                    } else if t.idles_in_retail && t.states.contains(&0x6E) {
                        retail_idle.push(format!("{id:#04x}/{spell:#04x}"));
                        None
                    } else {
                        Some(format!(
                            "no special motion: staged {:?} committed {:02x?} states {:02x?} \
                             ({} ticks) tags {tags:02x?}",
                            t.staged_clip, t.committed, t.states, t.ticks
                        ))
                    }
                }
            };
            if let Some(l) = line {
                broken.push(format!("monster {id:#04x} spell {spell:#04x}: {l}"));
            }
        }
    }
    eprintln!(
        "[ran] [monster-special-anim-sweep] {total} (monster, spell) casts: {animated} animate, \
         {} hold idle as retail does ({}), {} without special motion; \
         {damaged} damaging capture specials landed their hit; \
         {walked} of {approached} melee capture specials walked into reach",
        retail_idle.len(),
        retail_idle.join(" "),
        broken.len()
    );
    for b in &broken {
        eprintln!("  {b}");
    }
    // A caster seated inside its reach does not move, so not every melee
    // cast walks; that none of them does is the regression.
    assert!(
        approached == 0 || walked > 0,
        "no melee capture special walked into reach"
    );
    assert!(
        broken.is_empty(),
        "{} casts without special motion:\n{}",
        broken.len(),
        broken.join("\n")
    );
}
