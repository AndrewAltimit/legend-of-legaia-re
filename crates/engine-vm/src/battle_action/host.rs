//! `BattleActionHost` trait: the engine-side callbacks the battle-action state machine dispatches into.

use super::*;

/// Engine-side callbacks the battle action state machine dispatches into.
///
/// All methods have default impls so a minimal host (no rendering / no
/// effects) compiles. Each method documents which retail function it stands
/// in for. The host owns the full actor table - the state machine asks for
/// pointers via [`BattleActionHost::actor`] / [`BattleActionHost::actor_mut`]
/// and treats the returned `&mut BattleActor` as `(&DAT_801C9370)[idx]`.
pub trait BattleActionHost {
    /// Equivalent of `(&DAT_801C9370)[slot]` - read-only access to the actor
    /// pointed at by the table slot. Returning `None` aborts the step (the
    /// retail dispatcher silently exits when the active actor pointer is
    /// null).
    fn actor(&self, slot: u8) -> Option<&BattleActor>;

    /// Equivalent of `(&DAT_801C9370)[slot]` - mutable access. Same null
    /// semantics as [`BattleActionHost::actor`].
    fn actor_mut(&mut self, slot: u8) -> Option<&mut BattleActor>;

    /// Equivalent of `FUN_801D5854(actor_id, pose_id)` - per-actor pose
    /// driver. Default no-op.
    fn pose(&mut self, _actor_id: u8, _pose: Pose) {}

    /// Equivalent of `FUN_801D8DE8(effect_id, mode)` - battle UI element
    /// scheduler. `mode == 0` spawns / resets; `mode == 1` terminates /
    /// unloads. Default no-op.
    fn ui_element(&mut self, _effect_id: u8, _mode: u8) {}

    /// Equivalent of `FUN_8004E2F0(actor, target)` - battle range / LOS
    /// check. Returns 0 = "in range," non-zero = distance metric. Default
    /// returns 0 (always in range - useful for unit tests).
    fn range_check(&self, _actor_slot: u8, _target_slot: u8) -> u16 {
        0
    }

    /// The slot's **live** battle-world position - the `(x, z)` pair retail
    /// reads as `actor[+0x34]` / `actor[+0x38]`.
    ///
    /// [`BattleActor`] deliberately does not carry the pair: the position is
    /// world state the host owns (the battle setup seeds it from the
    /// formation seats and the anim tick's root-motion drive moves it), not
    /// action state. But the action SM does read it, at the cast-begin
    /// facing store (`overlay_0898_801e295c.txt` `0x801E4334..0x801E43A4`)
    /// and the attack band's per-frame facing recompute, which need both the
    /// acting actor's position and either the target's or the centroid
    /// [`crate::battle_target_group::target_group_aim`] folds out of the
    /// whole table. This accessor is that read.
    ///
    /// `None` means the slot carries no actor at all - an empty seat, which the
    /// group walk skips and the single-target arm bails on. The default is
    /// `None` for every slot, so a host that tracks no positions keeps the
    /// pre-accessor behaviour (the facing is left alone) rather than having a
    /// bearing computed from zeros.
    fn actor_position(&self, _slot: u8) -> Option<(i16, i16)> {
        None
    }

    /// Write the slot's live position pair (`+0x34`/`+0x38`) - the mutation
    /// half of [`BattleActionHost::actor_position`]. The SM needs it for
    /// exactly one arm: state `0x16`'s arrival shove
    /// ([`motion::arrival_shove_step`]), which displaces the *target* when
    /// the walker arrives. Default no-op (a positionless host stays
    /// positionless).
    fn set_actor_position(&mut self, _slot: u8, _x: i16, _z: i16) {}

