//! The present-party composition a host installs on request: the native
//! window's `--party` flag and the browser play page's
//! `set_active_party_spec`. Both hosts used to carry the parser and the
//! install steps; this is the one copy, and each host keeps only its own
//! error and log plumbing.

use legaia_engine_core::world::World;

/// Parse a composition spec: comma-separated character names
/// (case-insensitive `vahn` / `noa` / `gala` / `terra`) or 0-based roster
/// indices, in battle order. Empty items are skipped, so an empty spec
/// parses to an empty list.
pub fn parse_party_spec(spec: &str) -> Result<Vec<u8>, String> {
    spec.split(',')
        .map(|t| t.trim())
        .filter(|t| !t.is_empty())
        .map(|t| match t.to_ascii_lowercase().as_str() {
            "vahn" => Ok(0u8),
            "noa" => Ok(1),
            "gala" => Ok(2),
            "terra" => Ok(3),
            other => other.parse::<u8>().map_err(|_| {
                format!("unknown party member '{t}' (use vahn/noa/gala/terra or a roster index)")
            }),
        })
        .collect()
}

/// Install `slots` (roster slots, battle order) as the present party, then
/// fold the records into the battle stat mirrors and seed the MP ceiling the
/// battle HUD draws.
///
/// `set_active_party` copies HP / MP only, so without the stat fold
/// (attack / defence split / SPD / AP base) a newly seated member fights at
/// zero attack; and the MP ceiling is the only one the world carries, which
/// nothing else seeds from a record. The composition caps at the three
/// on-screen positions; a caller compares `world.party.active_party` against
/// `slots` to report the cut.
///
/// Seeding roster slots that carry no stats yet
/// (`World::seed_party_members`) is the caller's step before this one: the
/// hosts read the SCUS new-game template from different places.
pub fn install_party_composition(world: &mut World, slots: Vec<u8>) {
    world.set_active_party(slots);
    world.seed_party_battle_stats();
    for member in 0..world.party.active_party.len() {
        let rslot = world.party_roster_slot(member);
        let mp_max = world
            .party
            .roster
            .members
            .get(rslot)
            .map(|r| r.hp_mp_sp().mp_max)
            .unwrap_or(0);
        world.set_character_max_mp(member as u8, mp_max);
    }
}

/// Split Terra out of a parsed spec: she is retail's battle companion, not a
/// field member. The party-table rebuild `FUN_801DE190` (PROT 0897) writes
/// the present members, then appends her, last, while system flag `0x13` is
/// up (`0x801DE204..0x801DE220`); retail never lists her in the field party,
/// never makes her the leader and never seats her ahead of a member. So a
/// spec naming her (by name or roster index `3`) seats the rest as the field
/// list and asks for the companion, wherever she was written. She needs one
/// or two members beside her, the shapes the engine's battle entry appends
/// her to (`terra_party_battle`: Noa and the wolf).
///
/// Returns the field list and whether the companion was named.
pub fn split_ai_companion(slots: &[u8]) -> Result<(Vec<u8>, bool), String> {
    let companion = World::AI_COMPANION_ROSTER_SLOT;
    if !slots.contains(&companion) {
        return Ok((slots.to_vec(), false));
    }
    let field: Vec<u8> = slots.iter().copied().filter(|&s| s != companion).collect();
    if field.is_empty() || field.len() > 2 {
        return Err(
            "terra fights only as the battle companion, last, beside one or two other members \
             (e.g. noa,terra)"
                .to_string(),
        );
    }
    Ok((field, true))
}

/// Seat a parsed spec: fill the roster slots it names from the new-game
/// template (`starting`, when the host could read it), then install the
/// composition. Records already carrying stats are left alone. Terra is
/// seated as retail seats her, the battle companion behind the field list
/// ([`split_ai_companion`]); a spec without her leaves the companion flag as
/// the story set it. Returns the field slots seated, which a caller compares
/// against the request for the cut to the three on-screen positions, or the
/// spec's error. The native window's `--party` and Tools row and the
/// browser's party field all come through here, so a member who has not
/// joined yet fights with stats on every host.
pub fn seat_party_spec(
    world: &mut World,
    starting: Option<&legaia_game_tables::new_game::StartingParty>,
    spec: &str,
) -> Result<Vec<u8>, String> {
    let slots = parse_party_spec(spec)?;
    if slots.is_empty() {
        return Err("empty party spec".to_string());
    }
    let (field, companion) = split_ai_companion(&slots)?;
    if let Some(starting) = starting {
        world.seed_party_members(starting, &slots);
    }
    if companion {
        world.seat_ai_companion();
    }
    install_party_composition(world, field.clone());
    Ok(field)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_and_indices_parse_in_battle_order() {
        assert_eq!(parse_party_spec("Noa, terra,0"), Ok(vec![1, 3, 0]));
        assert_eq!(parse_party_spec(" , "), Ok(vec![]));
        assert!(parse_party_spec("vahn,rim").is_err());
    }

    #[test]
    fn terra_seats_as_the_battle_companion_behind_the_field_list() {
        assert_eq!(split_ai_companion(&[1, 3]), Ok((vec![1], true)));
        // Written first, she still follows: the rebuild appends her last.
        assert_eq!(split_ai_companion(&[3, 0, 1]), Ok((vec![0, 1], true)));
        assert_eq!(split_ai_companion(&[0, 1, 2]), Ok((vec![0, 1, 2], false)));
        // Alone, or behind a full field list, is no seat retail makes.
        assert!(split_ai_companion(&[3]).is_err());
        assert!(split_ai_companion(&[0, 1, 2, 3]).is_err());
    }

    #[test]
    fn a_terra_spec_raises_the_companion_flag_and_keeps_her_off_the_field_list() {
        let mut world = World::new();
        assert_eq!(seat_party_spec(&mut world, None, "terra,noa"), Ok(vec![1]));
        assert_eq!(world.party.active_party, vec![1]);
        assert!(world.system_flag_test(0x13));
    }
}
