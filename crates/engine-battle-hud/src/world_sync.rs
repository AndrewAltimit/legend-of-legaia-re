//! Filling the HUD from a live `World`: slot rows, enemy target rows, intro names and the stage banner.

use super::*;

/// Numeric half of one battle-HUD row, staged while the world is still
/// borrowed and applied to the HUD model afterwards.
struct SlotRow {
    is_party: bool,
    alive: bool,
    hp: u16,
    hp_max: u16,
    mp: u16,
    mp_max: u16,
    /// Index into `World::battle.ap_gauges` for party rows; `None` for monsters.
    ap_slot: Option<usize>,
    /// Displayed level (char record `+0x130`) for party rows; `0` for
    /// monsters, which retail's status element never draws a count for.
    level: u8,
}

/// Fold the live battle-actor table into `hud`'s per-slot rows, so a host's
/// shared `battle_hud_draws_for` builder has something to draw.
///
/// Without this the HUD model carries only popups and status icons and every
/// slot reads `active == false`, so the builder emits an empty draw list.
/// Shared by the native window and the browser play page - each host calls it
/// once per battle frame, then projects `BattleHud` into its renderer's view
/// types.
///
/// Slot indices are **absolute actor-table indices**, not a compacted list:
/// party ordinals `0..party_count`, monsters above that. Renderers key popup
/// anchoring off the same index space, so compacting here would mis-anchor
/// every damage number.
pub fn sync_battle_hud_rows(hud: &mut BattleHud, world: &legaia_engine_core::world::World) {
    if world.mode == legaia_engine_mode::mode::SceneMode::MuscleDome {
        sync_dome_hud_rows(hud, world);
        return;
    }
    let pc = (world.party.party_count.clamp(1, 3) as usize).min(world.actors.len());
    let party_names = world.party.roster_names();

    // Party rows. `character_max_mp` is the only MP ceiling the world carries
    // (`BattleActor` has live `mp` but no max), and it is keyed by battle
    // ordinal - the same index `build_battle_item_session` uses.
    //
    // HP is the **displayed** value, not the live one: retail's readout draws
    // actor `+0x172`, the bar the per-frame ramp `FUN_80047430` walks toward
    // live HP a quarter of the debt at a time. The sim maintains that ramp in
    // `BattleActor::hp_display` (`World::apply_battle_hp_delta` seeds it,
    // `tick_battle_hp_bars` drains it); reading live `hp` here showed the
    // ramp's end state instantly and left the animation computed but unseen.
    // `None` means "never armed / settled at live HP".
    let mut rows: Vec<(u8, String, SlotRow)> = Vec::new();
    for (i, a) in world.actors.iter().take(pc).enumerate() {
        let name = party_names
            .get(world.party_roster_slot(i))
            .filter(|n| !n.is_empty())
            .cloned()
            .unwrap_or_else(|| format!("P{}", i + 1));
        rows.push((
            i as u8,
            name,
            SlotRow {
                is_party: true,
                alive: a.battle.liveness != 0,
                hp: a.battle.hp_display.unwrap_or(a.battle.hp),
                hp_max: a.battle.max_hp,
                mp: a.battle.mp,
                mp_max: world.tables.character_max_mp.get(i).copied().unwrap_or(0),
                ap_slot: (i < world.battle.ap_gauges.len()).then_some(i),
                // The status element's no-ailment arm draws the character
                // record's `+0x130` beside the base marker (`FUN_8002C2E4`
                // reads it as the display record's `+0x6F8`).
                level: world
                    .party
                    .roster
                    .members
                    .get(world.party_roster_slot(i))
                    .map(|m| m.magic_rank())
                    .unwrap_or(0),
            },
        ));
    }

    // Monster rows. Named from the live catalog when the formation resolved
    // one; `M<n>` otherwise. Monsters have no MP ceiling in the model, so the
    // builder draws no MP field for them.
    let mut cleared: Vec<u8> = Vec::new();
    for slot in pc..world.actors.len().min(hud.slots.len()) {
        let a = &world.actors[slot];
        if a.battle.max_hp == 0 {
            cleared.push(slot as u8);
            continue;
        }
        let name = a
            .battle_monster_id
            .and_then(|id| world.tables.monster_catalog.get(id))
            .map(|d| d.name.clone())
            .unwrap_or_else(|| format!("M{}", slot - pc + 1));
        rows.push((
            slot as u8,
            name,
            SlotRow {
                is_party: false,
                alive: a.battle.liveness != 0,
                // Same displayed-HP read as the party rows; monster slots
                // (>= 3) settle in one frame per FUN_80047430's slot split,
                // so their display only ever lags by a single tick.
                hp: a.battle.hp_display.unwrap_or(a.battle.hp),
                hp_max: a.battle.max_hp,
                mp: 0,
                mp_max: 0,
                ap_slot: None,
                level: 0,
            },
        ));
    }
    // Slots past the actor table are stale from a previous formation.
    for slot in world.actors.len()..hud.slots.len() {
        cleared.push(slot as u8);
    }

    for (slot, name, row) in &rows {
        let ap = row.ap_slot.map(|i| &world.battle.ap_gauges[i]);
        hud.sync_slot(
            *slot,
            SlotSyncInfo {
                name,
                is_party: row.is_party,
                alive: row.alive,
                hp: row.hp,
                hp_max: row.hp_max,
                mp: row.mp,
                mp_max: row.mp_max,
                ap,
            },
        );
        hud.sync_level(*slot, row.level);
    }
    for slot in cleared {
        hud.clear_slot(slot);
    }
    hud.arm_combo(battle_combo_style(world), world.battle_ctx.active_actor);
    hud.close_combo_on_fade_down(
        world.battle_ctx.action_state,
        world.battle_ctx.done_ui_torn_down != 0,
    );
}

