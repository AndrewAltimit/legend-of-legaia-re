//! The fighter's turn: the pad for the current battle surface, the battle
//! loop that drives a fight to its end, and the battle diagnostics.

use super::*;

/// The pad mask a player presses this frame in a battle - every one a pad
/// press through the engine's own battle menus, no engine call:
///
/// - a member who is down, or in danger, gets the revive or heal
///   [`wanted_item`] picks, aimed at that member (the ring's up arm);
/// - otherwise a member with an affordable damaging Seru spell casts the
///   strongest one (the right arm);
/// - otherwise it attacks through the `Command` chip, entering the longest
///   art its command pool pays for ([`arts_plan`]);
/// - a random encounter on a pad travel leg is fled (the round prompt's
///   Run) unless the fight forbids running;
/// - message boxes and results screens are paged with Cross.
///
/// A sparring tutorial validates each commit against its lesson, so there
/// the hand keeps to the lesson: the up arm (using the first item) for the
/// Items lesson, the down arm for Spirit, `Auto` for the attack lesson and
/// `Command` with the Somersault for the hyper-arts drill (an Auto attack is
/// that lesson's wrong-lesson rewind).
/// Ticks the fighter asks the command ring for Magic without a spell window
/// opening before it stops asking for this battle (two seconds of presses,
/// well past the ring's own turn). A pick that opens the window resets the
/// count, so this only ever stops a ring that refuses.
const MAGIC_RING_TRIES: u32 = 120;

/// Calls of [`fight_pad`] the hand spends stepping one target cursor toward
/// its aim before it confirms wherever the cursor stands: about a dozen
/// presses, past any ring of five foes.
const AIM_TRIES: u32 = 24;

thread_local! {
    /// [`fight_pad`] calls spent on the open target cursor ([`aim_press`]).
    static AIM_SPENT: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
    /// Over a run: the frames [`aim_press`] asked for a cursor step (half of
    /// them land: the loop alternates each press with neutral), and the confirms
    /// it made on a foe other than the one the cursor opened on.
    pub(super) static AIM_TALLY: std::cell::Cell<(u32, u32)> = const { std::cell::Cell::new((0, 0)) };
    /// Per member: the foe row its last strike confirm stood on, and whether
    /// its commit has been seen in the round's pending list
    /// ([`check_aimed_dispatch`]).
    static AIM_EXPECT: std::cell::Cell<[Option<(u8, bool)>; 3]> = const { std::cell::Cell::new([None; 3]) };
    /// Over a run: strikes dispatched ([`check_aimed_dispatch`]) and those
    /// whose target was not the foe the cursor confirmed, that foe standing.
    pub(super) static AIM_DISPATCH: std::cell::Cell<(u32, u32)> = const { std::cell::Cell::new((0, 0)) };
}

/// The engine-side check behind the aim: a strike confirmed on foe row `s`
/// must dispatch (`World::dispatch_pending_party_action`) against actor
/// `party_count + s` while that foe stands - the picker's cursor, the
/// commit's target and the dispatch's `+0x1DD` are three writes of one
/// byte. A foe that fell before the member's turn is the dead-target
/// redirect's (`FUN_801DB124`) and is not counted.
fn check_aimed_dispatch(w: &legaia_engine_core::world::World) {
    let n = w.party.party_count.clamp(1, 3) as usize;
    let mut e = AIM_EXPECT.with(std::cell::Cell::get);
    for (m, slot) in e.iter_mut().enumerate().take(n) {
        let Some((row, seen)) = *slot else { continue };
        let pending = w.battle.round_flow.pending[m].is_some();
        if pending {
            *slot = Some((row, true));
            continue;
        }
        // A step back or a Reselect clears the commit too; the member is
        // then back at its ring, not dispatched.
        let choosing = w.battle.command.is_some() || w.battle.arts_input.is_some();
        if !seen || choosing || usize::from(w.battle_ctx.active_actor) != m {
            continue;
        }
        *slot = None;
        let foe = n + usize::from(row);
        if !w.actors.get(foe).is_some_and(|a| a.battle.hp > 0) {
            continue;
        }
        // A confused member's strike is retargeted at the dispatch
        // (`FUN_801E7320` rewrites `+0x1DD`; `World::maybe_confuse_retarget`).
        let statuses: Vec<_> = w
            .battle
            .status_effects
            .statuses(m as u8)
            .iter()
            .map(|s| s.kind)
            .collect();
        if statuses.contains(&legaia_engine_battle_vm::status_effects::StatusKind::Confuse) {
            continue;
        }
        let got = w.actors[m].battle.active_target;
        let miss = usize::from(got) != foe;
        AIM_DISPATCH.with(|t| {
            let (a, b) = t.get();
            t.set((a + 1, b + u32::from(miss)));
        });
        if miss {
            eprintln!(
                "    [aim] member {m} confirmed foe row {row} (actor {foe}), dispatched at actor {got}; statuses {statuses:?}"
            );
        }
    }
    AIM_EXPECT.with(|c| c.set(e));
}