    /// The slot's **seat** (anchor) pair - retail `actor[+0x3C]`/`+0x40`,
    /// the pair the battle setup authored and the range law measures the
    /// target by. Defaults to the live position (an unmoved actor's two
    /// pairs are equal by construction - the setup copies seat into live).
    fn actor_anchor(&self, slot: u8) -> Option<(i16, i16)> {
        self.actor_position(slot)
    }

    /// Write the slot's seat pair (`+0x3C`/`+0x40`). The arrival shove moves
    /// the target's seat together with its live position (retail
    /// `0x801E3444..0x801E3490` stores all four halfwords). Default no-op.
    fn set_actor_anchor(&mut self, _slot: u8, _x: i16, _z: i16) {}

    /// Equivalent of `FUN_801EFE44` - battle camera bounds. Walks the 8-slot
    /// table for min/max. Default no-op.
    fn camera_bounds(&mut self) {}

    /// Body-size / bulk class of the monster seated in `actor_slot` - the
    /// monster record's `+0x1F` byte that retail reads through the per-enemy
    /// record-pointer table `0x801C9348[slot - 3]`. Returns `0` for a slot with
    /// no monster (a party slot, or an empty one), which clamps the framing to
    /// the retail default [`camera_height_for_frame`] floor.
    ///
    /// REF: FUN_801F0348
    fn monster_size_class(&self, _actor_slot: u8) -> u8 {
        0
    }

    /// Receives the battle camera's framing height / distance (`ctx+0x6D0`)
    /// computed by [`camera_height_for_frame`] at action seed. Default no-op -
    /// a host that does not draw a camera has nothing to do with it.
    ///
    /// REF: FUN_801F0348
    fn camera_frame_height(&mut self, _height: i16) {}

    /// Equivalent of `FUN_801EED1C` - party setup hook (called for actors
    /// with slot < 3). Default no-op.
    fn party_setup(&mut self, _actor_slot: u8) {}

    /// Equivalent of `FUN_801E7320` - monster-AI setup hook. Default no-op.
    fn monster_setup(&mut self, _actor_slot: u8) {}

    /// Equivalent of `FUN_801DABA4` - recompute battle ordering. Default
    /// no-op.
    fn recompute_battle_order(&mut self) {}

    /// First monster id of the battle formation (`DAT_8007BD0C[0]`). The
    /// monster-wipe victory arm special-cases ids `0xB3` / `0xB4` (retail
    /// `0x801E6728..0x801E676C`): those formations force the victory-pose
    /// actor to party slot `2` / `1` respectively. Default `0` = no
    /// override.
    fn first_monster_id(&self) -> u8 {
        0
    }

    /// Victory staging at the monster-wipe branch of the end-of-action gate
    /// (retail `0x801E6770..0x801E6790`): retail reads the acting slot's
    /// roster character id `DAT_8007BD10[slot]` and arms the win-pose "ME"
    /// archive side-band request `FUN_80055B4C(char_id * 3 - 1)` (see
    /// `docs/formats/summon-readef.md` § streaming state machine). The
    /// engine hands the host the **party slot** instead of the char id;
    /// `end_of_action` guarantees it is a living party slot - retail does
    /// not (see `docs/subsystems/battle.md` § enemy-ally charm at the
    /// end-of-action gate). Default no-op.
    fn victory_stage(&mut self, _party_slot: u8) {}

    /// Capture-pose animation pick for the captured monster: retail
    /// `FUN_80050E2C(record + 0x4C, 1, record[0x4A])` selects an anim id
    /// from the monster archive record's action table
    /// (`(&DAT_801C9348)[slot - 3]`). `None` keeps the actor's queued anim
    /// unchanged (hosts that don't resolve monster records). Called by the
    /// `FUN_801E7824` port during `CaptureStart`.
    fn capture_anim(&mut self, _monster_slot: u8) -> Option<u8> {
        None
    }

    /// Equivalent of `func_0x80056798()` (PSX rand BIOS, `A0 0x2E`). Default
    /// returns 0 for deterministic tests.
    fn rng(&mut self) -> u32 {
        0
    }

