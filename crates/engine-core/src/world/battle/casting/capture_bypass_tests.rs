use super::*;
use legaia_engine_battle::monster_catalog::{MonsterCatalog, MonsterDef};
use legaia_engine_battle::spells::{SpellDef, SpellEffect, SpellElement, SpellTarget};
use legaia_engine_mode::mode::SceneMode;
use legaia_game_tables::move_power::{
    MOVE_ID_INDEX_MAP_FILE_OFFSET, MOVE_POWER_RECORD_STRIDE, MOVE_POWER_TABLE_FILE_OFFSET,
    MOVE_POWER_TABLE_LEN,
};

/// A move id the bypass census does **not** list, mapped to the same power
/// record as the bypass id so the two rolls are otherwise identical.
const RESPECT_MOVE_ID: u8 = 0x5B;
/// Bloody Horns - PROT 952, one of the six bypass modules.
const BYPASS_MOVE_ID: u8 = 0x5C;

/// Synthetic PROT-0898-shaped buffer mapping both ids to one power record.
fn overlay_with_two_ids(power: u16) -> Vec<u8> {
    let mut buf =
        vec![0u8; MOVE_POWER_TABLE_FILE_OFFSET + MOVE_POWER_RECORD_STRIDE * MOVE_POWER_TABLE_LEN];
    // `from_overlay_0898`'s structural guard.
    buf[MOVE_ID_INDEX_MAP_FILE_OFFSET + 4] = 1;
    buf[MOVE_ID_INDEX_MAP_FILE_OFFSET + RESPECT_MOVE_ID as usize] = 1;
    buf[MOVE_ID_INDEX_MAP_FILE_OFFSET + BYPASS_MOVE_ID as usize] = 1;
    buf[MOVE_POWER_TABLE_FILE_OFFSET + MOVE_POWER_RECORD_STRIDE] = power as u8;
    buf[MOVE_POWER_TABLE_FILE_OFFSET + MOVE_POWER_RECORD_STRIDE + 1] = (power >> 8) as u8;
    buf
}

/// One party member carrying the element-0 resist passive, one monster
/// caster of element 0. Same seed every time.
fn world_with_resisting_party() -> World {
    let mut world = World {
        party: legaia_engine_menus::party_state::PartyState {
            party_count: 1,
            ..Default::default()
        },
        ..World::default()
    };
    world.mode = SceneMode::Battle;
    let mut catalog = MonsterCatalog::new();
    let mut caster = MonsterDef::new(5, "Boss", 4000, 200);
    caster.element = 0;
    catalog.insert(caster);
    world.tables.monster_catalog = catalog;

    // Party slot 0 resists element 0 (`record+0xF4` bit 0x20000000) and
    // carries no All-Guard absorb bit, so the ladder's halve arm is the one
    // that would run. Loaded before the actor stats: `load_party` reseeds
    // the battle mirrors from the record.
    world.load_party(legaia_save::Party::zeroed(1));
    if let Some(m) = world.party.roster.members.get_mut(0) {
        let mut bits = m.ability_bits();
        bits[3] |= 0x20;
        bits[4..8].fill(0);
        m.set_ability_bits(bits);
    }

    world.actors[0].battle.max_hp = 9999;
    world.actors[0].battle.hp = 9999;
    world.actors[0].battle.liveness = 1;
    world.battle.accuracy[0] = 5;
    world.battle.defense[0] = 1;

    world.actors[1].battle.max_hp = 4000;
    world.actors[1].battle.hp = 4000;
    world.actors[1].battle.mp = 200;
    world.actors[1].battle.liveness = 1;
    world.actors[1].battle_monster_id = Some(5);
    world.battle.accuracy[1] = 200;

    world.tables.move_power = Some(
        legaia_engine_battle::move_power::MovePowerCatalog::from_overlay_0898(
            &overlay_with_two_ids(500),
        )
        .expect("synthetic catalog parses"),
    );
    world.rng_state = 0xC0FF_EE01;
    world
}

fn spell(id: u8) -> SpellDef {
    SpellDef {
        id,
        name: "boss cast".into(),
        mp_cost: 0,
        element: SpellElement::Neutral,
        target: SpellTarget::OneEnemy,
        effect: SpellEffect::Damage {
            base_power: 100,
            element: SpellElement::Neutral,
        },
        anim_id: 0,
        ..Default::default()
    }
}