/// The press for an open single-foe target cursor: Right while it stands
/// off `aim` ([`aimed_foe`]), Cross once it is on it, when there is no aim,
/// or when [`AIM_TRIES`] steps have not brought it there (the ring covers
/// four seats, so a fifth foe may be one it never visits).
fn aim_press(
    picker: &legaia_engine_battle::target_picker::TargetPickerSession,
    aim: Option<u8>,
    striker: Option<u8>,
) -> u16 {
    use legaia_engine_battle::target_picker::{CursorRow, PickerState, TargetKind};
    let off_aim = match (picker.state(), aim) {
        (
            PickerState::Cursor {
                row: CursorRow::Enemy,
                slot,
            },
            Some(want),
        ) => picker.kind() == TargetKind::SingleEnemy && slot != want,
        _ => false,
    };
    if off_aim && AIM_SPENT.with(|c| c.replace(c.get() + 1)) < AIM_TRIES {
        AIM_TALLY.with(|t| t.set((t.get().0 + 1, t.get().1)));
        return PadButton::Right.mask();
    }
    let spent = AIM_SPENT.with(|c| c.replace(0));
    if spent > 0 && !off_aim {
        AIM_TALLY.with(|t| t.set((t.get().0, t.get().1 + 1)));
    }
    if off_aim && std::env::var_os("LEGAIA_FGL_TRACE").is_some() {
        eprintln!(
            "    [aim] cursor never reached foe row {aim:?} in {AIM_TRIES} asks; confirming {:?}",
            picker.state()
        );
    }
    if let (
        Some(m),
        PickerState::Cursor {
            row: CursorRow::Enemy,
            slot,
        },
    ) = (striker, picker.state())
        && continuous::PLAYS_ON.with(std::cell::Cell::get)
    {
        AIM_EXPECT.with(|c| {
            let mut e = c.get();
            if let Some(x) = e.get_mut(usize::from(m)) {
                *x = Some((slot, false));
            }
            c.set(e);
        });
    }
    PadButton::Cross.mask()
}

