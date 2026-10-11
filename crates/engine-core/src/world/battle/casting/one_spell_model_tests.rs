use super::*;
use crate::world::vm_hosts::BattleHostImpl;
use legaia_engine_battle::spells::SpellCatalog;
use legaia_engine_battle_vm::battle_action::BattleActionHost;
use legaia_game_tables::spell_names::{CAPTURE_CLASS, SpellEntry, SpellNameTable};

/// **The canary.** For every spell the catalog prices, the host the engine
/// actually installs must quote that exact price - so a spell the player
/// pays 24 MP for through the menu costs 24 MP through the state machine
/// too.
///
/// A host method that has drifted off the catalog (wired to a table nobody
/// fills, deleted back to the trait default, pointed at a second model)
/// shows up here as a `0`, or as a mismatch.
#[test]
fn the_battle_host_prices_every_catalog_spell_exactly_as_the_catalog_does() {
    let mut world = World::new();
    world.set_spell_catalog(SpellCatalog::vanilla());
    let priced: Vec<(u8, u8)> = world
        .tables
        .spell_catalog
        .iter()
        .map(|s| (s.id, s.mp_cost))
        .collect();
    assert!(
        priced.iter().filter(|(_, mp)| *mp > 0).count() >= 10,
        "the fixture must actually price spells, or this test is vacuous"
    );

    let host = BattleHostImpl { world: &mut world };
    for (id, mp) in priced {
        assert_eq!(
            host.spell_mp_cost(id),
            mp,
            "spell {id:#04x} is priced {mp} by the catalog the live cast path \
             charges from; the battle-action host must not answer anything else"
        );
        if mp > 0 {
            assert_ne!(
                host.spell_mp_cost(id),
                0,
                "spell {id:#04x} costs MP - a host quoting 0 is free magic"
            );
        }
    }
}

/// The same canary for the retail block: the ids a real save can actually
/// cast are priced by the host at their byte-exact SCUS costs.
#[test]
fn the_battle_host_prices_the_retail_seru_block() {
    let mut world = World::new();
    world.set_spell_catalog(legaia_engine_battle::retail_magic::retail_seru_magic_catalog());
    let host = BattleHostImpl { world: &mut world };
    for s in legaia_engine_battle::retail_magic::SERU_MAGIC {
        assert_eq!(
            host.spell_mp_cost(s.id),
            s.mp,
            "{} ({:#04x}) costs {} MP in retail",
            s.name,
            s.id,
            s.mp
        );
    }
}

/// The state machine's capture route and the live path's damage-kernel
/// pick read the **same** class byte, so they cannot disagree about a
/// record. Contrast both ways: no table installed → neither fires.
#[test]
fn the_capture_route_and_the_kernel_pick_share_one_class_byte() {
    const ID: u8 = 0x37;

    let mut bare = World::new();
    assert!(
        !bare.is_capture_class_move(ID),
        "no disc table: the kernel pick sees no capture class"
    );
    {
        let host = BattleHostImpl { world: &mut bare };
        assert!(
            !host.is_capture_spell(ID),
            "no disc table: the SM must not route to the capture branch either"
        );
    }

    let mut world = World::new();
    let mut entries = vec![SpellEntry::default(); 0x100];
    entries[ID as usize].class = CAPTURE_CLASS;
    world.menu.text = Some(legaia_engine_menus::pause_screens::MenuTextTables {
        spell_names: Some(SpellNameTable::from_entries(entries)),
        ..Default::default()
    });
    assert!(world.is_capture_class_move(ID));
    let host = BattleHostImpl { world: &mut world };
    assert!(
        host.is_capture_spell(ID),
        "the class byte that routes the damage kernel must also route the SM"
    );
    assert!(
        !host.is_capture_spell(ID.wrapping_add(1)),
        "and only that record - the predicate is per-id, not a constant"
    );
}