fn damage_from(move_id: u8) -> u16 {
    let mut world = world_with_resisting_party();
    let before = world.actors[0].battle.hp;
    world.cast_spell_on_slots(1, &spell(move_id), &[0]);
    before - world.actors[0].battle.hp
}

/// Same world minus the party defender's element-0 resist bit, so the
/// finisher's ladder has nothing to match.
fn damage_without_resist(move_id: u8) -> u16 {
    let mut world = world_with_resisting_party();
    if let Some(m) = world.party.roster.members.get_mut(0) {
        let mut bits = m.ability_bits();
        bits[3] &= !0x20;
        m.set_ability_bits(bits);
    }
    let before = world.actors[0].battle.hp;
    world.cast_spell_on_slots(1, &spell(move_id), &[0]);
    before - world.actors[0].battle.hp
}

/// `FUN_801DD6B4` passes `param_5 = 1` to the finisher, so the party
/// defender's elemental-resistance ladder never runs: the same cast lands
/// for the same amount whether or not the defender carries the matching
/// resist bit.
#[test]
fn bypass_move_ignores_the_party_elemental_resist_ladder() {
    let resisted = damage_from(BYPASS_MOVE_ID);
    let unresisted = damage_without_resist(BYPASS_MOVE_ID);
    assert!(resisted > 0, "the bypassing cast lands");
    assert_eq!(
        resisted, unresisted,
        "the resist bit must not change a bypass-wrapper hit"
    );
}

/// The control: an ordinary move-power special reaches the finisher with
/// `param_5 = 0`, so the same resist bit halves it.
#[test]
fn respecting_move_is_halved_by_the_party_elemental_resist_ladder() {
    let resisted = damage_from(RESPECT_MOVE_ID);
    let unresisted = damage_without_resist(RESPECT_MOVE_ID);
    assert!(resisted > 0, "the respecting cast still lands");
    assert_eq!(
        unresisted,
        resisted * 2,
        "the ladder halves a respecting hit"
    );
}

/// The wrappers are not the shared kernel with a flag flipped: with the
/// resist ladder disarmed on both sides (so the finisher stages agree),
/// the two ids still resolve to different magnitudes, because
/// `FUN_801DD6B4` rolls its own attacker / defender / bonus arithmetic.
#[test]
fn the_bypass_wrapper_rolls_a_different_kernel_not_just_a_different_flag() {
    assert_ne!(
        damage_without_resist(RESPECT_MOVE_ID),
        damage_without_resist(BYPASS_MOVE_ID),
        "the bypass id must route through the wrapper roll, not FUN_801DD0AC"
    );
}

/// Mark `id` capture-class in a synthetic spell table and install it as
/// the world's disc-derived menu text - the same table
/// `World::install_menu_text` fills from `SCUS_942.54`.
fn with_capture_class(world: &mut World, id: u8) {
    use legaia_game_tables::spell_names::{CAPTURE_CLASS, SpellEntry, SpellNameTable};
    let mut entries = vec![SpellEntry::default(); 0x100];
    entries[id as usize].class = CAPTURE_CLASS;
    // Sub-id 19 pages PROT 0954 (Fatal Decision), a body with no damage
    // site, so no baked power stands in for the table's: these fixtures
    // isolate the wrapper choice, not the power seed.
    entries[id as usize].sub_class = 19;
    world.menu.text = Some(legaia_engine_menus::pause_screens::MenuTextTables {
        spell_names: Some(SpellNameTable::from_entries(entries)),
        ..Default::default()
    });
}

fn damage_as_capture_class(move_id: u8) -> u16 {
    let mut world = world_with_resisting_party();
    with_capture_class(&mut world, move_id);
    let before = world.actors[0].battle.hp;
    world.cast_spell_on_slots(1, &spell(move_id), &[0]);
    before - world.actors[0].battle.hp
}

/// The default fixture rolls a huge attacker against a bare defender, so
/// **neither** kernel's bonus arm fires and the two agree exactly - the
/// rolls outside that arm are instruction-identical. This variant is the
/// opposite corner: a small power against a heavy defence, where both
/// arms fire and the two rebuilds differ.
fn world_on_the_bonus_floor() -> World {
    let mut world = world_with_resisting_party();
    world.battle.accuracy[1] = 20; // attacker `+0x168`
    world.battle.defense[0] = 4000; // defender `+0x15C`
    world.tables.move_power = Some(
        legaia_engine_battle::move_power::MovePowerCatalog::from_overlay_0898(
            &overlay_with_two_ids(40),
        )
        .expect("synthetic catalog parses"),
    );
    world
}