pub(super) fn fight_pad(session: &NativeSession) -> u16 {
    use legaia_engine_battle::arts_command_input::ArtsInputPhase;
    use legaia_engine_battle::battle_magic::SpellPhase;
    use legaia_engine_battle::battle_tutorial::TutorialLesson;
    use legaia_engine_battle_menus::battle_input::CommandPhase;
    use legaia_engine_menus::inventory_use::InventoryUseState;
    let w = &session.host.world;
    check_aimed_dispatch(w);
    if !w.battle.tutorial_boxes.is_empty() {
        return PadButton::Cross.mask();
    }
    let lesson = w.battle.tutorial.as_ref().map(|t| t.lesson());
    if let Some(menu) = w.battle.item_menu.as_ref() {
        if lesson == Some(TutorialLesson::Items) {
            return match &menu.state {
                InventoryUseState::Browsing { .. } if !menu.filtered_items.is_empty() => {
                    PadButton::Cross.mask()
                }
                InventoryUseState::TargetSelect { .. } => PadButton::Cross.mask(),
                _ => PadButton::Circle.mask(),
            };
        }
        let listed = |i: usize| {
            menu.filtered_items
                .get(i)
                .and_then(|&k| menu.items.get(k))
                .copied()
        };
        let actor = w
            .battle
            .command
            .as_ref()
            .map_or(w.battle_ctx.active_actor, |c| c.actor);
        // A race against a timed finisher spends no turn on a heal, here as
        // at the command menu.
        let heal_want =
            if going_for_the_kill(w, actor) || racing_a_finisher(w) && !race_heal(w, actor) {
                None
            } else {
                wanted_item(w, (0..menu.filtered_items.len()).filter_map(listed))
            };
        let want = heal_want
            .or_else(|| wanted_mp_item(w, (0..menu.filtered_items.len()).filter_map(listed), actor))
            .or_else(|| {
                wanted_fury_item(w, (0..menu.filtered_items.len()).filter_map(listed), actor)
            })
            .or_else(|| {
                (!finisher_round(w))
                    .then(|| {
                        wanted_power_item(
                            w,
                            (0..menu.filtered_items.len()).filter_map(listed),
                            actor,
                        )
                    })
                    .flatten()
            });
        return match &menu.state {
            InventoryUseState::Browsing { cursor } => match want {
                Some((id, _)) if listed(*cursor) == Some(id) => PadButton::Cross.mask(),
                Some(_) => PadButton::Down.mask(),
                None => {
                    let key = (actor, party_hp_key(w));
                    NO_ITEM.with(|n| n.borrow_mut().insert(key));
                    PadButton::Circle.mask()
                }
            },
            // The session seeds the cursor on the first ally the item helps;
            // the hand steps it to the one it chose, and remembers the aim so
            // the round's later members count the heal as coming.
            InventoryUseState::TargetSelect { cursor, .. } => {
                let aim = want.map(|(_, t)| t);
                let on = menu.targets.get(*cursor).map(|t| t.slot);
                if aim.is_none() || on == aim {
                    if let Some(slot) = on {
                        ITEM_TARGET.with(|t| {
                            if let Some(a) = t.borrow_mut().get_mut(usize::from(actor)) {
                                *a = Some(slot);
                            }
                        });
                    }
                    PadButton::Cross.mask()
                } else {
                    PadButton::Down.mask()
                }
            }
            _ => PadButton::Circle.mask(),
        };
    }
    if let Some(m) = w.battle.spell_menu.as_ref() {
        MAGIC_REFUSED.with(|r| r.borrow_mut().remove(&m.actor));
        return match &m.phase {
            SpellPhase::Select { cursor } => match wanted_spell(w, m) {
                Some(i) if i == usize::from(*cursor) => PadButton::Cross.mask(),
                Some(_) => PadButton::Down.mask(),
                // A heal the window cannot cast is not asked for again until
                // someone's HP moves; a window with no damage spell worth
                // casting is not reopened for one this battle. The two are
                // kept apart: Gala's list is Spoon and Orb alone, and reading
                // the first round's "no damage spell" as "no spell" kept her
                // Spoon unspent through Zora's party-wide hits.
                None if spell_heal(w, m.actor).is_some() => {
                    NO_HEAL_SPELL.with(|n| n.borrow_mut().insert((m.actor, party_hp_key(w))));
                    PadButton::Circle.mask()
                }
                None => {
                    NO_MAGIC.with(|n| n.borrow_mut().insert(m.actor));
                    PadButton::Circle.mask()
                }
            },
            SpellPhase::Targeting { spell_id, picker } => {
                aim_press(picker, aimed_foe(w, Some(*spell_id)), None)
            }
            _ => PadButton::Cross.mask(),
        };
    }
    if let Some(arts) = w.battle.arts_input.as_ref() {
        return match &arts.phase {
            ArtsInputPhase::Entering => {
                // The hyper-arts lesson's drill is `[High] [Low] [High]` -
                // Vahn's Somersault, `Up Down Up` - checked at the entry's
                // confirm.
                let plan = if lesson == Some(TutorialLesson::HyperArts) {
                    use legaia_art::queue::Command;
                    [Command::Up, Command::Down, Command::Up]
                        .map(Command::as_byte)
                        .to_vec()
                } else {
                    arts_plan(w, arts)
                };
                if arts.buffer.len() < plan.len() && plan.starts_with(&arts.buffer) {
                    direction_mask(plan[arts.buffer.len()])
                } else {
                    PadButton::Cross.mask()
                }
            }
            ArtsInputPhase::Targeting { picker } => {
                aim_press(picker, aimed_foe(w, None), Some(arts.actor))
            }
            _ => PadButton::Cross.mask(),
        };
    }
    if let Some(cmd) = w.battle.command.as_ref() {
        let heal = || {
            if racing_a_finisher(w) && !race_heal(w, cmd.actor) {
                return false;
            }
            let bag: Vec<u8> = w
                .party
                .inventory
                .iter()
                .filter(|(_, c)| **c > 0)
                .map(|(id, _)| *id)
                .collect();
            wanted_item(w, bag.into_iter()).is_some()
                && !NO_ITEM.with(|n| n.borrow().contains(&(cmd.actor, party_hp_key(w))))
        };
        // A Miracle Art the live pool pays for outdamages any summon the
        // hand can cast (Koru: ~2000 against Aluru's ~1300).
        let refused = || {
            MAGIC_REFUSED.with(|r| r.borrow().get(&cmd.actor).copied().unwrap_or(0))
                > MAGIC_RING_TRIES
        };
        let magic = || {
            !NO_MAGIC.with(|n| n.borrow().contains(&cmd.actor))
                && !refused()
                && !miracle_ready(w, cmd.actor)
        };
        let heal_spell = || {
            !refused()
                && !NO_HEAL_SPELL.with(|n| n.borrow().contains(&(cmd.actor, party_hp_key(w))))
                && spell_heal(w, cmd.actor).is_some()
        };
        // A Magic pick from the ring, counted so a ring that never opens the
        // spell window is asked only a few times this battle.
        let ask_magic = || {
            MAGIC_REFUSED.with(|r| {
                *r.borrow_mut().entry(cmd.actor).or_insert(0) += 1;
            });
            if refused() {
                PadButton::Left.mask()
            } else {
                PadButton::Right.mask()
            }
        };
        let drink = || {
            let bag: Vec<u8> = w
                .party
                .inventory
                .iter()
                .filter(|(_, c)| **c > 0)
                .map(|(id, _)| *id)
                .collect();
            wanted_mp_item(w, bag.into_iter(), cmd.actor).is_some()
                && !NO_ITEM.with(|n| n.borrow().contains(&(cmd.actor, party_hp_key(w))))
        };
        let fury = || {
            let bag: Vec<u8> = w
                .party
                .inventory
                .iter()
                .filter(|(_, c)| **c > 0)
                .map(|(id, _)| *id)
                .collect();
            (wanted_fury_item(w, bag.clone().into_iter(), cmd.actor).is_some()
                || (!finisher_round(w)
                    && wanted_power_item(w, bag.into_iter(), cmd.actor).is_some()))
                && !NO_ITEM.with(|n| n.borrow().contains(&(cmd.actor, party_hp_key(w))))
        };
        return match &cmd.phase {
            CommandPhase::Menu { .. } if lesson == Some(TutorialLesson::Items) => {
                PadButton::Up.mask()
            }
            CommandPhase::Menu { .. } if lesson == Some(TutorialLesson::Spirit) => {
                PadButton::Down.mask()
            }
            // The finishing blow: damage ahead of every heal, item and
            // stance - a spell when that is what the count rests on.
            CommandPhase::Menu { .. } if lesson.is_none() && going_for_the_kill(w, cmd.actor) => {
                let spell = finishing_damage(w, cmd.actor).is_some_and(|(_, s)| s);
                if spell && !NO_MAGIC.with(|n| n.borrow().contains(&cmd.actor)) && !refused() {
                    ask_magic()
                } else {
                    PadButton::Left.mask()
                }
            }
            // The foe's heavy round is due: the Spirit stance halves it.
            CommandPhase::Menu { .. } if lesson.is_none() && wants_guard(w, cmd.actor) => {
                PadButton::Down.mask()
            }
            // A party fight's heal spell ([`spell_heal`]) comes before the
            // stance against the next party-wide hit and before the bag: a
            // group heal lifts every member out of its reach, where the
            // stance halves it only on the member who takes it (Gala held the
            // stance with Spoon in hand while Zora's single hit dropped Noa
            // beside her), and an item lifts one.
            CommandPhase::Menu { .. } if lesson.is_none() && !solo_duel(w) && heal_spell() => {
                ask_magic()
            }
            CommandPhase::Menu { .. } if lesson.is_none() && guard_before_heal(w, cmd.actor) => {
                PadButton::Down.mask()
            }
            // Evil Seru Magic can come this round: the stance.
            CommandPhase::Menu { .. }
                if lesson.is_none() && wants_evil_magic_guard(w, cmd.actor) =>
            {
                PadButton::Down.mask()
            }
            CommandPhase::Menu { .. } if lesson.is_none() && heal() => PadButton::Up.mask(),
            // Out of MP for its best spell in a boss fight: a drink.
            CommandPhase::Menu { .. } if lesson.is_none() && drink() => PadButton::Up.mask(),
            // A boss too big for spells and short strings: the turn that buys
            // a Miracle Art every round after.
            CommandPhase::Menu { .. } if lesson.is_none() && fury() => PadButton::Up.mask(),
            // One party-wide hit from death with nothing to heal: the stance.
            CommandPhase::Menu { .. } if lesson.is_none() && wants_aoe_guard(w, cmd.actor) => {
                PadButton::Down.mask()
            }
            // A duellist's bag run dry: a spell heal.
            CommandPhase::Menu { .. } if lesson.is_none() && heal_spell() => ask_magic(),
            // A solo duel builds its gauge between Arts turns.
            CommandPhase::Menu { .. } if lesson.is_none() && duel_wants_spirit(w, cmd.actor) => {
                PadButton::Down.mask()
            }
            // A duel's damage is its Arts; a summon spends the turn and the
            // MP for less.
            // A ring that answers Magic by reopening itself (a fight whose
            // special word forbids the Ra-Seru chip, `battle_raseru_forbidden`,
            // or a caster with no spell session) is asked a few times and
            // then not again this battle: pressing it for good left Gala on
            // the ring with a dead foe and a live one for the whole budget.
            CommandPhase::Menu { .. } if lesson.is_none() && magic() && !solo_duel(w) => {
                ask_magic()
            }
            CommandPhase::AttackMode { .. }
                if lesson.is_none() || lesson == Some(TutorialLesson::HyperArts) =>
            {
                PadButton::Right.mask()
            }
            // A random encounter on a pad travel leg is fled: the prompt's
            // Right takes Run. A lone member worn down by a string of fights
            // wipes on whichever one the RNG happens to deal - a finding
            // about the route, not the port. A fight that forbids running
            // (`no_escape`) is fought. A member low enough to want a heal
            // takes it first: monster swings play their own clips and roll
            // their own power bytes, so a failed run under a full enemy
            // round can now take a worn member from half HP to zero. But a
            // heal does not win a race the foes are winning: once the last
            // round took half of what the party still holds, the turn spent
            // healing is a turn under another such round, and the run is
            // the better draw (`vozz`'s three-monster F4 after a caught Run:
            // a Healing Leaf a round into ~250 a round, two members down).
            // Two runs that failed (two finished rounds of a fight the hand
            // only ever fled) and nothing to heal with make running a third
            // time the worse draw against foes the party can beat: a lone
            // chapter-1 Vahn on `map01` fled a pair of 99 HP monsters six
            // rounds running and fell at the last.
            CommandPhase::RoundPrompt { .. }
                if FLEE_ENCOUNTERS.with(std::cell::Cell::get)
                    && !w.battle.no_escape
                    && (!heal() || losing_race(w))
                    && !runs_failing(w)
                    && !worth_fighting(w) =>
            {
                PadButton::Right.mask()
            }
            // A member whose every limb is rotted cannot Attack: the ring
            // refuses the arm and stays up, and pressing it for good left the
            // fight unresolved (Sol's monster 120). The Spirit stance spends
            // the turn instead.
            CommandPhase::Menu { .. } if attack_refused(w, cmd.actor) => PadButton::Down.mask(),
            CommandPhase::RoundPrompt { .. }
            | CommandPhase::Menu { .. }
            | CommandPhase::AttackMode { .. }
            | CommandPhase::CommitConfirm { .. } => PadButton::Left.mask(),
            CommandPhase::Targeting { picker, .. } => {
                aim_press(picker, aimed_foe(w, None), Some(cmd.actor))
            }
            _ => 0,
        };
    }
    // Between command sessions (intro, victory banner, results): page on.
    PadButton::Cross.mask()
}

