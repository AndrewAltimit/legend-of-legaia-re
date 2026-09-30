//! The running Muscle Dome leg (`MuscleDomeSession`).
//! Split out of `muscle_dome.rs`.

use super::*;

/// The running Muscle Dome contest. Slot 0 = the player's fighter, slot 1 =
/// the opponent ([`HP_LEFT_SLOT`], the record the HUD's percentage reads).
#[derive(Debug, Clone)]
pub struct MuscleDomeSession {
    pub(super) f: [DomeFighter; 2],
    pub(super) phase: MusclePhase,
    /// Turns played out so far - the `ctx+0x28a` battle turn counter. It is
    /// a counter, not a budget: nothing in retail bounds a battle by it.
    pub(super) turn: u32,
    /// The caption's Seru index (`ctx+0x269`); the captioned spell id is
    /// `REWARD_SPELL_ID_BASE + index`. Display only - see
    /// [`Self::reward_spell_id`].
    pub(super) reward_seru_index: u8,
    /// Damage applied to each fighter in the last resolution, for the HUD.
    pub(super) last_turn_damage: [i32; 2],
    /// The installed retail damage kernel, when the host has disc tables to
    /// give it ([`MuscleDomeSession::install_damage_model`]).
    pub(super) damage: Option<DomeDamageModel>,
    /// The round time meter's `0..=`[`TIME_METER_MAX`] counter, advanced by
    /// [`Self::tick_time_meter`].
    pub(super) time_meter: u8,
    /// The meter bar sprite's Y offset for the current counter value.
    pub(super) time_meter_bar_y: i16,
    /// Per-fighter **normal-art catalog** in grid order (ascending action
    /// constant), when the host has the character's art records. Empty =
    /// "this fighter knows no arts", which is retail's own answer for an
    /// unmatched string.
    pub(super) art_catalog: [Vec<(legaia_art::ActionConstant, Vec<legaia_art::Command>)>; 2],
    /// Per-fighter magic loadout, when the host has one to give
    /// ([`Self::install_magic`]). `None` is a fighter whose Ra-Seru chip
    /// reads `-` and whose ring arm refuses, which is retail's own answer for
    /// a member with no Ra-Seru equipped.
    pub(super) magic: [Option<DomeMagic>; 2],
    /// The spell each fighter has committed for this turn (`actor+0x1DF[0]`
    /// with `actor+0x1DE = 2`). A cast **replaces** the direction string; it
    /// spends MP, not AP.
    pub(super) cast: [Option<u8>; 2],
    /// Whether the player's Ra-Seru list is open over the ring, and where its
    /// cursor sits. Retail's list is its own phase (`ctx+6 = 0x46`); the port
    /// keeps [`MusclePhase`] as it is and carries the list as a sub-state of
    /// `Select`, so a host that never opens it behaves exactly as before.
    pub(super) magic_open: bool,
    pub(super) magic_cursor: u8,
    /// The **special-battle word** `0x8007BAC0` for this leg
    /// ([`Self::set_special_word`]).
    ///
    /// Retail keeps exactly one of these, and it is per *battle*, not per
    /// fighter: `FUN_801CEA6C` seeds it once at arena entry and the ring's
    /// mark / arm tests (`FUN_801D0748`, `0x801D12C0..`) read that one word
    /// for whichever fighter is choosing. So the session owns it and
    /// [`Self::ring`] overlays it on whatever a fighter's installed
    /// [`DomeMagic`] carried, which is also what makes the Item chip gate for
    /// a fighter with **no** magic loadout at all.
    pub(super) special: u32,
}

impl MuscleDomeSession {
    /// Start one leg: per-fighter deals (deck command ids + that fighter's
    /// costs), turn-budget pools (record `+0x154`), HP, and the Seru index
    /// the victory caption names (display only - a leg pays nothing).
    pub fn new(
        player_hand: [MuscleCard; HAND_SLOTS],
        opponent_hand: [MuscleCard; HAND_SLOTS],
        budget_pools: [u16; 2],
        hp: [i32; 2],
        reward_seru_index: u8,
    ) -> Self {
        Self {
            f: [
                DomeFighter::new(player_hand, budget_pools[0], hp[0]),
                DomeFighter::new(opponent_hand, budget_pools[1], hp[1]),
            ],
            phase: MusclePhase::Select,
            turn: 0,
            reward_seru_index,
            last_turn_damage: [0, 0],
            damage: None,
            time_meter: 0,
            time_meter_bar_y: time_meter_step(0, 0, false, false).1,
            art_catalog: [Vec::new(), Vec::new()],
            magic: [None, None],
            cast: [None, None],
            magic_open: false,
            magic_cursor: 0,
            special: 0,
        }
    }

