#![allow(clippy::field_reassign_with_default)]

use super::*;
use std::cell::RefCell;

/// Recording host. Captures every callback so tests can assert exact
/// dispatch order.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Event {
    Pose(u8, Pose),
    Ui(u8, u8),
    PartySetup(u8),
    MonsterSetup(u8),
    Camera,
    SpellAnim(u8, u8),
    SpellSustain(u8, u8),
    ApplyDamage(u8, u8, u8, u8),
    ApplyArtStrike(ArtStrikeInfo),
    ScreenShake(u16),
    Brightness(u8),
    BattleEnd(BattleEndCause),
    LoadCapture(u8),
    Recompute,
    VictoryStage(u8),
}

#[derive(Default)]
struct RecHost {
    actors: Vec<BattleActor>,
    events: RefCell<Vec<Event>>,
    capture_spells: std::collections::HashSet<u8>,
    spell_costs: std::collections::HashMap<u8, u8>,
    /// Spell-table `+0` class bytes. Absent = "no table for this id", which
    /// is what a host without a disc image reports.
    spell_classes: std::collections::HashMap<u8, u8>,
    ability_bits: std::collections::HashMap<u8, u32>,
    ranges: std::collections::HashMap<(u8, u8), u16>,
    prev_cleared: bool,
    sound_ready: bool,
    rng_seq: Vec<u32>,
    rng_pos: RefCell<usize>,
    pad_word: u16,
    party_count: u8,
    slot_count: u8,
    first_monster_id: u8,
    /// Pre-staged art records returned by `art_record(character, action)`
    /// - keyed by `(character_byte, action_byte)`.
    art_records: std::collections::HashMap<(u8, u8), legaia_art::ArtRecord>,
    /// Per-slot equipment ATK bonus bytes returned by
    /// `equip_attack_bonuses`.
    equip_atk: std::collections::HashMap<u8, [u8; 5]>,
    /// Party slots reported as NOT seated by `slot_seated` (empty = every
    /// slot seated, the trait default).
    unseated: std::collections::HashSet<u8>,
    /// How many more times `capture_stager_tick` reports busy - the stand-in
    /// for a resident slot-B module that stages for N frames.
    capture_busy_frames: RefCell<u32>,
    /// How many times `capture_stager_tick` was entered.
    capture_ticks: RefCell<u32>,
}

impl RecHost {
    fn with_n_actors(n: usize) -> Self {
        Self {
            actors: (0..n).map(|_| BattleActor::new()).collect(),
            prev_cleared: true,
            sound_ready: true,
            party_count: 3,
            slot_count: ACTOR_SLOTS as u8,
            ..Default::default()
        }
    }
    fn record(&self, e: Event) {
        self.events.borrow_mut().push(e);
    }
    fn take(&self) -> Vec<Event> {
        std::mem::take(&mut self.events.borrow_mut())
    }
}

impl BattleActionHost for RecHost {
    fn actor(&self, slot: u8) -> Option<&BattleActor> {
        self.actors.get(slot as usize)
    }
    fn slot_seated(&self, slot: u8) -> bool {
        !self.unseated.contains(&slot)
    }
    fn actor_mut(&mut self, slot: u8) -> Option<&mut BattleActor> {
        self.actors.get_mut(slot as usize)
    }
    fn pose(&mut self, actor_id: u8, pose: Pose) {
        self.record(Event::Pose(actor_id, pose));
    }
    fn ui_element(&mut self, effect_id: u8, mode: u8) {
        self.record(Event::Ui(effect_id, mode));
    }
    fn range_check(&self, a: u8, t: u8) -> u16 {
        self.ranges.get(&(a, t)).copied().unwrap_or(0)
    }
    fn camera_bounds(&mut self) {
        self.record(Event::Camera);
    }
    fn party_setup(&mut self, s: u8) {
        self.record(Event::PartySetup(s));
    }
    fn monster_setup(&mut self, s: u8) {
        self.record(Event::MonsterSetup(s));
    }
    fn recompute_battle_order(&mut self) {
        self.record(Event::Recompute);
    }
    fn first_monster_id(&self) -> u8 {
        self.first_monster_id
    }
    fn victory_stage(&mut self, party_slot: u8) {
        self.record(Event::VictoryStage(party_slot));
    }
    fn pad_word(&self) -> u16 {
        self.pad_word
    }
    fn rng(&mut self) -> u32 {
        let mut p = self.rng_pos.borrow_mut();
        let v = self.rng_seq.get(*p).copied().unwrap_or(0);
        *p += 1;
        v
    }
    fn previous_action_cleared(&self, _: u8) -> bool {
        self.prev_cleared
    }
    fn sound_bank_ready(&self, _: u8) -> bool {
        self.sound_ready
    }
    fn load_capture_archive(&mut self, idx: u8) {
        self.record(Event::LoadCapture(idx));
    }
    fn capture_stager_tick(&mut self) -> bool {
        *self.capture_ticks.borrow_mut() += 1;
        let mut left = self.capture_busy_frames.borrow_mut();
        if *left == 0 {
            return false;
        }
        *left -= 1;
        true
    }
    fn spell_anim_trigger(&mut self, p: u8, s: u8) {
        self.record(Event::SpellAnim(p, s));
    }
    fn spell_anim_sustain(&mut self, a: u8, anim: u8) {
        self.record(Event::SpellSustain(a, anim));
    }
    fn apply_damage(&mut self, a: u8, b: u8, c: u8, d: u8) {
        self.record(Event::ApplyDamage(a, b, c, d));
    }
    fn apply_art_strike(&mut self, info: ArtStrikeInfo) {
        self.record(Event::ApplyArtStrike(info));
    }
    fn equip_attack_bonuses(&self, party_slot: u8) -> [u8; 5] {
        self.equip_atk.get(&party_slot).copied().unwrap_or([0; 5])
    }
    fn art_record(
        &self,
        character: legaia_art::Character,
        action: legaia_art::ActionConstant,
    ) -> Option<&legaia_art::ArtRecord> {
        self.art_records
            .get(&(character_byte(character), action.as_byte()))
    }
    fn spell_class_byte(&self, id: u8) -> Option<u8> {
        self.spell_classes.get(&id).copied()
    }
    fn is_capture_spell(&self, id: u8) -> bool {
        self.capture_spells.contains(&id)
    }
    fn spell_mp_cost(&self, id: u8) -> u8 {
        self.spell_costs.get(&id).copied().unwrap_or(0)
    }
    fn character_ability_bits(&self, slot: u8) -> u32 {
        self.ability_bits.get(&slot).copied().unwrap_or(0)
    }
    fn screen_shake(&mut self, m: u16) {
        self.record(Event::ScreenShake(m));
    }
    fn duck_audio_level(&mut self, p: u8) {
        self.record(Event::Brightness(p));
    }
    fn battle_end(&mut self, c: BattleEndCause) {
        self.record(Event::BattleEnd(c));
    }
    fn frame_dt(&self) -> i16 {
        1
    }
    fn party_count(&self) -> u8 {
        self.party_count
    }
    fn slot_count(&self) -> u8 {
        self.slot_count
    }
}

/// Cheap byte encoding for tests. `Character` is a 3-variant enum with
/// no public byte-mapping accessor - this mirrors the `0/1/2` ordering
/// of `Character::all()`.
fn character_byte(c: legaia_art::Character) -> u8 {
    match c {
        legaia_art::Character::Vahn => 0,
        legaia_art::Character::Noa => 1,
        legaia_art::Character::Gala => 2,
    }
}

fn fresh(category: ActionCategory, slot: u8) -> (BattleActionCtx, RecHost) {
    let mut host = RecHost::with_n_actors(ACTOR_SLOTS);
    // Mark all slots alive.
    for a in &mut host.actors {
        a.liveness = 1;
    }
    host.actors[slot as usize].action_category = category.as_byte();
    let mut ctx = BattleActionCtx::new();
    ctx.active_actor = slot;
    (ctx, host)
}

#[test]
fn action_state_byte_roundtrip() {
    for s in [
        ActionState::Begin,
        ActionState::ActionSeed,
        ActionState::AttackChain,
        ActionState::DoneCleanup,
        ActionState::EndOfAction,
        ActionState::RoundEnd,
    ] {
        assert_eq!(ActionState::from_byte(s.as_byte()).unwrap(), s);
    }
    // Unmapped byte returns None.
    assert!(ActionState::from_byte(0x07).is_none());
}

#[test]
fn action_category_byte_roundtrip() {
    for c in [
        ActionCategory::TacticalArts,
        ActionCategory::Item,
        ActionCategory::Magic,
        ActionCategory::Attack,
        ActionCategory::Spirit,
        ActionCategory::Run,
    ] {
        assert_eq!(ActionCategory::from_byte(c.as_byte()), c);
    }
    // Reserved bytes fold to TacticalArts.
    assert_eq!(
        ActionCategory::from_byte(0x42),
        ActionCategory::TacticalArts
    );
}

#[test]
fn begin_with_menu_open_routes_to_queued_from_menu() {
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 0);
    ctx.action_state = ActionState::Begin.as_byte();
    ctx.queued_action = 5;
    ctx.menu_open = 1;
    ctx.turn_cursor = 9;
    let out = step(&mut host, &mut ctx);
    assert!(matches!(
        out,
        StepOutcome::Transition {
            to,
            ..
        } if to == ActionState::QueuedFromMenu.as_byte()
    ));
    // `Begin` seeds the turn cursor from the formation advantage, which is
    // `0` here - it does NOT copy `queued_action` anywhere.
    assert_eq!(ctx.turn_cursor, 0);
}

/// The three storing arms of `Begin`'s `ctx[+0x290]` switch, plus the fourth
/// that stores nothing (`0x801E2AC0..0x801E2B24`).
#[test]
fn begin_seeds_the_turn_cursor_from_the_formation_advantage() {
    let seed = |advantage: u8, was: u8| {
        let (mut ctx, mut host) = fresh(ActionCategory::Attack, 0);
        ctx.action_state = ActionState::Begin.as_byte();
        ctx.formation_advantage = advantage;
        ctx.turn_cursor = was;
        step(&mut host, &mut ctx);
        ctx.turn_cursor
    };
    // The default host seats 3 party slots of 8.
    assert_eq!(seed(0, 9), 0, "no advantage: head of the order");
    assert_eq!(seed(1, 9), 3, "party count");
    assert_eq!(seed(2, 9), 5, "monster count");
    assert_eq!(seed(3, 9), 9, "unmapped advantage stores nothing");
}

#[test]
fn begin_without_menu_routes_to_pre_action_wait() {
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 0);
    ctx.action_state = ActionState::Begin.as_byte();
    let out = step(&mut host, &mut ctx);
    assert!(matches!(
        out,
        StepOutcome::Transition {
            to,
            ..
        } if to == ActionState::PreActionWait.as_byte()
    ));
}

#[test]
fn pre_action_wait_holds_until_cleared() {
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 0);
    ctx.action_state = ActionState::PreActionWait.as_byte();
    host.prev_cleared = false;
    let out = step(&mut host, &mut ctx);
    assert_eq!(out, StepOutcome::Stay);
    host.prev_cleared = true;
    let out = step(&mut host, &mut ctx);
    assert!(matches!(
        out,
        StepOutcome::Transition {
            to,
            ..
        } if to == ActionState::ActionSeed.as_byte()
    ));
}

#[test]
fn queued_from_menu_holds_then_releases() {
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 0);
    ctx.action_state = ActionState::QueuedFromMenu.as_byte();
    ctx.menu_open = 1;
    assert_eq!(step(&mut host, &mut ctx), StepOutcome::Stay);
    ctx.menu_open = 0;
    let out = step(&mut host, &mut ctx);
    assert!(matches!(
        out,
        StepOutcome::Transition {
            to,
            ..
        } if to == ActionState::PreActionWait.as_byte()
    ));
}

#[test]
fn action_seed_attack_routes_to_attack_face_and_emits_ui() {
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 1);
    ctx.action_state = ActionState::ActionSeed.as_byte();
    let out = step(&mut host, &mut ctx);
    assert!(matches!(
        out,
        StepOutcome::Transition {
            to,
            ..
        } if to == ActionState::AttackFace.as_byte()
    ));
    // Party slot < 3 → fires UI element 7.
    let events = host.take();
    assert!(events.contains(&Event::PartySetup(1)));
    assert!(events.contains(&Event::Camera));
    assert!(events.contains(&Event::Pose(1, Pose::Idle)));
    assert!(events.contains(&Event::Ui(7, 0)));
}

/// The action-dispatch liveness gate: a DEAD acting actor never enters an
/// action band. Retail holds the invariant upstream (`FUN_801DABA4`'s dead
/// sweep zeroes the key, bumps `ctx[+0x25]` and can never pick the slot into
/// `ctx[+0x274]`); the port's hosts can kill an actor between arming and
/// seed, so the seed itself routes the dead actor where retail's
/// cleared-action category-0 arm routes - state `0x50` - with no setup hook,
/// no camera, no attack band.
#[test]
fn action_seed_dead_actor_routes_to_done_cleanup_not_the_attack_band() {
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 5);
    // Stage the multi-strike shape the defect ran: a monster with a queued
    // swing stream, killed before its staged action fires.
    host.actors[5].params[0] = 0x01;
    host.actors[5].liveness = 0;
    ctx.action_state = ActionState::ActionSeed.as_byte();
    let out = step(&mut host, &mut ctx);
    assert!(matches!(
        out,
        StepOutcome::Transition {
            to,
            ..
        } if to == ActionState::DoneCleanup.as_byte()
    ));
    // The gate sits ahead of every seed side effect.
    let events = host.take();
    assert!(!events.contains(&Event::MonsterSetup(5)));
    assert!(!events.contains(&Event::Camera));
    assert!(!events.iter().any(|e| matches!(e, Event::Pose(5, _))));
}

/// The whole arc: a dead monster's staged attack is consumed as a spent turn
/// (the SM walks the Done band to `EndOfAction`) and the attack band is never
/// entered - so no strike can ever land from a corpse.
#[test]
fn dead_actor_staged_action_is_spent_without_a_single_attack_state() {
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 5);
    host.actors[5].liveness = 0;
    ctx.action_state = ActionState::Begin.as_byte();
    let attack_band = [
        ActionState::AttackFace.as_byte(),
        ActionState::AttackWindup.as_byte(),
        ActionState::AttackAdvance.as_byte(),
        ActionState::AttackCloseRange.as_byte(),
        ActionState::AttackStrike.as_byte(),
        ActionState::AttackShortStep.as_byte(),
        ActionState::AttackChain.as_byte(),
        ActionState::AttackRecovery.as_byte(),
        ActionState::AttackReturn.as_byte(),
    ];
    for _ in 0..400 {
        step(&mut host, &mut ctx);
        assert!(
            !attack_band.contains(&ctx.action_state),
            "dead actor reached attack state {:#04x}",
            ctx.action_state
        );
        if ctx.action_state == ActionState::EndOfAction.as_byte() {
            break;
        }
    }
    assert_eq!(
        ctx.action_state,
        ActionState::EndOfAction.as_byte(),
        "the dead actor's turn is spent through the Done band, not parked"
    );
    assert!(
        !host
            .take()
            .iter()
            .any(|e| matches!(e, Event::ApplyDamage(..) | Event::ApplyArtStrike(_))),
        "no damage may originate from a dead acting actor"
    );
}

/// A LIVING actor with the same staged action still dispatches into the
/// attack band - the gate reads liveness, not the staging.
#[test]
fn action_seed_living_actor_still_reaches_the_attack_band() {
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 5);
    host.actors[5].params[0] = 0x01;
    ctx.action_state = ActionState::ActionSeed.as_byte();
    let out = step(&mut host, &mut ctx);
    assert!(matches!(
        out,
        StepOutcome::Transition {
            to,
            ..
        } if to == ActionState::AttackFace.as_byte()
    ));
}

#[test]
fn action_seed_run_party_routes_to_run_begin() {
    let (mut ctx, mut host) = fresh(ActionCategory::Run, 1);
    ctx.action_state = ActionState::ActionSeed.as_byte();
    let out = step(&mut host, &mut ctx);
    assert!(matches!(
        out,
        StepOutcome::Transition {
            to,
            ..
        } if to == ActionState::RunBegin.as_byte()
    ));
    // Camera not called for run actions.
    assert!(!host.take().contains(&Event::Camera));
}

#[test]
fn action_seed_run_monster_routes_to_capture_start() {
    let (mut ctx, mut host) = fresh(ActionCategory::Run, 5);
    ctx.action_state = ActionState::ActionSeed.as_byte();
    let out = step(&mut host, &mut ctx);
    assert!(matches!(
        out,
        StepOutcome::Transition {
            to,
            ..
        } if to == ActionState::CaptureStart.as_byte()
    ));
}

#[test]
fn action_seed_magic_routes_to_magic_cast_begin() {
    let (mut ctx, mut host) = fresh(ActionCategory::Magic, 1);
    ctx.action_state = ActionState::ActionSeed.as_byte();
    let out = step(&mut host, &mut ctx);
    assert!(matches!(
        out,
        StepOutcome::Transition {
            to,
            ..
        } if to == ActionState::MagicCastBegin.as_byte()
    ));
}