    /// Equivalent of `func_0x8003F2B8(1)` - "pause until previous animation
    /// cleared" gate. Returns `true` when the previous action has fully
    /// drained. Default returns `true` (always cleared - useful for tests
    /// that fast-forward through transitions).
    fn previous_action_cleared(&self, _arg: u8) -> bool {
        true
    }

    /// Equivalent of `func_0x8003DE7C(1)` - sound-bank-ready gate. Default
    /// returns `true`.
    fn sound_bank_ready(&self, _arg: u8) -> bool {
        true
    }

    /// Equivalent of `func_0x8003EAE4(0, idx)` - load capture archive.
    /// Default no-op.
    fn load_capture_archive(&mut self, _idx: u8) {}

    /// Equivalent of `FUN_801DBF9C(party_slot, spell_id)` - spell-anim
    /// trigger. Default no-op.
    fn spell_anim_trigger(&mut self, _party_slot: u8, _spell_id: u8) {}

    /// Equivalent of `FUN_801DC0A0(actor_id, anim_id)` - sustained spell
    /// animation. Default no-op.
    fn spell_anim_sustain(&mut self, _actor_id: u8, _anim_id: u8) {}

    /// Equivalent of `func_0x800402F4(class, tier, target_slot, party_index)` -
    /// the item / restore **applier**, retail's damage-and-effect primitive.
    ///
    /// The SM has exactly one call site for it, state `0x3F`
    /// (`ActionState::SpiritFireDamage`, retail `jal 0x800402f4` at
    /// `0x801E4134`), and the four arguments there are
    /// `(actor[+0x1E8], actor[+0x1E9], actor[+0x1DD], DAT_8007BD10[slot] - 1)`,
    /// i.e. [`BattleActor::cast_class`], [`BattleActor::cast_sub_class`], the
    /// target slot and the acting slot's **roster character index**
    /// ([`BattleActionHost::roster_character_id`] less one), not the battle
    /// slot. The parameter names are kept from the pre-`+0x1E8` port for
    /// source compatibility.
    ///
    /// Default no-op.
    fn apply_damage(&mut self, _icon: u8, _page: u8, _target_slot: u8, _party_slot: u8) {}

    /// `DAT_8007BD10[slot]` - the **roster character id** seated in a battle
    /// slot, 1-based on the party side (`id - 1` indexes the character record
    /// at `0x80084708 + (id-1)*0x414`) and `4` for the enemy side.
    ///
    /// Three consumers read the same byte and none of them can be given the
    /// battle slot instead: the applier's fourth argument
    /// ([`BattleActionHost::apply_damage`]), the cast-audio dispatcher's
    /// per-character cue band ([`crate::battle_cast_cue::cast_audio_cue`],
    /// whose `char_kind * 0x10` term is what separates one character's cues
    /// from the next) and the auto-fill arm's per-character queue floor
    /// ([`crate::battle_arts_auto_combo::auto_fill_floor`]).
    ///
    /// Default is `slot + 1`, the identity seating a host with no roster
    /// indirection has (battle slot `0` = character `1`).
    fn roster_character_id(&self, slot: u8) -> u8 {
        slot.saturating_add(1)
    }

    /// The `(class, tier)` pair of the **item-effect descriptor** item
    /// `item_id` resolves to - `+0` / `+1` of `0x800752C0 + subtype*4`, where
    /// `subtype` is the item property record's `+1` byte. This is
    /// `legaia_asset::item_effect::ItemEffectTable::effect(id)` mapped to
    /// `(ItemEffect::class, ItemEffect::tier)`.
    ///
    /// Seeds [`BattleActor::cast_class`] / [`BattleActor::cast_sub_class`] on
    /// the Item leg of state `0x3C`. `None` = no disc item-effect table, and
    /// the pair is left at zero (the applier's class-`0` HP-restore arm,
    /// which is what an all-zero descriptor means in retail too).
    fn item_effect_class_pair(&self, _item_id: u8) -> Option<(u8, u8)> {
        None
    }