/// The seed discriminator reads the same accessor: a low-id, low-class
/// record is the one shape retail sends to the Spirit band instead of the
/// magic band.
#[test]
fn the_seed_discriminator_reads_the_same_class_byte() {
    let mut world = World::new();
    let mut entries = vec![SpellEntry::default(); 0x100];
    entries[0x10].class = 0x02; // < 0x14, id < 0x65 -> Spirit band
    entries[0x11].class = 0x32; // >= 0x14          -> magic band
    world.menu.text = Some(legaia_engine_menus::pause_screens::MenuTextTables {
        spell_names: Some(SpellNameTable::from_entries(entries)),
        ..Default::default()
    });
    let host = BattleHostImpl { world: &mut world };
    assert_eq!(host.spell_class_byte(0x10), Some(0x02));
    assert_eq!(host.spell_class_byte(0x11), Some(0x32));
    assert_eq!(
        host.spell_class_byte(0x81),
        Some(0),
        "an unpopulated record still exists - only a missing table is None"
    );
}

/// MP is charged **once**, by whichever path runs the cast.
///
/// The live path debits in [`World::cast_spell_on_slots`]; the state
/// machine debits at `MagicCastBegin` / `SpiritPreArm` from
/// `spell_mp_cost`. They now quote the same number, which is what makes
/// "run both for one cast" a visible double charge rather than a silent
/// discrepancy - and why the live path parks at `EndOfAction` instead of
/// entering the magic band.
#[test]
fn both_cast_paths_quote_the_same_price_so_a_double_charge_would_be_visible() {
    let mut world = World::new();
    world.party.party_count = 1;
    world.set_spell_catalog(legaia_engine_battle::retail_magic::retail_seru_magic_catalog());
    for i in 0..2usize {
        let a = world.spawn_actor(i);
        a.battle.liveness = 1;
        a.battle.hp = 500;
        a.battle.max_hp = 500;
        a.battle.mp = 100;
    }
    let def = world
        .tables
        .spell_catalog
        .get(0x82)
        .cloned()
        .expect("Theeder is in the retail block");
    assert_eq!(def.mp_cost, 24);

    world.cast_spell_on_slots(0, &def, &[1]);
    let live_charge = 100 - world.actors[0].battle.mp;
    assert_eq!(live_charge, 24, "the live path charges the catalog price");

    let host = BattleHostImpl { world: &mut world };
    assert_eq!(
        host.spell_mp_cost(0x82) as u16,
        live_charge,
        "the state machine would charge the very same MP for this cast"
    );
}

/// The prepaid fold re-checks affordability through the shared
/// `cast_spell` gate, so that gate must read the **discounted** price the
/// band's `0x28` charged (`0x801E4568`, the inline `FUN_80035394` fold).
/// A Half-bit caster (`+0xF4 & 0x20`) who had exactly the discounted
/// Theeder price (24 -> 12) used to fold a `NotEnoughMp` failure against
/// the raw 24 and deal no damage.
#[test]
fn a_prepaid_mp_saver_cast_resolves_at_exactly_the_discounted_price() {
    let mut world = World::new();
    world.party.party_count = 1;
    world.party.character_ability_bits[0] = 0x20;
    world.set_spell_catalog(legaia_engine_battle::retail_magic::retail_seru_magic_catalog());
    for i in 0..2usize {
        let a = world.spawn_actor(i);
        a.battle.liveness = 1;
        a.battle.hp = 500;
        a.battle.max_hp = 500;
        a.battle.mp = 0;
    }
    world.battle.magic[0] = 40;
    let def = world
        .tables
        .spell_catalog
        .get(0x82)
        .cloned()
        .expect("Theeder is in the retail block");
    assert_eq!(legaia_engine_battle::spells::caster_mp_cost(&def, 0x20), 12);
    // The band already charged 12 of the caster's 12: 0 left.
    world.cast_spell_on_slots_prepaid(0, &def, &[1]);
    assert!(
        world.actors[1].battle.hp < 500,
        "the cast resolved against the discounted price"
    );
    // The direct path charges the same discounted price.
    world.actors[0].battle.mp = 12;
    world.cast_spell_on_slots(0, &def, &[1]);
    assert_eq!(world.actors[0].battle.mp, 0);
}