/// The Magic arm's class-byte discriminator (`0x801E2EEC..0x801E2EF8`): the
/// override to the Spirit band needs **both** `class < 0x14` *and* `id <
/// 0x65`, so each test on its own must leave the default `0x28` store alone.
#[test]
fn action_seed_magic_class_discriminator_takes_both_tests() {
    let band = |class: Option<u8>, spell_id: u8| -> u8 {
        let (mut ctx, mut host) = fresh(ActionCategory::Magic, 1);
        ctx.action_state = ActionState::ActionSeed.as_byte();
        host.actors[1].params[0] = spell_id;
        if let Some(c) = class {
            host.spell_classes.insert(spell_id, c);
        }
        step(&mut host, &mut ctx);
        ctx.action_state
    };
    let cast_begin = ActionState::MagicCastBegin.as_byte();
    let pre_arm = ActionState::SpiritPreArm.as_byte();

    // Both tests pass -> 0x3C.
    assert_eq!(band(Some(0x02), 0x10), pre_arm);
    assert_eq!(band(Some(0x13), 0x64), pre_arm, "0x13 / 0x64 are inclusive");
    // Class fails the `< 0x14` test -> the default 0x28 store stands.
    assert_eq!(band(Some(0x14), 0x10), cast_begin);
    assert_eq!(band(Some(0x32), 0x10), cast_begin);
    // Id fails the `< 0x65` test -> likewise. The player Seru block is here.
    assert_eq!(band(Some(0x02), 0x65), cast_begin);
    assert_eq!(band(Some(0x02), 0x81), cast_begin);
}

/// A host that supplies no spell table cannot evaluate the class test, so it
/// keeps retail's non-override branch for every id - which is what every
/// disc-free battle in this engine does.
#[test]
fn action_seed_magic_without_a_spell_table_always_takes_the_magic_band() {
    for spell_id in [0x00u8, 0x10, 0x64, 0x81, 0xFF] {
        let (mut ctx, mut host) = fresh(ActionCategory::Magic, 1);
        ctx.action_state = ActionState::ActionSeed.as_byte();
        host.actors[1].params[0] = spell_id;
        step(&mut host, &mut ctx);
        assert_eq!(ctx.action_state, ActionState::MagicCastBegin.as_byte());
    }
}

/// With no override, `is_capture_spell` falls out of the same class byte the
/// band pick reads - one record, one answer.
#[test]
fn the_default_capture_predicate_derives_from_the_class_byte() {
    struct ClassOnly(std::collections::HashMap<u8, u8>);
    impl BattleActionHost for ClassOnly {
        fn actor(&self, _: u8) -> Option<&BattleActor> {
            None
        }
        fn actor_mut(&mut self, _: u8) -> Option<&mut BattleActor> {
            None
        }
        fn spell_class_byte(&self, id: u8) -> Option<u8> {
            self.0.get(&id).copied()
        }
    }
    let mut classes = std::collections::HashMap::new();
    classes.insert(0x37u8, legaia_asset::spell_names::CAPTURE_CLASS);
    classes.insert(0x38u8, 0x32u8);
    let host = ClassOnly(classes);
    assert!(host.is_capture_spell(0x37));
    assert!(
        !host.is_capture_spell(0x38),
        "a non-'c' record is not capture"
    );
    assert!(
        !host.is_capture_spell(0x39),
        "no record at all is not capture"
    );
}

/// The Item arm's own override (`0x801E2E6C..0x801E2EAC`): item ids `0x98` /
/// `0x99` leave the Spirit band for the cast band, staged as a summon.
#[test]
fn action_seed_item_summon_ids_take_the_cast_band_as_a_summon() {
    for (item_id, staged) in [(0x98u8, 0x96u8), (0x99, 0x97)] {
        let (mut ctx, mut host) = fresh(ActionCategory::Item, 1);
        ctx.action_state = ActionState::ActionSeed.as_byte();
        host.actors[1].params[0] = item_id;
        step(&mut host, &mut ctx);
        assert_eq!(ctx.action_state, ActionState::MagicCastBegin.as_byte());
        assert_eq!(host.actors[1].sub_route, 9, "staged as a summon");
        assert_eq!(host.actors[1].params[0], staged, "id is rebased by -2");
    }
}

/// Every other item id keeps the arm's default `0x3C` store, untouched.
#[test]
fn action_seed_ordinary_items_stay_on_the_spirit_band() {
    for item_id in [0x00u8, 0x01, 0x97, 0x9A, 0xFF] {
        let (mut ctx, mut host) = fresh(ActionCategory::Item, 1);
        ctx.action_state = ActionState::ActionSeed.as_byte();
        host.actors[1].params[0] = item_id;
        step(&mut host, &mut ctx);
        assert_eq!(
            ctx.action_state,
            ActionState::SpiritPreArm.as_byte(),
            "item {item_id:#04x}"
        );
        assert_eq!(host.actors[1].params[0], item_id, "and is not rewritten");
    }
}

/// The seed body's own `rand()` (`0x801E2D04`) plus the Item arm's
/// (`0x801E2E3C`) are two draws off the shared cursor, in that order - the
/// property the RNG stream depends on, independent of what the values mean.
/// A Magic action makes only the seed draw, so the two categories are not
/// interchangeable in the stream.
#[test]
fn action_seed_item_draws_twice_and_magic_once() {
    for (category, expected) in [
        (ActionCategory::Item, 2usize),
        // The Spirit arm's own draw sits at `0x801E2FFC`, the same
        // unconditional shape the Item arm's has.
        (ActionCategory::Spirit, 2),
        (ActionCategory::Magic, 1),
        (ActionCategory::Attack, 1),
        (ActionCategory::TacticalArts, 1),
    ] {
        let (mut ctx, mut host) = fresh(category, 1);
        ctx.action_state = ActionState::ActionSeed.as_byte();
        host.rng_seq = vec![7, 7, 7, 7];
        step(&mut host, &mut ctx);
        assert_eq!(
            *host.rng_pos.borrow(),
            expected,
            "{category:?} draws off the shared cursor"
        );
    }
}

/// `ctx[+0xD]`, the camera-angle variant: the seed rolls `rand() % 4`, then
/// the category arm narrows it - Item to `(rand % 2) * 2` (so `0` or `2`),
/// Magic and Tactical Arts to `0` outright.
#[test]
fn action_seed_camera_variant_follows_the_category_arm() {
    // Item, second draw odd -> (1 % 2) * 2 == 2.
    let (mut ctx, mut host) = fresh(ActionCategory::Item, 1);
    ctx.action_state = ActionState::ActionSeed.as_byte();
    host.rng_seq = vec![3, 1];
    step(&mut host, &mut ctx);
    assert_eq!(ctx.camera_variant, 2);

    // Item, second draw even -> 0.
    let (mut ctx, mut host) = fresh(ActionCategory::Item, 1);
    ctx.action_state = ActionState::ActionSeed.as_byte();
    host.rng_seq = vec![3, 4];
    step(&mut host, &mut ctx);
    assert_eq!(ctx.camera_variant, 0);

    // The summon route re-stamps `0` even on an odd draw (`0x801E2E94`).
    let (mut ctx, mut host) = fresh(ActionCategory::Item, 1);
    ctx.action_state = ActionState::ActionSeed.as_byte();
    host.actors[1].params[0] = 0x98;
    host.rng_seq = vec![3, 1];
    step(&mut host, &mut ctx);
    assert_eq!(ctx.action_state, ActionState::MagicCastBegin.as_byte());
    assert_eq!(ctx.camera_variant, 0);

    // Magic / Tactical Arts pin `0` whatever the seed rolled.
    for category in [ActionCategory::Magic, ActionCategory::TacticalArts] {
        let (mut ctx, mut host) = fresh(category, 1);
        ctx.action_state = ActionState::ActionSeed.as_byte();
        host.rng_seq = vec![3];
        step(&mut host, &mut ctx);
        assert_eq!(ctx.camera_variant, 0, "{category:?}");
    }
}

/// The banner byte is cleared at the head of every action, so a tail extended
/// by one action's level-up cannot leak into the next one.
#[test]
fn action_seed_clears_the_level_up_banner_byte() {
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 1);
    ctx.action_state = ActionState::ActionSeed.as_byte();
    ctx.levelup_banner_element = 0x65;
    step(&mut host, &mut ctx);
    assert_eq!(ctx.levelup_banner_element, 0);
}

#[test]
fn action_seed_monster_with_ai_flag_calls_monster_setup() {
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 4);
    host.actors[4].field_flags = 0x380;
    ctx.action_state = ActionState::ActionSeed.as_byte();
    step(&mut host, &mut ctx);
    let events = host.take();
    assert!(events.contains(&Event::MonsterSetup(4)));
    assert!(!events.iter().any(|e| matches!(e, Event::PartySetup(_))));
}

// --- target banner raised at the ActionSeed tail (FUN_801E6D84) ------------

/// Every category arm of retail's seed body falls into the same
/// `jal 0x801e6d84`, and the caster banner `0x44` is what that routine raises
/// before any of the target arms - so it is unconditional on the seed path,
/// for every category except the one that returns first.
#[test]
fn action_seed_raises_the_caster_banner_for_every_category_but_run() {
    for category in [
        ActionCategory::TacticalArts,
        ActionCategory::Item,
        ActionCategory::Magic,
        ActionCategory::Attack,
        ActionCategory::Spirit,
    ] {
        let (mut ctx, mut host) = fresh(category, 0);
        host.party_count = 3;
        ctx.action_state = ActionState::ActionSeed.as_byte();
        step(&mut host, &mut ctx);
        assert!(
            host.take().contains(&Event::Ui(0x44, 0)),
            "no caster banner for {category:?}"
        );
    }
}

/// The one category `FUN_801E6D84` returns from before raising anything is
/// `5` - which is **Run / Defend**, not Spirit (`4`).
#[test]
fn action_seed_raises_no_banner_for_the_run_category() {
    let (mut ctx, mut host) = fresh(ActionCategory::Run, 0);
    host.party_count = 3;
    ctx.action_state = ActionState::ActionSeed.as_byte();
    step(&mut host, &mut ctx);
    assert!(!host.take().iter().any(|e| matches!(e, Event::Ui(0x44, _))));
}

/// Categories `0` and `4` raise the caster banner and then skip the target
/// arm outright - the caster banner has already been raised by then.
#[test]
fn spirit_and_arts_raise_only_the_caster_banner() {
    for category in [ActionCategory::TacticalArts, ActionCategory::Spirit] {
        let (mut ctx, mut host) = fresh(category, 0);
        host.party_count = 3;
        host.actors[0].active_target = 4; // a named monster
        ctx.action_state = ActionState::ActionSeed.as_byte();
        step(&mut host, &mut ctx);
        let events = host.take();
        assert!(events.contains(&Event::Ui(0x44, 0)), "{category:?}");
        assert!(!events.contains(&Event::Ui(0x51, 0)), "{category:?}");
    }
}

/// A named monster target adds the single-target banner `0x51` on top of the
/// caster banner; a party-slot target does not.
#[test]
fn a_named_monster_target_adds_the_target_banner() {
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 0);
    host.party_count = 3;
    host.actors[0].active_target = 4;
    ctx.action_state = ActionState::ActionSeed.as_byte();
    step(&mut host, &mut ctx);
    let events = host.take();
    assert!(events.contains(&Event::Ui(0x44, 0)));
    assert!(events.contains(&Event::Ui(0x51, 0)));

    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 0);
    host.party_count = 3;
    host.actors[0].active_target = 1; // an ally
    ctx.action_state = ActionState::ActionSeed.as_byte();
    step(&mut host, &mut ctx);
    let events = host.take();
    assert!(events.contains(&Event::Ui(0x44, 0)));
    assert!(!events.contains(&Event::Ui(0x51, 0)));
}

#[test]
fn attack_face_in_range_routes_to_chain() {
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 1);
    ctx.action_state = ActionState::AttackFace.as_byte();
    host.actors[1].active_target = 4;
    // No range entry → returns 0 (in range).
    let out = step(&mut host, &mut ctx);
    assert!(matches!(
        out,
        StepOutcome::Transition {
            to,
            ..
        } if to == ActionState::AttackChain.as_byte()
    ));
}

#[test]
fn attack_face_out_of_range_party_routes_to_short_step() {
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 1);
    ctx.action_state = ActionState::AttackFace.as_byte();
    host.actors[1].active_target = 4;
    host.ranges.insert((1, 4), 100);
    let out = step(&mut host, &mut ctx);
    assert!(matches!(
        out,
        StepOutcome::Transition {
            to,
            ..
        } if to == ActionState::AttackShortStep.as_byte()
    ));
}

#[test]
fn attack_face_out_of_range_monster_routes_to_windup() {
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 4);
    ctx.action_state = ActionState::AttackFace.as_byte();
    host.actors[4].active_target = 1;
    host.ranges.insert((4, 1), 100);
    let out = step(&mut host, &mut ctx);
    assert!(matches!(
        out,
        StepOutcome::Transition {
            to,
            ..
        } if to == ActionState::AttackWindup.as_byte()
    ));
}

#[test]
fn the_attack_x2_refill_replays_the_stream_once_and_demotes_the_starters() {
    // Retail's `0x801E39B4..0x801E3A64` arm: a party actor whose character
    // record carries the War God Icon bit replays the whole action stream
    // once, with every marked starter demoted from 0x1A to 0x19.
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 1);
    ctx.action_state = ActionState::AttackChain.as_byte();
    host.ability_bits
        .insert(1, crate::battle_action::WAR_GOD_ATTACK_X2_BIT);
    host.actors[1].params[0] = 0x1A;
    host.actors[1].params[1] = 0x27;

    // Stage both bytes; the second step reads the terminator at the new
    // cursor and takes the refill instead of dropping to recovery.
    for _ in 0..2 {
        assert_eq!(step(&mut host, &mut ctx), StepOutcome::Stay);
        host.actors[1].flag_bits.clear(ActorFlags::ADVANCE_DONE);
    }
    assert_eq!(
        ctx.action_state,
        ActionState::AttackChain.as_byte(),
        "the refill keeps the band in the strike loop"
    );
    assert_eq!(ctx.attack_x2_pass, 1);
    assert_eq!(host.actors[1].strike_index, 0, "the cursor is rewound");
    assert_eq!(
        host.actors[1].params[0], 0x19,
        "the marked starter is demoted for the second pass"
    );
    assert_eq!(host.actors[1].params[1], 0x27, "the art constant is kept");

    // The second pass runs to the terminator and then drops to recovery -
    // the counter is no longer zero, so the arm does not fire twice.
    for _ in 0..2 {
        step(&mut host, &mut ctx);
        host.actors[1].flag_bits.clear(ActorFlags::ADVANCE_DONE);
    }
    assert_eq!(ctx.action_state, ActionState::AttackRecovery.as_byte());
    assert_eq!(ctx.attack_x2_pass, 1, "the pair runs exactly twice");
}

#[test]
fn without_the_war_god_bit_the_stream_is_not_replayed() {
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 1);
    ctx.action_state = ActionState::AttackChain.as_byte();
    host.actors[1].params[0] = 0x1A;
    host.actors[1].params[1] = 0x27;
    for _ in 0..2 {
        step(&mut host, &mut ctx);
        host.actors[1].flag_bits.clear(ActorFlags::ADVANCE_DONE);
    }
    assert_eq!(ctx.action_state, ActionState::AttackRecovery.as_byte());
    assert_eq!(ctx.attack_x2_pass, 0);
    assert_eq!(host.actors[1].params[0], 0x1A, "the queue is untouched");
}