/// A capture-class cast whose module calls the **respecting** wrapper is
/// `FUN_801DD4B0`, not the shared kernel `FUN_801DD0AC`. The two share an
/// attacker and defender roll but not a bonus arm: this one fires on
/// `attacker < defender + power` and rebuilds with one draw, the shared
/// kernel widens the threshold by `agl >> 1` and rebuilds with two. So the
/// same move id resolves to a different figure once the spell table says
/// the cast is capture-class.
///
/// This is the contrast that makes the routing non-vacuous: the *only*
/// thing that differs between the two runs is the class byte.
#[test]
fn a_capture_class_cast_leaves_the_shared_kernel() {
    let cast = |capture: bool| {
        let mut world = world_on_the_bonus_floor();
        if capture {
            with_capture_class(&mut world, RESPECT_MOVE_ID);
        }
        let before = world.actors[0].battle.hp;
        world.cast_spell_on_slots(1, &spell(RESPECT_MOVE_ID), &[0]);
        before - world.actors[0].battle.hp
    };
    let shared = cast(false);
    let wrapped = cast(true);
    assert!(shared > 0 && wrapped > 0, "both paths land a hit");
    assert_ne!(
        shared, wrapped,
        "the class byte must route the cast off FUN_801DD0AC"
    );
}

/// The other half of the same fact: away from the bonus floor the two
/// kernels agree to the unit, so this routing is not a blanket damage
/// change - it is a change to what happens when a hit fails to clear the
/// defender's mitigation.
#[test]
fn off_the_bonus_floor_the_two_kernels_agree() {
    assert_eq!(
        damage_from(RESPECT_MOVE_ID),
        damage_as_capture_class(RESPECT_MOVE_ID),
        "outside the bonus arm FUN_801DD4B0 and FUN_801DD0AC are the same roll"
    );
}

/// ...and it is still a *respecting* hit: the party defender's elemental
/// resist ladder halves it, unlike the six bypass ids.
#[test]
fn a_capture_class_respecting_cast_still_takes_the_resist_ladder() {
    let mut world = world_with_resisting_party();
    with_capture_class(&mut world, RESPECT_MOVE_ID);
    if let Some(m) = world.party.roster.members.get_mut(0) {
        let mut bits = m.ability_bits();
        bits[3] &= !0x20;
        m.set_ability_bits(bits);
    }
    let before = world.actors[0].battle.hp;
    world.cast_spell_on_slots(1, &spell(RESPECT_MOVE_ID), &[0]);
    let unresisted = before - world.actors[0].battle.hp;
    let resisted = damage_as_capture_class(RESPECT_MOVE_ID);
    assert!(resisted > 0);
    assert_eq!(unresisted, resisted * 2, "the ladder halves this hit");
}

/// A bypass id stays on the bypass wrapper even when the table also calls
/// it capture-class: the census list is checked first, because a shared
/// module dispatches per spell and the class byte cannot tell the two
/// ticks apart.
#[test]
fn the_bypass_census_wins_over_the_class_byte() {
    assert_eq!(
        damage_from(BYPASS_MOVE_ID),
        damage_as_capture_class(BYPASS_MOVE_ID),
        "a bypass id must not be re-routed by the class byte"
    );
}

/// The wrapper's defence weighting, asserted on the pure roll where it is
/// unconditionally observable: `FUN_801DD6B4`'s defender stage folds
/// `+0x15C` / `+0x160` in at `>> 1`, eight times as heavily as the
/// INT wrapper's `>> 4`, and reads no `+0x168` at all.
///
/// This is deliberately *not* measured end-to-end. See
/// [`the_bonus_arm_floor_makes_a_bypass_hit_defence_insensitive`] for why
/// the end-to-end signal does not exist.
#[test]
fn the_bypass_wrapper_weights_the_defence_terms_more_heavily() {
    use legaia_engine_battle_vm::battle_damage_wrappers::{
        WrapperDefender, atk_defender_roll, int_defender_roll,
    };
    let d = |stat: u16| WrapperDefender {
        hp: 0,
        agl: 0,
        stat_a: stat,
        stat_b: stat,
        status: 0,
        guard: 0,
    };
    // rand 0 isolates the deterministic terms from the modulus draw.
    let spell_slope = atk_defender_roll(&d(64), 0) as i32 - atk_defender_roll(&d(0), 0) as i32;
    let phys_slope = int_defender_roll(&d(64), 0) as i32 - int_defender_roll(&d(0), 0) as i32;
    assert_eq!(spell_slope, 64, "two terms at >> 1");
    assert_eq!(phys_slope, 8, "two terms at >> 4");
    assert_eq!(spell_slope, phys_slope * 8);
}