    /// Seed the leg's [`special`](Self::special) word - the value retail's
    /// arena entry wrote to `0x8007BAC0`, i.e. [`contest_entry_word`] of the
    /// visit's course-unlock flags.
    ///
    /// Both hosts call this at dome entry. Leaving it unset keeps the word at
    /// `0`, which forbids nothing - the shape every synthetic session that
    /// never seeds one keeps.
    ///
    /// REF: FUN_801cea6c (`0x801CEB88..0x801CEBC8`; the seed itself is ported
    /// at [`contest_entry_word`])
    pub fn set_special_word(&mut self, word: u32) {
        self.special = word;
    }

    /// The leg's special-battle word.
    pub fn special_word(&self) -> u32 {
        self.special
    }

    /// Advance the round **time meter** one frame by the frame delta `dt`.
    ///
    /// The counter climbs while the contest is in its selection phase (retail's
    /// phase tag `'P'`) and drains otherwise, and the bar sprite's Y offset
    /// follows it ([`time_meter_step`]). Retail additionally gates the climb on
    /// a separate ramp flag; nothing in the port lowers that flag mid-selection,
    /// so the session passes it up and the phase is the whole gate here.
    ///
    /// Returns the bar's new Y offset.
    pub fn tick_time_meter(&mut self, dt: u8) -> i16 {
        let in_select = self.phase == MusclePhase::Select;
        let (counter, bar_y) = time_meter_step(self.time_meter, dt, in_select, in_select);
        self.time_meter = counter;
        self.time_meter_bar_y = bar_y;
        bar_y
    }

    /// The time meter's current counter, `0..=`[`TIME_METER_MAX`].
    pub fn time_meter(&self) -> u8 {
        self.time_meter
    }

    /// The time-meter bar sprite's current Y offset (`-0x92` empty, `+0xE`
    /// full).
    pub fn time_meter_bar_y(&self) -> i16 {
        self.time_meter_bar_y
    }

    /// Current phase.
    pub fn phase(&self) -> MusclePhase {
        self.phase
    }

    /// Turns played out so far (0-based) - the `ctx+0x28a` counter. Unbounded:
    /// a leg runs until one fighter drops.
    pub fn turn(&self) -> u32 {
        self.turn
    }

    /// A fighter's current HP.
    pub fn hp(&self, slot: usize) -> i32 {
        self.f[slot].hp
    }

    /// A fighter's dealt directions.
    pub fn hand(&self, slot: usize) -> &[MuscleCard; HAND_SLOTS] {
        &self.f[slot].hand
    }

    /// Remaining turn budget (`ctx+0x6dc`).
    pub fn budget(&self, slot: usize) -> u16 {
        self.f[slot].budget
    }

    /// Points spent this turn (`ctx+0x6d8`).
    pub fn spent(&self, slot: usize) -> u16 {
        self.f[slot].spent
    }

    /// The committed command-id queue (`actor+0x1df`).
    pub fn queue(&self, slot: usize) -> &[u8] {
        &self.f[slot].queue
    }

    /// Damage each side took in the last resolved turn.
    pub fn last_turn_damage(&self) -> [i32; 2] {
        self.last_turn_damage
    }

    /// A fighter's HP as a plain percentage of its maximum:
    /// `hp * 100 / max_hp`.
    pub fn hp_left_percent(&self, slot: usize) -> i32 {
        self.f[slot].hp * HP_LEFT_SCALE / self.f[slot].max_hp
    }

    /// The HUD's **HP Left** readout: the *opponent's* remaining HP as a
    /// percentage ([`HP_LEFT_SLOT`]). This is the quantity the dome scores a
    /// timed-out leg on - not a per-fighter score, and the scale is 100, not
    /// the `0x6C` an earlier reading took off the shift-add chain.
    ///
    /// PORT: FUN_801d0748 phase 0x14 (`DAT_801f6959 =
    /// DAT_801c937c[+0x14c] * 100 / DAT_801c937c[+0x14e]`)
    pub fn hp_left(&self) -> i32 {
        self.hp_left_percent(HP_LEFT_SLOT)
    }

    /// The spell id the victory caption names (`REWARD_SPELL_ID_BASE +
    /// ctx+0x269`, an id into the shared spell-name table's player Seru-magic
    /// block).
    ///
    /// This is a **caption** input, not a payout: nothing in the arena
    /// overlay grants the named Seru, and a contest's real reward is coins
    /// ([`DomeContest::settle`]). Hosts display it; they must not credit it.
    pub fn reward_spell_id(&self) -> u8 {
        REWARD_SPELL_ID_BASE.wrapping_add(self.reward_seru_index)
    }