#[test]
fn attack_chain_walks_param_stream_until_terminator() {
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 1);
    ctx.action_state = ActionState::AttackChain.as_byte();
    // Strike sequence: 0x10, 0x12, 0xFF (terminator).
    host.actors[1].params[0] = 0x10;
    host.actors[1].params[1] = 0x12;
    host.actors[1].params[2] = 0xFF;

    // First step: queue 0x10 and fire damage.
    assert_eq!(step(&mut host, &mut ctx), StepOutcome::Stay);
    assert_eq!(host.actors[1].queued_anim, 0x10);
    assert_eq!(host.actors[1].strike_index, 1);
    assert!(host.actors[1].flag_bits.has(ActorFlags::ADVANCE_DONE));
    // The strike loop fires no `apply_damage`: retail's one `jal 0x800402f4`
    // is in the spirit band, not here.
    assert!(
        !host
            .take()
            .iter()
            .any(|e| matches!(e, Event::ApplyDamage(..)))
    );

    // While ADVANCE_DONE is set the staged swing is in flight - the
    // chain holds without reading the next byte (the 0x801E370C gate).
    assert_eq!(step(&mut host, &mut ctx), StepOutcome::Stay);
    assert_eq!(host.actors[1].strike_index, 1, "gated - no byte read");
    assert!(host.take().is_empty(), "gated - no damage fired");

    // Anim system signals clip end (clears ADVANCE_DONE).
    host.actors[1].flag_bits.clear(ActorFlags::ADVANCE_DONE);

    // Second step: queue 0x12 and fire damage.
    assert_eq!(step(&mut host, &mut ctx), StepOutcome::Stay);
    assert_eq!(host.actors[1].queued_anim, 0x12);
    assert_eq!(host.actors[1].strike_index, 2);
    assert!(
        !host
            .take()
            .iter()
            .any(|e| matches!(e, Event::ApplyDamage(..)))
    );
    host.actors[1].flag_bits.clear(ActorFlags::ADVANCE_DONE);

    // Third step: terminator → recovery; SM clears ADVANCE_DONE. The cursor
    // is NOT rewound - retail never touches `ctx[+0x15]` in the `0x1E` loop
    // beyond its post-increment (`0x801E3734..0x801E3748`); it stays on the
    // terminator index until `0x1F -> 0x20` parks it at `0xFF`.
    let out = step(&mut host, &mut ctx);
    assert!(matches!(
        out,
        StepOutcome::Transition {
            to,
            ..
        } if to == ActionState::AttackRecovery.as_byte()
    ));
    assert_eq!(
        host.actors[1].strike_index, 2,
        "cursor left on the terminator"
    );
    assert!(!host.actors[1].flag_bits.has(ActorFlags::ADVANCE_DONE));

    // `0x1F`: nothing in flight, so idle is staged over the last clip and the
    // cursor parks at `0xFF` on the way to `0x20`
    // (`0x801E3B04..0x801E3B1C`).
    ctx.action_state = ActionState::AttackRecovery.as_byte();
    let out = step(&mut host, &mut ctx);
    assert!(matches!(
        out,
        StepOutcome::Transition {
            to,
            ..
        } if to == ActionState::AttackReturn.as_byte()
    ));
    assert_eq!(host.actors[1].strike_index, STRIKE_CURSOR_PARKED);
    assert_eq!(
        host.actors[1].queued_anim, 0,
        "idle staged over the last clip"
    );
}

#[test]
fn attack_recovery_holds_until_advance_done_clears() {
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 1);
    ctx.action_state = ActionState::AttackRecovery.as_byte();
    host.actors[1].flag_bits.set(ActorFlags::ADVANCE_DONE);
    assert_eq!(step(&mut host, &mut ctx), StepOutcome::Stay);
    host.actors[1].flag_bits.clear(ActorFlags::ADVANCE_DONE);
    let out = step(&mut host, &mut ctx);
    assert!(matches!(
        out,
        StepOutcome::Transition {
            to,
            ..
        } if to == ActionState::AttackReturn.as_byte()
    ));
}

#[test]
fn attack_return_with_counter_attack_loops_back_to_chain() {
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 1);
    ctx.action_state = ActionState::AttackReturn.as_byte();
    ctx.counter_attack_a = 1;
    ctx.counter_attack_b = 1;
    let out = step(&mut host, &mut ctx);
    assert!(matches!(
        out,
        StepOutcome::Transition {
            to,
            ..
        } if to == ActionState::AttackChain.as_byte()
    ));
    // Bumped turn cursor (the "swap" signal, retail `0x801E36D0`).
    assert_eq!(ctx.turn_cursor, 1);
}

#[test]
fn attack_return_without_counter_attack_routes_to_done_cleanup() {
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 1);
    ctx.action_state = ActionState::AttackReturn.as_byte();
    let out = step(&mut host, &mut ctx);
    assert!(matches!(
        out,
        StepOutcome::Transition {
            to,
            ..
        } if to == ActionState::DoneCleanup.as_byte()
    ));
}

#[test]
fn magic_cast_begin_capture_spell_routes_to_capture_branch() {
    let (mut ctx, mut host) = fresh(ActionCategory::Magic, 1);
    ctx.action_state = ActionState::MagicCastBegin.as_byte();
    host.actors[1].params[0] = 0x42;
    host.capture_spells.insert(0x42);
    let out = step(&mut host, &mut ctx);
    assert!(matches!(
        out,
        StepOutcome::Transition {
            to,
            ..
        } if to == ActionState::MagicCaptureBranch.as_byte()
    ));
    assert!(host.take().contains(&Event::LoadCapture(0x42)));
}

#[test]
fn magic_cast_begin_subtracts_mp_with_ability_bits() {
    let (mut ctx, mut host) = fresh(ActionCategory::Magic, 1);
    ctx.action_state = ActionState::MagicCastBegin.as_byte();
    host.actors[1].mp = 50;
    host.actors[1].params[0] = 0x10;
    host.spell_costs.insert(0x10, 20);
    host.ability_bits.insert(1, 0x20); // half cost
    step(&mut host, &mut ctx);
    // 50 - 10 = 40
    assert_eq!(host.actors[1].mp, 40);
    assert_eq!(host.actors[1].last_mp_cost, 10);
}

#[test]
fn magic_cast_begin_quarter_cost_with_bit_10() {
    let (mut ctx, mut host) = fresh(ActionCategory::Magic, 1);
    ctx.action_state = ActionState::MagicCastBegin.as_byte();
    host.actors[1].mp = 50;
    host.actors[1].params[0] = 0x10;
    host.spell_costs.insert(0x10, 20);
    host.ability_bits.insert(1, 0x10); // quarter (shave 25%: cost - cost>>2)
    step(&mut host, &mut ctx);
    // cost 20 -> 20 - (20>>2) = 15; 50 - 15 = 35
    assert_eq!(host.actors[1].mp, 35);
}

#[test]
fn magic_pre_cast_wait_summon_route() {
    let (mut ctx, mut host) = fresh(ActionCategory::Magic, 1);
    ctx.action_state = ActionState::MagicPreCastWait.as_byte();
    ctx.frame_timer = 1;
    host.actors[1].sub_route = 9;
    // First step: timer goes to 0 (still positive). Stay.
    assert_eq!(step(&mut host, &mut ctx), StepOutcome::Stay);
    // Second step: timer crosses 0 → next state.
    let out = step(&mut host, &mut ctx);
    assert!(matches!(
        out,
        StepOutcome::Transition {
            to,
            ..
        } if to == ActionState::SummonInvoke.as_byte()
    ));
}

#[test]
fn done_cleanup_sets_recoil_per_category() {
    let (mut ctx, mut host) = fresh(ActionCategory::Spirit, 1);
    ctx.action_state = ActionState::DoneCleanup.as_byte();
    step(&mut host, &mut ctx);
    // Spirit category → recoil = 0x20.
    assert_eq!(host.actors[1].action_recoil, 0x20);
    assert!(host.actors[1].flag_bits.has(ActorFlags::EXIT));
    assert_eq!(ctx.frame_timer, 0x3C);
}

#[test]
fn done_cleanup_attack_uses_recover_pose() {
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 1);
    ctx.action_state = ActionState::DoneCleanup.as_byte();
    step(&mut host, &mut ctx);
    assert!(host.take().contains(&Event::Pose(1, Pose::Recover)));
}

#[test]
fn done_cleanup_run_screen_shakes() {
    let (mut ctx, mut host) = fresh(ActionCategory::Run, 1);
    ctx.action_state = ActionState::DoneCleanup.as_byte();
    step(&mut host, &mut ctx);
    assert!(host.take().contains(&Event::ScreenShake(0x500)));
}

/// `DoneCleanup`'s tail `jal`s the gauge re-arm (`FUN_801E93C8` at
/// `0x801E5F64`): every one of the seven pool slots gets the neutral arm-width
/// seed and any `+0x21C` latch holding exactly `1` is cleared.
#[test]
fn done_cleanup_rearms_the_command_gauge_slots() {
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 1);
    ctx.action_state = ActionState::DoneCleanup.as_byte();
    ctx.gauge_rearm_latch = 7;
    // Party gate: `+0x1D9 < 0x10` (a plain direction command).
    host.actors[1].current_anim = 0x0C;
    for (i, a) in host.actors.iter_mut().enumerate() {
        a.render_flag = if i % 2 == 0 { 1 } else { 200 };
        a.anim_rate = crate::battle_anim_rate::AnimRate(0);
    }
    step(&mut host, &mut ctx);
    for i in 0..crate::battle_gauge_rearm::GAUGE_SLOTS {
        assert_eq!(
            host.actors[i].anim_rate.get(),
            crate::battle_gauge_rearm::ARM_WIDTH_SEED,
            "slot {i} arm width seeded"
        );
        let expect_latch = if i % 2 == 0 { 0 } else { 200 };
        assert_eq!(host.actors[i].render_flag, expect_latch, "slot {i} latch");
    }
    // Slot 7 is outside retail's `while (i < 7)` walk.
    assert_eq!(host.actors[7].anim_rate.get(), 0);
    assert_eq!(ctx.gauge_rearm_latch, 0);
}

/// The gate is real: a materialised art (`+0x1D9 >= 0x10`) on a party slot
/// closes it and nothing is touched.
#[test]
fn done_cleanup_skips_the_rearm_for_a_materialised_art() {
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 1);
    ctx.action_state = ActionState::DoneCleanup.as_byte();
    ctx.gauge_rearm_latch = 7;
    host.actors[1].current_anim = 0x1B;
    for a in &mut host.actors {
        a.render_flag = 1;
        a.anim_rate = crate::battle_anim_rate::AnimRate(0);
    }
    step(&mut host, &mut ctx);
    assert!(host.actors.iter().all(|a| a.anim_rate.get() == 0));
    assert!(host.actors.iter().all(|a| a.render_flag == 1));
    assert_eq!(ctx.gauge_rearm_latch, 7);
}

/// A monster slot reads the art record's `+0x87` flag instead of the staged
/// id, through the host hook - the default (`0`) leaves the gate open.
#[test]
fn done_cleanup_rearm_gate_for_a_monster_slot_uses_the_record_flag() {
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 4);
    ctx.action_state = ActionState::DoneCleanup.as_byte();
    // A monster's staged id is irrelevant on this arm; the default hook
    // reports a clear record flag, so the re-arm runs.
    host.actors[4].current_anim = 0xFF;
    for a in &mut host.actors {
        a.anim_rate = crate::battle_anim_rate::AnimRate(0);
    }
    step(&mut host, &mut ctx);
    assert_eq!(
        host.actors[0].anim_rate.get(),
        crate::battle_gauge_rearm::ARM_WIDTH_SEED
    );
}

/// The `0x50` seed's one override (`0x801E5F2C..0x801E5F3C`): a level-up
/// banner on the context re-seeds `0x96` in place of the `0x3C` just stored,
/// for every category arm.
#[test]
fn done_cleanup_extends_the_tail_for_a_level_up_banner() {
    for category in [
        ActionCategory::Attack,
        ActionCategory::Magic,
        ActionCategory::Run,
    ] {
        let (mut ctx, mut host) = fresh(category, 1);
        ctx.action_state = ActionState::DoneCleanup.as_byte();
        ctx.levelup_banner_element = 0x65;
        step(&mut host, &mut ctx);
        assert_eq!(
            ctx.frame_timer,
            crate::battle_action::done::DONE_LEVELUP_BANNER_FRAMES,
            "{category:?}"
        );
    }
}

/// The banner skip (`0x801E6078..0x801E60B4`) is two gates deep: a pad word
/// AND a countdown already below `0x5B`. Above the threshold the press does
/// nothing, so the banner is guaranteed its opening frames.
#[test]
fn done_fade_down_banner_skip_needs_both_the_pad_and_the_threshold() {
    use crate::battle_action::done::{DONE_BANNER_SKIP_BELOW, DONE_LEVELUP_BANNER_FRAMES};

    // Pad held, but the countdown is still above the threshold: no skip.
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 1);
    host.pad_word = 0x40;
    ctx.action_state = ActionState::DoneFadeDown.as_byte();
    ctx.levelup_banner_element = 0x65;
    ctx.frame_timer = DONE_LEVELUP_BANNER_FRAMES;
    step(&mut host, &mut ctx);
    assert_eq!(ctx.frame_timer, DONE_LEVELUP_BANNER_FRAMES - 1);

    // Below the threshold with the pad held: the tail is cut immediately.
    ctx.frame_timer = DONE_BANNER_SKIP_BELOW;
    step(&mut host, &mut ctx);
    assert_eq!(ctx.frame_timer, -1);

    // Same frame, pad idle: the plain decrement.
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 1);
    ctx.action_state = ActionState::DoneFadeDown.as_byte();
    ctx.levelup_banner_element = 0x65;
    ctx.frame_timer = DONE_BANNER_SKIP_BELOW;
    step(&mut host, &mut ctx);
    assert_eq!(ctx.frame_timer, DONE_BANNER_SKIP_BELOW - 1);

    // Same frame, pad held, but no banner: the skip is banner-gated.
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 1);
    host.pad_word = 0x40;
    ctx.action_state = ActionState::DoneFadeDown.as_byte();
    ctx.frame_timer = DONE_BANNER_SKIP_BELOW;
    step(&mut host, &mut ctx);
    assert_eq!(ctx.frame_timer, DONE_BANNER_SKIP_BELOW - 1);
}

#[test]
fn done_fade_down_holds_then_routes_to_end_of_action() {
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 1);
    ctx.action_state = ActionState::DoneFadeDown.as_byte();
    ctx.frame_timer = 2;
    // Two ticks bring timer below 0.
    assert_eq!(step(&mut host, &mut ctx), StepOutcome::Stay);
    assert_eq!(step(&mut host, &mut ctx), StepOutcome::Stay);
    let out = step(&mut host, &mut ctx);
    assert!(matches!(
        out,
        StepOutcome::Transition {
            to,
            ..
        } if to == ActionState::EndOfAction.as_byte()
    ));
}

#[test]
fn done_fade_down_with_multi_cast_routes_to_multi_cast() {
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 1);
    ctx.action_state = ActionState::DoneFadeDown.as_byte();
    ctx.frame_timer = 0;
    ctx.multi_cast_gate = 1;
    let out = step(&mut host, &mut ctx);
    assert!(matches!(
        out,
        StepOutcome::Transition {
            to,
            ..
        } if to == ActionState::DoneMultiCast.as_byte()
    ));
}

/// Retail's Done-band exit is a test on the timer's **value**, re-run every
/// pass (`bgez v0,0x801E6158` at `0x801E60F0`), not on the one frame the
/// countdown crosses zero.
///
/// The difference only shows when something else holds the band on that exact
/// frame: `ctx[+0x276]` up on the crossing pass used to consume the crossing
/// and leave the state with no way out at all.
#[test]
fn the_done_band_still_leaves_after_the_menu_flag_clears_on_the_crossing_frame() {
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 1);
    ctx.action_state = ActionState::DoneFadeDown.as_byte();
    ctx.frame_timer = 0;
    ctx.menu_open = 1;
    // The flag is up on the frame the countdown would have crossed, and for
    // a good while after.
    for _ in 0..40 {
        assert_eq!(step(&mut host, &mut ctx), StepOutcome::Stay);
    }
    // Retail pins the countdown at the `0xC` floor rather than letting it
    // sink, so the tail still has twelve frames to run once the flag drops.
    assert_eq!(ctx.frame_timer, super::done::DONE_MENU_HOLD_FRAMES);
    ctx.menu_open = 0;
    let mut passes = 0;
    loop {
        match step(&mut host, &mut ctx) {
            StepOutcome::Stay => {
                passes += 1;
                assert!(passes < 64, "the Done band never left");
            }
            StepOutcome::Transition { to, .. } => {
                assert_eq!(to, ActionState::EndOfAction.as_byte());
                break;
            }
            other => panic!("unexpected {other:?}"),
        }
    }
    assert_eq!(
        passes,
        super::done::DONE_MENU_HOLD_FRAMES as usize,
        "the floor's twelve frames still had to run"
    );
}

/// The multi-cast branch re-seeds the countdown (`li v0,0xb4` at
/// `0x801E6134`). Without it the state inherits an expired timer and the
/// action never ends.
#[test]
fn the_multi_cast_branch_reseeds_its_own_timer_and_then_ends_the_action() {
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 1);
    ctx.action_state = ActionState::DoneFadeDown.as_byte();
    ctx.frame_timer = 0;
    ctx.multi_cast_gate = 1;
    let out = step(&mut host, &mut ctx);
    assert!(matches!(
        out,
        StepOutcome::Transition { to, .. } if to == ActionState::DoneMultiCast.as_byte()
    ));
    assert_eq!(ctx.frame_timer, super::done::DONE_MULTI_CAST_FRAMES);

    let mut passes = 0;
    loop {
        match step(&mut host, &mut ctx) {
            StepOutcome::Stay => {
                passes += 1;
                assert!(passes < 256, "DoneMultiCast parked on an expired timer");
            }
            StepOutcome::Transition { to, .. } => {
                assert_eq!(to, ActionState::EndOfAction.as_byte());
                break;
            }
            other => panic!("unexpected {other:?}"),
        }
    }
    assert_eq!(passes, super::done::DONE_MULTI_CAST_FRAMES as usize);
    assert_eq!(ctx.multi_cast_gate, 0, "the gate is consumed");
}

