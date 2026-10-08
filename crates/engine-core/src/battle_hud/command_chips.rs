//! Battle command chips (party and Muscle Dome), the commit log and the combo
//! style. Split out of `battle_hud.rs`; no logic change.

use super::*;

pub use crate::battle_input::{BattleCommandChips, CommandChipPhase};

/// The chip cluster this frame draws, or `None` when no command prompt
/// owns the frame (a submenu, an arts session or a dialogue box wins).
///
/// One projector for both hosts, so the two cannot disagree about whether
/// the menu is up or what its right arm says: the ring's element chip is
/// [`battle_magic_chip`] - the member's Ra-Seru name or `-` - not a fixed
/// word.
pub fn battle_command_chips(world: &crate::world::World) -> Option<BattleCommandChips> {
    use crate::battle_input::{AttackMode, BattleCommand, CommandPhase, CommitChoice, RoundChoice};
    use legaia_asset::battle_ui_strings::BattleUiLabel;
    if world.mode == crate::world::SceneMode::MuscleDome {
        return dome_command_chips(world);
    }
    if world.mode != crate::world::SceneMode::Battle {
        return None;
    }
    if world.dialog.current.is_some() || world.dialog.inline.is_some() {
        return None;
    }
    if world.arts_input_active()
        || world.battle.arts_menu.is_some()
        || world.battle.spell_menu.is_some()
        || world.battle.item_menu.is_some()
    {
        return None;
    }
    let cmd = world.battle.command.as_ref()?;
    let no_escape = world.battle.no_escape;
    let chip = |label: &str, enabled: bool| (label.to_string(), enabled);
    match cmd.phase {
        // Both round-prompt chips always carry their word. The records are
        // static SCUS labels (`0x8007B688` / `0x8007B684`) and the flow SM
        // `FUN_801D0748` never reads the no-escape byte `ctx[+0x287]` - its
        // only readers are the escape roll, the monster flee roll and the
        // action SM - so a boss fight's prompt reads `Begin | Run` exactly
        // as a random encounter's does (the Gaza fight's capture, whose
        // `ctx[+0x287]` is `4`). The refusal is the roll's, not the chip's.
        CommandPhase::RoundPrompt { cursor } => Some(BattleCommandChips {
            chips: RoundChoice::PROMPT
                .iter()
                .map(|c| chip(c.label(), true))
                .collect(),
            cursor: cursor as usize,
            phase: CommandChipPhase::RoundPrompt,
        }),
        CommandPhase::Menu { cursor } => Some(BattleCommandChips {
            chips: BattleCommand::MENU
                .iter()
                .map(|c| match c {
                    BattleCommand::Magic => battle_magic_chip(world, cmd.party_slot),
                    _ => chip(c.label(), c.available(no_escape)),
                })
                .collect(),
            cursor: cursor as usize,
            phase: CommandChipPhase::CommandRing,
        }),
        CommandPhase::AttackMode { cursor } => Some(BattleCommandChips {
            chips: AttackMode::PROMPT
                .iter()
                .map(|m| chip(m.label(), true))
                .collect(),
            cursor: cursor as usize,
            phase: CommandChipPhase::AttackMode,
        }),
        // The left chip's word is the one the round prompt's `Begin` arm
        // stamped into record `0x10` from the overlay pool (`0x801D1060`);
        // the right chip's is record `0x13`'s SCUS pointer. Both off the disc.
        CommandPhase::CommitConfirm { cursor } => Some(BattleCommandChips {
            chips: CommitChoice::PROMPT
                .iter()
                .map(|c| {
                    let disc = match c {
                        CommitChoice::Begin => BattleUiLabel::CommitBegin,
                        CommitChoice::Reselect => BattleUiLabel::Reselect,
                    };
                    let label = world
                        .battle
                        .ui_strings
                        .get(disc)
                        .filter(|s| !s.is_empty())
                        .unwrap_or(c.label());
                    chip(label, true)
                })
                .collect(),
            cursor: cursor as usize,
            phase: CommandChipPhase::CommitConfirm,
        }),
        _ => None,
    }
}

