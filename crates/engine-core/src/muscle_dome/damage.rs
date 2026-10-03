//! Fighter stat profiles, the damage model and each fighter's magic / art loadout.
//! Split out of `muscle_dome.rs`.

use super::*;

/// One fighter's battle-actor stat profile, as the retail damage kernel
/// reads it off the actor record.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DomeCombatant {
    /// Max HP (`+0x14e`) - the spirit-gauge fill divides by it.
    pub hp_max: u16,
    /// INT working value (`+0x168`) - the damage roll's own stat.
    pub int: u16,
    /// UDF (`+0x15c`) - defender roll term A.
    pub udf: u16,
    /// LDF (`+0x160`) - defender roll term B.
    pub ldf: u16,
    /// Element id (`0..=7`) for the affinity scale.
    pub element: u8,
}

/// One resolved command play of the last turn, for a host's playback.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DomePlay {
    /// Fighter slot that acted.
    pub attacker: usize,
    /// The queued direction-command id (`0xC..=0xF`).
    pub cmd: u8,
    /// The move-power record's power for that command.
    pub power: i32,
    /// Damage the defender took.
    pub damage: i32,
    /// Both fighters' HP straight after the play.
    pub hp_after: [i32; 2],
}

/// The **retail damage kernel** for a dome turn: the PROT 0898 battle tables
/// plus both fighters' stat profiles and the contest's PsyQ `rand()` cursor.
///
/// A queued direction resolves exactly as retail plays a battle action - the
/// move-power record via the `0x801F4E63` id → index map
/// ([`legaia_asset::move_power`]), the arts/physical predamage roll
/// (`FUN_801dd0ac`), the element-affinity scale (`FUN_801dd864`) and the
/// damage finisher (`FUN_801ddb30`), drawing from the `rand()` stream in
/// retail call order (3 draws, +2 when the bonus arm fires, +1 when
/// mitigation floors the hit). The defender's spirit gauge accrues from each
/// hit (`spirit_gauge_fill`).
///
/// Install it on a session with
/// [`MuscleDomeSession::install_damage_model`] and drive it with
/// [`MuscleDomeSession::resolve_turn_retail`]; both the native and browser
/// hosts share this one kernel rather than each inventing a damage rule.
///
/// The model keeps its own HP mirror because the roll reads the defender's
/// *live* `+0x14c`, which drops mid-turn: the session applies the same
/// damage in the same order, so the mirror stays in step with it.
///
/// PORT: FUN_801dd0ac / FUN_801dd864 / FUN_801ddb30 (through
/// [`legaia_engine_vm::battle_formulas`])
#[derive(Debug, Clone)]
pub struct DomeDamageModel {
    pub(super) move_power: Vec<MoveRecord>,
    pub(super) move_map: [u8; move_power::MOVE_ID_INDEX_MAP_LEN],
    pub(super) affinity: Option<ElementAffinity>,
    pub(super) combatants: [DomeCombatant; 2],
    pub(super) rng: u32,
    pub(super) hp: [i32; 2],
    pub(super) spirit: [u16; 2],
    pub(super) log: Vec<DomePlay>,
}

impl DomeDamageModel {
    /// Build from already-parsed tables.
    pub fn new(
        move_power: Vec<MoveRecord>,
        move_map: [u8; move_power::MOVE_ID_INDEX_MAP_LEN],
        affinity: Option<ElementAffinity>,
        combatants: [DomeCombatant; 2],
        hp: [i32; 2],
        rng_seed: u32,
    ) -> Self {
        Self {
            move_power,
            move_map,
            affinity,
            combatants,
            rng: rng_seed,
            hp,
            spirit: [0, 0],
            log: Vec::new(),
        }
    }

    /// Parse the tables straight off the **raw** PROT 0898 entry (the
    /// move-power table, its id → index map and the element-affinity matrix
    /// are all pinned at raw-entry file offsets; the entry is stored
    /// uncompressed, so the raw and as-loaded views agree). Returns `None`
    /// when the move-power table does not decode.
    pub fn from_battle_overlay(
        raw: &[u8],
        combatants: [DomeCombatant; 2],
        hp: [i32; 2],
        rng_seed: u32,
    ) -> Option<Self> {
        let table = move_power::parse(raw)?;
        let map = move_power::parse_id_index_map(raw)?;
        let affinity = legaia_asset::element_affinity::parse(raw);
        Some(Self::new(table, map, affinity, combatants, hp, rng_seed))
    }

    /// Both fighters' stat profiles.
    pub fn combatants(&self) -> &[DomeCombatant; 2] {
        &self.combatants
    }

    /// A fighter's spirit gauge (`actor+0x170`, `0..=100`) - the value the
    /// shared battle status plate displays.
    ///
    /// REF: FUN_801d8de8 elems 0x52/0x53 (stage `actor+0x170` into the
    /// plate globals; the plate is shared battle chrome, not dome-specific)
    pub fn spirit(&self, slot: usize) -> u16 {
        self.spirit[slot]
    }