/// The whole Done band is bounded by `ctx[+0x6D8]`, and a settling HP bar
/// only *freezes* the countdown - it never removes the bound. Once the
/// readout has converged the remaining frames run out on schedule.
#[test]
fn a_settling_hp_bar_freezes_the_done_countdown_without_unbounding_it() {
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 0);
    ctx.action_state = ActionState::DoneFadeDown.as_byte();
    ctx.frame_timer = 4;
    // Target the acting party slot and leave its readout short of live HP.
    host.actors[0].active_target = 0;
    host.actors[0].hp = 40;
    host.actors[0].hp_display = Some(90);
    for _ in 0..30 {
        assert_eq!(step(&mut host, &mut ctx), StepOutcome::Stay);
    }
    assert_eq!(ctx.frame_timer, 4, "the countdown is frozen, not spent");
    host.actors[0].hp_display = Some(40);
    for _ in 0..4 {
        assert_eq!(step(&mut host, &mut ctx), StepOutcome::Stay);
    }
    assert!(matches!(
        step(&mut host, &mut ctx),
        StepOutcome::Transition { to, .. } if to == ActionState::EndOfAction.as_byte()
    ));
}

#[test]
fn end_of_action_party_wipe_signals_battle_end() {
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 0);
    ctx.action_state = ActionState::EndOfAction.as_byte();
    // Kill all party.
    host.actors[0].liveness = 0;
    host.actors[1].liveness = 0;
    host.actors[2].liveness = 0;
    let out = step(&mut host, &mut ctx);
    assert_eq!(out, StepOutcome::BattleComplete);
    assert!(
        host.take()
            .contains(&Event::BattleEnd(BattleEndCause::PartyWipe))
    );
}

/// "No party seated" is not "party dead". A battle whose party slots were
/// never seated (`slot_seated == false`, the port-only state retail cannot
/// represent - see `BattleActionHost::slot_seated`) must NOT resolve as a
/// party wipe when those hollow slots read dead; the round continues.
#[test]
fn end_of_action_unseated_party_is_not_reported_as_a_party_wipe() {
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 0);
    ctx.action_state = ActionState::EndOfAction.as_byte();
    for i in 0..3u8 {
        host.actors[i as usize].liveness = 0;
        host.unseated.insert(i);
    }
    let out = step(&mut host, &mut ctx);
    assert_ne!(
        out,
        StepOutcome::BattleComplete,
        "an unseeded battle must not end on the party-wipe arm"
    );
    assert!(
        !host
            .take()
            .contains(&Event::BattleEnd(BattleEndCause::PartyWipe)),
        "no PartyWipe may be signalled for a party that was never seated"
    );
}

/// The unseeded state must still be able to TERMINATE: with no seated party
/// and the monsters down, the monster-wipe arm still fires, so a host that
/// reaches this port-only state tears down to victory rather than spinning.
#[test]
fn end_of_action_unseated_party_still_resolves_a_monster_wipe() {
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 0);
    ctx.action_state = ActionState::EndOfAction.as_byte();
    for i in 0..3u8 {
        host.actors[i as usize].liveness = 0;
        host.unseated.insert(i);
    }
    for i in 3..ACTOR_SLOTS {
        host.actors[i].liveness = 0;
    }
    let out = step(&mut host, &mut ctx);
    assert_eq!(out, StepOutcome::BattleComplete);
    let events = host.take();
    assert!(events.contains(&Event::BattleEnd(BattleEndCause::MonsterWipe)));
    assert!(!events.contains(&Event::BattleEnd(BattleEndCause::PartyWipe)));
}

#[test]
fn end_of_action_monster_wipe_signals_battle_end() {
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 0);
    ctx.action_state = ActionState::EndOfAction.as_byte();
    // Kill all monsters.
    for i in 3..ACTOR_SLOTS {
        host.actors[i].liveness = 0;
    }
    let out = step(&mut host, &mut ctx);
    assert_eq!(out, StepOutcome::BattleComplete);
    assert!(
        host.take()
            .contains(&Event::BattleEnd(BattleEndCause::MonsterWipe))
    );
}

#[test]
fn end_of_action_monster_wipe_stages_victory_for_acting_party_slot() {
    // Retail baseline: a living party member dealt the kill - the victory
    // arm keeps the acting slot (0x801E6690 alive-skip) and stages the win
    // pose for it (0x801E6770..0x801E6790).
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 1);
    ctx.action_state = ActionState::EndOfAction.as_byte();
    for i in 3..ACTOR_SLOTS {
        host.actors[i].liveness = 0;
    }
    let out = step(&mut host, &mut ctx);
    assert_eq!(out, StepOutcome::BattleComplete);
    let events = host.take();
    assert!(events.contains(&Event::BattleEnd(BattleEndCause::MonsterWipe)));
    assert!(events.contains(&Event::VictoryStage(1)));
    assert_eq!(ctx.active_actor, 1);
}

#[test]
fn end_of_action_monster_wipe_repicks_pose_actor_when_acting_actor_dead() {
    // Retail re-pick (0x801E66A4..0x801E6724): the acting actor died during
    // its own action; the pose actor re-rolls onto a living, non-0x404
    // party slot. Slot 0 is dead and slot 1 carries 0x404, so slot 2 is the
    // only eligible pick.
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 0);
    ctx.action_state = ActionState::EndOfAction.as_byte();
    for i in 3..ACTOR_SLOTS {
        host.actors[i].liveness = 0;
    }
    host.actors[0].liveness = 0;
    host.actors[1].field_flags = 0x404;
    let out = step(&mut host, &mut ctx);
    assert_eq!(out, StepOutcome::BattleComplete);
    assert!(host.take().contains(&Event::VictoryStage(2)));
    assert_eq!(ctx.active_actor, 2);
}

#[test]
fn end_of_action_retail_mask_keeps_living_charmed_ally_blocking_victory() {
    // Without the widen (retail mask 0x4), a living charmed monster
    // (`+0x16E & 0x380`) still counts as standing - the battle continues.
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 3);
    ctx.action_state = ActionState::EndOfAction.as_byte();
    for i in 4..ACTOR_SLOTS {
        host.actors[i].liveness = 0;
    }
    host.actors[3].field_flags = 0x380;
    let out = step(&mut host, &mut ctx);
    assert!(matches!(
        out,
        StepOutcome::Transition { to, .. } if to == ActionState::PreActionWait.as_byte()
    ));
    assert!(!host.take().iter().any(|e| matches!(e, Event::BattleEnd(_))));
}

#[test]
fn charm_widen_victory_with_living_charmed_acting_ally_repicks_party_slot() {
    // The charm-softlock wedge condition: the charmed ally (slot 3, alive,
    // `0x380`) dealt the killing blow to the last real enemy, and the
    // randomizer's widened wipe mask (0x384) counts the ally itself as
    // down - victory fires with a living MONSTER as the acting actor.
    // Retail's alive-skip would then index the 3-byte party roster
    // `DAT_8007BD10` with slot 3 and arm a garbage win-pose stream (the
    // pinned softlock). The port must re-pick a living party slot and
    // terminate.
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 3);
    ctx.action_state = ActionState::EndOfAction.as_byte();
    ctx.charm_widen = true;
    for i in 4..ACTOR_SLOTS {
        host.actors[i].liveness = 0;
    }
    host.actors[3].field_flags = 0x380;
    host.rng_seq = vec![1];
    let out = step(&mut host, &mut ctx);
    assert_eq!(out, StepOutcome::BattleComplete);
    let events = host.take();
    assert!(events.contains(&Event::BattleEnd(BattleEndCause::MonsterWipe)));
    // Pose slot re-picked into the party band - never the monster slot.
    let staged = events.iter().find_map(|e| match e {
        Event::VictoryStage(s) => Some(*s),
        _ => None,
    });
    assert_eq!(staged, Some(1), "rng picks uniformly among 3 living slots");
    assert!(ctx.active_actor < 3);
}

#[test]
fn charm_widen_victory_terminates_even_when_no_party_slot_is_eligible() {
    // Where retail's rejection loop is unbounded: every living party member
    // carries 0x404. The port falls back to the first living party slot
    // instead of spinning.
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 3);
    ctx.action_state = ActionState::EndOfAction.as_byte();
    ctx.charm_widen = true;
    for i in 4..ACTOR_SLOTS {
        host.actors[i].liveness = 0;
    }
    host.actors[3].field_flags = 0x380;
    host.actors[0].liveness = 0;
    host.actors[1].field_flags = 0x404;
    host.actors[2].field_flags = 0x400;
    let out = step(&mut host, &mut ctx);
    assert_eq!(out, StepOutcome::BattleComplete);
    // Slot 1 is alive but 0x404 (non-targetable): the party-alive scan
    // masks 0x4, so slot 2 (0x400 - targetable) kept the party standing,
    // and the fallback picks the first living slot (1).
    assert!(host.take().contains(&Event::VictoryStage(1)));
}

#[test]
fn victory_pose_formation_override_forces_songi_slot() {
    // Retail 0x801E6728..0x801E676C: first monster id 0xB3 forces the
    // victory pose onto party slot 2 regardless of who acted.
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 0);
    ctx.action_state = ActionState::EndOfAction.as_byte();
    host.first_monster_id = 0xB3;
    for i in 3..ACTOR_SLOTS {
        host.actors[i].liveness = 0;
    }
    let out = step(&mut host, &mut ctx);
    assert_eq!(out, StepOutcome::BattleComplete);
    assert!(host.take().contains(&Event::VictoryStage(2)));
    assert_eq!(ctx.active_actor, 2);
}

#[test]
fn end_of_action_captured_monster_counts_as_down_under_retail_mask() {
    // Retail mask 0x4: an alive but non-targetable monster (captured) does
    // not block the monster-wipe victory.
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 0);
    ctx.action_state = ActionState::EndOfAction.as_byte();
    for i in 4..ACTOR_SLOTS {
        host.actors[i].liveness = 0;
    }
    host.actors[3].field_flags = 0x4;
    let out = step(&mut host, &mut ctx);
    assert_eq!(out, StepOutcome::BattleComplete);
    assert!(
        host.take()
            .contains(&Event::BattleEnd(BattleEndCause::MonsterWipe))
    );
}

#[test]
fn end_of_action_continues_when_both_sides_alive() {
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 0);
    ctx.action_state = ActionState::EndOfAction.as_byte();
    ctx.turn_cursor = 0;
    let out = step(&mut host, &mut ctx);
    // 8 alive total → bumped cursor (1) < 8 → restart at PreActionWait.
    assert!(matches!(
        out,
        StepOutcome::Transition {
            to,
            ..
        } if to == ActionState::PreActionWait.as_byte()
    ));
}

#[test]
fn run_begin_sets_timer_and_emits_run_ui() {
    let (mut ctx, mut host) = fresh(ActionCategory::Run, 1);
    ctx.action_state = ActionState::RunBegin.as_byte();
    step(&mut host, &mut ctx);
    assert_eq!(ctx.frame_timer, 0x3C);
    assert!(host.take().contains(&Event::Ui(0x43, 0)));
}

#[test]
fn run_begin_successful_escape_floors_downed_party_at_1() {
    // PORT: FUN_801E295C case 0x64 success branch - every party actor
    // with +0x14C == 0 is set to 1 (downed / petrified members leave the
    // battle alive). Monsters are untouched (the retail loop bound is
    // the party count ctx[+0]).
    let (mut ctx, mut host) = fresh(ActionCategory::Run, 1);
    ctx.action_state = ActionState::RunBegin.as_byte();
    ctx.multi_cast_gate = 1; // run roll succeeded
    host.actors[0].liveness = 0;
    host.actors[0].hp = 0;
    host.actors[2].liveness = 0;
    host.actors[4].liveness = 0; // monster stays down
    step(&mut host, &mut ctx);
    assert_eq!(host.actors[0].liveness, 1);
    // Retail's store is ONE halfword (+0x14C = 1), which the port models as
    // both `liveness` and `hp` - a floor that writes only the flag is undone
    // by any driver that re-derives liveness from HP (the live loop's
    // dead-marking pass does, every tick).
    assert_eq!(host.actors[0].hp, 1, "the +0x14C floor covers live HP too");
    assert_eq!(host.actors[2].liveness, 1);
    assert_eq!(host.actors[4].liveness, 0, "monsters are not revived");
    assert_eq!(ctx.multi_cast_gate, 1, "outcome gate left for RunWait");
}

#[test]
fn run_begin_failed_run_leaves_downed_party_down() {
    // The retail revive loop lives only in the success branch of case
    // 0x64; a failed run changes no HP.
    let (mut ctx, mut host) = fresh(ActionCategory::Run, 1);
    ctx.action_state = ActionState::RunBegin.as_byte();
    ctx.multi_cast_gate = 0; // run roll failed
    host.actors[0].liveness = 0;
    step(&mut host, &mut ctx);
    assert_eq!(host.actors[0].liveness, 0);
}

#[test]
fn run_wait_failed_run_routes_to_done_cleanup_and_battle_continues() {
    // Retail 0x65 failure branch: the action is consumed (Done band),
    // the battle continues - no battle-end signal.
    let (mut ctx, mut host) = fresh(ActionCategory::Run, 1);
    ctx.action_state = ActionState::RunWait.as_byte();
    ctx.frame_timer = 0;
    ctx.multi_cast_gate = 0; // run roll failed
    let out = step(&mut host, &mut ctx);
    assert!(matches!(
        out,
        StepOutcome::Transition {
            to,
            ..
        } if to == ActionState::DoneCleanup.as_byte()
    ));
}

#[test]
fn run_wait_escape_routes_to_run_escape_teardown() {
    // Retail 0x65 success branch -> 0x66: the escape teardown ends the
    // battle with the typed Escaped cause (DAT_8007BD71 = 0xFE, no wipe
    // cause byte).
    let (mut ctx, mut host) = fresh(ActionCategory::Run, 1);
    ctx.action_state = ActionState::RunWait.as_byte();
    ctx.frame_timer = 0;
    ctx.multi_cast_gate = 1; // run roll succeeded
    let out = step(&mut host, &mut ctx);
    assert!(matches!(
        out,
        StepOutcome::Transition {
            to,
            ..
        } if to == ActionState::RunEscape.as_byte()
    ));
    let out = step(&mut host, &mut ctx);
    assert!(matches!(out, StepOutcome::BattleComplete));
    assert!(
        host.take()
            .contains(&Event::BattleEnd(BattleEndCause::Escaped)),
        "escape teardown signals the typed Escaped cause"
    );
}

#[test]
fn capture_start_uses_rng_for_combo_offset() {
    let (mut ctx, mut host) = fresh(ActionCategory::Run, 5);
    ctx.action_state = ActionState::CaptureStart.as_byte();
    host.rng_seq = vec![1];
    step(&mut host, &mut ctx);
    // combo_timer += 0x780 + 0x80 (since rng%2 == 1) = 0x800 (2048).
    assert_eq!(ctx.combo_timer, 0x780 + 0x80);
    assert_eq!(ctx.frame_timer, 0x1E);
}

#[test]
fn capture_start_takedown_removes_the_monster() {
    // PORT: FUN_801E7824 - the state-0x68 arm zeroes the captured
    // monster's HP pair (+0x172 / +0x14C) and facing (+0x46), bumps the
    // +0x1DC flag byte by 1 (a raw increment, not a bit set), retargets
    // to 8 ("all"), and opens the run-UI banner (FUN_801D8DE8(0x43, 0)).
    let (mut ctx, mut host) = fresh(ActionCategory::Run, 5);
    ctx.action_state = ActionState::CaptureStart.as_byte();
    host.actors[5].hp = 120;
    host.actors[5].hp_display = Some(120);
    host.actors[5].facing_angle = 0x800;
    host.actors[5].flag_bits = ActorFlags(0x02);
    host.actors[5].active_target = 0;
    step(&mut host, &mut ctx);
    let a = &host.actors[5];
    assert_eq!(a.hp, 0, "+0x172 zeroed");
    assert_eq!(a.hp_display, Some(0));
    assert_eq!(a.liveness, 0, "+0x14C zeroed");
    assert_eq!(a.facing_angle, 0, "+0x46 zeroed");
    assert_eq!(a.flag_bits.0, 0x03, "+0x1DC incremented by 1");
    assert_eq!(a.active_target, 8, "+0x1DD = 8");
    assert!(
        host.take().contains(&Event::Ui(0x43, 0)),
        "run banner opened"
    );
}

#[test]
fn hp_bar_drain_freezes_done_fade_down() {
    // PORT: FUN_801E7250 - the state-0x51 arm only decrements the
    // +0x6D8 countdown when the settle check returns 0; a party target
    // (+0x1DD < 3) with live HP (+0x14C) != bar display (+0x172) holds
    // the timer.
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 1);
    ctx.action_state = ActionState::DoneFadeDown.as_byte();
    ctx.frame_timer = 0;
    host.actors[1].active_target = 0;
    host.actors[0].hp = 50;
    host.actors[0].hp_display = Some(80); // drain still animating
    assert_eq!(step(&mut host, &mut ctx), StepOutcome::Stay);
    assert_eq!(ctx.frame_timer, 0, "timer frozen while draining");
    // Drain settles → the timer counts down and the state advances.
    host.actors[0].hp_display = Some(50);
    let out = step(&mut host, &mut ctx);
    assert!(matches!(
        out,
        StepOutcome::Transition { to, .. } if to == ActionState::EndOfAction.as_byte()
    ));
}