/// One line of battle state for a stall report.
pub(super) fn battle_snapshot(session: &NativeSession) -> String {
    let w = &session.host.world;
    let n = w.party.party_count.clamp(1, 3) as usize;
    let hp = |i: usize| format!("{}/{}", w.actors[i].battle.hp, w.actors[i].battle.max_hp);
    let party: Vec<String> = (0..n).map(hp).collect();
    // The engine seats monsters right behind the party, not at retail's
    // fixed seat 3 - a lone fighter's opponent sits in slot 1.
    let mobs: Vec<String> = (n..w.actors.len())
        .filter(|&i| w.actors[i].battle.max_hp > 0)
        .map(hp)
        .collect();
    let formation = w.battle.active_formation.as_ref().map_or_else(
        || "F-".to_string(),
        |f| {
            let ids: Vec<String> = f.slots.iter().map(|s| s.monster_id.to_string()).collect();
            format!("F{}[{}]", f.formation_id, ids.join(","))
        },
    );
    let phase = w.battle.command.as_ref().map_or_else(
        || "no command session".to_string(),
        |c| format!("{:?}", c.phase),
    );
    let phase: String = phase.chars().take(60).collect();
    let clip = |s: String| s.chars().take(90).collect::<String>();
    let sm = w.battle_ctx.action_state;
    let sm_name = legaia_engine_battle_vm::battle_action::ActionState::from_byte(sm)
        .map_or_else(|| "?".to_string(), |s| format!("{s:?}"));
    format!(
        "{formation} party[{}] mobs[{}] action SM ctx[7]=0x{sm:02X} {sm_name} actor {} boxes {} phase {phase}; driven {} end {:?} victory {} flow {} round {}",
        party.join(" "),
        mobs.join(" "),
        w.battle_ctx.active_actor,
        w.battle.tutorial_boxes.len(),
        w.battle.player_driven,
        w.battle.end,
        w.battle.victory.is_some(),
        clip(format!("{:?}", w.battle.flow)),
        clip(format!("{:?}", w.battle.round_flow)),
    )
}