    /// The `rand()` cursor, so a host can persist or reseed the stream.
    pub fn rng_seed(&self) -> u32 {
        self.rng
    }

    /// Replace the `rand()` cursor - how a host lends its own stream in
    /// ([`MuscleDomeSession::resolve_turn_on_stream`]).
    pub fn set_rng_seed(&mut self, state: u32) {
        self.rng = state;
    }

    /// The last resolved turn's play-by-play.
    pub fn plays(&self) -> &[DomePlay] {
        &self.log
    }

    /// Open a turn: clear the play log and re-sync the HP mirror to the
    /// session's live values.
    pub fn begin_turn(&mut self, hp: [i32; 2]) {
        self.log.clear();
        self.hp = hp;
    }

    /// Resolve one queued command - the function
    /// [`MuscleDomeSession::resolve_turn`] takes as its damage closure.
    pub fn damage(&mut self, attacker: usize, cmd: u8) -> i32 {
        let defender = attacker ^ 1;
        // The kernel's `a0` is `map[actor+0x1DF]`, the move-power table
        // index (`FUN_801E09F8` `0x801E1874..0x801E188C`, then the 26-byte
        // stride at `0x801DD1A0`), and the power is that row's `+0` word
        // arithmetic-shifted right by two.
        //
        // The four **direction** ids map to index 0, and the disc ships row 0
        // as 26 zero bytes - so a plain swing carries no power through this
        // table at all. Its tier is the melee kernel's per-command scalar
        // `0x801F64EC[(id - 0x0C) % 5]` instead
        // ([`vm::battle_formulas::command_power_scalar`]), which is the same
        // 12 / 18 / 20 / 22 / 28 scale an art record's power byte decodes to.
        // Falling back to it is what stops every dome swing resolving at
        // power zero.
        let power = match move_power::record_for_move_id(&self.move_power, &self.move_map, cmd) {
            Some(r) => r.power(),
            None => legaia_engine_vm::battle_formulas::command_power_scalar(cmd) as i32,
        };
        let hp = self.hp;
        let combatants = self.combatants;
        let actor = |slot: usize| SummonRollActor {
            hp: hp[slot].clamp(0, u16::MAX as i32) as u16,
            agl: combatants[slot].int,
            stat_a: combatants[slot].udf,
            stat_b: combatants[slot].ldf,
            status: 0,
            guard: 0,
        };
        let affinity_pct = self
            .affinity
            .as_ref()
            .and_then(|a| {
                a.affinity_pct(combatants[attacker].element, combatants[defender].element)
            })
            .unwrap_or(100);
        let damage = {
            let rng = &mut self.rng;
            let rng3 = [
                world_rand(rng) as u16,
                world_rand(rng) as u16,
                world_rand(rng) as u16,
            ];
            let (att_roll, def_roll) = arts_physical_predamage_lazy(
                power,
                &actor(attacker),
                &actor(defender),
                affinity_pct,
                rng3,
                || [world_rand(rng) as u16, world_rand(rng) as u16],
            );
            let finish = DamageFinish {
                predamage: att_roll.saturating_sub(def_roll),
                attacker_slot: if attacker == 0 { 0 } else { 3 },
                defender_slot: if defender == 0 { 0 } else { 3 },
                attacker_element: combatants[attacker].element,
                defender_resist: DefenderResist::default(),
                defender_guarding: false,
                enemy_defender_halve: false,
                bypass_party_resist: false,
                summon_power_pct: 100,
                floor_rand: 0,
            };
            damage_finish_lazy(&finish, || world_rand(rng) as u16) as i32
        };
        self.hp[defender] = (self.hp[defender] - damage).max(0);
        self.spirit[defender] = spirit_gauge_fill(
            damage as u32,
            combatants[defender].hp_max,
            self.spirit[defender],
            DefenderResist::default(),
            defender == 0,
        );
        self.log.push(DomePlay {
            attacker,
            cmd,
            power,
            damage,
            hp_after: self.hp,
        });
        damage
    }
}

/// The equipment-slot index the Ra-Seru gate reads: `+0x199` for every
/// character but Noa, whose arm reads `+0x198`. Same pair
/// [`crate::battle_hud::battle_member_has_raseru`] carries - kept here as
/// its roster-slot twin, because a dome fighter has no battle ordinal until
/// the leg hands off to the battle.
///
/// REF: FUN_80053CB8 (`0x800541EC..0x80054258`)
pub(super) const RASERU_SLOT: usize = 3;
/// Noa's arm of the same gate.
pub(super) const RASERU_SLOT_NOA: usize = 2;
/// The roster slot Noa occupies (`DAT_8007BD10` character id `2`).
pub(super) const NOA_ROSTER_SLOT: usize = 1;