    /// The three-part **cast caption** as indices - the host resolves the
    /// strings.
    ///
    /// The table this indexes (`0x801F4DFC`) is the shared battle-family
    /// per-character label table, byte-identical across the battle-action,
    /// magic-capture, magic-level-up and dome overlays. It captions a cast;
    /// it does not describe a dome prize. Keep it for display and take the
    /// contest's payout from [`DomeContest::settle`].
    ///
    /// `char_id` is the winning fighter's 1-based character id (retail's
    /// `DAT_8007BD10[ctx+0x13]`); the lead-in line is entry `char_id - 1` of
    /// the victory-message pointer table
    /// ([`legaia_asset::muscle_dome::VICTORY_MSG_TABLE_VA`], which holds
    /// exactly the three party fighters' lines), then the reward spell's
    /// name, then a fixed suffix.
    ///
    /// Retail runs this assembly **inline** in `FUN_801D8DE8`'s HUD case
    /// `0x59` (`0x801D9154..0x801D91D0`); `FUN_801DBA90` is a standalone,
    /// instruction-identical twin of that arm which no image references. The
    /// port composes through the decode of the twin because it is the same
    /// rule; the *live* site is the case-`0x59` arm.
    // REF: FUN_801dba90 (the standalone twin this delegates to)
    // REF: FUN_801d8de8 (case 0x59, the live site of the same assembly)
    pub fn reward_banner(
        &self,
        char_id: u8,
    ) -> legaia_engine_vm::battle_cast_dispatch::RewardBanner {
        legaia_engine_vm::battle_cast_dispatch::reward_banner(char_id, self.reward_seru_index)
    }

    /// Whether the leg is over. A KO either way - there is no other way for a
    /// dome leg to end.
    pub fn decided(&self) -> bool {
        matches!(self.phase, MusclePhase::Won | MusclePhase::Lost)
    }

    /// Whether `slot` can commit dealt direction `card_slot` right now:
    /// selection phase, queue space, and the budget covers the cost.
    ///
    /// An open Ra-Seru list or a committed cast both close the direction
    /// input: retail reaches the four-direction screen only through the
    /// ring's Attack arm, and a cast leaves the ring by a different door.
    pub fn can_commit(&self, slot: usize, card_slot: usize) -> bool {
        self.phase == MusclePhase::Select
            && !self.magic_open
            && self.queued_cast(slot).is_none()
            && card_slot < HAND_SLOTS
            && self.f[slot].queue.len() < QUEUE_CAP
            && self.f[slot].budget >= self.f[slot].hand[card_slot].cost
    }

    /// Commit one dealt direction: append its command id to the fighter's
    /// action queue, debit the budget, accrue the spent total. Returns
    /// `false` (rejected) on an overspend or outside the selection phase.
    ///
    /// PORT: FUN_801d388c case 0xb (budget gate, `actor+0x1df` append,
    /// `ctx+0x6d8`/`ctx+0x6dc` accounting)
    pub fn commit_card(&mut self, slot: usize, card_slot: usize) -> bool {
        if !self.can_commit(slot, card_slot) {
            return false;
        }
        let card = self.f[slot].hand[card_slot];
        self.f[slot].queue.push(card.command_id);
        self.f[slot].spent += card.cost;
        self.f[slot].budget -= card.cost;
        true
    }

    /// The opponent's selection: the same commit logic in deal order while
    /// the budget lasts (retail reuses the shared deal/commit paths keyed on
    /// `ctx+0x13`; there is no dome-specific AI table - the in-order greedy
    /// walk is the host model).
    ///
    /// Host model, disclosed: the opponent draws from the *player's* four
    /// direction commands (`0xC..=0xF`), not from a monster action set. A
    /// monster fights the dome through its own PROT 867 action stream, which
    /// this session does not model.
    pub fn ai_commit_all(&mut self, slot: usize) {
        loop {
            let pick = (0..HAND_SLOTS).find(|&c| self.can_commit(slot, c));
            match pick {
                Some(c) => {
                    self.commit_card(slot, c);
                }
                None => break,
            }
        }
    }

    /// Close the selection phase (the player confirms their queue).
    pub fn end_selection(&mut self) {
        if self.phase == MusclePhase::Select {
            self.phase = MusclePhase::Resolve;
        }
    }