#[test]
fn state_51_waits_for_the_bar_ramp_and_exits_on_settle() {
    // End-to-end over the machinery the gate assumes: a hit lands on a party
    // slot, seeding the `+0x10` accumulator (`FUN_801EC3E4` convention), the
    // per-frame ramp (`FUN_80047430`) drains it a quarter at a time, and the
    // action SM sits in state `0x51` until the bar catches up.
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 1);
    ctx.action_state = ActionState::DoneFadeDown.as_byte();
    ctx.frame_timer = 0;
    host.actors[1].active_target = 0;

    // The host draws this actor's bar, and a 40-point hit lands.
    host.actors[0].hp = 200;
    host.actors[0].arm_hp_bar();
    host.actors[0].hp -= 40;
    host.actors[0].accumulate_hp_bar(40);
    assert_eq!(host.actors[0].hp_display, Some(200));
    assert_eq!(host.actors[0].hp_bar_pending, 40);

    // Frames pass: the ramp runs, the SM holds.
    let mut frames = 0;
    let out = loop {
        crate::battle_action::tick_hp_bars(&mut host);
        let out = step(&mut host, &mut ctx);
        frames += 1;
        assert!(frames < 100, "state 0x51 never released");
        if out != StepOutcome::Stay {
            break out;
        }
        assert_eq!(ctx.frame_timer, 0, "timer frozen while the bar ramps");
    };
    assert!(frames > 1, "the gate has to hold for at least one frame");
    assert!(matches!(
        out,
        StepOutcome::Transition { to, .. } if to == ActionState::EndOfAction.as_byte()
    ));
    // Settled exactly on live HP - the total bar travel equals the seed.
    assert_eq!(host.actors[0].hp_display, Some(160));
    assert_eq!(host.actors[0].hp_bar_pending, 0);
}

#[test]
fn state_51_parks_forever_on_a_desynced_bar_with_a_zero_accumulator() {
    // The softlock shape: `+0x14C != +0x172` with `+0x10 == 0` on a party
    // slot. `FUN_80047430`'s guard at `0x800474E8` leaves the bar alone, so
    // the mismatch is absorbing and the `0x51` gate never releases.
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 1);
    ctx.action_state = ActionState::DoneFadeDown.as_byte();
    ctx.frame_timer = 0;
    host.actors[1].active_target = 0;
    host.actors[0].hp = 199;
    host.actors[0].hp_display = Some(200);
    host.actors[0].hp_bar_pending = 0;

    for _ in 0..64 {
        crate::battle_action::tick_hp_bars(&mut host);
        assert_eq!(step(&mut host, &mut ctx), StepOutcome::Stay);
    }
    assert_eq!(host.actors[0].hp_display, Some(200), "bar never moved");

    // The one re-sync in the dumped corpus (`FUN_801E752C`, the per-round
    // status ticker) clears it, and the action completes.
    host.actors[0].resync_hp_bar();
    let out = step(&mut host, &mut ctx);
    assert!(matches!(
        out,
        StepOutcome::Transition { to, .. } if to == ActionState::EndOfAction.as_byte()
    ));
}

/// The kernel-level guard for direct runtime HP writers:
/// [`BattleActor::set_hp_synced`] is the retail ticker's write-then-re-sync
/// shape, so a direct write through it can never produce the absorbing
/// `hp != hp_display` / zero-accumulator pair the test above parks on.
#[test]
fn set_hp_synced_direct_write_does_not_park_the_drain_gate() {
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 1);
    ctx.action_state = ActionState::DoneFadeDown.as_byte();
    ctx.frame_timer = 0;
    host.actors[1].active_target = 0;
    host.actors[0].hp = 120;
    host.actors[0].arm_hp_bar();

    // The trap, for contrast: a bare `hp` write desyncs the pair and the
    // gate holds (the coherence tripwire names exactly this state).
    host.actors[0].hp = 250;
    assert!(crate::battle_action::hp_bar_drain_pending(&host, &ctx));

    // The guarded write settles the pair in the same call - the gate is
    // open on the very next step, no ramp frames needed.
    host.actors[0].set_hp_synced(250);
    host.actors[0].debug_assert_hp_bar_coherent(0);
    assert!(!crate::battle_action::hp_bar_drain_pending(&host, &ctx));
    assert_eq!(host.actors[0].hp_display, Some(250));
    assert_eq!(host.actors[0].hp_bar_pending, 0);
    let out = step(&mut host, &mut ctx);
    assert!(matches!(
        out,
        StepOutcome::Transition { to, .. } if to == ActionState::EndOfAction.as_byte()
    ));

    // A non-animating host (`hp_display == None`) stays non-animating.
    host.actors[2].hp_display = None;
    host.actors[2].set_hp_synced(77);
    assert_eq!(host.actors[2].hp, 77);
    assert_eq!(host.actors[2].hp_display, None);
}

#[test]
fn monster_bar_settles_in_one_frame() {
    // `FUN_80047430`'s monster arm takes the whole delta at once, so a
    // monster target can never hold the gate even when the host animates it.
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 1);
    ctx.action_state = ActionState::DoneFadeDown.as_byte();
    host.actors[4].hp = 100;
    host.actors[4].arm_hp_bar();
    host.actors[4].hp -= 60;
    host.actors[4].accumulate_hp_bar(60);
    crate::battle_action::tick_hp_bars(&mut host);
    assert_eq!(host.actors[4].hp_display, Some(40));
    assert_eq!(host.actors[4].hp_bar_pending, 0);
    let _ = &mut ctx;
}

#[test]
fn state_51_park_from_the_clamp_asymmetry() {
    // The softlock, reproduced end to end from the two appliers disagreeing
    // about what to clamp against (`FUN_801EC3E4`'s readout-side clamp at
    // `0x801EDB70` vs the live-HP-side commit at `0x801EEA10`). NB the
    // asymmetry only amplifies: the pre-lagged readout seeded below is the
    // precondition, and no retail-only capture has produced one that survives
    // an action's own commit + settle wait (battle-action.md, clamp section).
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 1);
    ctx.action_state = ActionState::DoneFadeDown.as_byte();
    ctx.frame_timer = 0;
    host.actors[1].active_target = 0;

    // A party slot whose readout is already behind live HP - the state a
    // previous action's assigning seed, or a truncated drain, leaves.
    host.actors[0].hp = 500;
    host.actors[0].arm_hp_bar();
    host.actors[0].hp_display = Some(40);
    host.actors[0].hp_bar_pending = 0;

    // A survivable hit, bigger than the readout but smaller than live HP.
    // The readout-side clamp caps the accumulator at the drawn 40; live HP
    // takes the whole 100.
    host.actors[0].hp -= 100;
    host.actors[0].accumulate_hp_bar(100);
    assert_eq!(host.actors[0].hp_bar_pending, 40, "clamped to the readout");

    // Drain it out. The readout lands on zero, live HP is still 400.
    for _ in 0..64 {
        crate::battle_action::tick_hp_bars(&mut host);
    }
    assert_eq!(
        host.actors[0].hp_display,
        Some(0),
        "readout drained to zero"
    );
    assert_eq!(host.actors[0].hp, 400, "live HP survived the hit");
    assert_eq!(host.actors[0].hp_bar_pending, 0, "accumulator spent");

    // Absorbing: `hp != hp_display` with a zero accumulator, so the ramp's
    // guard leaves the readout alone and the `0x51` gate never releases.
    for _ in 0..64 {
        crate::battle_action::tick_hp_bars(&mut host);
        assert_eq!(step(&mut host, &mut ctx), StepOutcome::Stay);
    }
    assert_eq!(ctx.frame_timer, 0, "the countdown never ran");
    assert_eq!(host.actors[0].hp_display, Some(0));

    // A later ordinary hit does not clear it - the offset rides along.
    host.actors[0].hp -= 10;
    host.actors[0].accumulate_hp_bar(10);
    for _ in 0..64 {
        crate::battle_action::tick_hp_bars(&mut host);
        assert_eq!(step(&mut host, &mut ctx), StepOutcome::Stay);
    }
    assert_ne!(
        host.actors[0].hp_display,
        Some(host.actors[0].hp),
        "still desynced"
    );
}

#[test]
fn cast_census_drives_the_magic_band_exit_gate() {
    // `magic_exit_gate` (ctx `+0x249`) had no writer outside tests. The
    // census makes it a live measurement over the actor pool, so a visible
    // actor stuck mid-animation holds `MagicExit` open and clearing it
    // releases the state.
    let (mut ctx, mut host) = fresh(ActionCategory::Magic, 1);
    ctx.action_state = ActionState::MagicExit.as_byte();
    for a in &mut host.actors {
        a.render_color = 0;
        a.current_anim = 0;
    }
    host.actors[4].render_color = 0x0100_0000;
    host.actors[4].current_anim = 3;

    crate::battle_action::tick_cast_census(&host, &mut ctx);
    assert_eq!(ctx.magic_exit_gate, 1, "one visible actor still animating");
    assert_eq!(step(&mut host, &mut ctx), StepOutcome::Stay);

    host.actors[4].current_anim = 0;
    crate::battle_action::tick_cast_census(&host, &mut ctx);
    assert_eq!(ctx.magic_exit_gate, 0);
    let out = step(&mut host, &mut ctx);
    assert!(matches!(
        out,
        StepOutcome::Transition { to, .. } if to == ActionState::DoneCleanup.as_byte()
    ));
}

#[test]
fn cast_census_latches_the_sole_survivor_targets() {
    let (mut ctx, host) = fresh(ActionCategory::Magic, 1);
    // `fresh` marks every slot alive, so neither latch survives the
    // "exactly one" test.
    crate::battle_action::tick_cast_census(&host, &mut ctx);
    assert_eq!((ctx.item_target_a, ctx.item_target_b), (0, 0));

    let (mut ctx, mut host) = fresh(ActionCategory::Magic, 1);
    for (i, a) in host.actors.iter_mut().enumerate() {
        a.liveness = u16::from(i == 2 || i == 5);
    }
    crate::battle_action::tick_cast_census(&host, &mut ctx);
    assert_eq!(ctx.item_target_a, 3, "party slot 2, stored 1-based");
    assert_eq!(ctx.item_target_b, 5, "monster slot 5, stored 0-based");
}

#[test]
fn hp_bar_drain_monster_target_never_pends() {
    // FUN_801E7250's `2 < bVar1` early-out: monster targets (3..=7)
    // return 0 (settled) without inspecting the HP pair.
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 1);
    ctx.action_state = ActionState::DoneFadeDown.as_byte();
    ctx.frame_timer = 0;
    host.actors[1].active_target = 4;
    host.actors[4].hp = 10;
    host.actors[4].hp_display = Some(99);
    let out = step(&mut host, &mut ctx);
    assert!(matches!(
        out,
        StepOutcome::Transition { to, .. } if to == ActionState::EndOfAction.as_byte()
    ));
}

#[test]
fn hp_bar_drain_target_8_scans_the_party_side_only() {
    // FUN_801E7250's target-8 arm walks slots `0 .. ctx[+0x00] - 1`, and
    // `ctx[+0x00]` is the **party member count**, not the total actor count.
    // So an unsettled monster readout does not hold the gate even on the
    // all-target arm - the same answer the `3..=7` early-out gives.
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 1);
    ctx.action_state = ActionState::DoneFadeDown.as_byte();
    ctx.frame_timer = 0;
    host.actors[1].active_target = 8;
    host.actors[6].hp = 10;
    host.actors[6].hp_display = Some(11);
    let out = step(&mut host, &mut ctx);
    assert!(
        matches!(out, StepOutcome::Transition { to, .. } if to == ActionState::EndOfAction.as_byte()),
        "a monster slot is outside the scan window"
    );

    // A party slot inside the window does hold it.
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 1);
    ctx.action_state = ActionState::DoneFadeDown.as_byte();
    ctx.frame_timer = 0;
    host.actors[1].active_target = 8;
    host.actors[2].hp = 10;
    host.actors[2].hp_display = Some(11);
    assert_eq!(step(&mut host, &mut ctx), StepOutcome::Stay);
    host.actors[2].hp_display = None; // host stops animating → settled
    let out = step(&mut host, &mut ctx);
    assert!(matches!(
        out,
        StepOutcome::Transition { to, .. } if to == ActionState::EndOfAction.as_byte()
    ));
}

#[test]
fn capture_wait_marks_capture_state_after_timer() {
    let (mut ctx, mut host) = fresh(ActionCategory::Run, 5);
    ctx.action_state = ActionState::CaptureWait.as_byte();
    ctx.frame_timer = 0;
    let out = step(&mut host, &mut ctx);
    assert!(matches!(
        out,
        StepOutcome::Transition {
            to,
            ..
        } if to == ActionState::CaptureSustain.as_byte()
    ));
    assert_eq!(host.actors[5].capture_state, 2);
    assert_eq!(host.actors[5].render_flag, 2);
}

#[test]
fn full_attack_flow_round_trips() {
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 1);
    ctx.action_state = ActionState::Begin.as_byte();
    ctx.queued_action = 1;

    // Begin → PreActionWait.
    let out = step(&mut host, &mut ctx);
    assert!(matches!(out, StepOutcome::Transition { .. }));
    assert_eq!(ctx.action_state, ActionState::PreActionWait.as_byte());

    // PreActionWait → ActionSeed (prev_cleared = true by default).
    step(&mut host, &mut ctx);
    assert_eq!(ctx.action_state, ActionState::ActionSeed.as_byte());

    // ActionSeed → AttackFace.
    step(&mut host, &mut ctx);
    assert_eq!(ctx.action_state, ActionState::AttackFace.as_byte());

    // AttackFace → AttackChain (in range by default).
    step(&mut host, &mut ctx);
    assert_eq!(ctx.action_state, ActionState::AttackChain.as_byte());

    // AttackChain: walk one anim then terminator.
    host.actors[1].params[0] = 0x10;
    host.actors[1].params[1] = 0xFF;
    step(&mut host, &mut ctx); // queue 0x10; the strike loop applies no damage
    assert!(
        !host
            .take()
            .iter()
            .any(|e| matches!(e, Event::ApplyDamage(..)))
    );
    // Anim system signals the staged swing finished (clears the
    // 0x801E370C read gate) before the chain reads the next byte.
    host.actors[1].flag_bits.clear(ActorFlags::ADVANCE_DONE);
    step(&mut host, &mut ctx); // terminator → AttackRecovery, SM clears ADVANCE_DONE
    assert_eq!(ctx.action_state, ActionState::AttackRecovery.as_byte());
    assert!(!host.actors[1].flag_bits.has(ActorFlags::ADVANCE_DONE));

    // AttackRecovery (advance_done cleared by SM) → AttackReturn.
    step(&mut host, &mut ctx);
    assert_eq!(ctx.action_state, ActionState::AttackReturn.as_byte());

    // AttackReturn → DoneCleanup.
    step(&mut host, &mut ctx);
    assert_eq!(ctx.action_state, ActionState::DoneCleanup.as_byte());

    // DoneCleanup → DoneFadeDown.
    step(&mut host, &mut ctx);
    assert_eq!(ctx.action_state, ActionState::DoneFadeDown.as_byte());

    // Tick timer down until it transitions to EndOfAction.
    loop {
        let out = step(&mut host, &mut ctx);
        match out {
            StepOutcome::Stay => continue,
            StepOutcome::Transition { to, .. } => {
                assert_eq!(to, ActionState::EndOfAction.as_byte());
                break;
            }
            other => panic!("unexpected outcome during fade-down: {other:?}"),
        }
    }

    // EndOfAction (both sides alive) → PreActionWait.
    let out = step(&mut host, &mut ctx);
    assert!(matches!(
        out,
        StepOutcome::Transition {
            to,
            ..
        } if to == ActionState::PreActionWait.as_byte()
    ));
}

#[test]
fn unmapped_state_byte_surfaces_unknown() {
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 0);
    ctx.action_state = 0x07; // gap in the table
    let out = step(&mut host, &mut ctx);
    assert_eq!(out, StepOutcome::UnknownState { state: 0x07 });
}

#[test]
fn idle_hold_stays_and_pose_recover() {
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 1);
    ctx.action_state = ActionState::IdleHold.as_byte();
    let out = step(&mut host, &mut ctx);
    assert_eq!(out, StepOutcome::Stay);
    assert!(host.take().contains(&Event::Pose(1, Pose::Recover)));
}

#[test]
fn round_end_is_a_round_boundary_not_a_battle_end() {
    // Retail state 0xFF is written only by the NON-wipe arm of 0x5A (both
    // sides standing, everyone has acted); wipes signal through
    // DAT_8007BD71 = 0xFE without writing a state byte. Reaching 0xFF must
    // therefore never end the battle - the earlier port mapped it to
    // battle_end(MonsterWipe), a spurious victory.
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 0);
    // Leave a stale turn cursor; the boundary rewinds it.
    ctx.turn_cursor = 7;
    ctx.action_state = ActionState::RoundEnd.as_byte();
    let out = step(&mut host, &mut ctx);
    assert!(
        matches!(
            out,
            StepOutcome::Transition { to, .. } if to == ActionState::EndOfAction.as_byte()
        ),
        "round boundary hands control back through EndOfAction, got {out:?}"
    );
    assert!(
        !host.take().iter().any(|e| matches!(e, Event::BattleEnd(_))),
        "the round boundary must not signal battle end"
    );
    assert_eq!(
        ctx.turn_cursor, 0,
        "the turn cursor rewinds for the new round"
    );
}

