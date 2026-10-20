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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_and_indices_parse_in_battle_order() {
        assert_eq!(parse_party_spec("Noa, terra,0"), Ok(vec![1, 3, 0]));
        assert_eq!(parse_party_spec(" , "), Ok(vec![]));
        assert!(parse_party_spec("vahn,rim").is_err());
    }
}