    /// Whether `slot`'s selection is exhausted: no dealt direction is
    /// affordable (or the queue is full). Retail ends the input automatically
    /// at this point - the phase byte advances off the input arm without a
    /// confirm press (recomp capture: three 30-cost commits on a 100 budget
    /// move `ctx+6` `0x50 -> 0x5a` on the third press).
    ///
    /// Retail's **Auto** arm is not modelled here, and it is not a picker: it
    /// commits the 16-byte command string `FUN_801DA34C` reloads out of the
    /// character's own save record (`+0x1A7` / `+0x1B7`, chosen on the
    /// `actor+0x156 < actor+0x154` AP-band test) and `FUN_801DA59C` writes back
    /// on the review confirm. A port of it is a record field plus a copy, not a
    /// selection rule. See docs/subsystems/minigame-muscle-dome.md, "The Auto
    /// arm picks nothing".
    ///
    /// REF: FUN_801d0748 phase 0x50
    pub fn selection_exhausted(&self, slot: usize) -> bool {
        self.phase == MusclePhase::Select && (0..HAND_SLOTS).all(|c| !self.can_commit(slot, c))
    }

    /// Reselect: throw the fighter's committed queue away and restore the
    /// turn budget (the retail confirm menu's "Reselect" arm returns to a
    /// clean input state - queue re-zeroed, budget back at the pool seed).
    ///
    /// REF: FUN_801d0748 phase 0x6e
    pub fn reset_selection(&mut self, slot: usize) {
        if self.phase != MusclePhase::Select && self.phase != MusclePhase::Resolve {
            return;
        }
        self.f[slot].queue.clear();
        self.f[slot].budget += self.f[slot].spent;
        self.f[slot].spent = 0;
        self.cast[slot] = None;
        self.magic_open = false;
        self.phase = MusclePhase::Select;
    }

