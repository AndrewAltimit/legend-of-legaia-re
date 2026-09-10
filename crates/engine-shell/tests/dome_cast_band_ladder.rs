//! The Ra-Seru cast, end to end, on the two seams a host actually drives:
//! the **Muscle Dome's** magic command class and the **ordinary battle's**
//! cast band.
//!
//! Both rungs assert by *output* rather than by call. Rung 1 charges MP,
//! leaves the AP budget alone and drops the opponent's HP through the shared
//! session; rung 2 drives the summon band's per-frame seam and requires the
//! resident slot-B module's own phase byte to have advanced, which only a
//! **ported** tick kernel can do - an unported entry leaves it where it was.
//!
//! Rung 1 is disc-gated (`LEGAIA_DISC_BIN`): its spell prices and its
//! opponent are the disc's. Rung 2 is disc-free by construction - the module
//! dispatch is table arithmetic and the tick kernels are pure - so it runs in
//! CI unconditionally and is the non-vacuous half when no disc is present.

use legaia_engine_core::muscle_dome::{
    DomeCastRefusal, DomeMagic, DomeRing, DomeRingChip, MuscleCard, MuscleDomeSession, MusclePhase,
};

/// Spell id whose band entry - PROT `903 + (0x87 - 0x81)` = 909 - has a
/// **ported** tick kernel (`cast_module_ticks::viguro_stager`). Nine of the
/// eleven player Seru ids name an entry with none, so the phase-advance
/// assertion below has to pick one of the two that do.
const PORTED_SERU_SPELL: u8 = 0x87;

/// The disc-derived corpus both disc-gated rungs read `SCUS_942.54` from.
/// Gated on `LEGAIA_DISC_BIN` first, so an unset env var skips exactly as
/// every other disc-gated test in the workspace does.
fn scus() -> Option<Vec<u8>> {
    std::env::var_os("LEGAIA_DISC_BIN")
        .map(std::path::PathBuf::from)
        .filter(|p| p.exists())?;
    ["extracted", "../extracted", "../../extracted"]
        .into_iter()
        .map(std::path::PathBuf::from)
        .find(|d| d.join("SCUS_942.54").is_file())
        .and_then(|d| std::fs::read(d.join("SCUS_942.54")).ok())
}

fn card(id: u8, cost: u16) -> MuscleCard {
    MuscleCard {
        command_id: id,
        cost,
    }
}

fn hand() -> [MuscleCard; 4] {
    [
        card(0x0C, 30),
        card(0x0D, 30),
        card(0x0E, 30),
        card(0x0F, 30),
    ]
}

/// Rung 1 - a dome fighter with a Ra-Seru casts, and the three observables
/// are the ones retail's arm produces: MP down by the discounted cost, the
/// AP budget untouched (a cast draws no pennant because it debits no AP),
/// and the opponent's HP down.
#[test]
fn a_dome_fighter_with_a_seru_casts_and_pays_in_mp_not_ap() {
    let Some(scus) = scus() else {
        eprintln!("[skip] no LEGAIA_DISC_BIN / extracted SCUS - the rung needs disc spell prices");
        return;
    };
    let catalog = legaia_engine_core::retail_magic::seru_magic_catalog_from_scus(&scus)
        .expect("the player Seru block parses off SCUS_942.54");
    let mut spells: Vec<_> = catalog.iter().cloned().collect();
    spells.sort_by_key(|s| s.id);
    assert!(
        !spells.is_empty(),
        "the disc's player Seru block is non-empty"
    );
    // The block's two ally-side entries heal rather than hit; the HP
    // assertion below needs one that damages.
    let first = spells
        .iter()
        .find(|s| {
            matches!(
                s.effect,
                legaia_engine_core::spells::SpellEffect::Damage { .. }
            ) && s.mp_cost > 0
        })
        .cloned()
        .expect("the disc's player Seru block carries a damaging spell");
    let cost = u16::from(first.mp_cost);

    let mut s = MuscleDomeSession::new(hand(), hand(), [100, 100], [900, 900], 1);
    s.install_magic(
        0,
        DomeMagic {
            ring: DomeRing {
                special: 0,
                status: 0,
                has_raseru: true,
            },
            mp: cost + 20,
            mp_max: cost + 20,
            ability_bits: 0,
            magic_power: 80,
            spells,
        },
    );

    // The chip is live, and it wears no mark: no dome round raises the
    // special-battle word's magic bit.
    assert!(s.chip_enabled(0, DomeRingChip::RaSeru));
    assert_eq!(s.chip_mark(0, DomeRingChip::RaSeru), None);

    let budget_before = s.budget(0);
    let charged = s.commit_cast(0, first.id).expect("the gauge covers it");
    assert_eq!(charged, cost, "the disc's own MP price");
    assert_eq!(
        s.budget(0),
        budget_before,
        "a cast spends MP, not AP - the arm never reads ctx+0x6DC"
    );
    assert_eq!(s.spent(0), 0, "and so it draws no pennant");

    s.ai_commit_all(1);
    s.end_selection();
    let foe_before = s.hp(1);
    s.resolve_turn(|_, _| 0);
    assert_eq!(s.mp(0), 20, "charged exactly once, at the play-out");
    assert!(
        s.hp(1) < foe_before,
        "the cast landed on the opponent (before {foe_before}, after {})",
        s.hp(1)
    );
    assert_ne!(s.phase(), MusclePhase::Select);
    eprintln!(
        "[ok] dome cast {:#04x} charged {cost} MP, left AP at {budget_before}, dealt {}",
        first.id,
        foe_before - s.hp(1)
    );
}