    /// The spell record's `+1` **sub-index** byte
    /// (`DAT_800754C8 + spell_id*0xC + 1`), the magic-leg source of
    /// [`BattleActor::cast_sub_class`].
    ///
    /// Sibling of [`BattleActionHost::spell_class_byte`], which is the same
    /// record's `+0`. `None` = the host has no spell table (or its parse
    /// carries no `+1`), leaving the tier at zero - see the module note on
    /// [`crate::battle_cue_group::cue_group_for`] for which cue groups that
    /// actually moves and which are literals regardless.
    fn spell_sub_class_byte(&self, _spell_id: u8) -> Option<u8> {
        None
    }

    /// The two cue-group tables the expander indexes:
    /// `(groups @ 0x801F6470, sfx_map @ 0x801F6418)`.
    ///
    /// Both are disc-parsed by
    /// `legaia_asset::move_power::EffectAuxTables` off PROT 0898. `None` =
    /// no battle-action overlay installed (disc-free battles), which makes
    /// the whole cue-group expansion a no-op rather than a synthetic one.
    fn cue_tables(&self) -> Option<(&[u8], &[u8])> {
        None
    }

    /// One expanded cue reaching the host's effect pool + SFX scheduler.
    ///
    /// The two [`crate::battle_cue_group::CueSpawn`] arms are retail's two
    /// spawn entry points: `Actor` is `FUN_801DFDF0(id, &pos, yaw)` (the 2D
    /// effect pool) and `Effect` is `FUN_80050ED4(&pos, &rot, proto, 0x1000)`
    /// over the `0x801F6324` prototype table, plus the `FUN_80058490` sound
    /// packet the SFX map's non-zero byte submits. `actor_slot` is the slot
    /// whose live position / facing retail builds the spawn transform from
    /// (`+0x34`/`+0x38` and `+0x46`).
    ///
    /// Default no-op - a host with no effect pool drops the plan, which is
    /// where the port was before the expander had a consumer.
    fn spawn_cue(&mut self, _actor_slot: u8, _spawn: crate::battle_cue_group::CueSpawn) {}

    /// One-shot battle SFX - retail `FUN_8004FCC8(cue_id)`, the dispatcher
    /// that classifies a cue id into the pending ring or a voice trigger
    /// (`legaia_engine_audio::classify_cue` is the routing port).
    ///
    /// The SM's one caller is the cast-audio dispatcher at state `0x3D`
    /// ([`crate::battle_cast_cue::cast_audio_cue`], retail `jal 0x801f3990`
    /// at `0x801E3E04`). Ids in the per-character band are `>= 0xF8` and the
    /// enemy band is `0x20C..=0x20E`, so this is a `u16` channel, not the
    /// `u8` one the move-FX cue rides.
    ///
    /// Default no-op.
    fn one_shot_sfx(&mut self, _cue_id: u16) {}

    /// The cast-audio dispatcher's `actor[+0x1DF] == 0xFE` special: give item
    /// `0xFE` x1 (`FUN_800421D4(0xFE, 1)`) and play the per-character voice
    /// cue `FUN_8003D53C(char_kind + 0x19, 0, 0x5A)`.
    ///
    /// `voice_arg` is that first argument. Default no-op.
    fn cast_item_give(&mut self, _voice_arg: u8) {}

    /// The acting character's learned-spell record: `(ids, levels)` - the
    /// `+0x13D` and `+0x161` parallel 36-byte arrays
    /// (`legaia_save::SpellList`), for the party slot's roster character.
    ///
    /// Read by the queued-magic follow-up guard
    /// ([`crate::move_no_effect_guard::queued_magic_message`]) at state
    /// `0x36`. `None` = no character records (monster slots, or a host with
    /// no roster), and the guard stays silent - retail reaches the record
    /// through `DAT_8007BD10[slot] - 1` and so has nothing to read either
    /// for an enemy caster.
    fn caster_spell_list(&self, _party_slot: u8) -> Option<(Vec<u8>, Vec<u8>)> {
        None
    }