    /// Play the turn out: each fighter's **entire** queued command string
    /// resolves as one turn - the player's first, then the opponent's -
    /// through `damage(attacker_slot, command_id) -> damage` (the host's
    /// battle-path stand-in), stopping at a KO. Retail plays a battle turn
    /// the same way: one actor's queued `+0x1df` string runs to completion
    /// through the shared battle-action machinery before the next actor
    /// takes its turn. The strings are **not** interleaved command-by-command.
    ///
    /// Bumps the [`turn`](Self::turn) counter (the `ctx+0x28a` analogue) and
    /// settles the phase: a KO decides the leg, and anything else continues at
    /// [`MusclePhase::TurnOver`]. Nothing else can end it - retail's only
    /// battle-end signal (`DAT_8007BD71 = 0xFE`) comes from the `0x5A`
    /// end-of-action KO scans, never from the turn counter.
    ///
    /// PORT: FUN_801d0748 commit phases 0x3c/0x46/0x50 (queue walk into
    /// `actor+0x1dd`/`+0x1de`, effect applied to the opposing record's HP)
    ///
    /// REF: FUN_801e295c case 0xff (`ctx[+0x28a] += 1`, phase back to 0x14 -
    /// the shared battle-action SM owns the turn counter this bumps)
    pub fn resolve_turn(&mut self, mut damage: impl FnMut(usize, u8) -> i32) {
        if self.phase != MusclePhase::Resolve {
            return;
        }
        self.last_turn_damage = [0, 0];
        'play: for attacker in 0..2usize {
            let defender = attacker ^ 1;
            // A committed cast is the whole turn: retail's ring arm stores
            // the spell id at `actor+0x1DF[0]` and the direction string never
            // reaches the queue, so there is nothing else to walk.
            if self.queued_cast(attacker).is_some() {
                let d = self.resolve_cast(attacker).max(0);
                self.last_turn_damage[defender] += d;
                self.f[defender].hp = (self.f[defender].hp - d).max(0);
                if self.f[defender].hp == 0 {
                    break 'play;
                }
                continue;
            }
            // The bytes retail actually plays: the tokenizer's action queue
            // when the fighter has an art catalog, the raw string otherwise.
            let queue = self.tokenized_queue(attacker);
            for &cmd in &queue {
                let d = damage(attacker, cmd).max(0);
                self.last_turn_damage[defender] += d;
                self.f[defender].hp = (self.f[defender].hp - d).max(0);
                if self.f[defender].hp == 0 {
                    break 'play;
                }
            }
        }
        self.turn += 1;
        self.phase = match (self.f[0].hp == 0, self.f[1].hp == 0) {
            (true, _) => MusclePhase::Lost,
            (false, true) => MusclePhase::Won,
            (false, false) => MusclePhase::TurnOver,
        };
    }

    /// Install fighter `slot`'s **normal-art catalog** - the rows the retail
    /// queue builder's inner loop walks, in grid order: `(action constant,
    /// its command bytes)`. Rows are sorted by constant here, so a host may
    /// pass them in any order.
    ///
    /// Without a catalog a queued string resolves as four plain swings, which
    /// is what the port did before and what retail does for a character with
    /// no arts. With one, the queue the turn resolves is the tokenizer's -
    /// arts overlap, leading arrows stay, and each matched art contributes
    /// its **own** move-power row instead of the swings it consumed.
    pub fn install_art_catalog(
        &mut self,
        slot: usize,
        mut rows: Vec<(legaia_art::ActionConstant, Vec<legaia_art::Command>)>,
    ) {
        if slot >= 2 {
            return;
        }
        // Only real arts, and only combos of two arrows or more: the
        // builder's `s1 == 1` exit refuses a fully matched one-arrow string,
        // and letting one match would steal an arrow from every art
        // containing it.
        rows.retain(|(a, c)| a.is_art() && c.len() >= 2);
        rows.sort_by_key(|(a, _)| a.as_byte());
        self.art_catalog[slot] = rows;
    }

    /// Fighter `slot`'s committed string as the retail **action queue** -
    /// the tokenizer's output over the installed catalog, or the raw
    /// direction string when the fighter has no catalog.
    ///
    /// This is the byte stream `actor[+0x1DF..]` holds when the turn plays
    /// out, so it is what the damage kernel must be walked over: an art's
    /// constant indexes a real move-power row, while a plain direction byte
    /// indexes row 0, which the disc ships as 26 zero bytes.
    ///
    /// PORT: FUN_801EED1C (the tokenizer pass only - the dome input has no
    /// Miracle / Super tail and no learn-on-use check, both of which live in
    /// the battle command flow's own builder)
    pub fn tokenized_queue(&self, slot: usize) -> Vec<u8> {
        let Some(f) = self.f.get(slot) else {
            return Vec::new();
        };
        let catalog = &self.art_catalog[slot];
        if catalog.is_empty() {
            return f.queue.clone();
        }
        let entries: Vec<legaia_art::tokenize::ArtEntry<'_>> =
            catalog.iter().map(|(a, c)| (*a, c.as_slice())).collect();
        // The committed string is action bytes `0x0C..=0x0F`; the tokenizer
        // reads the arrow space `1..=4` (Left / Right / Down / Up).
        let input: Vec<legaia_art::Command> = f
            .queue
            .iter()
            .filter_map(|b| dome_command_of_action_byte(*b))
            .collect();
        let tokens = legaia_art::tokenize(&entries, &input);
        legaia_art::tokenize::populated(&tokens).to_vec()
    }

    // --- The Ra-Seru (magic) command class ---------------------------------

    /// Install fighter `slot`'s magic loadout - the ring gates, the live MP
    /// gauge and the learned spells the Ra-Seru arm offers.
    ///
    /// Both dome hosts install one through this single door, so neither can
    /// grow a magic rule of its own. A fighter with no loadout keeps retail's
    /// behaviour for a member carrying no Ra-Seru: the chip's label is `-`
    /// and the arm refuses.
    pub fn install_magic(&mut self, slot: usize, magic: DomeMagic) {
        if slot < 2 {
            self.magic[slot] = Some(magic);
        }
    }

    /// The installed loadout, if any.
    pub fn magic(&self, slot: usize) -> Option<&DomeMagic> {
        self.magic.get(slot).and_then(|m| m.as_ref())
    }

    /// Fighter `slot`'s live MP.
    pub fn mp(&self, slot: usize) -> u16 {
        self.magic(slot).map_or(0, |m| m.mp)
    }

    /// The fighter's command-ring gates - the default (`status = 0`, no
    /// Ra-Seru) when no loadout is installed.
    ///
    /// The word is the **session's** ([`Self::set_special_word`]), never the
    /// installed loadout's, because retail has one per battle: a fighter with
    /// no [`DomeMagic`] at all still has its Item chip crossed out on a
    /// course that forbids it.
    pub fn ring(&self, slot: usize) -> DomeRing {
        DomeRing {
            special: self.special,
            ..self.magic(slot).map(|m| m.ring).unwrap_or_default()
        }
    }

    /// Whether `slot`'s ring arm for `chip` commits rather than refusing.
    /// Both hosts draw the chip enabled exactly when this is `true`.
    pub fn chip_enabled(&self, slot: usize, chip: DomeRingChip) -> bool {
        self.ring(slot).enabled(chip)
    }

    /// The mark retail lays over `chip`, if any. A chip that is merely
    /// unavailable because the member carries no Ra-Seru wears **none** -
    /// see [`DomeRing::mark`].
    pub fn chip_mark(&self, slot: usize, chip: DomeRingChip) -> Option<ChipMark> {
        self.ring(slot).mark(chip)
    }

    /// The MP a cast of `spell_id` actually charges `slot`: the spell table's
    /// `+3` byte after the caster's ability-bit discount, which is the number
    /// retail's phase-`0x46` arm compares against `actor+0x150`.
    ///
    /// PORT: FUN_801d0748 (`0x801D1A38..0x801D1B70` - the `DAT_800754C8`
    /// `+3` read and the `+0xF4` bit `0x20` / `0x10` discounts)
    pub fn spell_mp_cost(&self, slot: usize, spell_id: u8) -> Option<u16> {
        let m = self.magic(slot)?;
        let def = m.spells.iter().find(|s| s.id == spell_id)?;
        Some(crate::spells::caster_mp_cost(
            def,
            u32::from(m.ability_bits),
        ))
    }

    /// The Ra-Seru list as rows, priced through [`Self::spell_mp_cost`] so
    /// the displayed cost is the one the arm charges.
    pub fn spell_rows(&self, slot: usize) -> Vec<DomeSpellRow> {
        let Some(m) = self.magic(slot) else {
            return Vec::new();
        };
        m.spells
            .iter()
            .map(|def| {
                let mp_cost = self
                    .spell_mp_cost(slot, def.id)
                    .unwrap_or(def.mp_cost as u16);
                DomeSpellRow {
                    id: def.id,
                    name: def.name.clone(),
                    mp_cost,
                    affordable: m.mp >= mp_cost,
                }
            })
            .collect()
    }

    /// Whether the player's Ra-Seru list is open over the ring.
    pub fn magic_open(&self) -> bool {
        self.magic_open
    }

    /// The open list's cursor row.
    pub fn magic_cursor(&self) -> u8 {
        self.magic_cursor
    }

    /// Take the ring's Ra-Seru arm for fighter `slot`: refuse for the three
    /// retail reasons, or open the spell list.
    ///
    /// The refusal order is retail's own - the member gate first
    /// (`ctx[+0x25F + member]`, `0x801D1418`), then the sealed status
    /// (`actor+0x16E & 0x1000`, `0x801D143C`), then the special-battle word's
    /// magic bit (`0x801D1450`).
    ///
    /// PORT: FUN_801d0748 (`0x801D1400..0x801D145C`, the ring's Right arm up
    /// to the `ctx+6 = 0x46` store)
    pub fn open_magic(&mut self, slot: usize) -> Result<(), DomeCastRefusal> {
        if self.phase != MusclePhase::Select {
            return Err(DomeCastRefusal::WrongPhase);
        }
        if self.magic(slot).is_none() {
            return Err(DomeCastRefusal::NoLoadout);
        }
        let ring = self.ring(slot);
        if !ring.has_raseru {
            return Err(DomeCastRefusal::NoRaSeru);
        }
        if ring.status & STATUS_MAGIC_SEALED != 0 {
            return Err(DomeCastRefusal::Sealed);
        }
        if ring.special & SPECIAL_MAGIC_FORBIDDEN != 0 {
            return Err(DomeCastRefusal::Forbidden);
        }
        self.magic_open = true;
        self.magic_cursor = 0;
        Ok(())
    }

    /// Back out of the list without casting - retail's `0x8007BB94 == 3`
    /// cancel arm, which restores the saved ring state and returns to phase
    /// `0x28`.
    ///
    /// REF: FUN_801d0748 (`0x801D1B78..0x801D1BE8`)
    pub fn close_magic(&mut self) {
        self.magic_open = false;
        self.magic_cursor = 0;
    }

    /// Move the open list's cursor, wrapping over the fighter's rows.
    pub fn move_magic_cursor(&mut self, slot: usize, delta: i32) {
        let n = self.magic(slot).map_or(0, |m| m.spells.len());
        if !self.magic_open || n == 0 {
            return;
        }
        let n = n as i32;
        let next = (self.magic_cursor as i32 + delta).rem_euclid(n);
        self.magic_cursor = next as u8;
    }

    /// The spell id under the open list's cursor.
    pub fn magic_cursor_spell(&self, slot: usize) -> Option<u8> {
        let m = self.magic(slot)?;
        m.spells.get(self.magic_cursor as usize).map(|s| s.id)
    }

    /// Confirm the row under the cursor - the `0x8007BB94 == 2` arm.
    pub fn confirm_magic(&mut self, slot: usize) -> Result<u16, DomeCastRefusal> {
        let Some(id) = self.magic_cursor_spell(slot) else {
            return Err(DomeCastRefusal::UnknownSpell);
        };
        self.commit_cast(slot, id)
    }

    /// Commit a cast for fighter `slot`: re-run the ring gate, price the
    /// spell, and refuse when the live gauge does not cover it.
    ///
    /// On success the fighter's direction string is thrown away - retail's
    /// arm stores the spell id at `actor+0x1DF[0]`, so a cast **is** the
    /// whole queue - and the AP budget is left untouched, because the arm
    /// never reads `ctx+0x6D8` / `ctx+0x6DC`. The MP is charged where retail
    /// charges it, at the shared band's cast-begin, which is
    /// [`Self::resolve_turn`] here.
    ///
    /// Returns the effective MP cost.
    ///
    /// PORT: FUN_801d0748 (`0x801D1408..0x801D1528` the ring arm;
    /// `0x801D1BF0..0x801D1C58` the MP gate and confirm)
    pub fn commit_cast(&mut self, slot: usize, spell_id: u8) -> Result<u16, DomeCastRefusal> {
        if self.phase != MusclePhase::Select {
            return Err(DomeCastRefusal::WrongPhase);
        }
        if slot >= 2 {
            return Err(DomeCastRefusal::NoLoadout);
        }
        let ring = self.ring(slot);
        if self.magic(slot).is_none() {
            return Err(DomeCastRefusal::NoLoadout);
        }
        if !ring.has_raseru {
            return Err(DomeCastRefusal::NoRaSeru);
        }
        if ring.status & STATUS_MAGIC_SEALED != 0 {
            return Err(DomeCastRefusal::Sealed);
        }
        if ring.special & SPECIAL_MAGIC_FORBIDDEN != 0 {
            return Err(DomeCastRefusal::Forbidden);
        }
        let Some(cost) = self.spell_mp_cost(slot, spell_id) else {
            return Err(DomeCastRefusal::UnknownSpell);
        };
        if self.mp(slot) < cost {
            return Err(DomeCastRefusal::NotEnoughMp);
        }
        // The queue store: the spell id replaces the direction string, and
        // the budget the string spent comes back with it.
        self.f[slot].queue.clear();
        self.f[slot].budget += self.f[slot].spent;
        self.f[slot].spent = 0;
        self.cast[slot] = Some(spell_id);
        self.magic_open = false;
        Ok(cost)
    }

    /// The spell fighter `slot` has committed for this turn, if any.
    pub fn queued_cast(&self, slot: usize) -> Option<u8> {
        self.cast.get(slot).copied().flatten()
    }

    /// What fighter `slot` does with this turn: its cast, or the tokenizer's
    /// action queue.
    pub fn turn_action(&self, slot: usize) -> DomeTurnAction {
        match self.queued_cast(slot) {
            Some(id) => DomeTurnAction::Cast(id),
            None => DomeTurnAction::Commands(self.tokenized_queue(slot)),
        }
    }

    /// Resolve fighter `slot`'s committed cast against the other fighter -
    /// the MP debit plus the shared [`crate::spells::cast_spell`] rule the
    /// regular battle's cast band folds with
    /// (`World::cast_spell_on_slots_prepaid`), so a dome cast and a battle
    /// cast of the same spell resolve through one kernel.
    ///
    /// Returns the HP delta applied to the defender (positive = damage).
    pub(super) fn resolve_cast(&mut self, slot: usize) -> i32 {
        use crate::spells::{SpellOutcome, SpellSnapshot, cast_spell};
        let Some(spell_id) = self.queued_cast(slot) else {
            return 0;
        };
        let Some(cost) = self.spell_mp_cost(slot, spell_id) else {
            return 0;
        };
        let Some(def) = self
            .magic(slot)
            .and_then(|m| m.spells.iter().find(|s| s.id == spell_id))
            .cloned()
        else {
            return 0;
        };
        // Retail charges the cast at the shared band's `0x28`, not at the
        // ring - so the debit lands here, when the turn plays out.
        let caster_mag = self.magic(slot).map_or(0, |m| m.magic_power);
        let caster_ability_bits = self.magic(slot).map_or(0, |m| u32::from(m.ability_bits));
        if let Some(m) = self.magic[slot].as_mut() {
            m.mp = m.mp.saturating_sub(cost);
        }
        let defender = slot ^ 1;
        let target = defender as u8;
        let snap = SpellSnapshot {
            caster_mag,
            caster_hp: self.f[slot].hp.clamp(0, u16::MAX as i32) as u16,
            caster_max_hp: self.f[slot].max_hp.clamp(0, u16::MAX as i32) as u16,
            // The gauge is already debited; the shared rule re-checks it.
            caster_mp: self.mp(slot).saturating_add(cost),
            caster_ability_bits,
            target_mdef: self
                .damage
                .as_ref()
                .map_or(0, |m| m.combatants()[defender].ldf),
            target_hp: self.f[defender].hp.clamp(0, u16::MAX as i32) as u16,
            target_hp_max: self.f[defender].max_hp.clamp(0, u16::MAX as i32) as u16,
            target_mp: self.mp(defender),
            target_alive: self.f[defender].hp > 0,
            target_weakness: crate::spells::ElementMask::default(),
        };
        match cast_spell(&def, target, &snap) {
            SpellOutcome::Damage { amount, .. } => i32::from(amount),
            _ => 0,
        }
    }

    /// One frame of edge-triggered pad for the player's selection, shared by
    /// both dome hosts so neither can grow an input rule of its own.
    ///
    /// Two surfaces live under [`MusclePhase::Select`], exactly as retail's
    /// `ctx+6` keeps them apart:
    ///
    /// * the **Ra-Seru list** (`ctx+6 = 0x46`) while [`Self::magic_open`] -
    ///   up / down walk the rows, confirm commits the cast, cancel backs out;
    /// * the **direction input** (`ctx+6 = 0x50`) otherwise - the four
    ///   directions commit their dealt slot under the AP budget, and confirm
    ///   closes the turn.
    ///
    /// `pad.magic` is the surface that opens the list. Retail reaches it from
    /// the ring's Right chip (`0x801D1400`), which the port's collapsed
    /// selection cannot spare - the four directions are the input screen's -
    /// so each host binds it to a button of its own and both land here.
    ///
    /// Returns `true` when the player's selection closed this frame (the host
    /// then runs the opponent and resolves).
    pub fn select_input(&mut self, pad: DomeSelectPad) -> bool {
        if self.phase != MusclePhase::Select {
            return false;
        }
        if self.magic_open {
            if pad.cancel {
                self.close_magic();
                return false;
            }
            if pad.up {
                self.move_magic_cursor(0, -1);
            }
            if pad.down {
                self.move_magic_cursor(0, 1);
            }
            if pad.confirm {
                // A refusal leaves the list open, which is retail's answer to
                // an unaffordable pick (`0x8007BB94` is cleared and the arm
                // returns without committing).
                if self.confirm_magic(0).is_err() {
                    return false;
                }
                self.ai_commit_all(1);
                self.end_selection();
                return true;
            }
            return false;
        }
        if pad.magic {
            let _ = self.open_magic(0);
            return false;
        }
        let card = if pad.left {
            Some(0)
        } else if pad.right {
            Some(1)
        } else if pad.up {
            Some(2)
        } else if pad.down {
            Some(3)
        } else {
            None
        };
        if let Some(card) = card {
            self.commit_card(0, card);
        }
        if pad.confirm {
            self.ai_commit_all(1);
            self.end_selection();
            return true;
        }
        false
    }

    /// Install the shared [`DomeDamageModel`] so the turn can resolve through
    /// the **retail** battle formulas instead of a host stand-in.
    pub fn install_damage_model(&mut self, model: DomeDamageModel) {
        self.damage = Some(model);
    }

    /// The installed retail damage kernel, if any.
    pub fn damage_model(&self) -> Option<&DomeDamageModel> {
        self.damage.as_ref()
    }

    /// A fighter's spirit gauge (`actor+0x170`), `0` with no damage model
    /// installed.
    pub fn spirit(&self, slot: usize) -> u16 {
        self.damage.as_ref().map_or(0, |m| m.spirit(slot))
    }

    /// The last resolved turn's play-by-play, empty with no damage model
    /// installed.
    pub fn last_turn_plays(&self) -> &[DomePlay] {
        self.damage.as_ref().map_or(&[], |m| m.plays())
    }

    /// Play the turn out through the installed retail damage kernel - the
    /// path both hosts use. Returns `false` (and does nothing) when no
    /// [`DomeDamageModel`] is installed.
    pub fn resolve_turn_retail(&mut self) -> bool {
        let Some(mut model) = self.damage.take() else {
            return false;
        };
        model.begin_turn([self.f[0].hp, self.f[1].hp]);
        self.resolve_turn(|attacker, cmd| model.damage(attacker, cmd));
        self.damage = Some(model);
        true
    }

    /// Resolve the turn the way **both** hosts must: through the retail
    /// kernel when disc tables are installed, and otherwise by closing the
    /// turn with zero damage so the leg still advances.
    ///
    /// The fallback is a rule, not plumbing: without it a host that never
    /// installed a [`DomeDamageModel`] leaves the session parked in
    /// [`MusclePhase::Resolve`] with nothing able to move it, which is a hang
    /// rather than a degraded contest. It lives here so neither host can have
    /// it and the other not.
    ///
    /// Returns whether the retail kernel drove the turn.
    pub fn resolve_turn_or_zero(&mut self) -> bool {
        if self.resolve_turn_retail() {
            return true;
        }
        self.resolve_turn(|_, _| 0);
        false
    }

    /// Start the next turn after a non-terminal resolution: reseed the
    /// budgets from the pools, clear the queues. No-op once a KO has decided
    /// the leg.
    pub fn next_turn(&mut self) {
        if self.phase != MusclePhase::TurnOver {
            return;
        }
        self.f[0].reset_turn();
        self.f[1].reset_turn();
        self.cast = [None, None];
        self.magic_open = false;
        self.magic_cursor = 0;
        self.phase = MusclePhase::Select;
    }
}
