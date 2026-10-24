//! The battle's result windows: the spoils report a win raises (screen
//! element `0x41`) and the loss window a wipe raises (`0x42`), composed off
//! the world's rewards, roster and the executable's strings.
//!
//! When a window is up is the simulation's fact (the victory sequence's
//! `window_opened`, `World::battle_spoils_window_up`); what it says - the
//! drop and level-up lines, the leader's name, the defeat line - is composed
//! here, and both hosts draw it through `engine-screens`.
//!
//! REF: FUN_8004E568, FUN_801D84C0

use legaia_engine_battle_vm::battle_action::BattleEndCause;
use legaia_engine_core::world::World;

/// One line of the post-battle spoils panel's variable block, already
/// resolved against the world's item catalog / roster.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BattleSpoilsBanner {
    /// The per-member EXP share ([`BattleRewards::xp_share`](legaia_engine_core::world::BattleRewards::xp_share)) -
    /// what retail's result window prints, not the pool.
    pub xp: u32,
    pub gold: u32,
    /// The report window's drop line, one per item the loot roll surfaced
    /// (retail's roll surfaces at most one): the executable's template with
    /// the item name spliced in ([`drop_line`]), or `Got <name>` on a
    /// host without it.
    pub drops: Vec<String>,
    /// The level-up window's line - empty when nobody levelled. Retail
    /// opens one window, element `0x44 + mask` (bit `k` = character `k`),
    /// whose string names every character that crossed a threshold in one
    /// line, or nobody when all three did; the port reads the seven strings
    /// off the user's executable ([`level_up_line`]). Without them (a
    /// disc-free host) it falls back to one `"<name>'s level increased!"`
    /// line per character. The new level is not on the line (the status
    /// screen carries it).
    pub level_ups: Vec<String>,
    /// Who the victory line names: the lead alone when the second party seat
    /// is empty, else the lead's team (`FUN_801D84C0`'s two build arms).
    pub subject: legaia_engine_battle_vm::battle_party_panel::ResultSubject,
    /// `(level_up, report)` rows below rest - the windows' raise glide
    /// ([`BattleState::result_windows_dy`](legaia_engine_core::world::BattleState::result_windows_dy)).
    pub slide: (i32, i32),
}

/// The loss window's content ([`battle_defeat_banner`]).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BattleDefeatBanner {
    /// The one line the window shows, or `None` without the disc pool.
    pub line: Option<String>,
    /// Rows below rest - the window's raise glide, the report window's.
    pub slide_y: i32,
}

/// The party leader whose name opens the post-battle spoils line: the
/// member in party slot 0, or `Vahn` when that record carries no name
/// (a disc-free build's blank roster). Both hosts used to resolve it
/// with a copy of this lookup each.
pub fn battle_spoils_leader(world: &World) -> String {
    world
        .party
        .roster
        .members
        .get(world.party_roster_slot(0))
        .map(|m| m.name())
        .filter(|n| !n.trim().is_empty())
        .unwrap_or_else(|| "Vahn".to_string())
}

/// The post-battle spoils panel a host should be drawing this frame, or
/// `None` when the panel is not up.
///
/// Resolves drop item ids through [`DiscTables::item_catalog`](legaia_engine_core::world::DiscTables::item_catalog) and level-up
/// character slots through [`legaia_engine_menus::party_state::PartyState::roster`], so a host needs no table of
/// its own. Falls back to `Item <id>` / `Member <n>` when a name is
/// unavailable (a disc-free build's synthetic catalog).
pub fn battle_spoils_banner(world: &World) -> Option<BattleSpoilsBanner> {
    // The gate (results frame through the exit, the aging window, no
    // special battle, no wipe - [`battle_defeat_banner`] answers that) is the
    // simulation's: `World::battle_spoils_window_up`.
    if !world.battle_spoils_window_up() {
        return None;
    }
    let r = world.battle.last_rewards.as_ref()?;
    let drops = r
        .drops
        .iter()
        .map(|&id| {
            let name = world
                .tables
                .item_catalog
                .get(id)
                .map(|it| it.name.to_string())
                .unwrap_or_else(|| format!("Item {id}"));
            drop_line(world, &name).unwrap_or_else(|| format!("Got {name}"))
        })
        .collect();
    let mask = r
        .level_ups
        .iter()
        .filter(|lu| lu.char_id < 3)
        .fold(0u8, |m, lu| m | (1 << lu.char_id));
    let level_ups = if let Some(line) = level_up_line(world, mask) {
        vec![line]
    } else {
        r.level_ups
            .iter()
            .map(|lu| {
                let slot = lu.char_id as usize;
                let name = world
                    .party
                    .roster
                    .members
                    .get(slot)
                    .map(|m| m.name())
                    .filter(|n| !n.trim().is_empty())
                    .unwrap_or_else(|| format!("Member {}", slot + 1));
                format!("{name}\'s level increased!")
            })
            .collect()
    };
    Some(BattleSpoilsBanner {
        xp: r.xp_share,
        gold: r.gold,
        drops,
        level_ups,
        subject: battle_result_subject(world),
        slide: world.battle.result_windows_dy(),
    })
}