    /// The **upper** word of the character record's 64-bit accessory-passive
    /// bitfield - record `+0xF8`, the word after
    /// [`BattleActionHost::character_ability_bits`]'s `+0xF4`.
    ///
    /// It is a different word, not a different view of the same one: the
    /// auto-fill gate reads `record[+0xF8] & 0x2000`
    /// (`overlay_battle_action_801f0450.txt` `0x801F04D4`) while the MP-cost
    /// discount reads `record[+0xF4]` (`0x801E3D04`). Passive indices `32..63`
    /// live here (`docs/formats/accessory-passive-table.md`).
    ///
    /// Default `0` - no auto-fill passive, which is the normal party state.
    fn character_ability_bits_high(&self, _party_slot: u8) -> u32 {
        0
    }

    /// The character's displayed-skill list - record `+0x185` count and
    /// `+0x186..` ids (`legaia_save::DisplayedSkillList`), truncated to the
    /// count.
    ///
    /// This is the list the AI auto-fill arm draws from
    /// ([`crate::battle_arts_auto_combo::auto_fill_queue`], retail reads
    /// `record[+0x74D]` / `+0x74E` off the `0x80084140` base). Default empty,
    /// which makes the auto-fill arm bail exactly where retail's count gate
    /// does.
    fn learned_arts(&self, _party_slot: u8) -> Vec<u8> {
        Vec::new()
    }

    /// Apply one Tactical-Art strike with the power-byte / hit-timing values
    /// pulled from the active art record.
    ///
    /// Called by [`ActionState::AttackChain`] in place of [`apply_damage`]
    /// when the active actor's `chosen_art` is set and `art_record` returns
    /// a record. `info` carries the per-strike values the SM read from the
    /// art's `power` + `dmg_timing` + `enemy_effect` + `hit_cues`. Engines
    /// translate these into HP deduction + status effect + sound/visual
    /// cues - the SM only resolves the values, it does not apply them.
    ///
    /// Default no-op. Engines that don't override fall through to
    /// [`apply_damage`] as well (the SM still calls that for backward
    /// compatibility), so a host that hasn't wired arts yet keeps working.
    fn apply_art_strike(&mut self, _info: ArtStrikeInfo) {}

    /// Attack bonuses of the five items in this actor's equipment slots
    /// (character record `+0x196..+0x19B`), in slot order.
    ///
    /// Each entry is the equipment stat table's ATK byte
    /// (`DAT_80074F68 + row*8`, `+1`) for the id in that slot, resolved
    /// through the item property record's `+1` byte
    /// (`DAT_80074368 + id*0xC`) - the two-hop lookup
    /// `legaia_asset::equip_stats` implements. Retail applies no empty-slot
    /// or item-class guard on this path, so an empty slot reports whatever
    /// id `0` resolves to (normally `0`).
    ///
    /// Feeds [`crate::battle_formulas::arms_weapon_atk_fold`], the
    /// execution-time weapon fold in `FUN_801EC3E4`. Default returns zeros,
    /// which makes the fold a no-op for hosts that have not wired equipment.
    fn equip_attack_bonuses(&self, _party_slot: u8) -> [u8; 5] {
        [0; 5]
    }