/// The end-to-end consequence of `FUN_801DD6B4`'s **bonus arm**, and the
/// reason the heavier defence weighting above is not visible as HP loss.
///
/// `atk_wrapper_predamage` re-rolls the attacker whenever
/// `atk < def + power`, and the re-roll is
/// `def + power + rand % ((power >> 2) + 1)` (`wrapper_bonus_roll`,
/// instruction-identical between `FUN_801DD4B0` and `FUN_801DD6B4`). The
/// net `atk - def` it leaves is therefore `power + rand % …` - the
/// defender's contribution cancels exactly. For a high-power move the arm
/// fires at every defence worth testing, so a bypass hit is **defence
/// insensitive**, while the respecting cast, which reaches the shared
/// kernel, still falls off.
///
/// Stated as an inequality on `i32`, so an inversion is an assertion
/// failure in both profiles rather than an unsigned-subtraction panic in
/// debug and a silent pass in release.
///
/// REF: FUN_801DD6B4 (bonus arm), FUN_801DD4B0 (the same arm)
#[test]
fn the_bonus_arm_floor_makes_a_bypass_hit_defence_insensitive() {
    // The stream the bypass casts run on, chosen so the arm fires at every
    // sampled defence. Derived by hand: the synthetic record's power is
    // 500, so the attacker roll is `r0 % 126 + (4000 >> 8) + 500` and the
    // arm fires when that falls short of `def_roll + 500`, where
    // `def_roll >= (9999 >> 8) = 39` at any defence. Seed `0x000FEE15`'s
    // first shaped draw (`s' = s * 0x41C64E6D + 0x3039`, draw =
    // `(s' >> 16) & 0x7FFF`) is 23184 = `126 * 184`, so the attacker roll
    // is 515 and `515 < 39 + 500` holds for every defence. The world's
    // own seed draws `2929 % 126 = 31`.
    const BONUS_ARM_SEED: u32 = 0x000F_EE15;
    let with_defence = |move_id: u8, defence: u16| -> i32 {
        let mut world = world_with_resisting_party();
        if move_id == BYPASS_MOVE_ID {
            world.rng_state = BONUS_ARM_SEED;
        }
        if let Some(m) = world.party.roster.members.get_mut(0) {
            let mut bits = m.ability_bits();
            bits[3] &= !0x20;
            m.set_ability_bits(bits);
        }
        world.battle.defense[0] = defence;
        let before = world.actors[0].battle.hp as i32;
        world.cast_spell_on_slots(1, &spell(move_id), &[0]);
        before - world.actors[0].battle.hp as i32
    };

    // Every sampled defence lands the same bypass hit once the arm is
    // firing, and no sample is a heal.
    let bypass: Vec<i32> = [50u16, 100, 200, 400, 800]
        .iter()
        .map(|d| with_defence(BYPASS_MOVE_ID, *d))
        .collect();
    assert!(bypass.iter().all(|&x| x > 0), "bypass lands: {bypass:?}");
    assert!(
        bypass.windows(2).all(|w| w[0] == w[1]),
        "the bonus-arm floor pins the bypass hit: {bypass:?}"
    );

    // The respecting cast is the control: its damage falls monotonically
    // as defence climbs, and strictly over the same span.
    let respect: Vec<i32> = [50u16, 100, 200, 400, 800]
        .iter()
        .map(|d| with_defence(RESPECT_MOVE_ID, *d))
        .collect();
    assert!(respect.iter().all(|&x| x > 0), "respect lands: {respect:?}");
    assert!(
        respect.windows(2).all(|w| w[0] >= w[1]),
        "the shared kernel never rewards defence: {respect:?}"
    );
    assert!(
        respect[0] > respect[respect.len() - 1],
        "defence must actually mitigate a respecting hit: {respect:?}"
    );
}