/// The report window's drop line for an item named `item`: the
/// executable's template (`legaia_ui_formats::screen_elements::DROP_LINE_PTR_VA`)
/// with its `0xC2` item escape spliced, less the leading `0x7C` break
/// that puts it on the window's third row (the port keeps rows as
/// lines). `None` without the template.
///
/// REF: FUN_8004E568 (`0x8004F5C4..0x8004F600`)
pub fn drop_line(world: &World, item: &str) -> Option<String> {
    let raw = world.menu.text.as_ref()?.drop_line.as_ref()?;
    let mut out = String::new();
    let mut i = 0;
    while i < raw.len() {
        let b = raw[i];
        if (0xC0..=0xCF).contains(&b) {
            if matches!(b, 0xC2 | 0xC4) {
                out.push_str(item);
            }
            i += 2;
            continue;
        }
        if (0x20..0x7F).contains(&b) && b != 0x7C {
            out.push(b as char);
        }
        i += 1;
    }
    Some(out)
}

/// The level-up window's line for `mask` (bit `k` = character `k`): the
/// string record `0x44 + mask` points at, its `0xC1 k` name escapes
/// spliced with record `k`'s name (`0x63` = the party leader). `None`
/// for an empty mask or without the strings.
///
/// REF: FUN_8004E568 (`0x8004F6F8..0x8004F728`, the mask and the raise)
pub fn level_up_line(world: &World, mask: u8) -> Option<String> {
    let raw = world
        .menu
        .text
        .as_ref()?
        .level_up_lines
        .as_ref()?
        .get(usize::from(mask).checked_sub(1)?)?;
    Some(world.expand_name_escapes(raw))
}

/// Who the battle-result messages name: participant ids in panel order,
/// `0` for an empty seat - the shape of retail's `0x8007BD10` list the
/// two build arms of `FUN_801D84C0` key on.
fn battle_result_subject(
    world: &World,
) -> legaia_engine_battle_vm::battle_party_panel::ResultSubject {
    let seats: [u8; 3] = std::array::from_fn(|i| {
        if i < usize::from(world.party.party_count) {
            (world.party_roster_slot(i) as u8).wrapping_add(1)
        } else {
            0
        }
    });
    legaia_engine_battle_vm::battle_party_panel::result_subject(seats)
}

/// The loss window a host should be drawing this frame, or `None`.
///
/// Retail's wipe arm of the results frame opens screen element `0x42`
/// (`FUN_801D8DE8(0x42, 0)` at `0x8004F900`) unless the special-battle
/// word is set (`0x8004F8F0`) - the same gate as the win arm's result
/// window, carried by `VictorySequence::window_opened`. The element's
/// placement record is the win window's twin (same frame, same seat), and
/// its string is the defeat buffer `FUN_801D84C0` built at battle start:
/// the lead's name plus the solo suffix, or the lead's team line.
///
/// Up from the results frame through the exit, like the win window. The
/// line is `None` on a disc-free host (no PROT 0898 pool), where the
/// window opens empty.
pub fn battle_defeat_banner(world: &World) -> Option<BattleDefeatBanner> {
    let v = world.battle.victory?;
    if v.cause != BattleEndCause::PartyWipe || !v.window_opened || !v.results_shown() {
        return None;
    }
    let subject = battle_result_subject(world);
    let lead = world
        .party
        .roster
        .members
        .get(world.party_roster_slot(0))
        .map(|m| m.name())
        .filter(|n| !n.trim().is_empty());
    let line = world
        .tables
        .defeat_text
        .as_ref()
        .zip(lead)
        .map(|(t, lead)| t.compose(subject, &lead));
    Some(BattleDefeatBanner {
        line,
        slide_y: world.battle.result_windows_dy().1,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The window's line is the mask's string with its `0xC1` name escapes
    /// spliced: record `k` for operand `k`, the leader for `0x63`. Synthetic
    /// strings stand in for the executable's.
    #[test]
    fn the_level_up_line_splices_the_mask_string() {
        let mut w = World::default();
        w.party.roster = legaia_save::Party::zeroed(3);
        for (k, name) in ["Ana", "Bo", "Cy"].iter().enumerate() {
            w.party.roster.members[k].set_name(name);
        }
        let mut lines = vec![b"x".to_vec(); 7];
        lines[0] = [&[0xC1, 0x00][..], b" up"].concat();
        lines[2] = [&[0xC1, 0x00][..], b" & ", &[0xC1, 0x01], b" up"].concat();
        lines[6] = b"all up".to_vec();
        w.menu.text = Some(legaia_engine_menus::pause_screens::MenuTextTables {
            level_up_lines: Some(lines),
            ..Default::default()
        });
        assert_eq!(level_up_line(&w, 1).as_deref(), Some("Ana up"));
        assert_eq!(level_up_line(&w, 3).as_deref(), Some("Ana & Bo up"));
        assert_eq!(level_up_line(&w, 7).as_deref(), Some("all up"));
        assert_eq!(level_up_line(&w, 0), None, "nobody levelled");
        w.menu.text = None;
        assert_eq!(
            level_up_line(&w, 1),
            None,
            "no strings: the caller falls back"
        );
    }

    /// The drop line is the template with its item escape spliced and the
    /// leading row break dropped.
    #[test]
    fn the_drop_line_splices_the_item() {
        let mut w = World::default();
        assert_eq!(drop_line(&w, "Leaf"), None);
        w.menu.text = Some(legaia_engine_menus::pause_screens::MenuTextTables {
            drop_line: Some([&b"|Got the "[..], &[0xC2, 0x01], b"."].concat()),
            ..Default::default()
        });
        assert_eq!(drop_line(&w, "Leaf").as_deref(), Some("Got the Leaf."));
    }
}