    /// The **class byte** of the spell's static table record - `+0` of
    /// `DAT_800754C8 + spell_id*0xC` (see
    /// [`legaia_asset::spell_names`](legaia_asset::spell_names)).
    ///
    /// It is the one field the action SM keys two separate decisions on: the
    /// action-seed band pick ([`action_seed`](super::dispatch)'s Magic arm,
    /// retail `0x801E2EE4`) and the capture route
    /// ([`BattleActionHost::is_capture_spell`], retail's `'c'` test). Hosts
    /// therefore supply the byte once and both fall out of it.
    ///
    /// `None` = "this host has no spell table" - the SM then takes the branch
    /// retail takes for a record it cannot classify, which for both consumers
    /// is the non-override one (`MagicCastBegin`, not capture). Default is
    /// `None`.
    fn spell_class_byte(&self, _spell_id: u8) -> Option<u8> {
        None
    }

    /// Returns `true` if the spell at `spell_id` is a capture-class spell
    /// (first byte of its table entry is `'c'` = `0x63`). Drives the
    /// `MagicCastBegin → MagicCaptureBranch` route.
    ///
    /// The default derives it from [`BattleActionHost::spell_class_byte`], so
    /// a host that supplies the table gets the capture route for free and
    /// cannot disagree with the band pick about what the same record says.
    fn is_capture_spell(&self, spell_id: u8) -> bool {
        self.spell_class_byte(spell_id) == Some(legaia_asset::spell_names::CAPTURE_CLASS)
    }

    /// Lookup the MP cost for a spell. Retail reads the record's `+3` byte
    /// (`DAT_800754C8 + spell_id*0xC + 3`; the SM reaches it through the
    /// `+8`-shifted name-pointer base `DAT_800754D0`, same record).
    ///
    /// This must be **the same number the host's own cast path charges** -
    /// the SM debits MP at [`ActionState::MagicCastBegin`] /
    /// [`ActionState::SpiritPreArm`] and a host that prices a spell
    /// differently anywhere else has two spell models. Default returns 0.
    fn spell_mp_cost(&self, _spell_id: u8) -> u8 {
        0
    }

    /// The `+0x87` flag byte of the art record a **monster** slot's staged
    /// action id resolves to: `record_table[slot - 3][id]` then `+0x4C`, the
    /// monster arm of the gauge re-arm's gate (`FUN_801E93C8`
    /// `0x801E9458..0x801E94A4`). A non-zero flag closes the gate.
    ///
    /// Default returns `0` (gate open) - hosts with no monster art-record
    /// table get retail's behaviour for a record whose flag is clear.
    fn staged_art_record_flag(&self, _monster_slot: u8, _action_id: u8) -> u8 {
        0
    }

    /// Returns the character ability bitmask at `0x80084708 + (party_id-1) *
    /// 0x414 + 0xF4`. Bit `0x20` reduces MP cost by half, `0x10` by a quarter
    /// (`0x20` wins when both are set); `0x100` / `0x200` scale impact
    /// magnitude; etc. Default returns 0.
    fn character_ability_bits(&self, _party_slot: u8) -> u32 {
        0
    }

    /// Writes the camera translation-Y global `_DAT_800840BC`. Default no-op.
    ///
    /// The name is a **misnomer** kept for source compatibility: the SM arm it
    /// mirrors is not a shake. `overlay_battle_action_801e295c.txt`
    /// `0x801E4938..0x801E497C` (the magic-exit arm) tests the camera pitch
    /// `DAT_8007B790` against `0x191` and, when it is at or above, zeroes the
    /// pitch and writes the **absolute** value `0x500` into `_DAT_800840BC` -
    /// a framing snap to the close-up pose, one component of the camera
    /// translation trio. It is not an amplitude, and it is unrelated to the
    /// LCG jitter kernel `FUN_801D9D30`
    /// ([`crate::battle_camera::apply_shake`]), whose amplitude comes from
    /// `_DAT_8007B630` and whose only writer is a field-VM opcode.
    fn screen_shake(&mut self, _magnitude: u16) {}