/// Regression for the spurious-victory defect: drive a full attack action to
/// its end with BOTH sides alive and keep stepping without re-arming, the way
/// a driver that parks the SM at `EndOfAction` (e.g. a folded monster cast)
/// does. The turn cursor - seeded at `0` by `Begin` and bumped once per
/// end-of-action pass - reaches `alive_total` and routes the `0x5A` gate to
/// `0xFF`. Pre-fix that produced
/// `battle_end(BattleEndCause::MonsterWipe)` + `StepOutcome::BattleComplete`
/// after one round; post-fix it is a round boundary and the battle continues.
#[test]
fn full_round_with_both_sides_alive_does_not_end_the_battle() {
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 0);
    // 3 party + 1 monster alive = alive_total 4, the smallest retail-shaped
    // battle and the easiest for the stamped counter (3 + 1 bump) to reach.
    for i in 4..ACTOR_SLOTS {
        host.actors[i].liveness = 0;
    }
    // Arm exactly the way engine-core does (queued_action = 3, state Begin).
    ctx.queued_action = 3;
    ctx.action_state = ActionState::Begin.as_byte();
    // One swing then the terminator, so the attack chain resolves.
    host.actors[0].params[0] = 0x10;
    host.actors[0].params[1] = 0xFF;

    let mut saw_round_boundary = false;
    for _ in 0..0x400 {
        let out = step(&mut host, &mut ctx);
        // Stand in for the anim system: signal the staged swing finished so
        // the chain's 0x801E370C read gate opens (same edge the existing
        // full-flow test drives by hand).
        host.actors[0].flag_bits.clear(ActorFlags::ADVANCE_DONE);
        assert_ne!(
            out,
            StepOutcome::BattleComplete,
            "both sides alive: no step may complete the battle (state {:#04X})",
            ctx.action_state
        );
        if let StepOutcome::Transition { from, .. } = out
            && from == ActionState::RoundEnd.as_byte()
        {
            saw_round_boundary = true;
            break;
        }
    }
    assert!(
        saw_round_boundary,
        "the drive must cross the 0xFF round boundary to be non-vacuous"
    );
    assert!(
        !host.take().iter().any(|e| matches!(e, Event::BattleEnd(_))),
        "no battle-end cause may be signalled with both sides standing"
    );

    // The same gate still ends the battle on a GENUINE wipe: kill the last
    // monster and let the next EndOfAction pass resolve it.
    host.actors[3].liveness = 0;
    ctx.action_state = ActionState::EndOfAction.as_byte();
    let out = step(&mut host, &mut ctx);
    assert_eq!(out, StepOutcome::BattleComplete);
    assert!(
        host.take()
            .contains(&Event::BattleEnd(BattleEndCause::MonsterWipe)),
        "a genuine monster wipe still ends the battle"
    );
}

/// Full magic-spell flow walking from `MagicCastBegin` all the way to
/// `EndOfAction`, asserting each band transition. Mirrors the attack-flow
/// round-trip but exercises the magic dispatch table - `magic_cast_begin`
/// → `magic_pre_cast_wait` (with a cleared sub-route so we don't divert
/// to summon) → `magic_anim_chain` → `magic_sustain` → `magic_hit_loop`
/// → `magic_recovery` → `magic_exit` → `done_cleanup` → `done_fade_down`
/// → `end_of_action`.
#[test]
fn full_magic_flow_round_trips() {
    let (mut ctx, mut host) = fresh(ActionCategory::Magic, 1);
    ctx.action_state = ActionState::MagicCastBegin.as_byte();

    // Set spell ID + MP cost so MagicCastBegin doesn't crash on division.
    host.actors[1].params[0] = 0x10;
    host.actors[1].params[1] = 0x21; // first chain anim
    host.actors[1].params[2] = 0xFF; // chain terminator
    host.actors[1].mp = 100;
    host.spell_costs.insert(0x10, 20);
    host.actors[1].sub_route = 0; // not summon
    host.actors[1].current_anim = 0;
    host.actors[1].hit_count_bound = 0;

    // MagicCastBegin → MagicPreCastWait (no capture spell).
    step(&mut host, &mut ctx);
    assert_eq!(ctx.action_state, ActionState::MagicPreCastWait.as_byte());
    assert_eq!(host.actors[1].mp, 80); // 100 - 20

    // MagicPreCastWait gates on frame_timer; it was set to 0x14 by the
    // previous step. Tick until the timer fires the transition.
    let mut iters = 0;
    while ctx.action_state == ActionState::MagicPreCastWait.as_byte() {
        step(&mut host, &mut ctx);
        iters += 1;
        assert!(iters < 1000, "stuck in MagicPreCastWait");
    }
    assert_eq!(ctx.action_state, ActionState::MagicAnimChain.as_byte());
    // The wait's exit bumped the cursor past the spell id and staged the
    // first anim byte (`0x801E4644..0x801E4664`): `params[1] = 0x21` is on
    // the stage, and for a non-Seru id the second bump parked the cursor on
    // the terminator. The spell id itself is never staged as a clip.
    assert_eq!(host.actors[1].queued_anim, 0x21, "0x29 stages params[1]");
    assert_eq!(host.actors[1].strike_index, 2);

    // MagicAnimChain reads `params[strike_index]`: the terminator, so the
    // chain transitions on its first step.
    step(&mut host, &mut ctx);
    assert_eq!(ctx.action_state, ActionState::MagicSustain.as_byte());

    // MagicSustain holds while spell_iter != 0; we need to clear it.
    host.actors[1].spell_iter = 0;
    step(&mut host, &mut ctx);
    assert_eq!(ctx.action_state, ActionState::MagicHitLoop.as_byte());

    // MagicHitLoop exits when current_anim == 0 (default).
    step(&mut host, &mut ctx);
    assert_eq!(ctx.action_state, ActionState::MagicRecovery.as_byte());

    // MagicRecovery stays unless gate is 0 (default 0).
    step(&mut host, &mut ctx);
    assert_eq!(ctx.action_state, ActionState::MagicExit.as_byte());

    // MagicExit similarly stays unless gate is 0 (default 0).
    step(&mut host, &mut ctx);
    assert_eq!(ctx.action_state, ActionState::DoneCleanup.as_byte());

    // DoneCleanup → DoneFadeDown.
    step(&mut host, &mut ctx);
    assert_eq!(ctx.action_state, ActionState::DoneFadeDown.as_byte());

    // Drain DoneFadeDown's frame timer. Should land on EndOfAction.
    let mut tick_count = 0;
    while ctx.action_state == ActionState::DoneFadeDown.as_byte() {
        step(&mut host, &mut ctx);
        tick_count += 1;
        assert!(tick_count < 1000, "stuck in DoneFadeDown");
    }
    assert_eq!(ctx.action_state, ActionState::EndOfAction.as_byte());
}

/// `MagicCastBegin` with both `bits & 0x10` and `bits & 0x20` set - verifies
/// the cost path picks the **Half** (`0x20`) branch over Quarter (`0x10`).
/// Dump-confirmed against the retail state-`0x28` block (`FUN_801E295C`
/// `0x801E3D0C`): `andi 0x20; bne <half>` short-circuits the `0x10` test.
#[test]
fn magic_cast_begin_half_takes_priority_over_quarter() {
    let (mut ctx, mut host) = fresh(ActionCategory::Magic, 1);
    ctx.action_state = ActionState::MagicCastBegin.as_byte();
    host.actors[1].mp = 100;
    host.actors[1].params[0] = 0x10;
    host.spell_costs.insert(0x10, 40);
    // Both bits set - retail applies Half (0x20) and skips the 0x10 test.
    host.ability_bits.insert(1, 0x10 | 0x20);
    step(&mut host, &mut ctx);
    // Half: 40 - (40>>1) = 20; 100 - 20 = 80.
    assert_eq!(host.actors[1].mp, 80);
    assert_eq!(host.actors[1].last_mp_cost, 20);
}

/// `PreActionWait` is gated on `previous_action_cleared`. With the gate
/// closed, the state holds; flipping the gate transitions to `ActionSeed`
/// on the next step.
#[test]
fn pre_action_wait_holds_until_prev_cleared_flips() {
    let (mut ctx, mut host) = fresh(ActionCategory::Attack, 1);
    ctx.action_state = ActionState::PreActionWait.as_byte();
    host.prev_cleared = false;

    // Several steps with the gate closed must not transition.
    for _ in 0..8 {
        assert_eq!(step(&mut host, &mut ctx), StepOutcome::Stay);
        assert_eq!(ctx.action_state, ActionState::PreActionWait.as_byte());
    }

    // Flip the gate. Next step transitions.
    host.prev_cleared = true;
    let out = step(&mut host, &mut ctx);
    assert!(matches!(
        out,
        StepOutcome::Transition {
            to,
            ..
        } if to == ActionState::ActionSeed.as_byte()
    ));
}

// ---------------------------------------------------------------
// resolve_action_queue - Miracle / Super expansion glue tests.
// ---------------------------------------------------------------

#[test]
fn resolve_action_queue_triggers_miracle_art() {
    use legaia_art::{ActionConstant, Character, Command};
    // Vahn's Craze input: R D L U L U R D L
    let cmds = [
        Command::Right,
        Command::Down,
        Command::Left,
        Command::Up,
        Command::Left,
        Command::Up,
        Command::Right,
        Command::Down,
        Command::Left,
    ];
    let queue = resolve_action_queue(Character::Vahn, &cmds, &[]);
    // Miracle Art replacement ends with the Tornado Flame Miracle
    // finisher (0x2A).
    let last = queue.actions().last().copied().unwrap();
    assert_eq!(last, ActionConstant::Art2A);
    // First 4 are the directional unmasked bytes; 5th is the Special
    // Starter (0x1A).
    assert_eq!(queue.actions()[4], ActionConstant::SpecialStarter);
}

#[test]
fn resolve_action_queue_triggers_super_art_with_chained_arts() {
    use legaia_art::{ActionConstant, Character, Command};
    // Tri-Somersault find pattern (Vahn): 19 27 0F 19 1F 0E 19 27.
    // Equivalent player input: chained arts [Somersault, Cyclone, Somersault]
    // with directional inputs Up, Down between them.
    // Build the queue manually via the helper:
    let cmds = [Command::Up, Command::Down];
    let chained = [
        ActionConstant::Art27, // Somersault
        ActionConstant::Art1F, // Cyclone
        ActionConstant::Art27, // Somersault
    ];
    // Chained arts are bracketed by RegularStarter, so the queue
    // builds as [U, D, 19, 27, 19, 1F, 19, 27]. That doesn't match
    // the Tri-Somersault find pattern (which is 19 27 0F 19 1F 0E 19
    // 27). Manually reorder by feeding the directional inputs in the
    // exact slot order the retail UI would assemble:
    let _ = cmds; // commands aren't used in this fast-path test.

    // Instead, build the queue byte-equivalent to the find pattern.
    let mut q = legaia_art::ActionQueue::new();
    for b in [0x19u8, 0x27, 0x0F, 0x19, 0x1F, 0x0E, 0x19, 0x27] {
        q.push(ActionConstant::from_byte(b).unwrap());
    }
    let _ = chained;

    let matcher = legaia_art::SuperMatcher::with_default_table();
    let hit = matcher.try_trigger_at_tail(Character::Vahn, &mut q);
    assert!(hit.is_some(), "Tri-Somersault should fire");
}

#[test]
fn resolve_action_queue_no_special_match_keeps_chained() {
    use legaia_art::{ActionConstant, Character, Command};
    // Inputs that don't form a Miracle or Super Art - queue should
    // contain just the directional bytes + chained-art assembly with
    // no replacement.
    let cmds = [Command::Up, Command::Up];
    let chained = [ActionConstant::Art28]; // Charging Scorch
    let queue = resolve_action_queue(Character::Vahn, &cmds, &chained);
    let bytes: Vec<u8> = queue.actions().iter().map(|a| a.as_byte()).collect();
    assert_eq!(bytes, vec![0x0F, 0x0F, 0x19, 0x28]);
}

#[test]
fn art_record_default_returns_none() {
    // Default `BattleActionHost::art_record` returns `None`. Verify
    // the recording host returns `None` when no art records are
    // staged via `art_records`.
    use legaia_art::{ActionConstant, Character};
    let host = RecHost::default();
    assert!(
        host.art_record(Character::Vahn, ActionConstant::Art1B)
            .is_none()
    );
}

// ---------------------------------------------------------------
// Battle SM strike-band reads from art_record.
// ---------------------------------------------------------------

fn dmg_byte(target: legaia_art::PowerTarget, multiplier: u8) -> legaia_art::PowerByte {
    legaia_art::PowerByte::Damage(legaia_art::ArtPower {
        target,
        multiplier,
        alt_range: false,
    })
}

fn synthetic_art_record(
    action: legaia_art::ActionConstant,
    power: Vec<legaia_art::PowerByte>,
    dmg_timing: Vec<u8>,
) -> legaia_art::ArtRecord {
    legaia_art::ArtRecord {
        action,
        commands: vec![],
        anim_index: 0,
        anim_extra: vec![],
        name: None,
        power,
        dmg_timing,
        effect_cues: [legaia_art::EffectCue::default(); 2],
        hit_cues: vec![legaia_art::HitCue::from_word(0x0010_001A)],
        identifier: 0,
        anim_speed: 0x10,
        enemy_effect: legaia_art::EnemyEffect::Toxic,
        repeat_frames: legaia_art::RepeatFrames::default(),
        background: 0,
        runtime_address: None,
    }
}

#[test]
fn attack_chain_stages_art_bytes_and_calls_no_damage_kernel() {
    // Retail's strike loop (`FUN_801E295C` state 0x1E) never calls
    // `FUN_801EC3E4`: `jal 0x801ec3e4` does not occur in its 4099
    // instructions. The SM stages one byte per clip and leaves damage to the
    // anim tick's hit-event driver. With an art record staged AND a chosen
    // art, the band still resolves nothing itself.
    use legaia_art::{ActionConstant, Character, PowerTarget};

    let mut host = RecHost::with_n_actors(3);
    host.actors[0].character = Character::Vahn;
    host.actors[0].chosen_art = Some(ActionConstant::Art1B);
    host.actors[0].active_target = 1;
    host.actors[0].params[0] = 0x10;
    host.actors[0].params[1] = 0x11;
    host.actors[0].params[2] = 0xFF;
    host.art_records.insert(
        (
            character_byte(Character::Vahn),
            ActionConstant::Art1B.as_byte(),
        ),
        synthetic_art_record(
            ActionConstant::Art1B,
            vec![
                dmg_byte(PowerTarget::Udf, 18),
                dmg_byte(PowerTarget::Ldf, 22),
            ],
            vec![0x08, 0x14],
        ),
    );

    let mut ctx = BattleActionCtx::new();
    ctx.action_state = ActionState::AttackChain.as_byte();
    ctx.active_actor = 0;

    step(&mut host, &mut ctx);
    assert_eq!(host.actors[0].queued_anim, 0x10, "byte 0 staged");
    assert_eq!(host.actors[0].strike_index, 1);
    assert!(host.actors[0].flag_bits.has(ActorFlags::ADVANCE_DONE));
    host.actors[0].flag_bits.clear(ActorFlags::ADVANCE_DONE);
    step(&mut host, &mut ctx);
    assert_eq!(host.actors[0].queued_anim, 0x11, "byte 1 staged");
    host.actors[0].flag_bits.clear(ActorFlags::ADVANCE_DONE);
    let out = step(&mut host, &mut ctx);
    assert!(matches!(out, StepOutcome::Transition { to, .. }
        if to == ActionState::AttackRecovery.as_byte()));

    let events = host.take();
    assert!(
        events
            .iter()
            .all(|e| !matches!(e, Event::ApplyArtStrike(_) | Event::ApplyDamage(..))),
        "the strike loop is not a damage call site: {events:?}"
    );
    assert_eq!(
        host.actors[0].input_cursor, 0,
        "the hit index is the tick's"
    );
}

