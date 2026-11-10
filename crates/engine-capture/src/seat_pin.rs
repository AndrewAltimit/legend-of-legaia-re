//! The seat order a capture's fight was seated in, with its environment
//! hand-off (`LEGAIA_BATTLE_SEATS`).
//!
//! Battle init rolls a two-species formation's order (`FUN_80055B6C`,
//! `0x80055C7C..0x80055D2C`) and battle setup draws each body's opening clip
//! phase (`FUN_800513F0`). A retail capture stands after both: its formation
//! cells `0x8007BD0C[0..4]` hold the order as rolled, and its anim nodes the
//! phases as they ran on. An engine entry that rolled again would seat
//! another fight than the capture's half the time, so both comparison sides
//! hand the entry the capture's own order
//! (`EncounterState::seat_pin`), which the entry seats without a draw. A
//! play host never sets it.

use legaia_engine_core::world::World;

/// Pin the next battle entry to `ids`, the capture's monster ids in seat
/// order. An empty list pins nothing.
pub fn apply_seat_pin(world: &mut World, ids: &[u8]) {
    world.encounters.seat_pin =
        (!ids.is_empty()).then(|| ids.iter().map(|&i| u16::from(i)).collect());
}

/// `LEGAIA_BATTLE_SEATS`: the ids as comma-separated hex (`07,04`).
pub fn seat_pin_to_env(ids: &[u8]) -> String {
    ids.iter()
        .map(|i| format!("{i:02x}"))
        .collect::<Vec<_>>()
        .join(",")
}

/// The inverse of [`seat_pin_to_env`]; `None` for a malformed or empty list.
pub fn seat_pin_from_env(v: &str) -> Option<Vec<u8>> {
    let ids = v
        .split(',')
        .map(|t| u8::from_str_radix(t.trim(), 16).ok())
        .collect::<Option<Vec<u8>>>()?;
    (!ids.is_empty()).then_some(ids)
}

/// The order battle init's rebuild leaves `ids` in on an odd draw, when it
/// rewrites them at all (`battle_seats::rebuild_species_order`): the order a
/// capture of a row authored as `ids` may hold instead.
pub fn rebuilt_order(ids: &[u16]) -> Option<Vec<u16>> {
    let mut cells = [0u16; 4];
    for (c, id) in cells.iter_mut().zip(ids) {
        *c = *id;
    }
    legaia_engine_battle::battle_seats::rebuild_species_order(&mut cells, || 1).then(|| {
        cells
            .iter()
            .copied()
            .take_while(|&id| id != 0)
            .chain(ids.iter().skip(4).copied())
            .collect()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_round_trips() {
        let ids = vec![0x07, 0x04, 0xff];
        assert_eq!(seat_pin_from_env(&seat_pin_to_env(&ids)), Some(ids));
        assert_eq!(seat_pin_from_env(""), None);
        assert_eq!(seat_pin_from_env("zz"), None);
    }

    #[test]
    fn rebuilt_order_fronts_the_other_species() {
        assert_eq!(rebuilt_order(&[4, 7]), Some(vec![7, 4]));
        assert_eq!(rebuilt_order(&[4, 4, 7]), Some(vec![7, 4, 4]));
        assert_eq!(rebuilt_order(&[4, 4]), None);
        assert_eq!(rebuilt_order(&[4]), None);
    }

    #[test]
    fn a_pinned_entry_seats_the_capture_order_and_draws_nothing() {
        let mut world = World::new();
        let cell = crate::formation_cell::FormationCell {
            ids: vec![4, 4, 7],
            scripted: false,
        };
        let fid = cell.register(&mut world, "test", || None).unwrap();
        let def = world.tables.formation_table.formation(fid).unwrap().clone();
        apply_seat_pin(&mut world, &[7, 4, 4]);
        let rng = world.rng_state;
        let seated = world.enter_battle_from_formation(&def);
        let ids: Vec<u16> = seated.slots.iter().map(|s| s.monster_id).collect();
        assert_eq!(ids, vec![7, 4, 4]);
        assert_eq!(world.rng_state, rng, "a pinned entry draws nothing");
        assert!(
            world.encounters.seat_pin.is_none(),
            "the entry takes the pin"
        );
        // The next entry rolls again.
        world.enter_battle_from_formation(&def);
        assert_ne!(world.rng_state, rng);
    }
}