/// The Muscle Dome leg's rows for the same HUD model: the lead fighter at
/// ordinal 0 off the dome session (HP, and MP off its magic loadout), every
/// other slot cleared. Retail draws no enemy plate in a dome leg any more
/// than in a battle, so the opponent has no row here.
fn sync_dome_hud_rows(hud: &mut BattleHud, world: &legaia_engine_core::world::World) {
    let Some(s) = world.minigames.muscle_dome.as_ref() else {
        for slot in 0..hud.slots.len() {
            hud.clear_slot(slot as u8);
        }
        return;
    };
    let name = party_member_name(world, 0);
    let clamp = |v: i32| v.clamp(0, i32::from(u16::MAX)) as u16;
    hud.sync_slot(
        0,
        SlotSyncInfo {
            name: &name,
            is_party: true,
            alive: s.hp(0) > 0,
            hp: clamp(s.hp(0)),
            hp_max: clamp(s.hp_max(0)),
            mp: s.mp(0),
            mp_max: s.magic(0).map_or(0, |m| m.mp_max),
            ap: None,
        },
    );
    hud.sync_level(
        0,
        world
            .party
            .roster
            .members
            .get(world.party_roster_slot(0))
            .map(|m| m.magic_rank())
            .unwrap_or(0),
    );
    for slot in 1..hud.slots.len() {
        hud.clear_slot(slot as u8);
    }
}