/// The hit-event driver's resolution for one admitted art hit: the power
/// byte and beat come from the **clip entry** (what `FUN_801EC3E4` reads),
/// the status effect and cue from the record the host resolves.
#[test]
fn art_strike_info_for_hit_reads_the_clip_power_and_the_record_side_data() {
    use legaia_art::{ActionConstant, Character, PowerTarget};

    let mut host = RecHost::with_n_actors(3);
    host.actors[0].character = Character::Vahn;
    host.actors[0].active_target = 1;
    host.actors[0].latched_anim = ActionConstant::Art1B.as_byte();
    host.art_records.insert(
        (
            character_byte(Character::Vahn),
            ActionConstant::Art1B.as_byte(),
        ),
        synthetic_art_record(
            ActionConstant::Art1B,
            vec![
                dmg_byte(PowerTarget::Udf, 18),
                dmg_byte(PowerTarget::Ldf, 22),
            ],
            vec![0x08, 0x14],
        ),
    );
    // The clip entry's power run / event list - an art record's embedded
    // entry: UDF x28 then LDF x28 at frames 6 and 12.
    let power_run = [0x1A, 0x1F, 0, 0];
    let events = [6, 12, 0, 0];
    let hit0 = hit_event_admits(0x1E, &power_run, &events, 0, 5).expect("beat 0");
    let art = staged_art_constant(host.actors[0].latched_anim, None, true).unwrap();
    let info = art_strike_info_for_hit(&host, 0, art, &hit0);
    assert_eq!(info.strike_index, 0);
    assert_eq!(info.anim_byte, ActionConstant::Art1B.as_byte());
    assert_eq!(info.actor_slot, 0);
    assert_eq!(info.target_slot, 1);
    assert_eq!(info.character, Character::Vahn);
    assert_eq!(info.art, ActionConstant::Art1B);
    assert_eq!(
        info.dmg_timing,
        Some(6),
        "the clip's beat, not the record's"
    );
    assert_eq!(info.enemy_effect, legaia_art::EnemyEffect::Toxic);
    assert!(info.hit_cue.is_some());
    assert!(matches!(
        info.power,
        Some(legaia_art::PowerByte::Damage(legaia_art::ArtPower {
            multiplier: 28,
            target: PowerTarget::Udf,
            ..
        }))
    ));
    let hit1 = hit_event_admits(0x1E, &power_run, &events, 1, 11).expect("beat 1");
    let info = art_strike_info_for_hit(&host, 0, art, &hit1);
    assert_eq!(info.strike_index, 1);
    assert!(matches!(
        info.power,
        Some(legaia_art::PowerByte::Damage(legaia_art::ArtPower {
            target: PowerTarget::Ldf,
            ..
        }))
    ));
    // Only one cue in the synthetic record: the second hit carries none.
    assert!(info.hit_cue.is_none());
}

#[test]
fn art_strike_info_without_a_record_still_resolves_the_clip_power() {
    use legaia_art::ActionConstant;
    let mut host = RecHost::with_n_actors(3);
    host.actors[0].active_target = 1;
    host.actors[0].latched_anim = ActionConstant::Art1B.as_byte();
    let hit = hit_event_admits(0x1E, &[0x16, 0, 0, 0], &[3, 0, 0, 0], 0, 2).unwrap();
    let info = art_strike_info_for_hit(&host, 0, ActionConstant::Art1B, &hit);
    assert!(matches!(
        info.power,
        Some(legaia_art::PowerByte::Damage(legaia_art::ArtPower {
            multiplier: 12,
            ..
        }))
    ));
    assert_eq!(info.enemy_effect, legaia_art::EnemyEffect::None);
    assert!(info.hit_cue.is_none());
}

// --- arms execution-time weapon fold ----------------------------------------
// REF: FUN_801EC3E4 (kernel + `// PORT:` tags live in
// `battle_formulas::arms_fold`; these drive it through the hit-event driver,
// the seat retail runs it from - the SM's strike loop never calls it)

/// The admitted hit a swing entry produces at its beat: power byte = the
/// committed command, one event frame.
fn swing_hit(command: u8) -> HitEvent {
    hit_event_admits(
        ActionState::AttackChain.as_byte(),
        &[command, 0, 0, 0],
        &[6, 0, 0, 0],
        0,
        5,
    )
    .expect("a swing entry admits its single hit")
}

#[test]
fn arms_command_folds_the_equipped_weapon_into_atk_working() {
    let (ctx, mut host) = fresh(ActionCategory::Attack, 1);
    // Command 0x0C reads equipment slot 2 and folds half its ATK bonus.
    host.actors[1].current_anim = 0x0C;
    host.actors[1].atk_working = 100;
    host.equip_atk.insert(1, [0, 0, 30, 0, 0]);

    let hit = swing_hit(0x0C);
    assert_eq!(
        fold_weapon_atk_on_hit(&mut host, 1, ctx.action_state, &hit),
        Some(15)
    );
    assert_eq!(
        host.actors[1].atk_working, 115,
        "slot 2 bonus 30 folds as 30 >> 1 = 15"
    );
}

#[test]
fn arms_command_0x11_folds_half_the_sum_of_every_slot() {
    let (ctx, mut host) = fresh(ActionCategory::Attack, 1);
    host.actors[1].current_anim = 0x11;
    host.equip_atk.insert(1, [10, 20, 30, 40, 50]);

    let hit = swing_hit(0x11);
    fold_weapon_atk_on_hit(&mut host, 1, ctx.action_state, &hit);
    assert_eq!(host.actors[1].atk_working, 75, "(10+20+30+40+50) >> 1");
}

#[test]
fn non_arms_commands_leave_atk_working_alone() {
    let (ctx, mut host) = fresh(ActionCategory::Attack, 1);
    // 0x19 is an art starter - admitted by the head gate (0x0C..=0x1F) but
    // outside the six-arm dispatch table, so it folds nothing.
    host.actors[1].current_anim = 0x19;
    host.actors[1].atk_working = 100;
    host.equip_atk.insert(1, [10, 20, 30, 40, 50]);

    let hit = swing_hit(0x19);
    assert_eq!(
        fold_weapon_atk_on_hit(&mut host, 1, ctx.action_state, &hit),
        None
    );
    assert_eq!(host.actors[1].atk_working, 100);
}

#[test]
fn arms_fold_is_gated_by_the_input_cursor_bound() {
    let (ctx, mut host) = fresh(ActionCategory::Attack, 1);
    host.actors[1].current_anim = 0x0C;
    host.actors[1].atk_working = 100;
    // Cursor at the bound: the resolver's head guard rejects `+0x1F4 >= 4`.
    host.actors[1].input_cursor = 4;
    host.equip_atk.insert(1, [0, 0, 30, 0, 0]);

    let hit = swing_hit(0x0C);
    assert_eq!(
        fold_weapon_atk_on_hit(&mut host, 1, ctx.action_state, &hit),
        None
    );
    assert_eq!(host.actors[1].atk_working, 100, "cursor 4 bails");
}

#[test]
fn arms_fold_does_not_run_for_enemy_slots() {
    // Slot 3 and above take the resolver's enemy branch, which performs no
    // equipment fold - the hit itself still resolves (the branch only skips
    // the fold), so the kernel admits it and the fold answers `None`.
    let (ctx, mut host) = fresh(ActionCategory::Attack, 3);
    host.actors[3].current_anim = 0x0C;
    host.actors[3].atk_working = 100;
    host.equip_atk.insert(3, [0, 0, 30, 0, 0]);

    let hit = swing_hit(0x0C);
    assert_eq!(
        fold_weapon_atk_on_hit(&mut host, 3, ctx.action_state, &hit),
        None
    );
    assert_eq!(host.actors[3].atk_working, 100);
}

// --- target_cursor (FUN_801da6b4) -------------------------------------------

fn cursor_host() -> (RecHost, BattleActionCtx) {
    let mut host = RecHost::with_n_actors(ACTOR_SLOTS);
    for a in &mut host.actors {
        a.liveness = 100;
    }
    let mut ctx = BattleActionCtx::new();
    ctx.active_actor = 0;
    // Acting actor points at monster slot 4.
    host.actors[0].active_target = 4;
    (host, ctx)
}

#[test]
fn target_cursor_brightens_pointed_at_and_dims_rest() {
    let (mut host, ctx) = cursor_host();
    target_cursor_highlight(&mut host, &ctx, true);

    assert_eq!(host.actors[4].render_flag, CURSOR_FLAG_SELECTED);
    assert_eq!(host.actors[4].render_color, CURSOR_COLOR_BRIGHT);
    assert_eq!(host.actors[4].render_blend, CURSOR_BLEND_ON);

    for slot in [3usize, 5, 6] {
        assert_eq!(host.actors[slot].render_flag, CURSOR_FLAG_DIMMED);
        assert_eq!(host.actors[slot].render_color, CURSOR_COLOR_DIM);
        assert_eq!(host.actors[slot].render_blend, CURSOR_BLEND_ON);
    }
}

#[test]
fn target_cursor_window_is_slots_three_through_six() {
    let (mut host, ctx) = cursor_host();
    target_cursor_highlight(&mut host, &ctx, true);
    for slot in [0usize, 1, 2, 7] {
        assert_eq!(host.actors[slot].render_flag, 0);
        assert_eq!(host.actors[slot].render_blend, 0);
    }
}

#[test]
fn target_cursor_skips_dead_slots() {
    let (mut host, ctx) = cursor_host();
    host.actors[5].liveness = 0;
    host.actors[5].render_flag = 42;
    target_cursor_highlight(&mut host, &ctx, true);
    assert_eq!(host.actors[5].render_flag, 42);
    assert_eq!(host.actors[5].render_blend, 0);
}

#[test]
fn target_cursor_disable_clears_tint() {
    let (mut host, ctx) = cursor_host();
    target_cursor_highlight(&mut host, &ctx, true);
    target_cursor_highlight(&mut host, &ctx, false);
    for slot in 3usize..=6 {
        assert_eq!(host.actors[slot].render_flag, 0);
        assert_eq!(host.actors[slot].render_blend, 0);
        assert_eq!(host.actors[slot].render_color, CURSOR_COLOR_BRIGHT);
    }
}

// ---------------------------------------------------------------------------
// The Arts routing: an art constant staged inline in the action-parameter
// stream is what makes the per-art attack camera reachable, and what the
// strike loop resolves the art from.
// ---------------------------------------------------------------------------

/// An art action constant staged in the stream **is** the strike's art - the
/// SM does not need `chosen_art` to be set, and it does not need the art
/// record either when the engine staged the per-strike power alongside.
///
/// This is the seam `docs/subsystems/battle-action.md` § A Tactical Art is an
/// ordinary attack-band action describes: retail's `FUN_801EED1C` writes
/// `art_id + 0x1B` into `actor[+0x1DF..]` (`0x801EF7A0`) and the strike loop
/// stages that byte into `+0x1DA` (`0x801E3764`).
#[test]
fn an_inline_art_constant_stages_itself_as_the_anim_and_keys_the_hit_driver() {
    use legaia_art::{ActionConstant, Character};

    let mut host = RecHost::with_n_actors(4);
    host.actors[0].character = Character::Vahn;
    host.actors[0].active_target = 3;
    // The retail queue shape: two swings, the starter, the art constant,
    // the terminator - `FUN_801EED1C` keeps the leading arrows and inserts
    // the constant after the 0x19 it writes over the last one.
    let art = ActionConstant::Art1C;
    host.actors[0].params[0] = crate::battle_action::SWING_HIGH;
    host.actors[0].params[1] = ActionConstant::RegularStarter.as_byte();
    host.actors[0].params[2] = art.as_byte();
    host.actors[0].params[3] = 0x00;

    let mut ctx = BattleActionCtx::new();
    ctx.action_state = ActionState::AttackChain.as_byte();
    ctx.active_actor = 0;

    let mut staged = Vec::new();
    for _ in 0..3 {
        step(&mut host, &mut ctx);
        staged.push(host.actors[0].queued_anim);
        // The anim commit's latch (`+0x1DB = +0x1DA`, FUN_8004AD80) - the
        // engine's commit does this; the recording host has none.
        host.actors[0].latched_anim = host.actors[0].queued_anim;
        host.actors[0].flag_bits.clear(ActorFlags::ADVANCE_DONE);
    }
    assert_eq!(
        staged,
        vec![
            crate::battle_action::SWING_HIGH,
            ActionConstant::RegularStarter.as_byte(),
            art.as_byte()
        ],
        "every byte stages as itself: swings, starter and constant alike"
    );
    assert!(
        host.take()
            .iter()
            .all(|e| !matches!(e, Event::ApplyArtStrike(_) | Event::ApplyDamage(..))),
        "the band resolves nothing; the clip's hit events do"
    );
    // The latched constant is what the hit-event driver keys the art on ...
    assert_eq!(
        staged_art_constant(host.actors[0].latched_anim, None, true),
        Some(art)
    );
    // ... and it is inside the band the per-art attack camera dispatches
    // on, with a live arm for this character. Every action whose stream
    // carries only direction swings answers `None` here, which is why the
    // channel measured as dead before the Arts path was routed through this
    // band.
    use crate::battle_attack_camera::{CharacterArm, art_arm};
    assert!(art_arm(CharacterArm::One, art.as_byte()).is_some());
}

/// A **direction swing** in the stream never resolves as an art hit, even
/// with `chosen_art` set - it is a committed arms command and resolves
/// through the melee seam with its own command byte. Without this an arts
/// entry's leading arrows would be charged twice.
#[test]
fn a_direction_swing_never_keys_an_art_strike() {
    use legaia_art::{ActionConstant, Character};

    let mut host = RecHost::with_n_actors(4);
    host.actors[0].character = Character::Vahn;
    host.actors[0].active_target = 3;
    host.actors[0].chosen_art = Some(ActionConstant::Art1C);
    host.actors[0].params[0] = crate::battle_action::SWING_LEFT;
    host.actors[0].params[1] = 0x00;

    let mut ctx = BattleActionCtx::new();
    ctx.action_state = ActionState::AttackChain.as_byte();
    ctx.active_actor = 0;
    step(&mut host, &mut ctx);

    assert_eq!(host.actors[0].queued_anim, crate::battle_action::SWING_LEFT);
    assert_eq!(
        staged_art_constant(
            crate::battle_action::SWING_LEFT,
            host.actors[0].chosen_art,
            true
        ),
        None,
        "a swing byte is not an art hit"
    );
}

/// A **monster** slot's stream carries archive entry indices across the whole
/// byte range, so a value that names an art on a party slot must not be read
/// as one there - the slot's `character` key is meaningless for a monster.
#[test]
fn a_monster_slot_never_reads_its_stream_bytes_as_art_constants() {
    use legaia_art::ActionConstant;

    let mut host = RecHost::with_n_actors(5);
    // Slot 3 is a monster (`party_count` is 3 by default).
    host.actors[3].active_target = 0;
    host.actors[3].params[0] = ActionConstant::Art1C.as_byte();
    host.actors[3].params[1] = 0x00;

    let mut ctx = BattleActionCtx::new();
    ctx.action_state = ActionState::AttackChain.as_byte();
    ctx.active_actor = 3;
    step(&mut host, &mut ctx);

    assert_eq!(host.actors[3].queued_anim, ActionConstant::Art1C.as_byte());
    let party = 3 < host.party_count();
    assert!(!party);
    assert_eq!(
        staged_art_constant(host.actors[3].queued_anim, None, party),
        None,
        "a monster's clip index is not an art constant"
    );
}

// ---------------------------------------------------------------------------
// The capture band's per-frame hold - `0x70` (`0x801E504C..0x801E50E4`)
// ---------------------------------------------------------------------------

/// Retail re-enters `FUN_801F2160` every frame of phase `0x70` and holds on a
/// non-zero return (`jal` at `0x801E50C8`, `bne v0,zero` at `0x801E50D0`).
/// So a module that stages `n` frames keeps the phase for exactly `n` ticks
/// and leaves on the `n + 1`-th - and the tick is entered on every one of
/// them, including the frame it finally reports done.
#[test]
fn capture_phase2_holds_for_exactly_the_modules_staging_frames() {
    for n in [0u32, 1, 5, 40] {
        let (mut ctx, mut host) = fresh(ActionCategory::Magic, 1);
        ctx.action_state = ActionState::MagicCapturePhase2.as_byte();
        *host.capture_busy_frames.borrow_mut() = n;

        for frame in 0..n {
            step(&mut host, &mut ctx);
            assert_eq!(
                ctx.action_state,
                ActionState::MagicCapturePhase2.as_byte(),
                "n={n} frame={frame}: still staging"
            );
        }
        step(&mut host, &mut ctx);
        assert_eq!(
            ctx.action_state,
            ActionState::MagicCaptureFinalize.as_byte(),
            "n={n}: the zero return advances to 0x71"
        );
        assert_eq!(
            *host.capture_ticks.borrow(),
            n + 1,
            "n={n}: one tick per frame the phase ran"
        );
    }
}

/// `ctx[+0xD] = 1` in the `jal`'s delay slot (`li v0,0x1` at `0x801E50C4`,
/// `sb v0,0xd(v1)` at `0x801E50CC`): the capture is framed from the mirrored
/// side whatever the action seed rolled, and the write happens on every pass,
/// including the ones that hold.
#[test]
fn capture_phase2_pins_the_camera_variant() {
    let (mut ctx, mut host) = fresh(ActionCategory::Magic, 1);
    ctx.action_state = ActionState::MagicCapturePhase2.as_byte();
    ctx.camera_variant = 3;
    *host.capture_busy_frames.borrow_mut() = 2;
    for _ in 0..3 {
        step(&mut host, &mut ctx);
        assert_eq!(ctx.camera_variant, CAPTURE_CAMERA_VARIANT);
    }
}

/// The 75% duck is **gated** on `ctx[+0x287]` (`lbu v0,0x287(v0)` /
/// `beq v0,zero,0x801E50BC` at `0x801E5058`), the same gate state `0x6F`
/// runs. The port used to duck unconditionally.
#[test]
fn capture_phase2_ducks_only_behind_the_counter_flag() {
    let (mut ctx, mut host) = fresh(ActionCategory::Magic, 1);
    ctx.action_state = ActionState::MagicCapturePhase2.as_byte();
    ctx.counter_attack_a = 0;
    step(&mut host, &mut ctx);
    assert!(
        !host
            .take()
            .iter()
            .any(|e| matches!(e, Event::Brightness(_))),
        "flag clear: no ramp"
    );

    let (mut ctx, mut host) = fresh(ActionCategory::Magic, 1);
    ctx.action_state = ActionState::MagicCapturePhase2.as_byte();
    ctx.counter_attack_a = 1;
    step(&mut host, &mut ctx);
    assert!(
        host.take().contains(&Event::Brightness(75)),
        "flag set: the 75% ramp"
    );
}