/// The party a traced battle starts with: each member's level, MP, equipment
/// and spell list, and the bag's battle-usable items.
pub(super) fn party_dump(session: &NativeSession) {
    let w = &session.host.world;
    let n = w.party.party_count.clamp(1, 3) as usize;
    for i in 0..n {
        let slot = w.party_roster_slot(i);
        let Some(m) = w.party.roster.members.get(slot) else {
            continue;
        };
        let list = m.spell_list();
        let spells: Vec<String> = list.ids[..(list.count as usize).min(list.ids.len())]
            .iter()
            .map(|&id| match w.tables.spell_catalog.get(id) {
                Some(d) => format!("{id:#x} {} mp{} {:?}", d.name, d.mp_cost, d.effect),
                None => format!("{id:#x} ?"),
            })
            .collect();
        let a = &w.actors[i].battle;
        // `battle.character` is written only when an arts string commits, so
        // it reads Vahn (its zero) for every member before then: name the
        // member by its roster slot.
        let ch = legaia_engine_battle::battle_arts::character_for_slot(slot as u8);
        eprintln!(
            "    [party] #{i} char {ch:?} lv {} hp {}/{} mp {} equip {:?} spells [{}]",
            m.level(),
            a.hp,
            a.max_hp,
            a.mp,
            m.equipment(),
            spells.join("; ")
        );
    }
    for i in 0..n {
        let ch =
            legaia_engine_battle::battle_arts::character_for_slot(w.party_roster_slot(i) as u8);
        let mut arts: Vec<String> = w
            .tables
            .art_records
            .iter()
            .filter(|((c, _), _)| *c == ch)
            .map(|((_, a), r)| {
                let cmds: Vec<u8> = r.commands.iter().map(|c| c.as_byte()).collect();
                format!(
                    "{:#x} {:?} {cmds:?} pw{}",
                    a.as_byte(),
                    r.name,
                    r.power.len()
                )
            })
            .collect();
        arts.sort();
        eprintln!("    [arts] #{i} {ch:?}: {}", arts.join("; "));
        let roster = w.party_roster_slot(i);
        eprintln!(
            "    [pool] #{i} miracle armed {} pool {} base {} press costs {:?}",
            miracle_armed(w, i as u8),
            w.actors[i].battle.agl,
            w.actors[i].battle.agl_base,
            w.battle.swing_costs.get(roster)
        );
    }
    let bag: Vec<String> = w
        .party
        .inventory
        .iter()
        .filter(|(_, c)| **c > 0)
        .filter_map(|(&id, &c)| {
            let e = w.tables.item_catalog.get(id)?;
            e.usable_in_battle
                .then(|| format!("{id:#x} {} x{c} {:?}", e.name, e.effect))
        })
        .collect();
    eprintln!("    [party] bag: {}", bag.join("; "));
}