/// Build the deduplicated enemy target-menu rows straight off the live
/// world's monster slots - the host-facing entry to
/// [`legaia_engine_battle::target_picker::enemy_menu_rows`] (retail `FUN_801D9D3C`, the
/// intro-banner composer's dedup pass reused for the picker).
///
/// The engine seats a formation's monsters directly after the party
/// (`World::enter_battle`), and the pickers the hosts drive index enemies
/// the same way (`CursorRow::Enemy` slot `i` = actor `party_count + i`), so
/// a row's `first_slot` compares directly against the picker's cursor slot.
///
/// Occupancy stands in for retail's `_DAT_8007BD0C` monster-id table: a
/// dead or unseeded slot contributes `0` (retail's "no monster here"), and
/// distinct catalog names get distinct synthetic ids so identical adjacent
/// monsters collapse into one labelled run exactly as retail's identical-id
/// runs do. Names come from the same live catalog the HUD rows use.
///
/// The X each row averages is the monster actor's `+0x34` - its battle
/// **world** X (`lhu a0,0x34(v0)` at `0x801D9E00` and `0x801D9ED4`, on the
/// actor `0x801C9370[slot + 3]`), the same halfword the battle setup
/// `FUN_800513F0` stamps with the stage seat and the round-start squash
/// recentres - not a GTE projection. The layout maps it to the screen as
/// `(avg >> 3) - width / 2 + 0xA0`, so a label sits over its group's seat.
/// It is read here off the live position; callers run
/// [`legaia_engine_battle::target_picker::layout_enemy_menu_rows`] with their own text
/// measurer before drawing.
pub fn battle_enemy_target_rows(
    world: &legaia_engine_core::world::World,
) -> Vec<legaia_engine_battle::target_picker::EnemyMenuRow> {
    use legaia_engine_battle::target_picker::{DEDUP_SUFFIX, FORMATION_SLOTS, enemy_menu_rows};
    let pc = (world.party.party_count.clamp(1, 3) as usize).min(world.actors.len());
    let mut ids = [0u8; FORMATION_SLOTS];
    let mut names: Vec<String> = vec![String::new(); FORMATION_SLOTS];
    // The catalog name per seated slot (`max_hp != 0`), dead or alive: the
    // per-instance letter a duplicate wears is assigned over the seated
    // formation, so a twin dying does not re-letter the survivor.
    let seated: Vec<Option<String>> = (0..FORMATION_SLOTS)
        .map(|i| {
            let a = world.actors.get(pc + i)?;
            (a.battle.max_hp != 0).then(|| {
                a.battle_monster_id
                    .and_then(|id| world.tables.monster_catalog.get(id))
                    .map(|d| d.name.clone())
                    .unwrap_or_else(|| format!("M{}", i + 1))
            })
        })
        .collect();
    for i in 0..FORMATION_SLOTS {
        let Some(a) = world.actors.get(pc + i) else {
            continue;
        };
        let Some(name) = seated[i].clone() else {
            continue;
        };
        if a.battle.hp == 0 {
            continue;
        }
        let pos = names[..i].iter().position(|n| n == &name);
        ids[i] = (pos.unwrap_or(i) + 1) as u8;
        names[i] = name;
    }
    // The label source is the actor's display name (`+0x1BC`), which carries
    // an instance letter when the formation seats the same monster twice -
    // the letter the composer's run arm drops before it appends the suffix.
    let display = |slot: u8| -> String {
        let i = slot as usize;
        let name = names[i].clone();
        let twins: Vec<usize> = (0..FORMATION_SLOTS)
            .filter(|&j| seated[j].as_deref() == Some(name.as_str()))
            .collect();
        if twins.len() < 2 {
            return name;
        }
        let ordinal = twins.iter().position(|&j| j == i).unwrap_or(0) as u8;
        format!("{name} {}", (b'A' + ordinal) as char)
    };
    let world_x = |slot: u8| -> i16 {
        world
            .actors
            .get(pc + usize::from(slot))
            .map_or(0, |a| a.move_state.world_x)
    };
    enemy_menu_rows(ids, DEDUP_SUFFIX, display, world_x)
}