/// The Muscle Dome leg's command cluster, through the same projection the
/// battle's takes: the dome's command flow runs the battle's own command
/// session (`muscle_dome::DomeMenu`), and the labels come off the same
/// sources - the ring's right arm is the lead's Ra-Seru name (or `-` for a
/// fighter carrying none, `FUN_801D8DE8` record `0xA`), and the confirm pair
/// is the disc's `Begin` / `Reselect`.
pub(super) fn dome_command_chips(world: &crate::world::World) -> Option<BattleCommandChips> {
    use crate::battle_input::{BattleCommand, CommitChoice};
    use legaia_asset::battle_ui_strings::BattleUiLabel;
    let s = world.minigames.muscle_dome.as_ref()?;
    if world.dialog.current.is_some() || world.dialog.inline.is_some() {
        return None;
    }
    let char_id = world.party_roster_slot(0) as u8 + 1;
    let idx = if s.ring(0).has_raseru && (1..=3).contains(&char_id) {
        char_id
    } else {
        4
    };
    let raseru = world
        .battle
        .ui_strings
        .raseru_label(idx)
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| {
            if idx == 4 {
                "-".to_string()
            } else {
                BattleCommand::Magic.label().to_string()
            }
        });
    let label = |which: BattleUiLabel, fallback: CommitChoice| {
        world
            .battle
            .ui_strings
            .get(which)
            .filter(|l| !l.is_empty())
            .unwrap_or(fallback.label())
            .to_string()
    };
    let begin = label(BattleUiLabel::CommitBegin, CommitChoice::Begin);
    let reselect = label(BattleUiLabel::Reselect, CommitChoice::Reselect);
    s.command_chips(&raseru, [&begin, &reselect])
}

pub use legaia_engine_vm::battle_commit_log::{CommitLogRow, CommitLogTarget};

