//! How the walker gets through a random encounter: the fight policy's pad
//! and the battle drain.

use super::*;

/// Outcome of sitting out a random encounter.
pub(super) enum Fight {
    /// The battle ended and the walking mode came back.
    Survived,
    /// Still in [`SceneMode::Battle`] after [`BATTLE_FRAME_BUDGET`] ticks.
    Stuck,
    /// The battle handed the world to some third mode.
    Diverted(SceneMode),
    /// The party was killed. A wipe no longer leaves Battle mode on its own:
    /// the game-over hold (`World::game_over_hold`) freezes the scene in
    /// Battle until a host's GameOverSession resolves it, so it must be read
    /// off the hold flags, not off a mode change - and it must be scored as
    /// a wipe, never as `Survived` (the pre-hold engine returned the wiped
    /// party to the field and the ladder kept walking with dead heroes).
    Wiped,
}

/// The ladder's fighter, expressed as *the pad mask it presses this frame*.
///
/// There is no engine call anywhere in this type. Every decision it makes -
/// pick Begin, pick Attack, pick Auto, confirm the target, open the bag, walk
/// to the Healing Leaf row, confirm it on the leader - comes out as a d-pad or
/// face-button mask that [`drain_battle`] hands to `World::set_pad`, which is
/// the same call the walking legs make. That is deliberate: the whole claim
/// this ladder makes is "a person pressing buttons gets this far", and an
/// engine API called mid-battle would quietly weaken it to "a person pressing
/// buttons, plus a robot with debug access, gets this far".
///
/// ## What it plays
///
/// The command ring is retail's four-arm diamond, selected by **direction**,
/// committing on the press (`FUN_801D0748` state `0x28`; see
/// `crate::battle_input`). So:
///
/// | phase | press | chip |
/// |---|---|---|
/// | `RoundPrompt` (`0x1E`) | Left | `Begin` |
/// | `Menu` (`0x28`) | Left / Up | `Attack` / `Item` |
/// | `AttackMode` (`0x78`) | Left | `Auto` |
/// | `Targeting` | Cross | the cursor's enemy |
///
/// `Item` is taken only when the acting party is hurt past
/// [`Self::heal_below_pct`] **and** the bag still holds the heal id - a policy
/// that opens an empty bag has to back out again, which costs a turn and reads
/// as a hang if the back-out is ever missed.
///
/// A queued battle message box (the formation banner, a tutorial page) parks
/// the whole battle tick ahead of the command session
/// (`World::tick_battle_tutorial_boxes`), so a box gets Cross and nothing
/// else - pressing a ring direction into a box is how a run stalls with the
/// menu apparently open and no input arriving.
pub(super) struct FightPolicy {
    /// `false` restores the pre-existing neutral-pad model: no presses at all,
    /// and the live loop auto-commits `arm_party_physical` for every party
    /// turn. Kept as the ladder's own **contrast control** - it is the only
    /// way to show that a change in the score came from the fighter rather
    /// than from the route. Set by `LEGAIA_CPR_NEUTRAL_FIGHT=1`.
    pub(super) driven: bool,
    /// `false` leaves Rim Elm with an **empty bag** while still driving the
    /// command ring by pad. The ladder's second contrast control, and the one
    /// that answers the question the wipe was hiding: `Attack -> Auto` seeds
    /// the same two-swing queue `arm_party_physical` does
    /// (`seed_basic_attack_queue`, retail `FUN_801EED1C`'s no-input arm), so
    /// command selection alone changes *nothing* about the damage traded -
    /// with this off, a driven run and a neutral run should die in the same
    /// place. Set by `LEGAIA_CPR_NO_BAG=1`.
    pub(super) bag: bool,
    /// HP percentage at or below which the fighter reaches for the bag
    /// instead of swinging.
    pub(super) heal_below_pct: u32,
    /// Item id the fighter heals with ([`BAG_HEAL_ITEM`]).
    pub(super) heal_item: u8,
}

impl Default for FightPolicy {
    fn default() -> Self {
        Self {
            driven: std::env::var_os("LEGAIA_CPR_NEUTRAL_FIGHT").is_none(),
            bag: std::env::var_os("LEGAIA_CPR_NO_BAG").is_none(),
            heal_below_pct: 50,
            heal_item: BAG_HEAL_ITEM,
        }
    }
}