/// The battle-**intro** enemy-name banner this frame: one label per monster
/// group, laid out by retail's composer and pen-seated at `(x, 48)`.
///
/// Retail's flow state `0x0A` calls `FUN_801D9D3C`, which groups the
/// formation ([`legaia_engine_battle::target_picker::enemy_menu_rows`]), lays the labels out
/// ([`legaia_engine_battle::target_picker::layout_enemy_menu_rows`]) and spawns each as a
/// text actor with the message banner's class-0 frame; the labels live until
/// the `0x0B` expiry sweeps every text actor, which is the
/// [`legaia_engine_core::world::BattleState::intro_names_frames`] timer. The pair returned
/// is `(label, pen x)`; the drawer takes the pen y from
/// [`legaia_engine_battle::target_picker::MENU_ROW_Y`]'s value, `48`.
///
/// `font` is the width measure retail's `FUN_80035F04` is (the `legaia-font`
/// layout advance). The X each group averages is its monsters' battle world
/// X (see [`battle_enemy_target_rows`]), so each label sits over its group.
pub fn battle_intro_names(
    world: &legaia_engine_core::world::World,
    font: &legaia_font::Font,
) -> Vec<(String, i32)> {
    if world.mode == legaia_engine_mode::mode::SceneMode::MuscleDome {
        return dome_intro_names(world, font);
    }
    if world.mode != legaia_engine_mode::mode::SceneMode::Battle
        || world.battle.intro_names_frames == 0
        || world.battle_entry_sweeping()
    {
        return Vec::new();
    }
    // Retail holds the command flow in `0x0B` until the expiry sweep takes
    // the labels, so no action ever plays under them. The port does not park
    // the flow (see `world::battle::intro_names`), so an action that starts
    // inside the hold ends the labels here instead.
    if battle_action_in_flight(world.battle_ctx.action_state) {
        return Vec::new();
    }
    // The same hold keeps every command surface past the round prompt off
    // the screen while the labels are up: retail's sweep empties the text
    // actor list before the prompt builds, so a ring, picker, submenu or
    // Begin / Reselect confirm never shares a frame with the labels. One the
    // port opens inside the span ends them.
    if battle_hud_phase(world) == BattleHudPhase::CommandEntry {
        return Vec::new();
    }
    let mut rows = battle_enemy_target_rows(world);
    legaia_engine_battle::target_picker::layout_enemy_menu_rows(&mut rows, |s| {
        font.layout_ascii(s).advance_x as i16
    });
    rows.into_iter()
        .map(|r| (r.label, i32::from(r.x)))
        .collect()
}

/// The dome leg's battle-open banner: the composer `FUN_801D9D3C` over a
/// one-monster formation is one label, the opponent's display name, laid out
/// the way every group label is - `0xA0 + avg(+0x34) / 8 - width / 2` over
/// the monster's battle X, which the lone monster seat puts at `0`
/// (`legaia_engine_battle::battle_seats`) - and clamped into `6 ..= 0x13A - width`. Up for
/// the leg's battle-open hold ([`legaia_engine_battle_menus::muscle_dome_session::MuscleDomeSession::intro_up`])
/// once no hub screen covers the leg.
fn dome_intro_names(
    world: &legaia_engine_core::world::World,
    font: &legaia_font::Font,
) -> Vec<(String, i32)> {
    let Some(s) = world.minigames.muscle_dome.as_ref() else {
        return Vec::new();
    };
    if !s.intro_up() || world.minigames.muscle_hub.covers_leg() {
        return Vec::new();
    }
    let Some(name) = s.opponent_name().filter(|n| !n.is_empty()) else {
        return Vec::new();
    };
    let width = i32::from(font.layout_ascii(name).advance_x as i16);
    let seat_x = i32::from(legaia_engine_battle::battle_seats::monster_seat(1, 0, false).x);
    let x = (0xA0 + (seat_x >> 3) - width / 2).clamp(6, (0x13A - width).max(6));
    vec![(name.to_string(), x)]
}

/// The boss-name banner a battle-stage module has up this frame, as
/// `(label, pen x, pen y)` - the Cort arrival's phase 5 (PROT 0968,
/// `0x801F7054..0x801F709C`), which measures the first monster's name with
/// `FUN_80035F04` and spawns it as a frameless text actor at
/// `(0xA0 - (width >> 1), 0x96)`. `font` is that width measure. Up until the
/// module hands the round back.
pub fn battle_stage_banner(
    world: &legaia_engine_core::world::World,
    font: &legaia_font::Font,
) -> Option<(String, i32, i32)> {
    use legaia_engine_battle::battle_stage_module::{ARRIVAL_BANNER_CENTRE_X, ARRIVAL_BANNER_Y};
    if world.mode != legaia_engine_mode::mode::SceneMode::Battle {
        return None;
    }
    let label = world.battle.stage_banner.as_ref()?;
    let width = font.layout_ascii(label).advance_x as i32;
    Some((
        label.clone(),
        ARRIVAL_BANNER_CENTRE_X - (width >> 1),
        ARRIVAL_BANNER_Y,
    ))
}