/// The commit-log rows retail shows this frame, row 0 first.
///
/// Retail keeps the log up on the ring through the whole command phase and
/// **launches** it - slides it one display width off the left edge - when the
/// member leaves the ring for a sub-screen, sliding it back when they return
/// (`legaia_engine_vm::battle_commit_log::LogLaunch`, carried per row as
/// `slide_x`). The round's Begin is not a launch: the log leaves with the
/// command phase. A row belongs to each member the command
/// cursor has already walked past: every member ahead of the one entering a
/// command, and every committed member once the `Begin | Reselect` screen
/// (`0x6E`) is up. A member the `Reselect` step lands back on has its row
/// taken down (case `0x21` parks it at `x = 328`), which the "ahead of the
/// member entering" rule reproduces.
///
/// The Attack and Spirit rows' sources are read off the commit arms
/// (`FUN_801D388C` cases `0x20` / `0x11` / `0x23`); an Art commits through
/// the ring's `Attack` chip. Which chip record an Item or magic commit logs,
/// and whether an Item row carries a target, is inferred from the ring's
/// arm order (records `0x0C` / `0x0E`), not read off a commit arm.
pub fn battle_commit_log(world: &crate::world::World) -> Vec<CommitLogRow> {
    use crate::battle_input::CommandPhase;
    use crate::battle_round::{PendingPartyAction, RoundPhase};
    use crate::target_picker::CursorRow;
    use legaia_asset::battle_ui_strings::BattleUiLabel;
    use legaia_engine_vm::battle_commit_log as log;
    if world.mode != crate::world::SceneMode::Battle
        || world.battle.round_flow.phase != RoundPhase::Command
    {
        return Vec::new();
    }
    // An outbound launch that has landed has the log off the left edge.
    let slide_x = match world.battle.commit_log_launch {
        Some(l) if l.gone() => return Vec::new(),
        Some(l) => l.x_offset(),
        None => 0,
    };
    let pc = party_count(world) as u8;
    let confirm = world
        .battle
        .command
        .as_ref()
        .is_some_and(|c| matches!(c.phase, CommandPhase::CommitConfirm { .. }));
    let entering = world.battle_ctx.active_actor;
    let label = |disc: BattleUiLabel, fallback: &str| -> String {
        world
            .battle
            .ui_strings
            .get(disc)
            .filter(|s| !s.is_empty())
            .unwrap_or(fallback)
            .to_string()
    };
    let slot_target = |row: CursorRow, slot: u8| -> CommitLogTarget {
        let abs = match row {
            CursorRow::Enemy => pc.saturating_add(slot),
            CursorRow::Ally => slot,
        };
        CommitLogTarget::Single(actor_name(world, abs))
    };
    let mut rows = Vec::new();
    for slot in 0..pc {
        if !confirm && slot >= entering {
            break;
        }
        let Some(Some(action)) = world.battle.round_flow.pending.get(usize::from(slot)) else {
            continue;
        };
        let (command, command_record, target) = match action {
            PendingPartyAction::Attack { target } => (
                label(BattleUiLabel::Attack, "Attack"),
                log::RECORD_CHIP_ATTACK,
                CommitLogTarget::Single(actor_name(world, *target)),
            ),
            PendingPartyAction::Art {
                target_row,
                target_slot,
                ..
            } => (
                label(BattleUiLabel::Attack, "Attack"),
                log::RECORD_CHIP_ATTACK,
                slot_target(*target_row, *target_slot),
            ),
            PendingPartyAction::Spell {
                spell_id,
                target_row,
                target_slot,
            } => {
                use crate::spells::SpellTarget;
                let target = match world.tables.spell_catalog.get(*spell_id).map(|d| d.target) {
                    Some(SpellTarget::AllEnemies) => CommitLogTarget::AllEnemies,
                    Some(SpellTarget::AllAllies) => CommitLogTarget::AllAllies,
                    _ => slot_target(*target_row, *target_slot),
                };
                (
                    battle_magic_chip(world, slot).0,
                    log::RECORD_CHIP_MAGIC,
                    target,
                )
            }
            PendingPartyAction::Item { .. } => (
                label(BattleUiLabel::Item, "Item"),
                log::RECORD_CHIP_ITEM,
                CommitLogTarget::None,
            ),
            PendingPartyAction::Spirit => (
                label(BattleUiLabel::Spirit, "Spirit"),
                log::RECORD_CHIP_SPIRIT,
                CommitLogTarget::None,
            ),
            // `Run` begins the round at once and `StandBy` is no command:
            // neither reaches a logged commit.
            PendingPartyAction::Run | PendingPartyAction::StandBy => continue,
        };
        rows.push(CommitLogRow {
            name: party_member_name(world, slot),
            command,
            command_record,
            target,
            slide_x,
        });
    }
    rows
}

/// The combo cluster style the action in flight draws its hits in, or
/// `None` outside an action: a physical / arts chain counts `HIT` +
/// `TOTAL` (`player_steal_skeleton_banner`), a cast, item or spirit action
/// shows `DAMAGE` (`battle_gimard_tail_fire_a`). Capture-graded: the two
/// styles are read off those frames' display lists, not off a dispatch.
pub fn battle_combo_style(world: &crate::world::World) -> Option<ComboStyle> {
    use legaia_engine_vm::battle_action::ActionCategory;
    // The dome's play-out tally rides the status rows, not the battle
    // popup stream this cluster counts.
    if battle_hud_phase(world) != BattleHudPhase::Action
        || world.mode == crate::world::SceneMode::MuscleDome
        // A counterattack's strikes run under the monster's seeded HUD,
        // which opened no cluster ([`crate::world::BattleState::counter_hud`]).
        || world.battle.counter_hud.is_some()
    {
        return None;
    }
    let a = world.battle_ctx.active_actor;
    let cat = world.actors.get(a as usize)?.battle.action_category;
    if cat == ActionCategory::Attack.as_byte() || cat == ActionCategory::TacticalArts.as_byte() {
        Some(ComboStyle::HitTotal)
    } else if cat == ActionCategory::Magic.as_byte()
        || cat == ActionCategory::Item.as_byte()
        || cat == ActionCategory::Spirit.as_byte()
    {
        Some(ComboStyle::Damage)
    } else {
        None
    }
}