/// Rung 1b - the refusals, on the disc's own list. A gauge below the price
/// leaves the turn uncommitted and the gauge untouched, which is retail's
/// `0x8007BB94 = 0` arm.
#[test]
fn a_dome_cast_the_gauge_cannot_cover_is_refused() {
    let Some(scus) = scus() else {
        eprintln!("[skip] no LEGAIA_DISC_BIN / extracted SCUS - the refusal rung needs it");
        return;
    };
    let catalog = legaia_engine_core::retail_magic::seru_magic_catalog_from_scus(&scus)
        .expect("the player Seru block parses");
    let mut spells: Vec<_> = catalog.iter().cloned().collect();
    spells.sort_by_key(|s| s.id);
    let first = spells
        .iter()
        .find(|s| s.mp_cost > 0)
        .cloned()
        .expect("the block carries a spell that costs MP");

    let mut s = MuscleDomeSession::new(hand(), hand(), [100, 100], [900, 900], 1);
    s.install_magic(
        0,
        DomeMagic {
            ring: DomeRing {
                special: 0,
                status: 0,
                has_raseru: true,
            },
            mp: 0,
            mp_max: 40,
            ability_bits: 0,
            magic_power: 80,
            spells,
        },
    );
    assert_eq!(
        s.commit_cast(0, first.id),
        Err(DomeCastRefusal::NotEnoughMp)
    );
    assert_eq!(s.queued_cast(0), None);
    assert_eq!(s.mp(0), 0);
    eprintln!("[ok] an unaffordable dome cast commits nothing");
}

/// Rung 2 - a player Seru cast in an ordinary battle reaches the band's
/// per-frame seam and a **ported** module kernel runs there.
///
/// The observable is the resident module's own phase byte (`ctx+0x279`,
/// mirrored as `World::cast_module_phase`): the band arms it at zero, and
/// only a ported tick body advances it. An entry whose code half is unported
/// leaves it at zero, which is what makes this assertion non-vacuous.
#[test]
fn a_player_seru_cast_reaches_a_ported_module_kernel() {
    use legaia_engine_core::world::{Actor, World};

    // Two combatants is all the seam needs: the caster and something to
    // retarget onto.
    let mut world = World {
        party_count: 1,
        ..World::default()
    };
    while world.actors.len() < 8 {
        world.actors.push(Actor::default());
    }
    for a in world.actors.iter_mut() {
        a.active = true;
        a.battle.hp = 400;
        a.battle.max_hp = 400;
        a.battle.liveness = 1;
    }
    world.spell_catalog = legaia_engine_core::retail_magic::retail_seru_magic_catalog();
    world.battle_ctx.active_actor = 0;

    // The band's own arming seam, the one `spell_anim_trigger` runs for a
    // Seru id, then the per-frame stager tick the action SM drives at states
    // 0x34 / 0x35 / 0x36.
    let entry = world
        .cast_module_for(PORTED_SERU_SPELL)
        .expect("the Seru dispatch names a band entry");
    assert_eq!(entry, 909, "0x87 pages PROT 903 + (0x87 - 0x81)");
    world.arm_summon_stager(0, PORTED_SERU_SPELL);
    assert_eq!(world.cast_module_phase, 0, "the arm zeroes the phase pair");

    let busy = world.summon_stager_tick();
    assert!(busy, "the stager holds while it choreographs");
    assert_eq!(
        world.cast_module_phase, 1,
        "PROT 0909's ported kernel advanced the module phase at the band seam"
    );
    eprintln!(
        "[ok] cast {PORTED_SERU_SPELL:#04x} -> PROT {entry}, module phase {}",
        world.cast_module_phase
    );
}