/// Damage `move_id` lands with `+0x16E` words `caster` / `victim` on the two
/// seats and the party defender's resist bit cleared.
fn damage_with_status(move_id: u8, caster: u16, victim: u16) -> u16 {
    let mut world = world_with_resisting_party();
    if let Some(m) = world.party.roster.members.get_mut(0) {
        let mut bits = m.ability_bits();
        bits[3] &= !0x20;
        m.set_ability_bits(bits);
    }
    world.actors[1].battle.field_flags = caster;
    world.actors[0].battle.field_flags = victim;
    let before = world.actors[0].battle.hp;
    world.cast_spell_on_slots(1, &spell(move_id), &[0]);
    before - world.actors[0].battle.hp
}

/// `FUN_801DD864` (`0x801DD974..0x801DDA74`) scales the attacker roll by
/// the caster's `+0x16E` Venom (`x9/10`) / Toxic (`x7/10`) bits and the
/// defender roll by the target's: a Toxic caster's special lands for less,
/// a Toxic target's thinner defence lets more through.
#[test]
fn venom_and_toxic_scale_a_special_attack_on_both_sides() {
    let plain = damage_with_status(RESPECT_MOVE_ID, 0, 0);
    let weak_caster = damage_with_status(RESPECT_MOVE_ID, 0x2, 0);
    let soft_target = damage_with_status(RESPECT_MOVE_ID, 0, 0x2);
    assert!(weak_caster < plain, "toxic caster {weak_caster} vs {plain}");
    assert!(soft_target > plain, "toxic target {soft_target} vs {plain}");
}

/// Damage `move_id` deals to the resisting / non-resisting defender with the
/// jewel fix (`WorldToggles::jewel_fix`) set as given.
fn jewel_fix_damage(move_id: u8, jewel_fix: bool, resist: bool) -> u16 {
    let mut world = world_with_resisting_party();
    world.toggles.jewel_fix = jewel_fix;
    if !resist && let Some(m) = world.party.roster.members.get_mut(0) {
        let mut bits = m.ability_bits();
        bits[3] &= !0x20;
        m.set_ability_bits(bits);
    }
    let before = world.actors[0].battle.hp;
    world.cast_spell_on_slots(1, &spell(move_id), &[0]);
    before - world.actors[0].battle.hp
}

/// The jewel fix: with the toggle on, a bypass-census cast rolls the
/// respecting wrapper `FUN_801DD4B0` (the patch's `jal` retarget), so the
/// defender's resist ladder halves it. Off (retail mode), the hit is the
/// bypass wrapper's, bit for bit.
#[test]
fn jewel_fix_makes_a_bypass_cast_respect_the_resist_ladder() {
    let fixed_resisted = jewel_fix_damage(BYPASS_MOVE_ID, true, true);
    let fixed_unresisted = jewel_fix_damage(BYPASS_MOVE_ID, true, false);
    assert!(fixed_resisted > 0);
    assert_eq!(
        fixed_unresisted,
        fixed_resisted * 2,
        "the fixed cast is halved by the ladder"
    );
    // Retail mode: identical to the unfixed bypass hit.
    assert_eq!(
        jewel_fix_damage(BYPASS_MOVE_ID, false, true),
        damage_from(BYPASS_MOVE_ID)
    );
    assert_eq!(
        jewel_fix_damage(BYPASS_MOVE_ID, false, true),
        jewel_fix_damage(BYPASS_MOVE_ID, false, false),
        "unfixed, the resist bit changes nothing"
    );
    // The retarget swaps the whole roll (INT and the `>> 4` fold for ATK and
    // the `>> 1` fold), so the hit moves even with no resist to apply.
    assert_ne!(
        fixed_unresisted,
        jewel_fix_damage(BYPASS_MOVE_ID, false, false),
        "the fixed cast rolls FUN_801DD4B0's terms, not FUN_801DD6B4's"
    );
    // A move outside the census is untouched by the toggle.
    assert_eq!(
        jewel_fix_damage(RESPECT_MOVE_ID, true, true),
        jewel_fix_damage(RESPECT_MOVE_ID, false, true)
    );
}
