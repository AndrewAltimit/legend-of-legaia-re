//! Live battle session state: per-seat stat arrays, command / submenu sessions, flow + round state, tutorial, intro transition, escape timer, buffs, hit / effect queues and the end-of-battle latches.
//!
//! Split out of the composite [`World`] so the state one subsystem owns
//! reads as one unit. Fields keep their retail provenance notes.

use super::*;

/// Live battle session state: per-seat stat arrays, command / submenu sessions, flow + round state, tutorial, intro transition, escape timer, buffs, hit / effect queues and the end-of-battle latches.
pub struct BattleState {
    /// Per-slot weapon attack used by [`art_strike::apply_art_strike`] to
    /// compute Tactical-Art damage. Engines populate from the active
    /// character record's weapon power. Default zero - un-populated slots
    /// produce floor-clamped damage (`= 1`).
    pub attack: [u16; 8],
    /// Per-slot **equipment** attack bonuses, one byte per character-record
    /// equipment slot (`+0x196..+0x19A`: body, head, slot 2, slot 3,
    /// footwear), as the equipment table's `+1` attack byte resolves them.
    /// Retail never folds these into the actor's ATK at battle load - the
    /// arms execution resolver `FUN_801EC3E4` adds **half** of the slot the
    /// executing command reads (`arms_weapon_atk_fold`) on every swing, so
    /// [`crate::world::BattleState::attack`] stays the un-equipped base and this array
    /// supplies the per-command half. Party slots only; monster slots stay
    /// zero.
    pub equip_atk: [[u8; legaia_engine_vm::battle_formulas::EQUIP_SLOTS]; 8],
    /// Per-slot magic attack scalar used by [`spells::cast_spell`] for the
    /// caster's `mag` column when resolving a player-driven battle Magic cast.
    /// Engines populate from the active character record; default zero (which
    /// floors damage spells at 1).
    pub magic: [u16; 8],
    /// Per-slot defense facing the strike. The retail engine selects UDF
    /// or LDF based on the strike's `power_target`; this single field is a
    /// minimum-viable substitute that engines wishing to model both can
    /// override via `set_battle_defense_for_target`.
    pub defense: [u16; 8],
    /// Optional UDF / LDF defense override per slot. When set, the
    /// art-strike applier uses the matching half for the strike's target
    /// class instead of [`crate::world::BattleState::defense`]. Engines that don't
    /// distinguish UDF / LDF can leave this `None`.
    pub defense_split: [Option<(u16, u16)>; 8],
    /// Per-slot **base** halfword of each working stat above - retail's
    /// second `sh` of every pair in the record -> actor copy
    /// (`FUN_80054CB0` / `FUN_80053CB8`): ATK `+0x15A`, UDF `+0x15E`, LDF
    /// `+0x162`, SPD `+0x166`, INT `+0x16A`.
    ///
    /// The working half is what the damage kernels read; the base half is
    /// what **nothing during a fight writes except a debuff**, which is what
    /// makes it retail's own "has anything moved this stat since battle
    /// load?" probe - the compare the Seru side-effect stager gates on
    /// ([`legaia_engine_vm::seru_side_effect::StatCompare`],
    /// `FUN_801F3D3C`). Seeded working-equal by
    /// [`crate::world::World::sync_battle_stat_bases`] at battle entry, so a
    /// slot that never took a debuff answers "unchanged".
    ///
    /// AGL's pair is not here: it is the actor's own `agl` / `agl_base`
    /// (`+0x154` / `+0x156`), and MP's base half (`+0x152`) is the monster
    /// record's MP, which no battle write moves.
    ///
    /// REF: FUN_80054CB0, FUN_801F3D3C
    pub attack_base: [u16; 8],
    /// UDF / LDF base halves (`+0x15E` / `+0x162`) - see
    /// [`crate::world::BattleState::attack_base`]. `None` on a slot with no
    /// split configured, exactly like
    /// [`crate::world::BattleState::defense_split`].
    pub defense_base: [Option<(u16, u16)>; 8],
    /// SPD base half (`+0x166`) - see
    /// [`crate::world::BattleState::attack_base`].
    pub speed_base: [u16; 8],
    /// INT base half (`+0x16A`) - see
    /// [`crate::world::BattleState::attack_base`]. Its working half is
    /// [`crate::world::BattleState::accuracy`], which is the same retail
    /// halfword (`+0x168`).
    pub accuracy_base: [u16; 8],
    /// Per-slot SPD (turn-order initiative seed, retail actor `+0x164`).
    /// Party slots are seeded from each character record's live SPD in
    /// [`crate::world::World::load_party`]; monster slots from [`crate::monster_catalog::MonsterDef::speed`]
    /// at battle setup. When **every** living actor has SPD `0` (the
    /// disc-free / synthetic case) the battle stays on the round-robin
    /// turn-order fallback (`World::next_living_combatant`); when any actor
    /// carries real SPD the next-actor selector switches to the SPD-seeded
    /// initiative scheme (`World::next_combatant_by_initiative`, the port of
    /// `recompute_battle_order` / `FUN_801daba4`).
    ///
    /// REF: FUN_801DABA4
    pub speed: [u16; 8],
    /// Battle-camera framing height / distance - retail `ctx+0x6D0`. Recomputed
    /// at every action seed by the port of `FUN_801F0348`
    /// ([`legaia_engine_vm::battle_formulas::camera_height_for_frame`]) from the
    /// acting actor's target slot and its own slot, over the monster records'
    /// `+0x1F` size class ([`crate::monster_catalog::MonsterDef::size_class`]).
    ///
    /// Seeded to the retail floor `0x0C00`
    /// ([`legaia_engine_vm::battle_formulas::CAMERA_HEIGHT_MIN`]), which is also
    /// where every fight frames when no size classes are loaded - so a
    /// disc-free battle keeps the default distance.
    ///
    /// REF: FUN_801F0348
    pub camera_frame_height: i16,
    /// Per-slot accuracy stat (retail actor `+0x168`, the AGL-derived
    /// hit/dodge seed). Used as the **attacker's** term in the selector-9
    /// accuracy roll ([`legaia_engine_vm::battle_formulas::accuracy_roll`]).
    /// Party slots are seeded from each character's resolved `acc` in
    /// [`crate::world::World::seed_party_battle_stats`]; monster slots from
    /// [`crate::monster_catalog::MonsterDef::accuracy`] at battle setup. When
    /// the attacker's accuracy is `0` (the disc-free / synthetic case) the
    /// strike auto-hits and consumes no RNG, so battles that don't seed these
    /// stats keep their always-land behaviour and bit-identical RNG streams.
    pub accuracy: [u16; 8],
    /// Per-slot evasion stat - the **defender's** term in the selector-9
    /// accuracy roll. Same field as [`crate::world::BattleState::accuracy`] in retail
    /// (`+0x168` serves both rolls); kept separate here so equipment that
    /// modifies only accuracy or only evasion is representable. Seeded
    /// alongside accuracy.
    pub evasion: [u16; 8],
    /// Number of physical swings the active monster attacker lands on its
    /// current turn - the enemy multi-action budget. Computed at monster-turn
    /// arm ([`crate::world::World::arm_monster_strike_budget`]) from the monster's AGL gauge
    /// ([`crate::monster_catalog::MonsterDef::agl`]) + its swing costs
    /// (`action_costs`) via the port of `FUN_801E9FD4`'s budget loop
    /// ([`legaia_engine_vm::battle_action::enemy_action_budget`]), then consumed
    /// by [`crate::world::World::apply_basic_attack`]. Always `1` for a party attacker (its
    /// multi-hit is the AP/arts system) and for a monster with no AGL / swing
    /// data (the disc-free / synthetic catalog), so unbudgeted battles stay
    /// bit-identical. Defaults to `1`.
    ///
    /// REF: FUN_801E9FD4
    pub monster_strike_budget: u8,
    /// The AGL-budget picks of the monster whose physical strike is being
    /// armed, as archive **entry indices** (the anim ids the attack band
    /// stages) - filled by [`crate::world::World::arm_monster_strike_budget`] alongside
    /// [`crate::world::BattleState::monster_strike_budget`] and moved into the monster's action
    /// stream by its arming. Empty when the catalog carries no aligned
    /// entry list.
    pub monster_strike_entries: Vec<u8>,
    /// "Previous action cleared" gate - toggled by the engine when an
    /// animation transition completes.
    pub prev_action_cleared: bool,
    /// Last-issued battle-end cause (for inspection / engine side-effects).
    pub end: Option<BattleEndCause>,
    /// The armed end-of-battle presentation (retail's results sequencer
    /// `FUN_8004E568`, run every frame the battle-end signal is up). While
    /// `Some` the scene stays in [`crate::world::SceneMode::Battle`], the action SM does
    /// not step, and [`crate::world::World::tick_battle_end_sequence`] walks the load /
    /// results / exit-fade phases before [`crate::world::World::finish_battle`] runs. See
    /// `world::battle::victory`.
    pub victory: Option<crate::world::VictorySequence>,
    /// Set by the results frame once [`crate::world::World::apply_battle_loot`] has run for
    /// this battle, so the deferred [`crate::world::World::finish_battle`] does not credit
    /// the rewards a second time. Cleared by `finish_battle`.
    pub loot_applied: bool,
    /// Presentation-only per-strike HP deltas surfaced for HUD damage
    /// popups. The gameplay-state HP mutation has *already* happened by
    /// the time an entry lands here (the live battle loop folds art-strike
    /// damage and applies the generic physical strike before queuing the
    /// matching FX), so engines must NOT re-apply these to HP - they only
    /// drive the floating-number / status overlay. Drained by the host via
    /// [`crate::world::World::drain_battle_hit_fx`]; cleared on battle exit.
    pub hit_fx: Vec<BattleHitFx>,
    /// The hit events the attack band resolved this frame - one
    /// [`BattleHitEvent`] per `FUN_801EC3E4` resolution (index, power byte,
    /// damage, running combo total, whether the total landed on HP). The
    /// impact-FX and HIT / TOTAL counter layers consume it; cosmetic, like
    /// [`crate::world::BattleState::hit_fx`]. Drained via
    /// [`crate::world::World::drain_battle_hit_events`]; cleared on battle exit.
    pub hit_events: Vec<BattleHitEvent>,
    /// Battle **effect CLUT stages** queued this frame - one `0x801F6418`
    /// source x per table-form effect spawn whose map byte is non-zero
    /// (`FUN_801DEA50`, `0x801df0dc..0x801df134`). Cosmetic: each is a 16x1
    /// palette-row copy onto VRAM `(224, 476)` a host applies with
    /// [`crate::battle_effect_clut::stage_effect_clut`]. Drained via
    /// [`crate::world::World::drain_battle_clut_stages`]; cleared on battle exit.
    pub clut_stages: Vec<u8>,
    /// Battle effect-script spawn requests queued this frame - one per
    /// effect record the per-actor effect-script walk consumed
    /// ([`crate::action_effect_script::step_effect_script`], driven by
    /// [`crate::world::World::tick_battle_animations`]). Cosmetic: each names an effect id
    /// and a world position already rotated by the acting actor's facing;
    /// the host routes them into its battle FX layer (the direct form into
    /// the 2D effect pool, the table form into the `0x801F6324` scene-graph
    /// spawner). Drained via [`crate::world::World::drain_battle_effect_spawns`]; cleared
    /// on battle exit.
    pub effect_spawns: Vec<crate::battle_events::BattleEffectSpawn>,
    /// The scripted countdown timer the field VM arms with `0x4C 0xD3`
    /// (`SCHEDULE_TIMED_FLAGS`). Retail keeps it in three globals -
    /// `_DAT_800845A0` remaining, `_DAT_800845BC` below-threshold trigger,
    /// `_DAT_800845B8` armed - which is the triple
    /// [`legaia_engine_vm::escape_timer::EscapeTimer`] models.
    /// [`crate::world::World::tick_escape_timer`] drains it once per retail frame.
    pub escape_timer: vm::escape_timer::EscapeTimer,
    /// The packed flag word the same installer writes to `_DAT_800845C0`:
    /// low half = the below-threshold flag, high half = the expiry flag.
    /// Both are masked to 12 bits before they reach the system-flag bank.
    pub escape_timer_flag_word: u32,
    /// This frame's escape-timer HUD readout, recomputed by
    /// [`crate::world::World::tick_escape_timer`] while the timer is armed and cleared
    /// when it is not: `(minutes, seconds, hundredths, ink)`. Retail's
    /// `FUN_801D2EBC` decomposes and colours the readout in the same
    /// function that drains the counter, so the values are a per-frame
    /// product of the tick rather than something a renderer derives.
    pub escape_timer_hud: Option<(i32, i32, i32, vm::escape_timer::TimerInk)>,
    /// The HUD actor that owns the countdown (`FUN_801D2EBC` is its handler):
    /// alive from the arming op until its expired readout's hold runs out.
    pub escape_timer_actor: Option<vm::escape_timer::EscapeTimerHud>,
    /// Per-actor status-effect tracker (Toxic / Numb / Venom /
    /// Sleep / Confuse / Curse / Stone / Faint). Populated by
    /// [`crate::world::World::fold_battle_event`] on `ApplyArtStrike` events whose
    /// `enemy_effect` is non-`None`; ticked per turn by engines that
    /// drive a battle round. See [`legaia_engine_vm::status_effects`].
    pub status_effects: vm::status_effects::StatusEffectTracker,
    /// Per-character AP gauge - drives Tactical-Arts command input.
    /// Index 0..=2 maps to party slots; engines call
    /// [`crate::ap_gauge::ApGauge::reset_for_turn`] at turn start and
    /// [`crate::ap_gauge::ApGauge::charge_spirit`] when the player
    /// presses Spirit during command input.
    pub ap_gauges: [crate::ap_gauge::ApGauge; 3],
    /// Per-party-slot guard stance for the current battle: `true` after the
    /// slot's **Spirit** command until its next turn starts. The retail state
    /// is the actor's pending-action byte `+0x1DE == 4` (Spirit), consumed by
    /// the damage finisher's guard-halve stage
    /// ([`legaia_engine_vm::battle_formulas::DamageFinish::defender_guarding`]).
    pub guarding: [bool; 3],
    /// Per-party-slot Fury Boost state for the current battle: `Some(delta)` is
    /// the AP added to that slot's gauge by the class-5 Fury Boost item (retail
    /// actor `+0x1F9` flag). Reverted wholesale at battle end (`finish_battle`),
    /// the same lifecycle as [`crate::world::BattleState::buffs`]; `None` = not boosted.
    pub fury_boost: [Option<u8>; 3],
    /// Battle-scoped monster-AI state (cooldowns / phase counter / recent-target
    /// ring) read & written by the per-monster-id scripted-cast picker
    /// ([`crate::monster_ai::decide`]). Reset on each battle enter.
    pub monster_ai_state: crate::monster_ai::MonsterAiState,
    /// The stolen band + the once-per-strike-chain steal-attack latch
    /// (`0x801C8FE0`, `ctx[+0x27]`) - see [`crate::battle_steal`]. Zeroed
    /// at battle load beside [`Self::monster_ai_state`].
    pub steal: crate::battle_steal::StealBand,
    /// The death-spoils caption on screen (HUD element `0x5B`), if any.
    pub steal_caption: Option<crate::battle_steal::StealCaption>,
    /// The field-to-battle transition entity, live only while the encounter
    /// session sits in [`crate::encounter::EncounterPhase::Transition`].
    /// `None` outside that window.
    ///
    /// REF: FUN_801CF5BC
    pub intro: Option<vm::battle_intro_transition::TransitionEntity>,
    /// Effects the last [`crate::world::World::tick_encounter`] battle-intro tick asked for,
    /// in retail order. Hosts drain this to drive the loads the kernel cannot
    /// perform itself (mesh assembly, the battle bundle read). The world
    /// consumes two of them before publishing: `LoadBattleBgm` (the BGM swap)
    /// and `SetAudioCue` (the battle-start sound, pushed onto
    /// [`crate::world::AudioState::battle_sfx_cues`]) - see `World::tick_battle_intro` in
    /// `world/encounters.rs`.
    pub intro_effects: Vec<vm::battle_intro_transition::TransitionEffect>,
    /// Latched when the battle-intro spin performed retail's master mode
    /// hand-off (`_DAT_8007B83C = 0x14`) - see
    /// [`crate::world::World::battle_mode_word_held`]. Cleared when the transition ends.
    pub intro_mode_handoff: bool,
    /// Opt-in for a **player-driven** battle inside the live loop. When
    /// `false` (the default) the live loop auto-resolves each party turn with
    /// a physical Attack on the first living monster (the historical spine
    /// behaviour). When `true`, every party turn pauses the action SM and runs
    /// a [`crate::battle_input::BattleCommandSession`] that reads
    /// [`crate::world::World::input`]: the player selects a command from the battle command
    /// menu and a target before the strike commits. Requires
    /// [`crate::world::WorldToggles::live_gameplay_loop`]; hosts that want a playable battle
    /// (`legaia-engine play-window`) set both after boot. All four commands
    /// are wired: Attack strikes; Arts opens the per-press
    /// [`crate::world::BattleState::arts_input`] (the saved-chain
    /// [`crate::world::BattleState::arts_menu`] is the legacy path, behind
    /// `LEGAIA_ARTS_SAVED_LIST=1`); Magic / Item open
    /// [`crate::world::BattleState::spell_menu`] / [`crate::world::BattleState::item_menu`].
    pub player_driven: bool,
    /// Active command-selection session for the player-driven battle. `Some`
    /// only while a party member is choosing a command/target (the action SM
    /// is parked meanwhile); `None` when the SM is running or outside battle.
    /// Managed by the live loop; hosts read it to draw the command menu /
    /// target cursor.
    pub command: Option<crate::battle_input::BattleCommandSession>,
    /// Active inventory submenu for the player-driven battle, opened when the
    /// player picks **Item** from the command menu. `Some` while the player
    /// browses items / picks a target (both the action SM and
    /// [`crate::world::BattleState::command`] are parked meanwhile); `None` otherwise. The
    /// World owns it - not [`crate::battle_input::BattleCommandSession`] -
    /// because it needs the live inventory + party stats. Hosts read it to
    /// draw the item overlay.
    pub item_menu: Option<crate::inventory_use::InventoryUseSession>,
    /// Active spell submenu for the player-driven battle, opened when the
    /// player picks **Magic** from the command menu. `Some` while the player
    /// browses spells / picks a target (both the action SM and
    /// [`crate::world::BattleState::command`] are parked meanwhile); `None` otherwise. The
    /// World owns it because building the spell list needs the caster's
    /// learned spells + live MP. Hosts read it to draw the spell overlay.
    pub spell_menu: Option<crate::battle_magic::BattleSpellSession>,
    /// Active Arts submenu for the player-driven battle, opened when the player
    /// picks **Arts** from the command menu. `Some` while the player browses
    /// saved chains / picks a target (the action SM and [`crate::world::BattleState::command`]
    /// are parked meanwhile); `None` otherwise. The World owns it because each
    /// row's power profile is resolved from [`crate::world::PartyState::saved_chains`] +
    /// [`crate::world::DiscTables::art_records`] by `Self::build_battle_arts_rows`. Hosts read it
    /// to draw the arts overlay.
    pub arts_menu: Option<crate::battle_arts::BattleArtsSession>,
    /// Active retail-model **Arts command input** - the per-press
    /// directional entry the Arts command opens
    /// ([`crate::arts_command_input::ArtsCommandInputSession`], the port of
    /// the `FUN_801D0748` state-`0x50` gauge-input arm). `Some` while a
    /// party member is entering commands / reviewing / picking the Begin
    /// target; the action SM and [`crate::world::BattleState::command`] are parked
    /// meanwhile. The saved-chain list ([`crate::world::BattleState::arts_menu`]) is the
    /// legacy path, kept reachable via `LEGAIA_ARTS_SAVED_LIST=1`.
    pub arts_input: Option<crate::arts_command_input::ArtsCommandInputSession>,
    /// Per-party-slot **swing costs** for the Arts command gauge: the four
    /// `+0x74` AP prices (Left / Right / Down / Up = runtime action slots
    /// `0xC..=0xF`) the input session charges per directional press, for
    /// that slot's *equipped* set. Seeded from the player battle files at
    /// scene entry (`SceneHost::refresh_battle_swing_costs`, via
    /// `legaia_asset::battle_char_assembly::swing_command_costs` - the
    /// same reader the Muscle Dome prices its commands with); stays at the
    /// favored-class base `0x1E` without a disc. REF: FUN_800557B8
    pub swing_costs: [[u16; 4]; 3],
    /// The battle **command-flow** cursor - retail `ctx[+0x06]`, the byte the
    /// menu-half SM `FUN_801D0748` runs on and the key the sparring-tutorial
    /// hook table indexes. Recomposed each frame from the live command session
    /// plus submenus by [`crate::battle_flow::flow_state_for`]; the turn-start
    /// prompt is raised directly by `World::open_battle_command`.
    pub flow: crate::battle_flow::BattleFlowState,
    /// The live loop's round state - which of retail's two round bands the
    /// battle is in (the command band collects every party member's command
    /// before the execution band dispatches anyone by initiative), the
    /// commands committed so far, and the member cursor `ctx[+0x13]`. See
    /// [`crate::battle_round::RoundPhase`].
    pub round_flow: crate::battle_round::RoundFlow,
    /// The sparring fight's side-band phase byte - retail `ctx[+0x289]`,
    /// the SCUS tick `FUN_80056208`'s stage-1 cursor: `0` waiting for the
    /// round start, `1` the opening caption up (the flow SM held back),
    /// `2` the prompt machine live. See
    /// [`crate::world::World::raise_sparring_caption_if_due`]. Reset at battle entry.
    pub sparring_phase: u8,
    /// The sparring-tutorial prompt machine, armed only for the Tetsu
    /// tutorial fight (battle-stage id
    /// [`crate::battle_tutorial::TUTORIAL_STAGE_ID`]) via
    /// [`crate::world::World::arm_battle_tutorial`]. `None` in every other battle - which is
    /// every battle but one, matching retail's stage-overlay dispatch.
    pub tutorial: Option<crate::battle_tutorial::BattleTutorial>,
    /// Prompt text for [`crate::world::BattleState::tutorial`], read off the user's own disc
    /// copy of overlay 967. Empty when the host had no disc to read - the
    /// tutorial then emits no boxes rather than inventing text.
    pub tutorial_script: crate::battle_tutorial::BattleTutorialScript,
    /// Tutorial boxes waiting to be shown, front first. While non-empty the
    /// whole battle loop is parked - the port of retail's `ctx[+0x6B2]`
    /// message-box guard, which makes `FUN_801D0748` return early.
    ///
    /// The queue is retail's single battle message box, so it carries more
    /// than the tutorial: the battle-open formation banner
    /// ([`crate::world::World::raise_battle_open_banner`]) rides it too.
    pub tutorial_boxes: std::collections::VecDeque<crate::battle_flow::ActiveTutorialBox>,
    /// The battle screen's chip / banner labels, read off the user's own disc
    /// ([`legaia_asset::battle_ui_strings`]). Empty when the host had no disc
    /// to read - the port's own wording is used then, so the surfaces still
    /// draw rather than going blank.
    pub ui_strings: legaia_asset::battle_ui_strings::BattleUiStrings,
    /// The party cast trigger's per-spell anim-pair lists
    /// (`legaia_asset::spell_anim_pairs`), read off the user's PROT 0898 by
    /// the host next to [`Self::ui_strings`]. Empty without a disc read, in
    /// which case a spell id below `0x25` stages no clip and folds at once.
    pub spell_anim_pairs: legaia_asset::spell_anim_pairs::SpellAnimPairs,
    /// The next [`crate::world::World::enter_battle`] is the sparring fight and should arm
    /// [`crate::world::BattleState::tutorial`]. Set by
    /// [`crate::world::World::prime_battle_tutorial`]; the engine's stand-in for retail's
    /// per-formation battle-stage id.
    pub tutorial_pending: bool,
    /// Active stat buffs / debuffs applied by battle Magic, one entry per
    /// `(slot, stat)`. Each holds the exact delta written into the per-slot
    /// scalar so expiry can undo it, plus the remaining turn count (decremented
    /// at the start of the buffed actor's turn). Cleared - and their deltas
    /// reverted - by `World::finish_battle`.
    pub buffs: Vec<BattleBuff>,
    /// Set when an escape spell (`SpellEffect::Escape`) resolves. The live
    /// battle tick returns to the field on the next pass (no loot, no
    /// game-over). Cleared by `World::finish_battle`.
    pub escaped: bool,
    /// The **scripted-fight flag** `ctx[+0x287]` for the battle in progress.
    ///
    /// Retail derives it once, at battle init: `FUN_800513F0` reads the
    /// per-battle flags byte `DAT_8007BD60` and stores `(flags >> 5) & 4`
    /// into `ctx[+0x287]` (`0x800513F0..0x80051444`), so the flag is exactly
    /// "bit `0x80` of the per-battle flags is set" - which the entity SM
    /// raises for a formation row whose `record[+0]` header byte is non-zero
    /// ([`crate::monster_catalog::FormationDef::per_battle_flags`],
    /// `FUN_801DA51C`). Boss / story rows carry that byte; random-encounter
    /// rows do not.
    ///
    /// Three kernels read it, and each reads a different thing off it: the
    /// battle loader picks its stat boost profile
    /// ([`crate::monster_catalog::MonsterDef::installed_stats`]), the escape
    /// roll refuses to flee ([`crate::world::BattleState::no_escape`], which
    /// is the port's older and narrower latch on the same byte - set by the
    /// field VM's scripted-battle op rather than derived from the formation),
    /// and the Seru side-effect stager runs its suppression roll and its
    /// base-vs-record compares.
    ///
    /// Seeded at [`crate::world::World::enter_battle_from_formation`];
    /// cleared by [`crate::world::World::finish_battle`].
    ///
    /// REF: FUN_800513F0, FUN_801DA51C, FUN_80054CB0, FUN_801F3D3C
    pub scripted_fight: bool,
    /// Scripted "can't run from this battle" flag (retail battle ctx
    /// `+0x287`, the input `FUN_801E791C`'s escape roll tests). Set when a
    /// battle enters through the field-VM scripted-battle op
    /// ([`crate::world::World::trigger_scripted_battle`]) - the boss rows carry a non-zero
    /// first header byte the retail reader ORs `0x80` into a battle-setup
    /// flag for - so a staged boss fight refuses the Run command (fleeing
    /// would leave the stager's marker set and let the post-victory record
    /// spawn unearned). Cleared by [`crate::world::World::finish_battle`].
    pub no_escape: bool,
    /// One-per-pass latch for the monster flee roll (`FUN_801EC0DC`). Retail's
    /// action picker `FUN_801E9FD4` keeps a balance counter (`s8`, cleared at
    /// entry) and attempts the flee roll exactly once per picker pass, at the
    /// first monster iteration that reaches the loop-bottom checkpoint with the
    /// counter still zero. The engine picks per-slot, so the latch lives here
    /// and [`crate::battle_round::BattleRound::boundary`] re-arms it each round
    /// (retail re-enters the picker per round from `FUN_801DABA4`).
    pub monster_flee_attempted: bool,
    // The formation advantage (`ctx+0x290`) and its latched copy (`ctx+0x291`)
    // are **not** fields here. Retail has exactly one of each, both inside the
    // battle context the action SM owns, so the engine keeps them on
    // [`crate::world::World::battle_ctx`] and reaches them through
    // [`crate::world::World::battle_formation`] / [`crate::world::World::battle_formation_latched`]. They
    // used to be a second copy on `World` that nothing ever pushed into the
    // context, which left the SM's advantage-seeded turn-cursor arms
    // unreachable.
    /// Formation currently being fought, captured at the `Field -> Battle`
    /// transition. Drives [`crate::world::World::apply_battle_loot`] on victory. `None`
    /// outside battle.
    pub active_formation: Option<crate::monster_catalog::FormationDef>,
    /// Aggregated rewards from the most recent victory - surfaced for the
    /// post-battle banner / HUD. `None` until the first battle resolves.
    pub last_rewards: Option<BattleRewards>,
    /// Frames left on the post-battle spoils panel. [`crate::world::World::finish_battle`]
    /// arms it on a monster wipe ([`crate::world::World::SPOILS_BANNER_FRAMES`]) and
    /// [`crate::world::World::tick`] counts it down; a host draws
    /// [`crate::world::World::battle_spoils_banner`] while it is non-zero. Without this the
    /// XP / gold / drops in [`crate::world::BattleState::last_rewards`] were applied with no
    /// on-screen acknowledgement at all.
    pub spoils_frames: u16,
    /// Scene mode to return to when the current battle finishes. Captured at
    /// the transition into [`crate::world::SceneMode::Battle`]; `Self::finish_battle`
    /// restores it (an overworld encounter returns to [`crate::world::SceneMode::WorldMap`],
    /// a field encounter to [`crate::world::SceneMode::Field`]). Defaults to
    /// [`crate::world::SceneMode::Field`].
    pub return_mode: SceneMode,
}

