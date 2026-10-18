//! What the pad tier does in the field between hops: healing and equipping
//! a guard through the pause menu, burning an Incense, and resting at an inn.

use super::*;

thread_local! {
    /// Set while [`pad_ready`] readies the party at a staged fight's door.
    static AT_BOSS_DOOR: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Whether the field upkeep is spending toward a boss: on the walk to one
/// ([`continuous::SAVING_FOR_BOSS`]) or at its door ([`pad_ready`]). Its
/// group-heal MP then stays for the fight: Zora (`taiku` P2[19]) was met
/// with Vahn's pool spent from 197 to 17 on four field casts at the door,
/// the bag holding no restorative, and the Spoons the fight's party-wide
/// hits needed were the ones cast.
/// The weakest member's HP, per mille, under which a boss-bound party with
/// an empty bag spends its casters' whole pools on a field heal.
const NEAR_DEATH_PERMILLE: u32 = 250;

fn boss_bound() -> bool {
    continuous::SAVING_FOR_BOSS.with(std::cell::Cell::get)
        || AT_BOSS_DOOR.with(std::cell::Cell::get)
}

// ---------------------------------------------------------------------------
// Healing through the pause menu
// ---------------------------------------------------------------------------

/// Press `mask` for one frame and release on the next: every menu surface
/// reads `just_pressed`, so a held mask is one event.
/// Press Start and wait out the pause wipe: retail's field session raises
/// the wipe level from black-free to full before it spawns the menu
/// (`FUN_801ED308`), and the menu takes no input until then
/// (`NativeSession::pause_wipe`). A press sent into the wipe is lost, and the
/// field heal then walked out of a menu it never drove.
pub(super) fn open_pause_menu(session: &mut NativeSession) {
    tap_pad(session, PadButton::Start.mask());
    for _ in 0..120 {
        if session.field_menu.is_none() || session.pause_wipe().menu_spawned() {
            break;
        }
        let _ = session.tick();
    }
}

pub(super) fn tap_pad(session: &mut NativeSession, mask: u16) {
    session.host.world.set_pad(mask);
    let _ = session.tick();
    session.host.world.set_pad(0);
    let _ = session.tick();
}

/// Hand control to the next party member inside an armed three-actor talk:
/// press Square, the newly-pressed bit the talk controller's state-0 arm gate
/// reads (`FUN_801D27E0`, `_DAT_8007B874 & 0x80`), and wait out the
/// fade-swap-fade cycle. `true` when the leader changed.
pub(super) fn swap_leader(session: &mut NativeSession) -> bool {
    let armed = |s: &NativeSession| {
        s.host
            .world
            .dialog
            .three_actor_talk
            .as_ref()
            .is_some_and(|t| t.swap.phase == 0)
    };
    if !armed(session) || !released(session) {
        return false;
    }
    let before = session.host.world.party.party_leader_slot;
    tap_pad(session, PadButton::Square.mask());
    for _ in 0..240 {
        if armed(session) && session.host.world.party.party_leader_slot != before {
            break;
        }
        if session.tick().is_err() {
            break;
        }
    }
    let now = session.host.world.party.party_leader_slot;
    if std::env::var_os("LEGAIA_FGL_TRACE").is_some() {
        let (x, z) = player_xz(session);
        eprintln!(
            "    [swap] leader {before:?} -> {now:?}, now at {:?}",
            tile_of(x, z)
        );
    }
    now != before
}

/// The weakest living party member's HP as a fraction of its maximum, in
/// per-mille. `1000` for a party at full health.
pub(super) fn party_hp_permille(session: &NativeSession) -> u32 {
    let w = &session.host.world;
    let n = w.party.party_count.clamp(1, 3) as usize;
    (0..n)
        .filter_map(|i| {
            let b = &w.actors.get(i)?.battle;
            (b.max_hp > 0 && b.hp > 0).then(|| u32::from(b.hp) * 1000 / u32::from(b.max_hp))
        })
        .min()
        .unwrap_or(1000)
}

/// The pad edge that moves a pause list's hand from row `cursor` toward row
/// `want`, or Cross once it is there. Up and Down wrap inside the page
/// (the kind-4 list kernel `FUN_80032A44`), so a row on another page is
/// reached by paging first: Down from row 0 never reaches row 18.
pub(super) fn list_step_toward(cursor: usize, want: usize) -> u16 {
    use legaia_engine_menus::pause_screens::LIST_PAGE_ROWS;
    let (page, want_page) = (cursor / LIST_PAGE_ROWS, want / LIST_PAGE_ROWS);
    if page < want_page {
        PadButton::Right.mask()
    } else if page > want_page {
        PadButton::Left.mask()
    } else if cursor < want {
        PadButton::Down.mask()
    } else if cursor > want {
        PadButton::Up.mask()
    } else {
        PadButton::Cross.mask()
    }
}

/// Heal the party with the pad, as a player does before a boss or after a
/// fight that left it low: Start opens the pause menu, the cursor walks to
/// Items, Use, the first HP-restoring item, the weakest member; repeat while
/// the weakest is below `threshold` per-mille and the bag has a restorative;
/// then Circle back out to the field. Every step is a pad edge through the
/// same menu sessions both play hosts drive. Returns how many items it used.
pub(super) fn pad_field_heal(session: &mut NativeSession, threshold: u32) -> usize {
    pad_field_restore(session, threshold, false)
}

/// The weakest present member's MP, per mille of its maximum (members with
/// no MP pool count as full).
pub(super) fn party_mp_permille(session: &NativeSession) -> u32 {
    let w = &session.host.world;
    w.present_party_list()
        .into_iter()
        .filter_map(|s| {
            let v = w.party.roster.members.get(usize::from(s))?.hp_mp_sp();
            (v.mp_max > 0).then(|| u32::from(v.mp_cur) * 1000 / u32::from(v.mp_max))
        })
        .min()
        .unwrap_or(1000)
}

/// [`pad_field_heal`] for either pool: `mp` walks the same Items screen to
/// the first MP restorative and the member lowest on MP.
pub(super) fn pad_field_restore(session: &mut NativeSession, threshold: u32, mp: bool) -> usize {
    let gauge = |s: &NativeSession| {
        if mp {
            party_mp_permille(s)
        } else {
            party_hp_permille(s)
        }
    };
    let restores = move |effect: &ItemEffect| {
        if mp {
            matches!(effect, ItemEffect::HealMp { .. })
        } else {
            matches!(effect, ItemEffect::Heal { .. } | ItemEffect::HealAll)
        }
    };
    use legaia_engine_menu_runtime::field_menu_dispatch::FieldMenuSubsession;
    use legaia_engine_menus::field_menu::FieldMenuRow;
    use legaia_engine_menus::inventory_use::InventoryUseState;
    use legaia_engine_menus::items::ItemEffect;
    use legaia_engine_menus::pause_screens::PauseItemsFocus;
    // The continuous hand keeps its bag for the fights. Training from a
    // bed it spends nothing on the road; a party of two or more spends a
    // restorative there only under 45% (a lone member has nobody to bring
    // it back, and tops up under 65%); and it does not open
    // the menu on an empty bag.
    let plays_on = continuous::PLAYS_ON.with(std::cell::Cell::get);
    let threshold = if plays_on && threshold == 800 {
        if TRAINING.with(std::cell::Cell::get) {
            return 0;
        }
        if session.host.world.present_party_list().len() >= 2 {
            450
        } else {
            650
        }
    } else {
        threshold
    };
    if !walking(session) || !released(session) || gauge(session) >= threshold {
        return 0;
    }
    // The continuous hand heals with spells first, out of the MP above three
    // fifths of each caster's pool (the rest is for the fights), and with all of it
    // once the bag holds no restorative: the run met Koru (`nilboa`, a
    // 20000-HP race) with an empty bag, two members near death and a full
    // set of Spoons unspent.
    let mut spells = 0;
    if plays_on && !mp {
        spells = pad_field_spell_heal(session, threshold, true);
        if gauge(session) >= threshold {
            return spells;
        }
    }
    if plays_on {
        let w = &session.host.world;
        let holds_heal = w.party.inventory.iter().any(|(&id, &n)| {
            n > 0
                && !errand_item(w, id)
                && w.tables
                    .item_catalog
                    .get(id)
                    .is_some_and(|e| e.usable_in_field && restores(&e.effect))
        });
        if !holds_heal {
            // Bound for a boss with an empty bag, the casters' MP is the
            // boss's - unless a member is near death, where walking in is the
            // worse spend: the Delilas duels hand `nivora_late` a party with
            // Noa at 1 HP and no bed in `nilboa`, and Koru's race was lost
            // five times running with her down from its first round.
            let near_death = gauge(session) < NEAR_DEATH_PERMILLE;
            return spells
                + if mp || (boss_bound() && !near_death) {
                    0
                } else {
                    pad_field_spell_heal(session, threshold, false)
                };
        }
        // On the walk to a boss the bag's last heals are the boss's: the
        // road spends restoratives only above the reserve the shop buys
        // first ([`HEAL_RESERVE`] a member). `son`'s nineteen Healing
        // Berries were all drunk on `taiku`'s road and Zora was fought out
        // of an empty bag.
        if !mp
            && continuous::SAVING_FOR_BOSS.with(std::cell::Cell::get)
            && !AT_BOSS_DOOR.with(std::cell::Cell::get)
            && heals_held(w) <= HEAL_RESERVE * w.present_party_list().len().clamp(1, 3) as u32
        {
            return spells;
        }
    }
    open_pause_menu(session);
    if session.field_menu.is_none() {
        return 0;
    }
    let items_row = FieldMenuRow::Items.index();
    let mut used = 0usize;
    // The use count when the Items screen was last entered: a use's result
    // closes the screen back to the root menu, so a party still below the
    // threshold re-enters it - but only after a visit that used something.
    let mut used_at_entry: Option<usize> = None;
    for _ in 0..200 {
        let Some(menu) = session.field_menu.as_ref() else {
            break;
        };
        let pad = match session.field_menu_sub.as_ref() {
            None => match menu.phase() {
                legaia_engine_menus::field_menu::FieldMenuPhase::Browsing { cursor } => {
                    if gauge(session) >= threshold
                        || used >= 4
                        || used_at_entry.is_some_and(|u| u == used)
                        || used > 0 && cursor != items_row
                    {
                        PadButton::Circle.mask()
                    } else if cursor != items_row {
                        PadButton::Down.mask()
                    } else {
                        used_at_entry = Some(used);
                        PadButton::Cross.mask()
                    }
                }
                _ => 0,
            },
            Some(FieldMenuSubsession::Items(p)) => {
                let heals = |id: u8| {
                    !errand_item(&session.host.world, id)
                        && p.inner
                            .catalog
                            .get(id)
                            .is_some_and(|e| e.usable_in_field && restores(&e.effect))
                };
                // The hand walks every bag row, usable here or not
                // (`PauseItemsSession::list_cursor`): a row the field cannot
                // use above the first restorative (a Fury Boost bought at
                // the same counter) is stepped over, not confirmed.
                let want = p.rows.iter().position(|r| heals(r.id));
                match (p.focus, &p.inner.state) {
                    _ if gauge(session) >= threshold || want.is_none() || used >= 4 => {
                        PadButton::Circle.mask()
                    }
                    (PauseItemsFocus::Command, _) => {
                        if p.command_cursor == 0 {
                            PadButton::Cross.mask()
                        } else {
                            PadButton::Up.mask()
                        }
                    }
                    (PauseItemsFocus::List, InventoryUseState::Browsing { .. }) => {
                        list_step_toward(p.list_cursor(), want.unwrap_or(0))
                    }
                    (PauseItemsFocus::List, InventoryUseState::TargetSelect { cursor, .. }) => {
                        let weakest = p
                            .inner
                            .targets
                            .iter()
                            .enumerate()
                            .filter(|(_, t)| !t.is_enemy && t.alive && t.hp_max > 0)
                            .min_by_key(|(_, t)| {
                                if mp {
                                    u32::from(t.mp) * 1000 / u32::from(t.mp_max.max(1))
                                } else {
                                    u32::from(t.hp) * 1000 / u32::from(t.hp_max)
                                }
                            })
                            .map_or(0, |(k, _)| k);
                        if *cursor < weakest {
                            PadButton::Down.mask()
                        } else if *cursor > weakest {
                            PadButton::Up.mask()
                        } else {
                            used += 1;
                            PadButton::Cross.mask()
                        }
                    }
                    _ => PadButton::Circle.mask(),
                }
            }
            Some(_) => PadButton::Circle.mask(),
        };
        if std::env::var_os("LEGAIA_FGL_HEAL_DEBUG").is_some() {
            let sub = match session.field_menu_sub.as_ref() {
                None => format!("root {:?}", menu.phase()),
                Some(FieldMenuSubsession::Items(p)) => format!(
                    "items focus {:?} cmd {} state {:?} items {:02X?} filtered {:?} targets {:?}",
                    p.focus,
                    p.command_cursor,
                    p.inner.state,
                    p.inner.items,
                    p.inner.filtered_items,
                    p.inner
                        .targets
                        .iter()
                        .map(|t| (t.is_enemy, t.alive, t.hp, t.hp_max))
                        .collect::<Vec<_>>()
                ),
                Some(_) => "other".into(),
            };
            eprintln!("      [heal-debug] pad {pad:04X} used {used} {sub}");
        }
        if pad == 0 {
            let _ = session.tick();
            continue;
        }
        tap_pad(session, pad);
    }
    // Whatever is still open closes on Circle.
    for _ in 0..16 {
        if session.field_menu.is_none() {
            break;
        }
        tap_pad(session, PadButton::Circle.mask());
    }
    if std::env::var_os("LEGAIA_FGL_TRACE").is_some() {
        eprintln!(
            "    [heal] used {used} {} item(s); weakest now {} per-mille; menu {}",
            if mp { "MP" } else { "HP" },
            gauge(session),
            if session.field_menu.is_some() {
                "STILL OPEN"
            } else {
                "closed"
            }
        );
    }
    used + spells
}

/// The heal cast worth making from the field Magic screen, as
/// `(caster row, spell id)`: among the present casters' field-usable healing
/// spells they can pay for (keeping half their MP when `keep_half`), the one
/// that restores the most HP across the party's actual deficits - a group
/// heal counts every member's, a single heal the worst-off member's. The
/// kept share is three fifths of the pool.
fn field_heal_cast(
    s: &legaia_engine_menus::spell_menu::SpellMenuSession,
    keep_half: bool,
) -> Option<(usize, u8)> {
    use legaia_engine_battle::spells::SpellEffect;
    let deficits: Vec<u32> = s
        .targets()
        .iter()
        .filter(|t| t.alive())
        .map(|t| u32::from(t.hp_max.saturating_sub(t.hp)))
        .collect();
    let worst = deficits.iter().copied().max().unwrap_or(0);
    let mut best: Option<(u32, usize, u8)> = None;
    for (row, c) in s.party().iter().enumerate() {
        if !c.alive() || c.ra_seru_missing {
            continue;
        }
        for &id in &c.spells {
            let Some(def) = s.catalog().get(id) else {
                continue;
            };
            let cost = c.mp_cost(def);
            // The kept share is the line [`pad_ready`] tops MP up to before a
            // staged fight (three fifths): casting below it on the road only
            // turns the cast into a Magic Fruit drunk at the boss's door.
            if c.mp < cost || (keep_half && u32::from(c.mp - cost) * 5 < u32::from(c.mp_max) * 3) {
                continue;
            }
            let gain = match def.effect {
                SpellEffect::HealAll { amount } => deficits
                    .iter()
                    .map(|&d| d.min(u32::from(amount)))
                    .sum::<u32>(),
                SpellEffect::Heal { amount } => worst.min(u32::from(amount)),
                _ => continue,
            };
            if gain > 0 && best.is_none_or(|(g, _, _)| gain > g) {
                best = Some((gain, row, id));
            }
        }
    }
    best.map(|(_, row, id)| (row, id))
}

/// Heal the party with spells from the field Magic screen while its weakest
/// member is under `threshold` per-mille: Start, Magic, the caster, the
/// spell, the weakest member (or the group confirm). Each cast goes through
/// the engine's own field cast (`apply_spell_outcome`: the discounted MP
/// debit and the heal). With `keep_half`, a caster keeps three fifths of
/// its MP for the fights. Returns the casts made.
pub(super) fn pad_field_spell_heal(
    session: &mut NativeSession,
    threshold: u32,
    keep_half: bool,
) -> usize {
    use legaia_engine_menu_runtime::field_menu_dispatch::FieldMenuSubsession;
    use legaia_engine_menus::field_menu::{FieldMenuPhase, FieldMenuRow};
    use legaia_engine_menus::spell_menu::SpellMenuPhase;
    if !walking(session) || !released(session) || party_hp_permille(session) >= threshold {
        return 0;
    }
    open_pause_menu(session);
    if session.field_menu.is_none() {
        return 0;
    }
    let magic_row = FieldMenuRow::Magic.index();
    let mut cast = 0usize;
    // Set once a visit to the Magic screen finds nothing to cast.
    let mut dry = false;
    let mut plan: Option<(usize, u8)> = None;
    for _ in 0..240 {
        let Some(menu) = session.field_menu.as_ref() else {
            break;
        };
        let done = party_hp_permille(session) >= threshold || dry || cast >= 8;
        let pad = match session.field_menu_sub.as_ref() {
            None => match menu.phase() {
                FieldMenuPhase::Browsing { cursor } => {
                    if done {
                        PadButton::Circle.mask()
                    } else if cursor != magic_row {
                        list_step_toward(usize::from(cursor), usize::from(magic_row))
                    } else {
                        plan = None;
                        PadButton::Cross.mask()
                    }
                }
                _ => 0,
            },
            Some(FieldMenuSubsession::Spells(s)) => {
                if plan.is_none() && !done {
                    plan = field_heal_cast(s, keep_half);
                    dry = plan.is_none();
                }
                match (plan, s.phase()) {
                    _ if done && plan.is_none() => PadButton::Circle.mask(),
                    (None, _) => PadButton::Circle.mask(),
                    (Some((row, _)), SpellMenuPhase::CharSelect { cursor }) => {
                        list_step_toward(usize::from(*cursor), row)
                    }
                    (Some((row, _)), SpellMenuPhase::SpellSelect { caster, .. })
                        if usize::from(*caster) != row =>
                    {
                        PadButton::Circle.mask()
                    }
                    (Some((_, id)), SpellMenuPhase::SpellSelect { cursor, .. }) => {
                        match s.current_spell_rows().iter().position(|r| r.spell_id == id) {
                            Some(want) if want != usize::from(*cursor) => {
                                if want < usize::from(*cursor) {
                                    PadButton::Up.mask()
                                } else {
                                    PadButton::Down.mask()
                                }
                            }
                            Some(_) => PadButton::Cross.mask(),
                            None => PadButton::Circle.mask(),
                        }
                    }
                    (Some(_), SpellMenuPhase::TargetSelect { cursor, .. }) => {
                        let weakest = s
                            .targets()
                            .iter()
                            .enumerate()
                            .filter(|(_, t)| t.alive() && t.hp_max > 0)
                            .min_by_key(|(_, t)| u32::from(t.hp) * 1000 / u32::from(t.hp_max))
                            .map_or(0, |(k, _)| k);
                        if usize::from(*cursor) == weakest {
                            cast += 1;
                            plan = None;
                            PadButton::Cross.mask()
                        } else if usize::from(*cursor) < weakest {
                            PadButton::Down.mask()
                        } else {
                            PadButton::Up.mask()
                        }
                    }
                    (Some(_), SpellMenuPhase::GroupConfirm { .. }) => {
                        cast += 1;
                        plan = None;
                        PadButton::Cross.mask()
                    }
                    _ => 0,
                }
            }
            Some(_) => PadButton::Circle.mask(),
        };
        if pad == 0 {
            let _ = session.tick();
            continue;
        }
        tap_pad(session, pad);
    }
    for _ in 0..16 {
        if session.field_menu.is_none() {
            break;
        }
        tap_pad(session, PadButton::Circle.mask());
    }
    if std::env::var_os("LEGAIA_FGL_TRACE").is_some() {
        eprintln!(
            "    [heal] cast {cast} field spell(s); weakest now {} per-mille; menu {}",
            party_hp_permille(session),
            if session.field_menu.is_some() {
                "STILL OPEN"
            } else {
                "closed"
            }
        );
    }
    cast
}

thread_local! {
    /// Set while the continuous hand trains from a bed ([`pad_train`]).
    pub(super) static TRAINING: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

thread_local! {
    /// The element of the boss (a fight that forbids running) the last pad
    /// battle was lost to, for the reload that prepares against it.
    pub(super) static LOST_TO_ELEMENT: std::cell::Cell<Option<u8>> = const { std::cell::Cell::new(None) };
}

/// The accessory passive that halves one element's hits on its wearer: the
/// elemental-guard band `0x1D + element` the damage finisher's resist ladder
/// tests (`FUN_801DDB30`; Ebony Jewel, Dark Stone and Dark Talisman for
/// Dark). Elements past 6 have no guard.
pub(super) fn guard_passive(element: u8) -> Option<u8> {
    if element == STONE_GUARD_KEY {
        return Some(STONE_GUARD);
    }
    (element <= 6).then_some(0x1D + element)
}

/// The [`LOST_TO_ELEMENT`] value a wipe records when the boss Stoned a
/// member (`+0x16E & 4`, which the `0x5A` wipe scan counts as down): the
/// reload then wears [`STONE_GUARD`] rather than the boss's element guard.
/// Above every element byte (`0..=7`).
pub(super) const STONE_GUARD_KEY: u8 = 0x80;

/// Stone Guard, the accessory passive that nullifies Stone (Stone Amulet;
/// `docs/formats/accessory-passive-table.md`, mask `0xFFFB`).
pub(super) const STONE_GUARD: u8 = 0x1A;

/// Master Guard (Wonder Amulet) nullifies every status, Stone included.
const MASTER_GUARD: u8 = 0x1C;

/// Does a worn or carried passive answer the guard `want`?
fn guards(passive: Option<u8>, want: u8) -> bool {
    passive == Some(want) || (want == STONE_GUARD && passive == Some(MASTER_GUARD))
}

/// The Goods slot a guard goes into: the third (engine slot 7), the way a
/// player who keeps the first two for the passives the party fights with
/// would swap. Against Songi the Defender Chain's evasion (on Vahn and Noa
/// the second Goods slot) outweighs the third slot's Wonder Amulet and
/// Spirit Talisman.
pub(super) const GUARD_SLOT: u8 = 7;

/// The bag's guard Goods item for `element`: the most-held item carrying
/// [`guard_passive`].
pub(super) fn guard_item_for(w: &legaia_engine_core::world::World, element: u8) -> Option<u8> {
    let want = guard_passive(element)?;
    w.party
        .inventory
        .iter()
        .filter(|&(&id, &qty)| {
            qty > 0 && guards(w.tables.accessory_passives.passive_index(id), want)
        })
        .max_by_key(|&(&id, &qty)| (qty, std::cmp::Reverse(id)))
        .map(|(&id, _)| id)
}

/// The present party members that wear no guard against `element`.
pub(super) fn unguarded_members(w: &legaia_engine_core::world::World, element: u8) -> Vec<u8> {
    let Some(want) = guard_passive(element) else {
        return Vec::new();
    };
    w.present_party_list()
        .into_iter()
        .filter(|&s| {
            w.party.roster.members.get(usize::from(s)).is_some_and(|m| {
                !m.equipment().slots[5..].iter().any(|&id| {
                    id != 0 && guards(w.tables.accessory_passives.passive_index(id), want)
                })
            })
        })
        .collect()
}

/// Equip the bag's guard against `element` on every present member that
/// lacks one, with the pad, as a player does after losing to a boss of that
/// element: Start, Equip, the member in the character picker, the third
/// Goods row, the guard in the candidate list, Yes - then Circle back to
/// the field. Each confirm commits through the menu's own applier
/// (`FUN_801E5A08`'s port), so the bag gives up the item and takes back the
/// one it replaced. Returns how many members it equipped.
pub(super) fn pad_equip_guard(session: &mut NativeSession, element: u8) -> usize {
    use legaia_engine_menu_runtime::field_menu_dispatch::FieldMenuSubsession;
    use legaia_engine_menus::equip_session::{EquipState, browse_row_for_slot};
    use legaia_engine_menus::field_menu::{FieldMenuPhase, FieldMenuRow};
    let Some(item) = guard_item_for(&session.host.world, element) else {
        return 0;
    };
    if !walking(session)
        || !released(session)
        || unguarded_members(&session.host.world, element).is_empty()
    {
        return 0;
    }
    let before = unguarded_members(&session.host.world, element).len();
    open_pause_menu(session);
    if session.field_menu.is_none() {
        return 0;
    }
    let equip_row = FieldMenuRow::Equip.index();
    let list = session.host.world.present_party_list();
    let pos = |s: u8| list.iter().position(|&x| x == s).unwrap_or(0);
    let mut skipped: HashSet<u8> = HashSet::new();
    for _ in 0..400 {
        let Some(menu) = session.field_menu.as_ref() else {
            break;
        };
        let target = unguarded_members(&session.host.world, element)
            .into_iter()
            .find(|s| !skipped.contains(s));
        let pad = match (session.field_menu_sub.as_ref(), target) {
            (None, None) => PadButton::Circle.mask(),
            (None, Some(_)) => match menu.phase() {
                FieldMenuPhase::Browsing { cursor } if cursor < equip_row => PadButton::Down.mask(),
                FieldMenuPhase::Browsing { cursor } if cursor > equip_row => PadButton::Up.mask(),
                FieldMenuPhase::Browsing { .. } => PadButton::Cross.mask(),
                _ => 0,
            },
            (
                Some(FieldMenuSubsession::Equip {
                    session: es,
                    char_slot,
                    picking,
                }),
                Some(t),
            ) => {
                if *picking {
                    match pos(*char_slot).cmp(&pos(t)) {
                        std::cmp::Ordering::Less => PadButton::Down.mask(),
                        std::cmp::Ordering::Greater => PadButton::Up.mask(),
                        std::cmp::Ordering::Equal => PadButton::Cross.mask(),
                    }
                } else if *char_slot != t {
                    PadButton::Circle.mask()
                } else {
                    match es.state() {
                        EquipState::SlotPicker { cursor } => {
                            match cursor.cmp(&browse_row_for_slot(GUARD_SLOT)) {
                                std::cmp::Ordering::Less => PadButton::Down.mask(),
                                std::cmp::Ordering::Greater => PadButton::Up.mask(),
                                std::cmp::Ordering::Equal => PadButton::Cross.mask(),
                            }
                        }
                        EquipState::ItemPicker { slot, cursor } if slot == GUARD_SLOT => {
                            let rows = es.items_for_slot(slot);
                            match rows.iter().position(|r| r.id == item && !r.equipped) {
                                Some(k) => match usize::from(cursor).cmp(&k) {
                                    std::cmp::Ordering::Less => PadButton::Down.mask(),
                                    std::cmp::Ordering::Greater => PadButton::Up.mask(),
                                    std::cmp::Ordering::Equal => PadButton::Cross.mask(),
                                },
                                None => {
                                    skipped.insert(t);
                                    PadButton::Circle.mask()
                                }
                            }
                        }
                        EquipState::Confirm {
                            item_id, cursor, ..
                        } if item_id == item => {
                            if cursor == 0 {
                                PadButton::Cross.mask()
                            } else {
                                PadButton::Left.mask()
                            }
                        }
                        EquipState::Done(_) => 0,
                        _ => PadButton::Circle.mask(),
                    }
                }
            }
            (Some(_), _) => PadButton::Circle.mask(),
        };
        if pad == 0 {
            let _ = session.tick();
            continue;
        }
        tap_pad(session, pad);
    }
    for _ in 0..16 {
        if session.field_menu.is_none() {
            break;
        }
        tap_pad(session, PadButton::Circle.mask());
    }
    let after = unguarded_members(&session.host.world, element).len();
    if std::env::var_os("LEGAIA_FGL_TRACE").is_some() {
        eprintln!(
            "    [equip] item 0x{item:02X} against element {element}: {} member(s) unguarded -> {after}; menu {}",
            before,
            if session.field_menu.is_some() {
                "STILL OPEN"
            } else {
                "closed"
            }
        );
    }
    before - after.min(before)
}

/// Put on what the bag holds, with the pad, as a player does after opening
/// a chest or leaving a shop: Start, Equip, the member, the Best Equipment
/// row at the top of the slot list (retail's `FUN_801CF760`), then Circle
/// back out. Once for each present member, and only when the bag holds
/// equipment at all. Returns how many members' equipment changed.
pub(super) fn pad_equip_best(session: &mut NativeSession) -> usize {
    use legaia_engine_menu_runtime::field_menu_dispatch::FieldMenuSubsession;
    use legaia_engine_menus::equip_session::{EquipState, SLOT_BROWSE_BEST_ROW};
    use legaia_engine_menus::field_menu::{FieldMenuPhase, FieldMenuRow};
    let worn = |s: &NativeSession| -> Vec<[u8; 8]> {
        let w = &s.host.world;
        w.present_party_list()
            .iter()
            .filter_map(|&m| w.party.roster.members.get(usize::from(m)))
            .map(|m| m.equipment().slots)
            .collect()
    };
    let holds_gear = |s: &NativeSession| {
        let w = &s.host.world;
        w.party
            .inventory
            .iter()
            .any(|(&id, &qty)| qty > 0 && w.tables.equipment_table.get(id).is_some())
    };
    if !walking(session) || !released(session) || !holds_gear(session) {
        return 0;
    }
    let before = worn(session);
    let equip_row = FieldMenuRow::Equip.index();
    let list = session.host.world.present_party_list();
    let pos = |s: u8| list.iter().position(|&x| x == s).unwrap_or(0);
    for &member in &list {
        open_pause_menu(session);
        if session.field_menu.is_none() {
            break;
        }
        let mut pressed = false;
        for _ in 0..200 {
            let Some(menu) = session.field_menu.as_ref() else {
                break;
            };
            let pad = match session.field_menu_sub.as_ref() {
                None if pressed => PadButton::Circle.mask(),
                None => match menu.phase() {
                    FieldMenuPhase::Browsing { cursor } if cursor < equip_row => {
                        PadButton::Down.mask()
                    }
                    FieldMenuPhase::Browsing { cursor } if cursor > equip_row => {
                        PadButton::Up.mask()
                    }
                    FieldMenuPhase::Browsing { .. } => PadButton::Cross.mask(),
                    _ => 0,
                },
                Some(FieldMenuSubsession::Equip { .. }) if pressed => PadButton::Circle.mask(),
                Some(FieldMenuSubsession::Equip {
                    session: es,
                    char_slot,
                    picking,
                }) => {
                    if *picking {
                        match pos(*char_slot).cmp(&pos(member)) {
                            std::cmp::Ordering::Less => PadButton::Down.mask(),
                            std::cmp::Ordering::Greater => PadButton::Up.mask(),
                            std::cmp::Ordering::Equal => PadButton::Cross.mask(),
                        }
                    } else if *char_slot != member {
                        PadButton::Circle.mask()
                    } else {
                        match es.state() {
                            EquipState::SlotPicker { cursor } if cursor > SLOT_BROWSE_BEST_ROW => {
                                PadButton::Up.mask()
                            }
                            EquipState::SlotPicker { .. } => {
                                pressed = true;
                                PadButton::Cross.mask()
                            }
                            EquipState::Done(_) => 0,
                            _ => PadButton::Circle.mask(),
                        }
                    }
                }
                Some(_) => PadButton::Circle.mask(),
            };
            if pad == 0 {
                let _ = session.tick();
                continue;
            }
            tap_pad(session, pad);
        }
        for _ in 0..16 {
            if session.field_menu.is_none() {
                break;
            }
            tap_pad(session, PadButton::Circle.mask());
        }
    }
    let after = worn(session);
    let changed = before.iter().zip(&after).filter(|(a, b)| a != b).count();
    if std::env::var_os("LEGAIA_FGL_TRACE").is_some() {
        eprintln!(
            "    [equip] best equipment: {changed} member(s) changed; worn {after:02X?}; menu {}",
            if session.field_menu.is_some() {
                "STILL OPEN"
            } else {
                "closed"
            }
        );
    }
    if continuous::PLAYS_ON.with(std::cell::Cell::get) {
        pad_equip_specialty(session);
        pad_equip_goods(session);
    }
    changed
}

/// The record byte (`+0x196 + i`) that holds a member's weapon: the retail
/// equip bytes put Noa's weapon where Vahn and Gala keep their Ra-Seru
/// (`RETAIL_WEAPON_EQUIP_BYTE`).
fn weapon_byte(member: u8) -> usize {
    legaia_engine_menus::equip_session::record_byte_for_engine_slot(
        legaia_engine_menus::equipment::EquipSlot::Weapon.as_index(),
        member,
    )
}

/// The weapon a member wears now.
pub(super) fn worn_weapon(session: &NativeSession, member: u8) -> u8 {
    session
        .host
        .world
        .party
        .roster
        .members
        .get(usize::from(member))
        .map_or(0, |r| r.equipment().slots[weapon_byte(member)])
}

/// The dearest arts command a member pays wearing `weapon` with the rest of
/// what it wears now, read off its player battle file (extraction
/// `863 + member`) the way the battle loader does (`FUN_800557B8`,
/// `docs/subsystems/arts-command-gauge.md`): `0x1E` for a favoured weapon,
/// `0x2A` / `0x36` off class.
pub(super) fn arm_cost(session: &NativeSession, member: u8, weapon: u8) -> Option<u8> {
    if member > 2 {
        return None;
    }
    let mut s = session
        .host
        .world
        .party
        .roster
        .members
        .get(usize::from(member))?
        .equipment()
        .slots;
    s[weapon_byte(member)] = weapon;
    let bytes = session
        .host
        .index
        .entry_bytes(863 + u32::from(member))
        .ok()?;
    let pack = legaia_actor_formats::battle_data_pack::detect(&bytes)?;
    let costs = legaia_actor_formats::battle_char_assembly::swing_command_costs(
        &bytes,
        &pack,
        &[s[0], s[1], s[2], s[3], s[4]],
    )
    .ok()?;
    costs.iter().flatten().copied().max()
}

thread_local! {
    /// The static equipment table (`DAT_80074F68`), read once from the
    /// executable the run is pointed at.
    static EQUIP_STATS: Option<legaia_game_tables::equip_stats::EquipStatTable> =
        std::env::var_os("LEGAIA_SCUS")
            .and_then(|p| std::fs::read(p).ok())
            .and_then(|b| legaia_game_tables::equip_stats::EquipStatTable::from_scus(&b));
}

/// The weapon in the bag a member swings cheapest, stats breaking ties, when
/// it swings cheaper than the one worn.
fn specialty_pick(session: &NativeSession, member: u8) -> Option<u8> {
    use legaia_game_tables::equip_stats::EquipSlot as Cat;
    let worn = worn_weapon(session, member);
    if worn == 0 {
        return None;
    }
    let worn_cost = arm_cost(session, member, worn)?;
    if u16::from(worn_cost) <= legaia_engine_battle::arts_command_input::FAVORED_COST {
        return None;
    }
    let w = &session.host.world;
    EQUIP_STATS.with(|t| {
        let t = t.as_ref()?;
        w.party
            .inventory
            .iter()
            .filter(|&(&id, &n)| {
                n > 0
                    && id != worn
                    && t.bonus(id)
                        .is_some_and(|b| b.slot() == Cat::Weapon && b.equips_party_slot(member))
            })
            .filter_map(|(&id, _)| {
                let c = arm_cost(session, member, id)?;
                let atk = t.bonus(id).map_or(0, |b| i32::from(b.attack()));
                (c < worn_cost).then_some((c, -atk, id))
            })
            .min()
            .map(|(_, _, id)| id)
    })
}

/// Put a favoured weapon back on a member that Best Equipment armed off
/// class. Retail's Best Equipment (`FUN_801CF760`) ranks a weapon by its
/// stats alone, and a weapon outside the wearer's class raises the arm
/// command's AP cost (`0x2A`, or `0x36` far off class): Vahn in the Astral
/// Sword pays 54 for a swing, so his nine-command art never fits a turn
/// and Koru (`F32`, monster 182) outlasts him. A player swaps the Beast
/// Buster back in. Start, Equip, the member, the weapon row, the cheapest
/// weapon to swing in the bag, best stats among equals, Yes. Returns how
/// many members it re-armed.
pub(super) fn pad_equip_specialty(session: &mut NativeSession) -> usize {
    use legaia_engine_menu_runtime::field_menu_dispatch::FieldMenuSubsession;
    use legaia_engine_menus::equip_session::{EquipState, browse_row_for_slot};
    use legaia_engine_menus::equipment::EquipSlot;
    use legaia_engine_menus::field_menu::{FieldMenuPhase, FieldMenuRow};
    if !walking(session) || !released(session) {
        return 0;
    }
    let weapon_slot = EquipSlot::Weapon.as_index();
    // Members with a cheaper swing in the bag, and the weapon each takes.
    let picks: Vec<(u8, u8)> = session
        .host
        .world
        .present_party_list()
        .into_iter()
        .filter_map(|m| specialty_pick(session, m).map(|id| (m, id)))
        .collect();
    if picks.is_empty() {
        return 0;
    }
    let off_class: Vec<u8> = picks.iter().map(|&(m, _)| m).collect();
    let before: Vec<u8> = off_class.iter().map(|&m| worn_weapon(session, m)).collect();
    open_pause_menu(session);
    if session.field_menu.is_none() {
        return 0;
    }
    let equip_row = FieldMenuRow::Equip.index();
    let list = session.host.world.present_party_list();
    let pos = |s: u8| list.iter().position(|&x| x == s).unwrap_or(0);
    let mut done: HashSet<u8> = HashSet::new();
    let mut aim: Option<u8> = None;
    for _ in 0..400 {
        let Some(menu) = session.field_menu.as_ref() else {
            break;
        };
        let target = off_class.iter().copied().find(|m| !done.contains(m));
        let pad = match (session.field_menu_sub.as_ref(), target) {
            (None, None) => PadButton::Circle.mask(),
            (None, Some(_)) => match menu.phase() {
                FieldMenuPhase::Browsing { cursor } if cursor < equip_row => PadButton::Down.mask(),
                FieldMenuPhase::Browsing { cursor } if cursor > equip_row => PadButton::Up.mask(),
                FieldMenuPhase::Browsing { .. } => PadButton::Cross.mask(),
                _ => 0,
            },
            (
                Some(FieldMenuSubsession::Equip {
                    session: es,
                    char_slot,
                    picking,
                }),
                Some(t),
            ) => {
                if *picking {
                    match pos(*char_slot).cmp(&pos(t)) {
                        std::cmp::Ordering::Less => PadButton::Down.mask(),
                        std::cmp::Ordering::Greater => PadButton::Up.mask(),
                        std::cmp::Ordering::Equal => PadButton::Cross.mask(),
                    }
                } else if *char_slot != t {
                    PadButton::Circle.mask()
                } else {
                    match es.state() {
                        EquipState::SlotPicker { cursor } => {
                            aim = None;
                            if worn_weapon(session, t)
                                != before[off_class.iter().position(|&m| m == t).unwrap_or(0)]
                            {
                                done.insert(t);
                                PadButton::Circle.mask()
                            } else {
                                match cursor.cmp(&browse_row_for_slot(weapon_slot)) {
                                    std::cmp::Ordering::Less => PadButton::Down.mask(),
                                    std::cmp::Ordering::Greater => PadButton::Up.mask(),
                                    std::cmp::Ordering::Equal => PadButton::Cross.mask(),
                                }
                            }
                        }
                        EquipState::ItemPicker { slot, cursor } if slot == weapon_slot => {
                            let rows = es.items_for_slot(slot);
                            let best = picks.iter().find(|&&(m, _)| m == t).map(|&(_, id)| id);
                            aim = best;
                            match best.and_then(|id| rows.iter().position(|r| r.id == id)) {
                                Some(k) => match usize::from(cursor).cmp(&k) {
                                    std::cmp::Ordering::Less => PadButton::Down.mask(),
                                    std::cmp::Ordering::Greater => PadButton::Up.mask(),
                                    std::cmp::Ordering::Equal => PadButton::Cross.mask(),
                                },
                                None => {
                                    done.insert(t);
                                    PadButton::Circle.mask()
                                }
                            }
                        }
                        EquipState::Confirm {
                            item_id, cursor, ..
                        } if Some(item_id) == aim => {
                            if cursor == 0 {
                                PadButton::Cross.mask()
                            } else {
                                PadButton::Left.mask()
                            }
                        }
                        EquipState::Done(_) => 0,
                        _ => {
                            done.insert(t);
                            PadButton::Circle.mask()
                        }
                    }
                }
            }
            (Some(_), _) => PadButton::Circle.mask(),
        };
        if pad == 0 {
            let _ = session.tick();
            continue;
        }
        tap_pad(session, pad);
    }
    for _ in 0..16 {
        if session.field_menu.is_none() {
            break;
        }
        tap_pad(session, PadButton::Circle.mask());
    }
    let rearmed = off_class
        .iter()
        .zip(&before)
        .filter(|&(&m, &b)| worn_weapon(session, m) != b)
        .count();
    if std::env::var_os("LEGAIA_FGL_TRACE").is_some() {
        let now: Vec<u8> = off_class.iter().map(|&m| worn_weapon(session, m)).collect();
        eprintln!(
            "    [equip] specialty: members {off_class:?} off class in {before:02X?}; now {now:02X?}; menu {}",
            if session.field_menu.is_some() {
                "STILL OPEN"
            } else {
                "closed"
            }
        );
    }
    rearmed
}

/// How much the continuous hand wants an accessory passive (the ability-bit
/// index of `w.tables.accessory_passives`) on a member: the fight-winning
/// ones first, then the guards and the rest. `0` for one it never wears -
/// Rage takes the member's commands away, Zero Defense and Last Attack
/// cost more than they give, No Escape keeps the party in a fight it is
/// running from - and for the passives that only pay on the road.
fn goods_score(passive: u8) -> u32 {
    match passive {
        0x01 => 100,                     // HP Boost 2
        0x0D => 95,                      // Attack x2
        0x06 => 90,                      // ATK Boost
        0x2C => 85,                      // Arts Power
        0x09 => 80,                      // UDF & LDF Boost
        0x00 => 75,                      // HP Boost 1
        0x2A | 0x29 => 70,               // Maximum AP, AP Boost 2
        0x0A | 0x11 => 65,               // SPD Boost, First Attack
        0x25 | 0x27 => 60,               // HP After, Final Heal
        0x07 | 0x08 => 55,               // UDF / LDF Boost
        0x0E | 0x0F | 0x13 => 50,        // Guard Attack, Counterattack, Bull's Eye
        0x0B | 0x05 | 0x04 => 45,        // INT Boost, MP Used Down
        0x28 | 0x2B | 0x14 => 40,        // AP Boost 1, AP Used Down, Shield Boost
        0x1C | 0x24 => 35,               // Master Guard, All Guard
        0x03 | 0x02 | 0x26 => 30,        // MP Boost, MP After
        0x0C => 25,                      // AGL Boost
        0x16..=0x1B | 0x1D..=0x23 => 20, // single status and element guards
        0x2F | 0x31 | 0x33 | 0x35 => 10,
        _ => 0,
    }
}

/// The Goods slots of the equipment record (engine slots 5..=7).
const GOODS_SLOTS: [u8; 3] = [5, 6, 7];

fn goods_value(w: &legaia_engine_core::world::World, id: u8) -> u32 {
    w.tables
        .accessory_passives
        .passive_index(id)
        .map_or(0, goods_score)
}

/// The next (member, Goods slot) [`pad_equip_goods`] fills: an empty slot
/// of a present member, while the bag holds an accessory worth wearing,
/// past the ones whose candidate list showed nothing wanted.
fn next_goods_slot(
    w: &legaia_engine_core::world::World,
    skipped: &HashSet<(u8, u8)>,
) -> Option<(u8, u8)> {
    if !w
        .party
        .inventory
        .iter()
        .any(|(&id, &n)| n > 0 && goods_value(w, id) > 0)
    {
        return None;
    }
    w.present_party_list().into_iter().find_map(|m| {
        let slots = w
            .party
            .roster
            .members
            .get(usize::from(m))?
            .equipment()
            .slots;
        GOODS_SLOTS
            .iter()
            .find(|&&s| slots[usize::from(s)] == 0 && !skipped.contains(&(m, s)))
            .map(|&s| (m, s))
    })
}

/// The passives member `m` already wears in its Goods slots.
fn worn_passives(w: &legaia_engine_core::world::World, m: u8) -> Vec<u8> {
    w.party
        .roster
        .members
        .get(usize::from(m))
        .map(|r| {
            GOODS_SLOTS
                .iter()
                .map(|&g| r.equipment().slots[usize::from(g)])
                .filter(|&id| id != 0)
                .filter_map(|id| w.tables.accessory_passives.passive_index(id))
                .collect()
        })
        .unwrap_or_default()
}

/// Fill each present member's empty Goods slots from the bag, with the pad,
/// as a player does: Best Equipment (retail's `FUN_801CF760`) ranks only the
/// four armament slots, so an accessory a chest or a counter gave the run
/// stays in the bag until it is put on by hand. Start, Equip, the member,
/// the Goods row, the accessory, Yes. A slot already worn is left alone, and
/// a member never wears two of one passive. Returns how many it put on.
pub(super) fn pad_equip_goods(session: &mut NativeSession) -> usize {
    use legaia_engine_menu_runtime::field_menu_dispatch::FieldMenuSubsession;
    use legaia_engine_menus::equip_session::{EquipState, browse_row_for_slot};
    use legaia_engine_menus::field_menu::{FieldMenuPhase, FieldMenuRow};
    let mut skipped: HashSet<(u8, u8)> = HashSet::new();
    if !walking(session)
        || !released(session)
        || next_goods_slot(&session.host.world, &skipped).is_none()
    {
        return 0;
    }
    let count_worn = |w: &legaia_engine_core::world::World| -> usize {
        w.present_party_list()
            .iter()
            .filter_map(|&m| w.party.roster.members.get(usize::from(m)))
            .map(|r| {
                GOODS_SLOTS
                    .iter()
                    .filter(|&&s| r.equipment().slots[usize::from(s)] != 0)
                    .count()
            })
            .sum()
    };
    let before = count_worn(&session.host.world);
    open_pause_menu(session);
    if session.field_menu.is_none() {
        return 0;
    }
    let equip_row = FieldMenuRow::Equip.index();
    let list = session.host.world.present_party_list();
    let pos = |s: u8| list.iter().position(|&x| x == s).unwrap_or(0);
    // The item the open candidate list is aiming at, once chosen.
    let mut aim: Option<u8> = None;
    for _ in 0..600 {
        let Some(menu) = session.field_menu.as_ref() else {
            break;
        };
        let target = next_goods_slot(&session.host.world, &skipped);
        let pad = match (session.field_menu_sub.as_ref(), target) {
            (None, None) => PadButton::Circle.mask(),
            (None, Some(_)) => match menu.phase() {
                FieldMenuPhase::Browsing { cursor } if cursor < equip_row => PadButton::Down.mask(),
                FieldMenuPhase::Browsing { cursor } if cursor > equip_row => PadButton::Up.mask(),
                FieldMenuPhase::Browsing { .. } => PadButton::Cross.mask(),
                _ => 0,
            },
            (
                Some(FieldMenuSubsession::Equip {
                    session: es,
                    char_slot,
                    picking,
                }),
                Some((t, slot)),
            ) => {
                if *picking {
                    match pos(*char_slot).cmp(&pos(t)) {
                        std::cmp::Ordering::Less => PadButton::Down.mask(),
                        std::cmp::Ordering::Greater => PadButton::Up.mask(),
                        std::cmp::Ordering::Equal => PadButton::Cross.mask(),
                    }
                } else if *char_slot != t {
                    PadButton::Circle.mask()
                } else {
                    match es.state() {
                        EquipState::SlotPicker { cursor } => {
                            aim = None;
                            match cursor.cmp(&browse_row_for_slot(slot)) {
                                std::cmp::Ordering::Less => PadButton::Down.mask(),
                                std::cmp::Ordering::Greater => PadButton::Up.mask(),
                                std::cmp::Ordering::Equal => PadButton::Cross.mask(),
                            }
                        }
                        EquipState::ItemPicker { slot: s, cursor } if s == slot => {
                            let w = &session.host.world;
                            let worn = worn_passives(w, t);
                            let rows = es.items_for_slot(slot);
                            let best = aim.or_else(|| {
                                rows.iter()
                                    .filter(|r| !r.equipped && r.owned && goods_value(w, r.id) > 0)
                                    .filter(|r| {
                                        w.tables
                                            .accessory_passives
                                            .passive_index(r.id)
                                            .is_none_or(|p| !worn.contains(&p))
                                    })
                                    .max_by_key(|r| (goods_value(w, r.id), std::cmp::Reverse(r.id)))
                                    .map(|r| r.id)
                            });
                            aim = best;
                            let row = best
                                .and_then(|id| rows.iter().position(|r| r.id == id && !r.equipped));
                            match row {
                                Some(k) => match usize::from(cursor).cmp(&k) {
                                    std::cmp::Ordering::Less => PadButton::Down.mask(),
                                    std::cmp::Ordering::Greater => PadButton::Up.mask(),
                                    std::cmp::Ordering::Equal => PadButton::Cross.mask(),
                                },
                                None => {
                                    skipped.insert((t, slot));
                                    PadButton::Circle.mask()
                                }
                            }
                        }
                        EquipState::Confirm {
                            item_id, cursor, ..
                        } if Some(item_id) == aim => {
                            if cursor == 0 {
                                PadButton::Cross.mask()
                            } else {
                                PadButton::Left.mask()
                            }
                        }
                        EquipState::Done(_) => 0,
                        _ => {
                            // Whatever else opened is backed out of, and the
                            // slot it was for is not asked about again.
                            skipped.insert((t, slot));
                            PadButton::Circle.mask()
                        }
                    }
                }
            }
            (Some(_), _) => PadButton::Circle.mask(),
        };
        if pad == 0 {
            let _ = session.tick();
            continue;
        }
        tap_pad(session, pad);
    }
    for _ in 0..16 {
        if session.field_menu.is_none() {
            break;
        }
        tap_pad(session, PadButton::Circle.mask());
    }
    let after = count_worn(&session.host.world);
    let put_on = after.saturating_sub(before);
    if std::env::var_os("LEGAIA_FGL_TRACE").is_some() {
        let w = &session.host.world;
        let worn: Vec<[u8; 3]> = w
            .present_party_list()
            .iter()
            .filter_map(|&m| w.party.roster.members.get(usize::from(m)))
            .map(|r| {
                let e = r.equipment().slots;
                [e[5], e[6], e[7]]
            })
            .collect();
        eprintln!(
            "    [equip] goods: {put_on} put on; worn {worn:02X?}; menu {}",
            if session.field_menu.is_some() {
                "STILL OPEN"
            } else {
                "closed"
            }
        );
    }
    put_on
}

/// Walk into a fight ready, as the continuous hand does: restoratives up
/// to nine tenths, and where the bag cannot do it and the scene has a rest,
/// the rest. The segmented passes heal with [`pad_field_heal`] alone.
pub(super) fn pad_ready(session: &mut NativeSession) -> Result<Option<String>, String> {
    if !continuous::PLAYS_ON.with(std::cell::Cell::get) {
        pad_field_heal(session, 900);
        return Ok(None);
    }
    struct Door(bool);
    impl Drop for Door {
        fn drop(&mut self) {
            AT_BOSS_DOOR.with(|d| d.set(self.0));
        }
    }
    let _door = Door(AT_BOSS_DOOR.with(|d| d.replace(true)));
    // What the last chest held goes on first.
    pad_equip_best(session);
    // A party that spent its MP on the way tops it up from the bag: the
    // casts are half of what a boss fight is won with.
    if party_mp_permille(session) < 600 {
        pad_field_restore(session, 900, true);
    }
    // A rest close by is free; the bag is for the fight, and is opened
    // before it only by a party under three fifths.
    if party_hp_permille(session) < 900
        && rest_close_by(session)
        && let Some(s) = pad_rest_to(session, 900, true)?
    {
        return Ok(Some(s));
    }
    if party_hp_permille(session) < 600 {
        pad_field_heal(session, 900);
    }
    if party_hp_permille(session) >= 700 {
        return Ok(None);
    }
    pad_rest_to(session, 900, true)
}

/// Burn an Incense with the pad, as a player does who is too hurt to survive
/// the next encounter and has nothing to heal with: when the weakest member
/// is still below `threshold` per-mille after [`pad_field_heal`], the scene
/// rolls encounters, the bag holds an Incense (`0x8A`) and its window
/// (`_DAT_8007B600`) has run out, Start opens the pause menu and Items, Use,
/// the Incense row and the confirm's Yes commit one. Retail's class-`0x82`
/// applier skips the whole region roll while the window is open, so one use
/// buys `0x40` walk-regen ticks (`0x800` walking vsyncs) with no encounter at
/// all, whatever the rand stream deals (see `field-menu.md`, the Incense
/// route `FUN_801D8D94`). Returns whether one was used.
pub(super) fn pad_field_repel(session: &mut NativeSession, threshold: u32) -> bool {
    use legaia_engine_menu_runtime::field_menu_dispatch::FieldMenuSubsession;
    use legaia_engine_menus::field_menu::FieldMenuRow;
    use legaia_engine_menus::inventory_use::InventoryUseState;
    use legaia_engine_menus::pause_screens::{INCENSE_ITEM_ID, PauseItemsFocus};
    let w = &session.host.world;
    if !walking(session)
        || !released(session)
        || party_hp_permille(session) >= threshold
        || !w.scene_can_roll_encounters()
        || w.locomotion.walk_regen_window != 0
        || !w
            .party
            .inventory
            .iter()
            .any(|(&id, &c)| id == INCENSE_ITEM_ID && c > 0)
    {
        return false;
    }
    open_pause_menu(session);
    if session.field_menu.is_none() {
        return false;
    }
    let items_row = FieldMenuRow::Items.index();
    let mut used = false;
    for _ in 0..200 {
        let Some(menu) = session.field_menu.as_ref() else {
            break;
        };
        let pad = match session.field_menu_sub.as_ref() {
            None => match menu.phase() {
                legaia_engine_menus::field_menu::FieldMenuPhase::Browsing { cursor } => {
                    if used {
                        PadButton::Circle.mask()
                    } else if cursor != items_row {
                        PadButton::Down.mask()
                    } else {
                        PadButton::Cross.mask()
                    }
                }
                _ => 0,
            },
            Some(FieldMenuSubsession::Items(p)) => {
                // The hand walks every bag row; an Incense row routes to its
                // own confirm, not through the usable-in-context filter.
                let want = p.rows.iter().position(|r| r.id == INCENSE_ITEM_ID);
                if p.focus == PauseItemsFocus::SpecialRoute {
                    // The confirm opens with its cursor on Yes.
                    used = true;
                    PadButton::Cross.mask()
                } else {
                    match (p.focus, &p.inner.state, want) {
                        _ if used => PadButton::Circle.mask(),
                        // The Use list's filter is built on entering it.
                        (PauseItemsFocus::Command, _, _) => {
                            if p.command_cursor == 0 {
                                PadButton::Cross.mask()
                            } else {
                                PadButton::Up.mask()
                            }
                        }
                        (PauseItemsFocus::List, InventoryUseState::Browsing { .. }, Some(k)) => {
                            list_step_toward(p.list_cursor(), k)
                        }
                        _ => PadButton::Circle.mask(),
                    }
                }
            }
            Some(_) => PadButton::Circle.mask(),
        };
        if pad == 0 {
            let _ = session.tick();
            continue;
        }
        tap_pad(session, pad);
    }
    for _ in 0..16 {
        if session.field_menu.is_none() {
            break;
        }
        tap_pad(session, PadButton::Circle.mask());
    }
    let window = session.host.world.locomotion.walk_regen_window;
    if std::env::var_os("LEGAIA_FGL_TRACE").is_some() {
        eprintln!(
            "    [repel] incense committed {used}; window now {window}; menu {}",
            if session.field_menu.is_some() {
                "STILL OPEN"
            } else {
                "closed"
            }
        );
    }
    used && window > 0
}

/// `(scene, door tile)` -> the flags the band refused under.
pub(super) type RefusedDoors = HashMap<(String, (i16, i16)), BTreeSet<u16>>;

thread_local! {
    /// Door bands a pad walk was turned back from this segment - a script
    /// on the approach fired and fired and changed nothing (`kor3`'s warp
    /// pad rooms: P0[3] / P0[5] answer "The power is out." while `0x403` is
    /// clear) - each with the flags it refused under. [`pad_hop`] tries the
    /// scene's other doors toward the same destination before it gives the
    /// hop up; new flags re-open the band.
    pub(super) static REFUSED_DOORS: std::cell::RefCell<RefusedDoors> =
        std::cell::RefCell::new(HashMap::new());
    /// The scene and door band [`pad_hop`] last walked to: a crossing that
    /// brought the player back to the side it left refuses that band, so the
    /// next round trip leaves by another door into the same scene.
    pub(super) static LAST_HOP_DOOR: std::cell::RefCell<Option<(String, (i16, i16))>> =
        const { std::cell::RefCell::new(None) };
}

thread_local! {
    /// Scenes the pad hand has tried to rest in this segment: a rest that
    /// did not lift the party is not walked again on the next hop.
    pub(super) static RESTED: std::cell::RefCell<BTreeSet<String>> =
        const { std::cell::RefCell::new(BTreeSet::new()) };
    /// Rests taken with the party under the line that lifted nobody, keyed
    /// by scene, kind (`0` a bed's band, `1` a keeper), record and the flags
    /// that record reads ([`rest_flags_key`]). A keeper's record
    /// carries the restore on one branch of its story: `cave01`'s Terra
    /// (P1[2]) restores on the `0x15C` stage and, once `0x15E` is up, only
    /// sends the party on to Mt. Rikuroa. Talking to her again under the
    /// same flags restores nothing, so she is not a rest until one moves.
    pub(super) static DRY_RESTS: std::cell::RefCell<BTreeSet<(String, u8, u8, u64)>> =
        const { std::cell::RefCell::new(BTreeSet::new()) };
}

/// What a rest's record branches on, as a [`DRY_RESTS`] key: the live state
/// of every system flag the record tests. A keeper that stopped restoring
/// may restore again once one of those moves; a room marker or a latch the
/// record never reads does not change its answer.
pub(super) fn rest_flags_key(
    session: &NativeSession,
    mf: &legaia_scene_formats::man_section::ManFile,
    man: &[u8],
    part: usize,
    rec: usize,
) -> u64 {
    use legaia_engine_field::man_field_scripts::{FlagBank, walk_partition_gflag_sites};
    use legaia_scene_formats::field_disasm::FlagKind;
    use std::hash::{Hash, Hasher};
    let w = &session.host.world;
    let tested: BTreeSet<u16> = walk_partition_gflag_sites(mf, man, part)
        .into_iter()
        .filter(|s| {
            s.clean && s.bank == FlagBank::System && s.kind == FlagKind::Test && s.record == rec
        })
        .map(|s| s.flag)
        .collect();
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for f in tested {
        (f, w.system_flag_test(f)).hash(&mut h);
    }
    h.finish()
}

/// Has the rest `(kind, rec)` of the loaded scene lifted nobody under the
/// flags its record reads ([`DRY_RESTS`])? `kind` `0` is a bed's
/// partition-2 band, `1` a partition-1 keeper.
fn rest_is_dry(
    session: &NativeSession,
    mf: &legaia_scene_formats::man_section::ManFile,
    man: &[u8],
    kind: u8,
    rec: u8,
) -> bool {
    let name = scene_name(session);
    let part = if kind == 0 { 2 } else { 1 };
    let key = rest_flags_key(session, mf, man, part, usize::from(rec));
    DRY_RESTS.with(|d| d.borrow().contains(&(name, kind, rec, key)))
}

/// Does the record body carry the restore - op `4C 82 <slot>`, the
/// per-party-slot HP/MP refill (`FieldPartyInventoryHost::restore_party_slot`)
/// every bed and inn stay ends in - at a clean decode boundary?
pub(super) fn record_restores(
    mf: &legaia_scene_formats::man_section::ManFile,
    man: &[u8],
    part: usize,
    rec: usize,
) -> bool {
    use legaia_engine_field::man_field_scripts::partition_record_span;
    use legaia_scene_formats::field_disasm::{OpKey, clean_hit_offsets};
    let Some((start, pc0, len)) = partition_record_span(mf, man, part, rec) else {
        return false;
    };
    let key = OpKey {
        opcode: 0x4C,
        sub: Some(0x82),
    };
    !clean_hit_offsets(&man[start..start + len], pc0, key).is_empty()
}

/// Rest before setting out, as a player with an empty bag does: when the
/// weakest member is still below half HP after [`pad_field_heal`], walk to
/// the scene's rest - a bed's walk-on band, or an NPC whose talk (or a
/// record it spawns) runs the `4C 82` restore - and take it. An inn's gold
/// gate is the record's own: a stay the purse cannot pay refuses itself.
/// `Ok(Some(scene))` when the rest's script left the scene.
///
/// Only where the walk to it is safe: in a scene that rolls encounters the
/// way to the bed is as dangerous as the way out. `dolk`'s mist-era bed
/// (P2[9], behind the inn's stair door) sits across a castle whose every
/// region rolls; a lone member at 17/219 walking to it wiped on 14 of 21
/// dealt streams (`LEGAIA_FGL_RNG_SEED`), against 3 of 21 walking straight
/// out to `map01`.
pub(super) fn pad_rest(session: &mut NativeSession) -> Result<Option<String>, String> {
    // The continuous hand keeps the party it has to finish the game with:
    // it rests whenever a member is under four fifths, saves its items for
    // the road, and takes a rest a rolling scene offers (`cave01`'s Terra).
    let plays_on = continuous::PLAYS_ON.with(std::cell::Cell::get);
    if !plays_on {
        return pad_rest_to(session, 500, false);
    }
    if !session.host.world.scene_can_roll_encounters() {
        return pad_rest_to(session, 800, true);
    }
    // Where encounters roll, the walk to a bed costs what the walk out
    // does: only a rest close by is worth it, and only to a party under
    // half.
    if !rest_close_by(session) {
        return Ok(None);
    }
    pad_rest_to(session, 500, true)
}

/// Is one of the scene's rests within [`REST_REACH`] tiles of the player?
pub(super) fn rest_close_by(session: &NativeSession) -> bool {
    let (px, pz) = player_xz(session);
    let me = dispatch_tile(px, pz);
    let (beds, keepers) = rest_sites(session);
    let w = &session.host.world;
    let near = |t: (i32, i32)| (t.0 - me.0).abs() + (t.1 - me.1).abs() <= REST_REACH;
    beds.values()
        .any(|&(x, z)| near((i32::from(x), i32::from(z))))
        || keepers.iter().any(|s| {
            w.npcs
                .positions
                .get(s)
                .is_some_and(|&(x, z)| near(dispatch_tile(x, z)))
        })
}

/// Tiles (Manhattan) the continuous hand walks to a rest in a scene that
/// rolls encounters.
const REST_REACH: i32 = 32;

/// The rests the loaded scene offers under the live flags: the first tile
/// of each walk-on band whose partition-2 record runs the restore, and the
/// talk NPCs whose record (or a record it spawns) does. A rest already
/// found to lift nobody under the flags its record reads ([`DRY_RESTS`]) is
/// still listed: it is where the scene's rest is, and whether it works is
/// [`rest_works_here`]'s question.
pub(super) fn rest_sites(session: &NativeSession) -> (BTreeMap<u8, (u8, u8)>, Vec<u8>) {
    rest_sites_live(session, false)
}

/// Does one of the loaded scene's rests still lift the party under the
/// flags its record reads - one not in [`DRY_RESTS`]?
pub(super) fn rest_works_here(session: &NativeSession) -> bool {
    let (beds, keepers) = rest_sites_live(session, true);
    !beds.is_empty() || !keepers.is_empty()
}

fn rest_sites_live(session: &NativeSession, live: bool) -> (BTreeMap<u8, (u8, u8)>, Vec<u8>) {
    use legaia_engine_field::man_field_scripts::partition2_record_gates;
    let Some((mf, man, triggers)) = scene_man_and_triggers(session) else {
        return (BTreeMap::new(), Vec::new());
    };
    let doors: BTreeSet<u8> = overworld_portal_sites(&mf, &man, &triggers)
        .iter()
        .map(|s| s.record)
        .collect();
    let mut beds: BTreeMap<u8, (u8, u8)> = BTreeMap::new();
    for t in triggers.iter().filter(|t| t.gate == 1) {
        let owner = triggers
            .iter()
            .find(|u| (u.tile_x, u.tile_z) == (t.tile_x, t.tile_z));
        if owner.is_none_or(|u| u.record != t.record || u.gate != 1) || doors.contains(&t.record) {
            continue;
        }
        let rec = usize::from(t.record);
        let open = partition2_record_gates(&mf, &man, rec)
            .is_some_and(|(c1, c2)| session.host.world.p2_record_gates_pass(&c1, &c2));
        if open
            && record_restores(&mf, &man, 2, rec)
            && !(live && rest_is_dry(session, &mf, &man, 0, t.record))
        {
            beds.entry(t.record).or_insert((t.tile_x, t.tile_z));
        }
    }
    let n0 = mf.partitions.first().map_or(0, Vec::len);
    let n1 = mf.partitions.get(1).map_or(0, Vec::len);
    let w = &session.host.world;
    let keepers: Vec<u8> = w
        .npcs
        .positions
        .keys()
        .copied()
        .filter(|s| w.npcs.dialog.contains_key(s) || w.npcs.dialog_prologue.contains_key(s))
        .filter(|&s| !(live && rest_is_dry(session, &mf, &man, 1, s)))
        // A record that stages a fight restores the party after it
        // (`cave01` P1[3..7], Terra's sparring monsters): not a rest.
        .filter(|&s| !record_stages_fight(&mf, &man, 1, usize::from(s)))
        .filter(|&s| {
            record_restores(&mf, &man, 1, usize::from(s))
                || spawned_p2(&mf, &man, 1, usize::from(s), n0 + n1, 3)
                    .iter()
                    .any(|&r| record_restores(&mf, &man, 2, r))
        })
        .collect();
    let mut keepers = keepers;
    keepers.sort_unstable();
    (beds, keepers)
}

/// What a rest restores, read as the continuous hand reads it: the lower
/// of the party's HP and MP per-mille. Its spells are half of what the
/// fights are healed with (a run that reached Nivora's duels with Gala at
/// 54 of 346 MP lost Che's on every stream), so a rest that brings the MP
/// back is a rest. The segmented passes read HP alone.
fn rest_gauge(session: &NativeSession) -> u32 {
    let hp = party_hp_permille(session);
    if continuous::PLAYS_ON.with(std::cell::Cell::get) {
        hp.min(party_mp_permille(session))
    } else {
        hp
    }
}

/// [`pad_rest`] with its line: rest when the weakest member is below `line`
/// per-mille. `anywhere` takes the rest of a scene that rolls encounters
/// too, and leaves the bag alone.
pub(super) fn pad_rest_to(
    session: &mut NativeSession,
    line: u32,
    anywhere: bool,
) -> Result<Option<String>, String> {
    use legaia_engine_field::man_field_scripts::partition2_record_gates;
    if session.host.world.mode != SceneMode::Field
        || (!anywhere && session.host.world.scene_can_roll_encounters())
        || !walking(session)
        || !released(session)
        || rest_gauge(session) >= line
    {
        return Ok(None);
    }
    if !anywhere {
        pad_field_heal(session, 500);
        if rest_gauge(session) >= 500 {
            return Ok(None);
        }
    }
    let name = scene_name(session);
    if !RESTED.with(|r| r.borrow_mut().insert(name.clone())) {
        return Ok(None);
    }
    let Some((mf, man, triggers)) = scene_man_and_triggers(session) else {
        return Ok(None);
    };
    let trace = std::env::var_os("LEGAIA_FGL_TRACE").is_some();
    let doors: BTreeSet<u8> = overworld_portal_sites(&mf, &man, &triggers)
        .iter()
        .map(|s| s.record)
        .collect();
    // Walk-on rests: the first tile of each gate-1 band whose live
    // partition-2 record restores.
    let mut beds: BTreeMap<u8, (u8, u8)> = BTreeMap::new();
    for t in triggers.iter().filter(|t| t.gate == 1) {
        let owner = triggers
            .iter()
            .find(|u| (u.tile_x, u.tile_z) == (t.tile_x, t.tile_z));
        if owner.is_none_or(|u| u.record != t.record || u.gate != 1) || doors.contains(&t.record) {
            continue;
        }
        let rec = usize::from(t.record);
        let open = partition2_record_gates(&mf, &man, rec)
            .is_some_and(|(c1, c2)| session.host.world.p2_record_gates_pass(&c1, &c2));
        if open
            && record_restores(&mf, &man, 2, rec)
            && !rest_is_dry(session, &mf, &man, 0, t.record)
        {
            beds.entry(t.record).or_insert((t.tile_x, t.tile_z));
        }
    }
    // Talk rests: an innkeeper's record restores itself or spawns one that
    // does.
    let n0 = mf.partitions.first().map_or(0, Vec::len);
    let n1 = mf.partitions.get(1).map_or(0, Vec::len);
    let w = &session.host.world;
    let keepers: Vec<u8> = w
        .npcs
        .positions
        .keys()
        .copied()
        .filter(|s| w.npcs.dialog.contains_key(s) || w.npcs.dialog_prologue.contains_key(s))
        .filter(|&s| !rest_is_dry(session, &mf, &man, 1, s))
        .filter(|&s| !anywhere || !record_stages_fight(&mf, &man, 1, usize::from(s)))
        .filter(|&s| {
            record_restores(&mf, &man, 1, usize::from(s))
                || spawned_p2(&mf, &man, 1, usize::from(s), n0 + n1, 3)
                    .iter()
                    .any(|&r| record_restores(&mf, &man, 2, r))
        })
        .collect();
    // The placements live in a hash map: fix the order the keepers are
    // tried in, so a run does not depend on it.
    let mut keepers = keepers;
    keepers.sort_unstable();
    if trace {
        eprintln!(
            "    [rest] {name}: weakest {} per-mille; beds {beds:?}; keepers {keepers:?}",
            rest_gauge(session)
        );
    }
    let dry = |session: &NativeSession, kind: u8, rec: u8, key: u64, before: u32| {
        // Under the line and lifted nobody: not a rest under these flags.
        if session.host.world.mode == SceneMode::Field
            && scene_name(session) == name
            && rest_gauge(session) <= before
        {
            DRY_RESTS.with(|d| d.borrow_mut().insert((name.clone(), kind, rec, key)));
        }
    };
    for (rec, tile) in beds {
        let before = rest_gauge(session);
        let key = rest_flags_key(session, &mf, &man, 2, usize::from(rec));
        let r = match pad_step_onto(session, tile) {
            Ok(Walk::Entered(s)) => Run::Entered(s),
            Ok(Walk::Arrived) => run_while_moving(session, DEEP_EXIT_TICKS),
            Err(e) => Run::Parked(e),
        };
        if let Some(w) = wiped(session) {
            return Err(w);
        }
        if trace {
            eprintln!(
                "    [rest] {name} bed P2[{rec}] at {tile:?} -> {r:?}; weakest now {} per-mille",
                rest_gauge(session)
            );
        }
        match r {
            Run::Entered(s) => return Ok(Some(s)),
            Run::Battle(b) => return Err(format!("rest P2[{rec}]: {b}")),
            Run::Error(e) => return Err(e),
            Run::Released | Run::Parked(_) => {}
        }
        if rest_gauge(session) >= line {
            // A rest that worked may be taken again after the next fights.
            RESTED.with(|r| r.borrow_mut().remove(&name));
            return Ok(None);
        }
        dry(session, 0, rec, key, before);
    }
    for slot in keepers {
        let before = rest_gauge(session);
        let key = rest_flags_key(session, &mf, &man, 1, usize::from(slot));
        RESTING.with(|r| r.set(anywhere));
        let r = talk_to(session, slot);
        RESTING.with(|r| r.set(false));
        if trace {
            eprintln!(
                "    [rest] {name} talk P1[{slot}] -> {r:?}; weakest now {} per-mille",
                rest_gauge(session)
            );
        }
        match r {
            Run::Entered(s) => return Ok(Some(s)),
            Run::Battle(b) => return Err(format!("rest talk P1[{slot}]: {b}")),
            Run::Error(e) => return Err(e),
            Run::Released | Run::Parked(_) => {}
        }
        if rest_gauge(session) >= line {
            // A rest that worked may be taken again after the next fights.
            RESTED.with(|r| r.borrow_mut().remove(&name));
            return Ok(None);
        }
        dry(session, 1, slot, key, before);
    }
    Ok(None)
}