// ---------------------------------------------------------------------------
// The capture band's camera - `0x6F` ramp (`0x801E4FFC..0x801E5020`) and the
// `0x70` -> `0x71` re-seed (`0x801E50DC`)
// ---------------------------------------------------------------------------

/// The `0x6F` hold does two things per frame the port used to drop: it ramps
/// `ctx[+0x6D0]` down by `frame_scalar * 16` (`lhu`/`subu`/`sh` at
/// `0x801E4FFC..0x801E5014`) and re-arms the framing program for the acting
/// slot (`FUN_801D5854(ctx[+0x13], 6)` at `0x801E5018`). Both are inside the
/// hold, so they repeat for as long as `FUN_8003F2B8(1)` reports busy.
#[test]
fn capture_fade_ramps_the_camera_and_rearms_the_framing_every_frame() {
    let (mut ctx, mut host) = fresh(ActionCategory::Magic, 1);
    ctx.action_state = ActionState::MagicCaptureFade.as_byte();
    ctx.camera_frame_height = crate::battle_formulas::CAMERA_HEIGHT_MIN;
    host.prev_cleared = false;

    for frame in 1..=4i16 {
        step(&mut host, &mut ctx);
        assert_eq!(
            ctx.action_state,
            ActionState::MagicCaptureFade.as_byte(),
            "the hold holds while the previous action has not cleared"
        );
        assert_eq!(
            ctx.camera_frame_height,
            crate::battle_formulas::CAMERA_HEIGHT_MIN - frame * CAPTURE_FADE_CAMERA_STEP,
            "frame {frame}: 16 units per frame, no floor"
        );
        let events = host.take();
        assert!(
            events.contains(&Event::Pose(1, Pose::Idle)),
            "frame {frame}: the framing program is re-armed"
        );
    }
}

/// The `0x71` store's own `jal 0x801f0348` (`0x801E50DC`, whose delay slot is
/// the state write) puts the framing back where the size class says it
/// belongs. Without it the pull-in above would leak into the rest of the
/// action.
#[test]
fn capture_phase2_reseeds_the_camera_on_the_way_to_0x71() {
    let (mut ctx, mut host) = fresh(ActionCategory::Magic, 1);
    ctx.action_state = ActionState::MagicCapturePhase2.as_byte();
    // Wherever the 0x6F ramp left it - deliberately not the seed value.
    ctx.camera_frame_height = 0x0100;
    *host.capture_busy_frames.borrow_mut() = 2;

    // The holding frames leave it alone: retail's re-seed is in the exit's
    // delay slot, not in the body.
    for _ in 0..2 {
        step(&mut host, &mut ctx);
        assert_eq!(
            ctx.camera_frame_height, 0x0100,
            "held frames do not re-seed"
        );
    }
    step(&mut host, &mut ctx);
    assert_eq!(
        ctx.action_state,
        ActionState::MagicCaptureFinalize.as_byte()
    );
    assert_eq!(
        ctx.camera_frame_height,
        crate::battle_formulas::CAMERA_HEIGHT_MIN,
        "the exit re-runs FUN_801F0348; a party caster frames at the floor"
    );
}

// ---------------------------------------------------------------------------
// The `0x51` teardown's tail sweep - `0x801E6218..0x801E6368`
// ---------------------------------------------------------------------------

/// `ctx[+0x269]` non-zero **raises** `0x59` (`a1 = 0` at `0x801E6244`), and
/// the target-banner close is gated on the acting actor's own two bytes:
/// `+0x1DD` in `3..=7` and `+0x1DE` in `1..=3`.
///
/// The window matters and it is narrow. `ctx[+0x269]` non-zero on the *exit*
/// pass re-seeds the countdown to `0xB4` before the teardown reloads it
/// (`sh v0,0x2(s7)` at `0x801E6138`, reload at `0x801E614C`), which fails the
/// `< 0xC` gate - so the only frames that see both a live capture byte and a
/// running teardown are the last twelve of the countdown, before it goes
/// negative. The tests below sit in that window deliberately.
#[test]
fn the_done_band_sweep_raises_the_capture_banner_and_closes_the_target_one() {
    // Category 2 (Magic), target slot 3: both gates pass, and a non-zero
    // capture byte raises 0x59.
    let (mut ctx, mut host) = fresh(ActionCategory::Magic, 1);
    ctx.action_state = ActionState::DoneFadeDown.as_byte();
    ctx.frame_timer = 5;
    ctx.multi_cast_gate = 0x8B;
    host.actors[1].active_target = 3;
    assert_eq!(step(&mut host, &mut ctx), StepOutcome::Stay);
    let events = host.take();
    assert!(
        events.contains(&Event::Ui(0x59, 0)),
        "the capture banner is RAISED, not unloaded: {events:?}"
    );
    assert!(
        events.contains(&Event::Ui(0x51, 1)),
        "the target banner is closed: {events:?}"
    );

    // A party-wide target (8) fails the slot gate, and a zero capture byte
    // raises nothing - while the rest of the teardown still runs, so this is
    // not vacuously asserting that nothing happened.
    let (mut ctx, mut host) = fresh(ActionCategory::Magic, 1);
    ctx.action_state = ActionState::DoneFadeDown.as_byte();
    ctx.frame_timer = 5;
    ctx.multi_cast_gate = 0;
    host.actors[1].active_target = 8;
    step(&mut host, &mut ctx);
    let events = host.take();
    assert!(
        events.contains(&Event::Ui(0x44, 1)),
        "the teardown block itself ran: {events:?}"
    );
    assert!(!events.contains(&Event::Ui(0x59, 0)), "{events:?}");
    assert!(!events.contains(&Event::Ui(0x51, 1)), "{events:?}");

    // The tail is behind the same latch as the block above it: the next pass
    // of the band re-runs neither.
    let (mut ctx, mut host) = fresh(ActionCategory::Magic, 1);
    ctx.action_state = ActionState::DoneFadeDown.as_byte();
    ctx.frame_timer = 5;
    ctx.multi_cast_gate = 0x8B;
    host.actors[1].active_target = 3;
    step(&mut host, &mut ctx);
    let _ = host.take();
    step(&mut host, &mut ctx);
    let events = host.take();
    assert!(
        !events.contains(&Event::Ui(0x59, 0)),
        "the latch makes the tail once-per-action: {events:?}"
    );
    assert!(!events.contains(&Event::Ui(0x51, 1)), "{events:?}");
}

/// The raise has a close. The `0x52` band's own tail
/// (`0x801E63F4..0x801E6424`) unloads `0x59` behind the same `ctx[+0x17]`
/// latch the `0x51` block set, and clears it - so a capture that raised the
/// banner in the fade-down cannot leave it standing.
#[test]
fn the_multi_cast_band_closes_the_capture_banner_and_clears_the_latch() {
    let (mut ctx, mut host) = fresh(ActionCategory::Magic, 1);
    ctx.action_state = ActionState::DoneMultiCast.as_byte();
    ctx.frame_timer = 4;
    ctx.done_ui_torn_down = 1;
    step(&mut host, &mut ctx);
    let events = host.take();
    assert!(
        events.contains(&Event::Ui(0x59, 1)),
        "the capture banner is closed: {events:?}"
    );
    assert_eq!(ctx.done_ui_torn_down, 0, "and the latch is cleared");

    // Once only: the next pass finds the latch clear.
    step(&mut host, &mut ctx);
    let events = host.take();
    assert!(!events.contains(&Event::Ui(0x59, 1)), "{events:?}");

    // Above the window the tail does not run at all, even with the latch set.
    let (mut ctx, mut host) = fresh(ActionCategory::Magic, 1);
    ctx.action_state = ActionState::DoneMultiCast.as_byte();
    ctx.frame_timer = 0x40;
    ctx.done_ui_torn_down = 1;
    step(&mut host, &mut ctx);
    let events = host.take();
    assert!(!events.contains(&Event::Ui(0x59, 1)), "{events:?}");
    assert_eq!(ctx.done_ui_torn_down, 1);
}

// ---------------------------------------------------------------------------
// The Attack x2 refill's mark compare - `0x801E3A44..0x801E3A64`
// ---------------------------------------------------------------------------

/// `bne v0,a1,0x801E3A58` with `a1 = 1` is an **exact** compare against the
/// build loop's mark, so the Super tail-replace's `4` is skipped: a Super's
/// `0x1A` starter survives the War God Icon's second pass while an ordinary
/// newly-learned art's is demoted to `0x19`.
#[test]
fn attack_x2_refill_demotes_build_starters_and_spares_super_starters() {
    let mut host = RecHost::with_n_actors(ACTOR_SLOTS);
    for a in &mut host.actors {
        a.liveness = 1;
    }
    host.ability_bits.insert(0, WAR_GOD_ATTACK_X2_BIT);
    // `[0x0F, 0x1A(art), 0x27, 0x1A(super), 0x2C, 0x00]`: two `0x1A`
    // starters, one from the build loop and one the Super applier wrote.
    let a = &mut host.actors[0];
    a.params[0] = SWING_HIGH;
    a.params[1] = SPECIAL_STARTER;
    a.params[2] = 0x27;
    a.params[3] = SPECIAL_STARTER;
    a.params[4] = 0x2C;
    a.params[5] = 0;
    let mut marks = [0u32; ACTION_QUEUE_CAP];
    marks[1] = BUILD_STARTER_MARK;
    marks[3] = SUPER_STARTER_MARK;
    a.starter_marks = Some(marks);
    // The refill is reached by staging the stream's last byte and finding the
    // terminator at the bumped cursor, so park one byte short of it.
    a.strike_index = 4;

    let mut ctx = BattleActionCtx::new();
    ctx.active_actor = 0;
    ctx.action_state = ActionState::AttackChain.as_byte();
    step(&mut host, &mut ctx);

    assert_eq!(
        host.actors[0].params[1], REGULAR_STARTER,
        "the build loop's starter is demoted"
    );
    assert_eq!(
        host.actors[0].params[3], SPECIAL_STARTER,
        "the Super applier's starter is not"
    );
    assert_eq!(ctx.attack_x2_pass, 1);
    assert_eq!(host.actors[0].strike_index, 0, "the stream replays from 0");
}

/// A queue no builder produced carries no marks; the refill then reconstructs
/// the build-loop marks from the bytes, which is the pre-carrier behaviour and
/// can only ever produce [`BUILD_STARTER_MARK`].
#[test]
fn attack_x2_refill_falls_back_to_reconstructed_marks() {
    let mut host = RecHost::with_n_actors(ACTOR_SLOTS);
    for a in &mut host.actors {
        a.liveness = 1;
    }
    host.ability_bits.insert(0, WAR_GOD_ATTACK_X2_BIT);
    let a = &mut host.actors[0];
    a.params[0] = SPECIAL_STARTER;
    a.params[1] = 0x27;
    a.params[2] = 0;
    a.starter_marks = None;
    a.strike_index = 1;

    let mut ctx = BattleActionCtx::new();
    ctx.active_actor = 0;
    ctx.action_state = ActionState::AttackChain.as_byte();
    step(&mut host, &mut ctx);
    assert_eq!(host.actors[0].params[0], REGULAR_STARTER);
}

// ---------------------------------------------------------------------------
// The Spirit seed arm - `0x801E2FF0..0x801E3024`
// ---------------------------------------------------------------------------

/// The Spirit arm bumps `ctx[+0x19]` (in the `jal`'s delay slot) and folds
/// its draw to `(rand % 2) * 2`, so a Spirit action is framed at variant `0`
/// or `2` and never from the mirrored side.
#[test]
fn spirit_seed_bumps_the_latch_and_folds_its_draw() {
    for (draw, expect) in [(0u32, 0u8), (1, 2), (4, 0), (7, 2)] {
        let (mut ctx, mut host) = fresh(ActionCategory::Spirit, 1);
        ctx.action_state = ActionState::ActionSeed.as_byte();
        host.rng_seq = vec![3, draw];
        step(&mut host, &mut ctx);
        assert_eq!(ctx.action_state, ActionState::SpiritArtsEntry.as_byte());
        assert_eq!(ctx.camera_variant, expect, "draw {draw}");
        assert_eq!(ctx.spirit_action_count, 1);
    }
}

/// Nothing in the dispatcher clears `ctx[+0x19]`, so it is a per-battle latch
/// and a second Spirit action counts on top of the first.
#[test]
fn spirit_latch_accumulates_across_actions() {
    let (mut ctx, mut host) = fresh(ActionCategory::Spirit, 1);
    for n in 1..=3u8 {
        ctx.action_state = ActionState::ActionSeed.as_byte();
        step(&mut host, &mut ctx);
        assert_eq!(ctx.spirit_action_count, n);
    }
}

// ---------------------------------------------------------------------------
// The Done band's UI teardown - `0x801E614C..0x801E6214`
// ---------------------------------------------------------------------------

/// The teardown is latched by `ctx[+0x17]` and gated on the countdown having
/// fallen below `0xC`, so every element - the level-up banner among them -
/// is unloaded exactly once per action, however many passes the band takes.
#[test]
fn done_band_unloads_the_level_up_banner_exactly_once() {
    let mut host = RecHost::with_n_actors(ACTOR_SLOTS);
    for a in &mut host.actors {
        a.liveness = 1;
    }
    host.actors[0].action_category = ActionCategory::Attack.as_byte();
    let mut ctx = BattleActionCtx::new();
    ctx.active_actor = 0;
    ctx.action_state = ActionState::DoneFadeDown.as_byte();
    ctx.levelup_banner_element = 0x65;
    ctx.action_ui_element = 7;
    ctx.frame_timer = 4;

    // Four passes, all of them inside `0x51` (the countdown reaches `0` on
    // the fourth and only goes negative on the fifth) - so every unload
    // recorded here is this arm's.
    for _ in 0..4 {
        step(&mut host, &mut ctx);
    }
    assert_eq!(
        ctx.action_state,
        ActionState::DoneFadeDown.as_byte(),
        "the band is still in 0x51"
    );
    let events = host.take();
    let unloads: Vec<u8> = events
        .iter()
        .filter_map(|e| match e {
            Event::Ui(id, UI_UNLOAD) => Some(*id),
            _ => None,
        })
        .collect();
    assert_eq!(
        unloads,
        vec![7, 0x65, DONE_ACTION_ELEMENT],
        "the action's element, the banner, then the non-Run element - once"
    );
    assert_eq!(ctx.done_ui_torn_down, 1);
}

/// The banner id round-trips: raised by whatever staged it, consumed by the
/// teardown at the id it was staged with, and cleared by the next action seed
/// (`sb zero,0x15(s5)` at `0x801E2CFC`).
#[test]
fn level_up_banner_element_round_trips_through_the_done_band() {
    for element in [0x65u8, 0x12, 0xFF] {
        let mut host = RecHost::with_n_actors(ACTOR_SLOTS);
        for a in &mut host.actors {
            a.liveness = 1;
        }
        host.actors[0].action_category = ActionCategory::Attack.as_byte();
        let mut ctx = BattleActionCtx::new();
        ctx.active_actor = 0;
        ctx.action_state = ActionState::DoneFadeDown.as_byte();
        ctx.levelup_banner_element = element;
        ctx.frame_timer = 2;
        step(&mut host, &mut ctx);
        assert!(
            host.take().contains(&Event::Ui(element, UI_UNLOAD)),
            "element {element:#04x} unloads at the id it was staged with"
        );
        // ...and the next seed clears the byte.
        ctx.action_state = ActionState::ActionSeed.as_byte();
        step(&mut host, &mut ctx);
        assert_eq!(ctx.levelup_banner_element, 0);
    }
}

/// A Run action skips element `0x44` (`lbu v1,0x1de(s3)` / `beq v1,0x5` at
/// `0x801E61F0..0x801E61F8`), and the Spirit latch adds the `0x0F` / `0x52`
/// pair (`0x801E61CC..0x801E61E8`).
#[test]
fn done_band_teardown_honours_the_category_and_spirit_gates() {
    let mut host = RecHost::with_n_actors(ACTOR_SLOTS);
    for a in &mut host.actors {
        a.liveness = 1;
    }
    host.actors[0].action_category = ActionCategory::Run.as_byte();
    let mut ctx = BattleActionCtx::new();
    ctx.active_actor = 0;
    ctx.action_state = ActionState::DoneFadeDown.as_byte();
    ctx.spirit_action_count = 1;
    ctx.frame_timer = 2;
    step(&mut host, &mut ctx);
    let unloads: Vec<u8> = host
        .take()
        .iter()
        .filter_map(|e| match e {
            Event::Ui(id, UI_UNLOAD) => Some(*id),
            _ => None,
        })
        .collect();
    assert_eq!(
        unloads,
        vec![DONE_SPIRIT_ELEMENT_A, DONE_SPIRIT_ELEMENT_B],
        "Run drops 0x44, the Spirit latch adds its pair"
    );
}