/// Build a dome fighter's [`DomeMagic`] out of a live world's roster - the
/// one door both native dome entry paths (the arena-door warp and the
/// window's own dome entry) install through, so neither grows a rule of its
/// own.
///
/// `roster_slot` is the character occupying the fighter seat; `special` is
/// the battle's [`SPECIAL_ITEM_FORBIDDEN`] / [`SPECIAL_MAGIC_FORBIDDEN`]
/// word. The learned block is the roster record's own spell list unioned
/// with anything captured this session, exactly as the regular battle's
/// magic submenu builds it (`World::build_battle_spell_session`), so a dome
/// cast offers the same rows the battle does.
///
/// Returns `None` when the roster has no such member.
pub fn magic_loadout_for(
    world: &crate::world::World,
    roster_slot: usize,
    special: u32,
) -> Option<DomeMagic> {
    let member = world.party.roster.members.get(roster_slot)?;
    let list = member.spell_list();
    let n = (list.count as usize).min(list.ids.len());
    let mut learned: Vec<u8> = list.ids[..n].to_vec();
    for &sid in world.seru.log.learned_spells(roster_slot as u8) {
        if !learned.contains(&sid) {
            learned.push(sid);
        }
    }
    let spells: Vec<crate::spells::SpellDef> = learned
        .iter()
        .filter_map(|id| world.tables.spell_catalog.get(*id).cloned())
        .collect();
    let live = member.live_stats();
    let gauge = member.hp_mp_sp();
    let slot = if roster_slot == NOA_ROSTER_SLOT {
        RASERU_SLOT_NOA
    } else {
        RASERU_SLOT
    };
    let has_raseru = member.equipment().slots[slot] != 0;
    Some(DomeMagic {
        ring: DomeRing {
            special,
            // A dome fighter enters the leg unafflicted: the status halfword
            // is a battle actor's, and the leg's actors are staged by the
            // battle the arena hands off to.
            status: 0,
            has_raseru,
        },
        mp: gauge.mp_cur,
        mp_max: gauge.mp_max,
        ability_bits: world
            .party
            .character_ability_bits
            .get(roster_slot)
            .copied()
            .unwrap_or(0) as u8,
        magic_power: live.int,
        spells,
    })
}

/// One fighter's **normal-art catalog** for [`MuscleDomeSession::install_art_catalog`],
/// filtered out of a world's art records the way the retail queue builder's
/// inner loop filters them: this character's rows only, the **normal** arts
/// only, and combos of two arrows or more.
///
/// "Normal" is the constant band `>= 0x1F`. The builder routes ordinals
/// `0..=3` - the Miracle Art and the three Hyper Arts - through a different
/// arm (`sltiu a1,a0,0x4` at `0x801EF330`) that, with the slot's `+0x25F`
/// marker clear, writes nothing, so their combo bytes never tokenize as arts.
/// The two-arrow floor is the builder's `s1 == 1` exit
/// (`0x801EF420..0x801EF434`): a fully matched one-arrow string is left
/// unrewritten, and letting one match would steal an arrow from every art
/// containing it. Sorted by constant, the grid order the loop walks.
///
/// Both dome hosts build their catalog through this one filter so neither can
/// grow a rule of its own.
///
/// REF: FUN_801EED1C (`0x801EF330`, `0x801EF420..0x801EF434`)
/// Lowest action constant that is a **normal** art - the band the queue
/// builder's inner loop tokenizes. Below it sit the Miracle Art and the three
/// Hyper Arts, which the builder routes elsewhere. The battle command flow's
/// own queue builder holds the same bound for the same reason.
pub(super) const NORMAL_ART_MIN_CONSTANT: u8 = 0x1F;

pub fn art_catalog_for(
    records: &std::collections::HashMap<
        (legaia_art::Character, legaia_art::ActionConstant),
        legaia_art::ArtRecord,
    >,
    character: legaia_art::Character,
) -> Vec<(legaia_art::ActionConstant, Vec<legaia_art::Command>)> {
    let mut rows: Vec<(legaia_art::ActionConstant, Vec<legaia_art::Command>)> = records
        .iter()
        .filter(|((ch, action), rec)| {
            *ch == character
                && action.is_art()
                && action.as_byte() >= NORMAL_ART_MIN_CONSTANT
                && rec.commands.len() >= 2
        })
        .map(|((_, action), rec)| (*action, rec.commands.clone()))
        .collect();
    rows.sort_by_key(|(a, _)| a.as_byte());
    rows
}

/// Map a dealt direction's action byte (`0x0C` Left, `0x0D` Right, `0x0E`
/// Down, `0x0F` Up - the deck table `DAT_801f4b8c`'s ids) onto the arrow the
/// tokenizer reads. `None` for anything outside that band.
pub(super) fn dome_command_of_action_byte(b: u8) -> Option<legaia_art::Command> {
    match b {
        0x0C => Some(legaia_art::Command::Left),
        0x0D => Some(legaia_art::Command::Right),
        0x0E => Some(legaia_art::Command::Down),
        0x0F => Some(legaia_art::Command::Up),
        _ => None,
    }
}