impl FightPolicy {
    /// The pad mask for this frame. `0` means "press nothing", which is what
    /// the neutral model does for the whole battle.
    pub(super) fn pad_for(&self, host: &SceneHost) -> u16 {
        use legaia_engine_battle_menus::battle_input::CommandPhase;
        use legaia_engine_menus::inventory_use::InventoryUseState;

        if !self.driven {
            return 0;
        }
        let w = &host.world;
        // A message box owns the frame ahead of everything else.
        if !w.battle.tutorial_boxes.is_empty() {
            return PadButton::Cross.mask();
        }
        if let Some(menu) = w.battle.item_menu.as_ref() {
            return match menu.state {
                InventoryUseState::Browsing { .. } => {
                    if menu.filtered_items.is_empty() {
                        // Nothing usable in here - back out and swing instead.
                        PadButton::Circle.mask()
                    } else if menu.current_item().map(|e| e.id) == Some(self.heal_item) {
                        PadButton::Cross.mask()
                    } else {
                        // Walk the list to the heal row, one press at a time.
                        PadButton::Down.mask()
                    }
                }
                // The target strip opens on the party leader, which is who
                // the heal is for in a solo party.
                InventoryUseState::TargetSelect { .. } => PadButton::Cross.mask(),
                _ => 0,
            };
        }
        if let Some(session) = w.battle.command.as_ref() {
            return match &session.phase {
                CommandPhase::RoundPrompt { .. } => PadButton::Left.mask(), // Begin
                CommandPhase::Menu { .. } => {
                    if self.wants_heal(host) {
                        PadButton::Up.mask() // Item
                    } else {
                        PadButton::Left.mask() // Attack
                    }
                }
                CommandPhase::AttackMode { .. } => PadButton::Left.mask(), // Auto
                CommandPhase::Targeting { .. } => PadButton::Cross.mask(),
                // The party's commit confirm (`0x6E`): Left is Begin.
                CommandPhase::CommitConfirm { .. } => PadButton::Left.mask(),
                _ => 0,
            };
        }
        0
    }

    /// Is any living party member hurt past the threshold, with a heal left
    /// in the bag to spend on them?
    pub(super) fn wants_heal(&self, host: &SceneHost) -> bool {
        let w = &host.world;
        if w.party.inventory.get(&self.heal_item).copied().unwrap_or(0) == 0 {
            return false;
        }
        (0..w.party.party_count.clamp(1, 3) as usize).any(|i| {
            let a = &w.actors[i].battle;
            a.max_hp > 0
                && a.hp > 0
                && u32::from(a.hp) * 100 <= u32::from(a.max_hp) * self.heal_below_pct
        })
    }
}