impl BattleState {
    pub fn new() -> Self {
        Self {
            attack: [0; 8],
            equip_atk: [[0; legaia_engine_vm::battle_formulas::EQUIP_SLOTS]; 8],
            magic: [0; 8],
            defense: [0; 8],
            defense_split: [None; 8],
            attack_base: [0; 8],
            defense_base: [None; 8],
            speed_base: [0; 8],
            accuracy_base: [0; 8],
            speed: [0; 8],
            camera_frame_height: legaia_engine_vm::battle_formulas::CAMERA_HEIGHT_MIN,
            accuracy: [0; 8],
            evasion: [0; 8],
            monster_strike_budget: 1,
            monster_strike_entries: Vec::new(),
            prev_action_cleared: true,
            end: None,
            hit_fx: Vec::new(),
            hit_events: Vec::new(),
            clut_stages: Vec::new(),
            effect_spawns: Vec::new(),
            escape_timer: Default::default(),
            escape_timer_flag_word: 0,
            escape_timer_hud: None,
            escape_timer_actor: None,
            status_effects: vm::status_effects::StatusEffectTracker::new(),
            ap_gauges: [crate::ap_gauge::ApGauge::default(); 3],
            guarding: [false; 3],
            fury_boost: [None; 3],
            buffs: Vec::new(),
            escaped: false,
            scripted_fight: false,
            no_escape: false,
            monster_flee_attempted: false,
            intro: None,
            intro_effects: Vec::new(),
            intro_mode_handoff: false,
            monster_ai_state: crate::monster_ai::MonsterAiState::new(),
            steal: crate::battle_steal::StealBand::default(),
            steal_caption: None,
            player_driven: false,
            command: None,
            item_menu: None,
            spell_menu: None,
            arts_menu: None,
            arts_input: None,
            swing_costs: [[crate::arts_command_input::FAVORED_COST; 4]; 3],
            flow: crate::battle_flow::BattleFlowState::Idle,
            round_flow: crate::battle_round::RoundFlow::default(),
            sparring_phase: 0,
            tutorial: None,
            tutorial_script: crate::battle_tutorial::BattleTutorialScript::default(),
            tutorial_boxes: std::collections::VecDeque::new(),
            ui_strings: legaia_asset::battle_ui_strings::BattleUiStrings::default(),
            spell_anim_pairs: legaia_asset::spell_anim_pairs::SpellAnimPairs::default(),
            tutorial_pending: false,
            active_formation: None,
            last_rewards: None,
            spoils_frames: 0,
            victory: None,
            loot_applied: false,
            return_mode: SceneMode::Field,
        }
    }
}

impl Default for BattleState {
    fn default() -> Self {
        Self::new()
    }
}