    /// The ramp at states `SummonSustain` / `MagicCaptureFade` - clamps
    /// `_DAT_8007B910` toward a percentage of the configured level
    /// `_DAT_8008457C`. That cell is the **live audio level**, not screen
    /// brightness: its readers are `SsSeqSetVol` (`FUN_80062004`),
    /// `SpuSetCommonAttr` and the per-slot vol pair, and none of the 26
    /// dumped read sites reaches a draw primitive. So a summon ducks the
    /// music to 75% (50% for spell ids `>= 0x99`) and `Done` restores it.
    /// Default no-op.
    fn duck_audio_level(&mut self, _target_pct: u8) {}

    /// Notify the host the battle is ending. The state machine sets the
    /// retail `DAT_8007BD71 = 0xFE`; engines wire this to "unload battle
    /// overlay." Default no-op.
    fn battle_end(&mut self, _cause: BattleEndCause) {}

    /// Frame delta-time tick used by `frame_timer` decrement. Retail reads
    /// `DAT_1F800393` (the per-frame dt byte). Default returns 1 - one tick
    /// per step.
    fn frame_dt(&self) -> i16 {
        1
    }

    /// Iteration helper - number of party slots in the table (slots `0..3`
    /// are party). Default is 3. Engines override if the layout differs.
    fn party_count(&self) -> u8 {
        3
    }

    /// Is party slot `slot` **seated** - does it hold a combatant the battle
    /// load actually placed there?
    ///
    /// Retail has no such query because the state it distinguishes cannot
    /// exist there: the wipe scan (`0x801E6510..0x801E664C`) walks the actor
    /// pointer table `0x801C9370` for exactly the seated-count byte
    /// (`*(0x8007BD24) + 0`), with no per-slot null check - seatedness is
    /// established once at battle load, from the present-party list, and a
    /// retail battle always seats at least one living member. (The
    /// disassembly's `beq v0,zero,0x801e6574` at `0x801E6524` even shows a
    /// count of zero would fall straight into the wipe compare - retail is
    /// saved by the count never being zero, not by a guard.)
    ///
    /// The port *can* represent the impossible state: a host may enter battle
    /// with party slots stamped but no roster projected onto them, and those
    /// hollow actors read as "dead party" to a scan that only asks
    /// `liveness != 0`. This hook is how the scan asks the host the question
    /// retail answers structurally. The default is `true` - a host that does
    /// not track seating keeps the historical "every party slot counts"
    /// behaviour.
    fn slot_seated(&self, _slot: u8) -> bool {
        true
    }

    /// Iteration helper - total slot count (default `8`).
    fn slot_count(&self) -> u8 {
        ACTOR_SLOTS as u8
    }

    /// The two four-entry effect-child arrays the cast tick censuses:
    /// `(ctx[+0x24E..=+0x251], ctx[+0x252..=+0x255])`.
    ///
    /// The first array is the per-slot **kind** byte (zero = the slot is not
    /// carrying an effect at all) and the second is the live child handle.
    /// [`crate::battle_action::tick_cast_census`] folds them into
    /// [`BattleActionCtx::magic_recovery_gate`].
    ///
    /// Default is all zeros - a host with no effect children reports "nothing
    /// outstanding", which is what the port did before the census existed, so
    /// the default is behaviour-preserving rather than a stub that lies.
    ///
    /// REF: FUN_801E09F8
    fn effect_child_slots(&self) -> ([u8; 4], [u8; 4]) {
        ([0; 4], [0; 4])
    }

    /// Look up the [`legaia_art::ArtRecord`] for an actor's chosen art. The
    /// state machine reads this on Tactical Arts windup to fetch power
    /// bytes, hit timing, repeat-frame data, and the status effect to
    /// apply on hit.
    ///
    /// Default returns `None` - pure-host tests don't need art data, and
    /// the SM falls back to attack-chain default damage when an art record
    /// is unavailable.
    fn art_record(
        &self,
        _character: legaia_art::Character,
        _action: legaia_art::ActionConstant,
    ) -> Option<&legaia_art::ArtRecord> {
        None
    }
}