/// Fight an encounter **through the battle command UI**, the way a player
/// does, and report how it ended.
///
/// **Sitting the encounter out is not an optional nicety, it is the difference
/// between a leg that measures locomotion and a leg that measures nothing.**
/// `map01` has a live region-keyed encounter table
/// (`World::set_world_map_regions`), so a pad-driven crossing enters
/// [`SceneMode::Battle`] every few hundred ticks. While that mode is up the
/// player actor's `move_state` is the **battle arena** transform, not an
/// overworld position, so a follower that keeps sampling [`player_world`]
/// reads a coordinate from a different space entirely - `(0, -825)` on the
/// first `map01` encounter, which is off the north-west corner of a map the
/// player is standing in the middle of. Every downstream number then lies in
/// the same direction at once: the distance check sees no progress and trips
/// the stall detector, and the stall site reports a tile the player was never
/// on with all four walls set (an out-of-grid probe reads as solid). Read as
/// locomotion it looks exactly like an inverted movement axis; it is a mode
/// confusion.
///
/// ## Why the pad presses, and why they are still pad presses
///
/// The first version of this held the pad neutral for the whole battle. That
/// is not "no fighting model", it is a specific one: with
/// [`legaia_engine_core::world::BattleState::player_driven`] off the live loop auto-commits
/// `arm_party_physical` for every party turn - a two-swing basic attack at the
/// first living monster, retail's own AI-party queue
/// (`FUN_801EED1C`'s `(&DAT_8007BD10)[slot] == 4` arm) - and nothing else. No
/// command choice, no items, no Spirit, no Run. A leg that dies under it
/// cannot distinguish "the port cannot walk this route" from "the ladder's
/// fighter is incompetent", which is exactly what the wipe on the
/// `map01 -> suimon` leg had been reporting.
///
/// So the fighter drives [`legaia_engine_core::world::BattleState::command`] - the same
/// [`crate::battle_input::BattleCommandSession`] the windowed host binds its
/// keyboard to - with **pad presses only** ([`FightPolicy::pad_for`]). No
/// engine API is called to pick a command, choose a target or use an item;
/// every one of those is a `set_pad` + `tick` pair, edge-triggered exactly as
/// a human press is. That keeps the rung's claim intact: what the ladder
/// scores is still reachable by a person holding a controller.
///
/// Battle ticks are charged to [`BATTLE_FRAME_BUDGET`], never to the leg's
/// frame or stall budget - fighting is not stalling.
pub(super) fn drain_battle(host: &mut SceneHost, resume: SceneMode, policy: &FightPolicy) -> Fight {
    let report = std::env::var_os("LEGAIA_CPR_FIGHT").is_some();
    let opened = fight_snapshot(host);
    let mut ticks = 0u32;
    let mut pad_prev = 0u16;
    let mut outcome = Fight::Stuck;
    for _ in 0..BATTLE_FRAME_BUDGET {
        // A press is an *edge*: the session reads `just_pressed`, so a held
        // mask pages exactly once and then does nothing. Alternate every
        // frame between the wanted mask and neutral rather than holding it,
        // which is the same duty cycle `drain_scripted` uses on Cross.
        let want = policy.pad_for(host);
        let pad = if pad_prev == 0 { want } else { 0 };
        pad_prev = pad;
        host.world.set_pad(pad);
        if host.tick().is_err() {
            outcome = Fight::Stuck;
            break;
        }
        ticks += 1;
        if host.world.game_over_hold || host.world.game_over {
            outcome = Fight::Wiped;
            break;
        }
        if host.world.mode == resume {
            outcome = Fight::Survived;
            break;
        }
        if host.world.mode != SceneMode::Battle {
            outcome = Fight::Diverted(host.world.mode);
            break;
        }
    }
    if report {
        let closed = fight_snapshot(host);
        eprintln!(
            "[fight] {opened} -> {closed} in {ticks} ticks: {}",
            match &outcome {
                Fight::Survived => "survived".to_string(),
                Fight::Wiped => "WIPED".to_string(),
                Fight::Stuck => "stuck".to_string(),
                Fight::Diverted(m) => format!("diverted to {m:?}"),
            }
        );
    }
    outcome
}

/// One line of battle state: the formation, the party's HP, the leader's
/// level, and the bag. Printed at the open and close of every encounter under
/// `LEGAIA_CPR_FIGHT=1`.
///
/// Deliberately reads the *live* mirrors rather than the roster record: the
/// battle actor's `hp`/`max_hp` is what the wipe scan and the damage fold both
/// consult, and a report keyed on the record would be a frame behind them.
pub(super) fn fight_snapshot(host: &SceneHost) -> String {
    let w = &host.world;
    let party: Vec<String> = (0..w.party.party_count.clamp(1, 3) as usize)
        .map(|i| {
            let a = &w.actors[i].battle;
            format!("{}/{}", a.hp, a.max_hp)
        })
        .collect();
    let monsters: Vec<String> = (w.party.party_count.clamp(1, 3) as usize..w.actors.len())
        .filter(|&i| w.actors[i].battle.max_hp > 0)
        .map(|i| {
            let a = &w.actors[i].battle;
            format!("{}/{}", a.hp, a.max_hp)
        })
        .collect();
    let formation = w
        .battle
        .active_formation
        .as_ref()
        .map(|f| {
            let ids: Vec<String> = f.slots.iter().map(|s| s.monster_id.to_string()).collect();
            format!("F{}[{}]", f.formation_id, ids.join(","))
        })
        .unwrap_or_else(|| "F-".to_string());
    let lvl = w.party.roster.members.first().map_or(0, |r| r.level());
    let bag: usize = w.party.inventory.values().map(|&n| n as usize).sum();
    format!(
        "{formation} party[{}] mob[{}] Lv{lvl} bag{bag}",
        party.join(" "),
        monsters.join(" ")
    )
}