/// Fight a battle with the pad ([`fight_pad`]). `None` when it ended and a
/// walking mode came back.
pub(super) fn drain_battle(session: &mut NativeSession) -> Option<Run> {
    // A fight's frames come off the pad segment's budget only past the
    // battle allowance: the deadline moves ahead of the fight by the most
    // it may need, and the unused part is taken back.
    let lent = if pad_hand() {
        PAD_BATTLE_LEFT
            .with(std::cell::Cell::get)
            .min(BATTLE_TICKS_CAP as u64)
    } else {
        0
    };
    PAD_DEADLINE.with(|d| d.set(d.get().saturating_add(lent)));
    let start = session.frames;
    let r = fight_battle(session);
    let used = session.frames.saturating_sub(start).min(lent);
    PAD_DEADLINE.with(|d| d.set(d.get().saturating_sub(lent - used)));
    PAD_BATTLE_LEFT.with(|l| l.set(l.get().saturating_sub(used)));
    r
}

pub(super) fn fight_battle(session: &mut NativeSession) -> Option<Run> {
    NO_ITEM.with(|n| n.borrow_mut().clear());
    AIM_SPENT.with(|c| c.set(0));
    AIM_EXPECT.with(|c| c.set([None; 3]));
    NO_MAGIC.with(|n| n.borrow_mut().clear());
    NO_HEAL_SPELL.with(|n| n.borrow_mut().clear());
    MAGIC_REFUSED.with(|n| n.borrow_mut().clear());
    DUEL_SPIRIT_ROUND.with(|d| d.set(None));
    BIGGEST_HIT.with(|b| b.set(0));
    ROUND_LOSS.with(|r| r.set([0; 3]));
    ROUND_HISTORY.with(|h| h.borrow_mut().clear());
    AOE_HIT.with(|a| a.set(0));
    CHARGE_SEEN.with(|c| c.set(false));
    AOE_GUARDED.with(|g| g.set([None; 3]));
    DEALT.with(|d| d.borrow_mut().clear());
    AOE_STREAK.with(|r| r.set(0));
    // What each member's latest commit deals damage as ([`dealt_kind`]), and
    // the action whose damage is being summed (member, kind, total).
    let mut act_kind: [Option<u16>; 3] = [None; 3];
    let mut acting: Option<(u8, u16, u32)> = None;
    let flush = |acting: &mut Option<(u8, u16, u32)>| {
        if let Some((a, k, d)) = acting.take() {
            DEALT.with(|m| {
                let mut m = m.borrow_mut();
                let e = m.entry((a, k)).or_insert(d);
                *e = (*e).min(d);
            });
        }
    };
    let mut guarded = [false; 3];
    let mut was_window = false;
    ITEM_TARGET.with(|t| *t.borrow_mut() = [None; 3]);
    let party_hp = |s: &NativeSession| -> Vec<u16> {
        let w = &s.host.world;
        let n = w.party.party_count.clamp(1, 3) as usize;
        (0..n).map(|i| w.actors[i].battle.hp).collect()
    };
    let mut party_prev = party_hp(session);
    let mut prev = 0u16;
    // The element of a boss (a fight that forbids running): its record's
    // `+0x1D`, read while its seat is still filled.
    let boss_element = {
        let w = &session.host.world;
        w.battle
            .no_escape
            .then(|| {
                w.actors
                    .iter()
                    .filter_map(|a| a.battle_monster_id)
                    .find_map(|id| w.tables.monster_catalog.get(id))
                    .map(|d| d.element)
            })
            .flatten()
    };
    let boss = boss_monster_id(&session.host.world);
    let trace = std::env::var_os("LEGAIA_FGL_TRACE").is_some();
    if trace {
        eprintln!("    [battle] start: {}", battle_snapshot(session));
        party_dump(session);
    }
    let trace_hits = trace && std::env::var_os("LEGAIA_FGL_TRACE_HITS").is_some();
    let hp_all = |s: &NativeSession| -> Vec<u16> {
        s.host.world.actors.iter().map(|a| a.battle.hp).collect()
    };
    let mut last_cmd: [String; 3] = Default::default();
    let mut hp_prev = hp_all(session);
    let foe_hp = |s: &NativeSession| -> u32 {
        let w = &s.host.world;
        let n = w.party.party_count.clamp(1, 3) as usize;
        w.actors
            .iter()
            .skip(n)
            .map(|a| u32::from(a.battle.hp))
            .sum()
    };
    let mut foe_prev = foe_hp(session);
    let mut foe_dropped_at = 0usize;
    // Whether the foe has Stoned a party member this battle.
    let mut stoned_seen = false;
    let mut spent = 0usize;
    for t in 0..BATTLE_TICKS_CAP {
        spent = t;
        if t >= BATTLE_TICKS && t - foe_dropped_at > BATTLE_STALL_TICKS {
            break;
        }
        if pad_budget(session).is_err() {
            break;
        }
        // A press is an edge: alternate the wanted mask with neutral.
        let want = fight_pad(session);
        if std::env::var_os("LEGAIA_FGL_PAD_DEBUG").is_some()
            && t - foe_dropped_at > 6000
            && t % 97 == 0
        {
            let w = &session.host.world;
            eprintln!(
                "      [pad-debug] t={t} want {want:#06x} cmd {:?} spell {:?} items {:?} arts {:?} flow {:?} ctx7 {:#04x}",
                w.battle
                    .command
                    .as_ref()
                    .map(|c| (c.actor, format!("{:?}", c.phase))),
                w.battle
                    .spell_menu
                    .as_ref()
                    .map(|m| format!("{:?}", m.phase)),
                w.battle
                    .item_menu
                    .as_ref()
                    .map(|m| format!("{:?}", m.state)),
                w.battle.arts_input.as_ref().map(|a| format!(
                    "actor {} {:?} pool {}/{} costs {:?} buffer {:?} preseed {:?}",
                    a.actor, a.phase, a.pool, a.pool_max, a.costs, a.buffer, a.preseed
                )),
                w.battle.round_flow.phase,
                w.battle_ctx.action_state,
            );
        }
        let pad = if prev == 0 { want } else { 0 };
        prev = pad;
        session.host.world.set_pad(pad);
        if let Err(e) = session.tick() {
            return Some(Run::Error(format!("{e:#}")));
        }
        if foe_charged(&session.host.world) {
            CHARGE_SEEN.with(|c| c.set(true));
        }
        {
            let w = &session.host.world;
            for (i, p) in w.battle.round_flow.pending.iter().enumerate().take(3) {
                if let Some(p) = p {
                    act_kind[i] = dealt_kind(p);
                }
            }
        }
        let foe_now = foe_hp(session);
        // A foe's HP drop is the acting member's, summed over the action's
        // hits; a change of actor closes the sum.
        let who = session.host.world.battle_ctx.active_actor;
        if acting.is_some_and(|(a, _, _)| a != who) {
            flush(&mut acting);
        }
        if foe_now < foe_prev
            && let Some(kind) = act_kind.get(usize::from(who)).copied().flatten()
        {
            match &mut acting {
                Some((_, k, d)) if *k == kind => *d += foe_prev - foe_now,
                _ => {
                    flush(&mut acting);
                    acting = Some((who, kind, foe_prev - foe_now));
                }
            }
        }
        if foe_now < foe_prev {
            foe_dropped_at = t;
            EXCHANGE.with(|e| {
                let (lost, dealt) = e.get();
                e.set((lost, dealt + u64::from(foe_prev - foe_now)));
            });
        }
        foe_prev = foe_now;
        let party_now = party_hp(session);
        // A command window opening ends the stretch the foes had.
        let window = session.host.world.battle.command.as_ref().is_some_and(|c| {
            matches!(
                c.phase,
                legaia_engine_battle_menus::battle_input::CommandPhase::RoundPrompt { .. }
            )
        });
        let mut run = ROUND_LOSS.with(std::cell::Cell::get);
        for (i, p) in session
            .host
            .world
            .battle
            .round_flow
            .pending
            .iter()
            .enumerate()
            .take(3)
        {
            if matches!(
                p,
                Some(legaia_engine_core::battle_round::PendingPartyAction::Spirit)
            ) {
                guarded[i] = true;
            }
        }
        if window && !was_window {
            flush(&mut acting);
            AOE_STREAK.with(|s| {
                s.set(if run.iter().filter(|&&l| l > 0).count() >= 2 {
                    s.get() + 1
                } else {
                    0
                })
            });
            ROUND_HISTORY.with(|h| h.borrow_mut().push(run.iter().sum()));
            if run.iter().filter(|&&l| l > 0).count() >= 2 {
                // A member that took the hit in the Spirit stance took half
                // of it: the hit itself is twice what it lost (Jette's Evil
                // Seru Magic, 612 a guarded member, 1224 the next round
                // unguarded - read as 612, the heal limit let the party sit
                // at 1130 into a hit that dropped all three).
                let peak = run
                    .iter()
                    .zip(guarded)
                    .map(|(&l, g)| if g { l * 2 } else { l })
                    .max()
                    .unwrap_or(0);
                AOE_HIT.with(|a| a.set(a.get().max(peak)));
            }
            if trace_hits {
                eprintln!(
                    "    [round] t={t} history {:?} due {} aoe {} hp {:?} mode {} foe mp {:?}",
                    ROUND_HISTORY.with(|h| h.borrow().clone()),
                    big_round_due(),
                    AOE_HIT.with(std::cell::Cell::get),
                    party_hp(session),
                    session.host.world.battle.monster_ai_state.mode_flags,
                    session
                        .host
                        .world
                        .actors
                        .iter()
                        .skip(session.host.world.party.party_count.clamp(1, 3) as usize)
                        .map(|a| a.battle.mp)
                        .filter(|&m| m > 0)
                        .collect::<Vec<_>>(),
                );
                last_cmd = Default::default();
            }
        }
        was_window = window;
        if window {
            run = [0; 3];
            guarded = [false; 3];
        }
        for (i, (a, b)) in party_prev.iter().zip(&party_now).enumerate().take(3) {
            run[i] += u32::from(a.saturating_sub(*b));
            EXCHANGE.with(|e| {
                let (lost, dealt) = e.get();
                e.set((lost + u64::from(a.saturating_sub(*b)), dealt));
            });
            BIGGEST_HIT.with(|h| h.set(h.get().max(run[i])));
        }
        ROUND_LOSS.with(|r| r.set(run));
        party_prev = party_now;
        if trace_hits {
            let w = &session.host.world;
            for (i, p) in w.battle.round_flow.pending.iter().enumerate() {
                if let Some(p) = p {
                    let s: String = format!("{p:?}").chars().take(70).collect();
                    if last_cmd[i] != s {
                        eprintln!("    [commit] t={t} actor {i}: {s}");
                    }
                    last_cmd[i] = s;
                }
            }
            let hp_now = hp_all(session);
            if hp_now != hp_prev {
                let who = w.battle_ctx.active_actor;
                let what = last_cmd.get(usize::from(who)).cloned().unwrap_or_default();
                let n = w.party.party_count.clamp(1, 3) as usize;
                let gauges: Vec<u16> = (0..n).map(|i| w.actors[i].battle.spirit_gauge).collect();
                eprintln!(
                    "    [hit] t={t} actor {who} ({what}) sm 0x{:02X} hp {:?} -> {:?} mob mp {} mode {} shield {} gauges {gauges:?}",
                    w.battle_ctx.action_state,
                    &hp_prev[..n + 1],
                    &hp_now[..n + 1],
                    w.actors.get(n).map_or(0, |a| a.battle.mp),
                    w.battle.monster_ai_state.mode_flags,
                    w.battle.monster_ai_state.flag_bd84,
                );
                hp_prev = hp_now;
            }
        }
        if trace && std::env::var_os("LEGAIA_FGL_TRACE_BATTLE").is_some() && t % 1000 == 0 {
            eprintln!("    [battle] t={t}: {}", battle_snapshot(session));
        }
        let w = &session.host.world;
        let n_party = w.party.party_count.clamp(1, 3) as usize;
        stoned_seen |= (0..n_party).any(|i| stoned(w, i));
        if w.game_over_hold || w.game_over {
            if trace {
                eprintln!("    [battle] wiped after {t} ticks");
            }
            // A boss fight (no running) lost: what the next attempt prepares
            // for is the foe's element. The wiped party's game-over screen
            // re-enters this loop with the monsters gone; it does not erase
            // what beat them.
            if trace {
                eprintln!("    [battle] lost to a boss of element {boss_element:?}");
            }
            // A boss that Stoned a member beat the party with the stone, not
            // its element: Zora's Glare petrifies one member a cast, and the
            // `0x5A` wipe scan counts a Stoned member as down, so a party
            // with HP to spare (Noa at 661 beside two statues) still wipes.
            // The reload wears the guard against that instead.
            if boss_element.is_some() {
                let key = if stoned_seen {
                    Some(STONE_GUARD_KEY)
                } else {
                    boss_element
                };
                if trace {
                    eprintln!("    [battle] guard to wear on the reload: {key:?}");
                }
                LOST_TO_ELEMENT.with(|l| l.set(key));
            }
            let aoe = AOE_HIT.with(std::cell::Cell::get);
            if let Some(id) = boss
                && aoe > 0
                && let Some(parity) = ROUND_HISTORY.with(|h| {
                    // The stretch the wipe ended is a round too: the hit
                    // that finished the party lands in it.
                    let mut h = h.borrow().clone();
                    h.push(ROUND_LOSS.with(std::cell::Cell::get).iter().sum());
                    alternating_heavy_parity(&h)
                })
            {
                if trace {
                    eprintln!(
                        "    [battle] learned: boss {id} hits party-wide ({aoe}) on stretch parity {parity}"
                    );
                }
                LEARNED_CADENCE.with(|l| l.set(Some((id, parity, aoe))));
            }
            return Some(Run::Battle(format!(
                "party wiped: {}",
                battle_snapshot(session)
            )));
        }
        if w.mode != SceneMode::Battle {
            if trace {
                eprintln!(
                    "    [battle] ended after {t} ticks: {}",
                    battle_snapshot(session)
                );
            }
            return None;
        }
    }
    if trace {
        // A cast band that never returned: the stager's phase and whom the
        // caster's creature was walking onto.
        let w = &session.host.world;
        let caster = usize::from(w.battle_ctx.active_actor);
        let target = w.actors.get(caster).map(|a| a.battle.active_target);
        eprintln!(
            "    [battle] stuck cast: stager {:?}; module phase {}; creature seat {:?}; caster {caster} target {target:?} (active {:?}, hp {:?}, liveness {:?}); queue {:02X?}",
            w.casting
                .summon_stager
                .as_ref()
                .map(|s| (s.spell_id, s.phase, s.walked, s.goal)),
            w.casting.module_phase,
            w.casting.summon_actor_slot,
            target.and_then(|t| w.actors.get(usize::from(t)).map(|a| a.active)),
            target.and_then(|t| w.actors.get(usize::from(t)).map(|a| a.battle.hp)),
            target.and_then(|t| w.actors.get(usize::from(t)).map(|a| a.battle.liveness)),
            w.actors.get(caster).map(|a| a.battle.params),
        );
        for (s, a) in w.actors.iter().enumerate().take(12) {
            eprintln!(
                "    [battle]   seat {s}: active {} hp {} mon {:?} anim {} queued {} reaction {:?}/{:?} pose {:?} color {:#x} rflag {:#x} flags {:#x}",
                a.active,
                a.battle.hp,
                a.battle_monster_id,
                a.battle.current_anim,
                a.battle.queued_anim,
                a.battle_reaction,
                a.battle_reaction_entry,
                a.battle_pose,
                a.battle.render_color,
                a.battle.render_flag,
                a.battle.field_flags,
            );
        }
    }
    Some(Run::Battle(format!(
        "battle unresolved after {spent} ticks: {}",
        battle_snapshot(session)
    )))
}
