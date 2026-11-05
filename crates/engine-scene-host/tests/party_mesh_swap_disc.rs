//! Disc-gated: the party's Ra-Seru mesh swap reaches the pool both hosts draw.
//!
//! `FUN_8001EBEC` copies one of two template groups over a visible group by the
//! character's Ra-Seru byte. `SceneHost::player_rig_mesh` and the party
//! placements both read `World`'s head of the TMD pool, so the descriptor in
//! that pool is what either host draws.
//!
//! Skips + passes when `LEGAIA_DISC_BIN` / extracted assets are missing.

use legaia_actor_formats::character_pack::equipment_swap::ACTIVE_PARTY_SLOTS;
use legaia_actor_formats::character_pack::{
    EQUIP_GROUP_NONZERO_OFFSET, EQUIP_GROUP_ZERO_OFFSET, FIRST_GROUP_DESCRIPTOR_OFFSET,
    GROUP_DESCRIPTOR_BYTES,
};
use legaia_engine_scene_host::scene_host::SceneHost;
use std::path::PathBuf;

fn extracted_dir() -> Option<PathBuf> {
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

/// Does each party slot's visible group hold the template `template_off`?
fn holds(host: &SceneHost, template_off: usize) -> Vec<bool> {
    ACTIVE_PARTY_SLOTS
        .iter()
        .map(|p| {
            let g = host.world.global_tmd_pool[usize::from(p.slot)]
                .as_ref()
                .expect("party mesh in the pool");
            let dst = FIRST_GROUP_DESCRIPTOR_OFFSET
                + usize::from(p.patched_group_index) * GROUP_DESCRIPTOR_BYTES;
            g.raw[dst..dst + GROUP_DESCRIPTOR_BYTES]
                == g.raw[template_off..template_off + GROUP_DESCRIPTOR_BYTES]
        })
        .collect()
}

#[test]
fn the_pool_follows_the_ra_seru_byte_at_entry_and_on_4c_3f() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        return;
    };
    let mut host = SceneHost::open_extracted(&extracted).expect("open extracted");
    host.world.party.roster.members = vec![legaia_save::character::CharacterRecord::zeroed(); 3];
    host.enter_field_scene("town01", 0).expect("enter town01");
    assert_eq!(
        holds(&host, EQUIP_GROUP_ZERO_OFFSET),
        [true; 3],
        "cold look"
    );
    // The entry's own pass, owed to a host that seats the roster late.
    host.tick().expect("tick");

    for p in ACTIVE_PARTY_SLOTS {
        let member = &mut host.world.party.roster.members[usize::from(p.slot)];
        let mut eq = member.equipment();
        eq.slots[usize::from(p.equip_byte_record_offset - 0x196)] = 2;
        member.set_equipment(eq);
    }
    // No request, no swap: a menu's equip waits for the next load or `4C 3F`.
    host.tick().expect("tick");
    assert_eq!(holds(&host, EQUIP_GROUP_ZERO_OFFSET), [true; 3]);

    host.world.request_party_mesh_swap();
    host.tick().expect("tick");
    assert_eq!(holds(&host, EQUIP_GROUP_NONZERO_OFFSET), [true; 3]);
    assert!(
        host.world.take_player_rig_change(),
        "a rewritten lead tells the hosts to rebuild its rig"
    );
}
