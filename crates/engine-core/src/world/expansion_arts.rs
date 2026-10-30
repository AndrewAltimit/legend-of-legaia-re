//! The `World` side of an expansion pack's `arts.toml` and `spells.toml`:
//! learning, the arts queue extension, the clips a pack art plays, the hit
//! and cast seams that read a pack's numbers, Seru capture of a pack
//! monster, and the learned set a New Game+ carries.
//!
//! Every hook is the identity while no pack (or a pack with no arts and no
//! spells) is active: [`World::pack_arts`] / [`World::pack_spells`] answer
//! `None`, the bank walk gets no extra rows, the art bank no extra clips, the
//! spell catalog no menu ids, and the save no tail record `5`.

use std::sync::Arc;

use legaia_engine_expansion::arts::{
    self as xa, ArtsProgress, CompiledArt, CompiledArts, FIRST_PACK_BYTE,
};
use legaia_engine_expansion::spells::{
    self as xs, CompiledSpell, CompiledSpells, PackCast, SpellProgress,
};
use legaia_expansion_format::arts::Learn;
use legaia_expansion_format::items::{Character as PackCharacter, Status};

use super::World;

/// The pack character of an art-table character.
pub(in crate::world) fn pack_character(c: legaia_art::Character) -> PackCharacter {
    match c {
        legaia_art::Character::Vahn => PackCharacter::Vahn,
        legaia_art::Character::Noa => PackCharacter::Noa,
        legaia_art::Character::Gala => PackCharacter::Gala,
    }
}

fn status_kind(s: Status) -> legaia_engine_battle_vm::status_effects::StatusKind {
    use legaia_engine_battle_vm::status_effects::StatusKind;
    match s {
        Status::Venom => StatusKind::Venom,
        Status::Toxic => StatusKind::Toxic,
        Status::Numb => StatusKind::Numb,
        Status::Sleep => StatusKind::Sleep,
        Status::Confuse => StatusKind::Confuse,
        Status::Rot => StatusKind::Rot,
        Status::Curse => StatusKind::Curse,
        Status::Stone => StatusKind::Stone,
    }
}

const HERO_NAMES: [&str; 3] = ["Vahn", "Noa", "Gala"];

impl World {
    /// Compile the active pack's arts and spells and lend the spells their
    /// menu ids. Called at activation.
    pub(in crate::world) fn compile_expansion_arts(&mut self) {
        let Some(pack) = self.expansion.pack.clone() else {
            return;
        };
        self.expansion.arts.compiled = (!pack.arts.arts.is_empty())
            .then(|| Arc::new(CompiledArts::compile(&pack.manifest.id, &pack.arts)));
        self.expansion.spells.compiled = (!pack.spells.spells.is_empty())
            .then(|| Arc::new(CompiledSpells::compile(&pack.spells)));
        self.expansion_install_spell_entries();
        self.expansion_refresh_learning(false);
    }

    /// Take the spell menu ids back out of the catalog. Called at
    /// deactivation.
    pub(in crate::world) fn expansion_release_arts(&mut self) {
        if let Some(s) = self.expansion.spells.compiled.clone() {
            for sp in &s.spells {
                if self
                    .tables
                    .spell_catalog
                    .get(sp.menu_id)
                    .is_some_and(|d| d.name == sp.name())
                {
                    self.tables.spell_catalog.remove(sp.menu_id);
                }
            }
        }
        self.expansion.spells.casting = None;
        self.expansion.arts.playing.clear();
    }

    /// The pack's compiled arts, when it has any.
    pub fn pack_arts(&self) -> Option<&CompiledArts> {
        self.expansion.arts.arts()
    }

    /// The pack's compiled spells, when it has any.
    pub fn pack_spells(&self) -> Option<&CompiledSpells> {
        self.expansion.spells.spells()
    }

    /// The pack arts known.
    pub fn pack_arts_progress(&self) -> &ArtsProgress {
        &self.expansion.quests.progress.arts
    }

    /// The pack spells known and their levels.
    pub fn pack_spell_progress(&self) -> &SpellProgress {
        &self.expansion.quests.progress.spells
    }

    /// Whether pack art `key` is known.
    pub fn pack_art_known(&self, key: &str) -> bool {
        self.expansion.quests.progress.arts.learned.contains(key)
    }

    /// Whether `learn`'s conditions hold for roster slot `roster`.
    fn learn_holds(&self, roster: u8, learn: &Learn) -> bool {
        if let Some(l) = learn.level {
            let level = self
                .party
                .level_up_tracker
                .level
                .get(usize::from(roster))
                .copied()
                .unwrap_or(1);
            if level < l {
                return false;
            }
        }
        if let Some(i) = &learn.item
            && self.held_item_count(i) == 0
        {
            return false;
        }
        self.expansion_prerequisites_hold(&learn.requires)
    }

    /// Teach every pack art and capture-free pack spell whose conditions now
    /// hold; with `announce` each one raises a toast. Run at activation, after
    /// a load, after every quest step and after a battle.
    pub fn expansion_refresh_learning(&mut self, announce: bool) {
        let mut lines = Vec::new();
        if let Some(arts) = self.expansion.arts.compiled.clone() {
            for a in &arts.arts {
                if self.pack_art_known(&a.key) {
                    continue;
                }
                let roster = a.character.slot();
                if self.learn_holds(roster, &a.learn) {
                    self.expansion
                        .quests
                        .progress
                        .arts
                        .learned
                        .insert(a.key.clone());
                    lines.push(format!(
                        "{} learned {}!",
                        HERO_NAMES[usize::from(roster)],
                        a.name
                    ));
                }
            }
        }
        if let Some(spells) = self.expansion.spells.compiled.clone() {
            for s in spells.spells.iter().filter(|s| s.learn().capture.is_none()) {
                for roster in 0..3u8 {
                    let Some(ch) = xs::character_of(roster) else {
                        continue;
                    };
                    if !s.def.learnable_by(ch)
                        || self.expansion.quests.progress.spells.knows(roster, &s.key)
                        || !self.roster_member_present(roster)
                        || !self.learn_holds(roster, &s.learn().when())
                    {
                        continue;
                    }
                    self.expansion.quests.progress.spells.learn(roster, &s.key);
                    lines.push(format!(
                        "{} learned {}!",
                        HERO_NAMES[usize::from(roster)],
                        s.name()
                    ));
                }
            }
        }
        if announce {
            for l in lines {
                self.expansion.toasts.push(l);
            }
        }
    }

    /// Whether roster slot `roster` has joined (its record is seeded).
    fn roster_member_present(&self, roster: u8) -> bool {
        self.party
            .roster
            .members
            .get(usize::from(roster))
            .is_some_and(|m| m.hp_mp_sp().hp_max != 0)
    }

    /// Teach pack art `key` now, whatever its conditions (a script, a test,
    /// a reward). `false` for an unknown key or one already known.
    pub fn expansion_teach_art(&mut self, key: &str) -> bool {
        if self.pack_arts().and_then(|a| a.get(key)).is_none() {
            return false;
        }
        self.expansion
            .quests
            .progress
            .arts
            .learned
            .insert(key.to_string())
    }

    /// Teach pack spell `key` to roster slot `roster` now. `false` for an
    /// unknown key, a character it is not for, or one already known.
    pub fn expansion_teach_spell(&mut self, roster: u8, key: &str) -> bool {
        let Some(s) = self.pack_spells().and_then(|s| s.get(key)) else {
            return false;
        };
        if !xs::character_of(roster).is_some_and(|c| s.def.learnable_by(c)) {
            return false;
        }
        self.expansion.quests.progress.spells.learn(roster, key)
    }

    // ---- New Game+ -----------------------------------------------------

    /// The pack arts and spells learned this playthrough - what a New Game+
    /// carries ([`Self::expansion_carry_learned`]).
    pub fn expansion_learned(&self) -> (ArtsProgress, SpellProgress) {
        (
            self.expansion.quests.progress.arts.clone(),
            self.expansion.quests.progress.spells.clone(),
        )
    }

    /// Merge a carried learned set into the active pack's progress (a New
    /// Game+ after [`Self::expansion_begin_new_game`]): an art or spell the
    /// pack no longer defines is dropped; a spell keeps the higher level.
    pub fn expansion_carry_learned(&mut self, arts: ArtsProgress, spells: SpellProgress) {
        let known_art = |k: &str| self.pack_arts().is_some_and(|a| a.get(k).is_some());
        let keep_arts: Vec<String> = arts.learned.into_iter().filter(|k| known_art(k)).collect();
        let known_spell = |k: &str| self.pack_spells().is_some_and(|s| s.get(k).is_some());
        let keep_spells: Vec<((u8, String), xs::SpellLevel)> = spells
            .known
            .into_iter()
            .filter(|((_, k), _)| known_spell(k))
            .collect();
        let p = &mut self.expansion.quests.progress;
        p.arts.learned.extend(keep_arts);
        for (k, lv) in keep_spells {
            let e = p.spells.known.entry(k).or_insert(lv);
            if lv.level > e.level {
                *e = lv;
            }
        }
    }

    // ---- Arts ------------------------------------------------------------

    /// The known pack arts' rows for `character`'s bank walk, in the form
    /// `legaia_art::bank_rows` takes. Empty with no pack art known.
    pub(in crate::world) fn expansion_bank_rows(
        &self,
        character: legaia_art::Character,
    ) -> Vec<(u8, Vec<legaia_art::Command>, [u8; 4])> {
        let Some(arts) = self.pack_arts() else {
            return Vec::new();
        };
        let spirit_known = |a: &CompiledArt| self.pack_art_known(&a.key);
        xa::bank_extension(arts, pack_character(character), spirit_known)
            .into_iter()
            .map(|(b, cmds)| {
                (
                    b,
                    cmds.into_iter()
                        .filter_map(legaia_art::Command::from_byte)
                        .collect(),
                    [0; 4],
                )
            })
            .collect()
    }

    /// After the retail queue finish: expand each pack art's segments and run
    /// the chain pass. Returns the pack arts the queue performs (a chain's
    /// finisher last). Identity with no pack art known.
    pub(in crate::world) fn expansion_finish_arts_queue(
        &self,
        character: legaia_art::Character,
        bytes: &mut [u8],
        marks: &mut [u32],
    ) -> Vec<usize> {
        let Some(arts) = self.pack_arts() else {
            return Vec::new();
        };
        let ch = pack_character(character);
        if !bytes
            .iter()
            .any(|&b| b >= FIRST_PACK_BYTE && b <= xa::LAST_PACK_BYTE)
            && arts.chains.is_empty()
        {
            return Vec::new();
        }
        let mut performed = xa::expand_segments(arts, ch, bytes, marks);
        let known = |a: &CompiledArt| self.pack_art_known(&a.key);
        if let Some(fin) = xa::apply_chains(arts, ch, known, bytes, marks) {
            performed.push(fin);
        }
        performed
    }

    /// Charge the pack arts a turn performs from the caster's Spirit gauge:
    /// each art's `ap`, halved under AP Used Down as retail's are.
    pub(in crate::world) fn expansion_charge_pack_arts(&mut self, caster: u8, performed: &[usize]) {
        let Some(arts) = self.pack_arts() else {
            return;
        };
        if performed.is_empty() {
            return;
        }
        let roster = self.party_roster_slot(caster as usize) as u8;
        let halved = self.character_ability_bits_word1(roster)
            & legaia_engine_battle::ap_gauge::AP_USED_DOWN_BIT
            != 0;
        let cost = xa::pack_ap_cost(arts, performed, halved);
        if let Some(a) = self.actors.get_mut(caster as usize) {
            a.battle.spirit_gauge = a.battle.spirit_gauge.saturating_sub(cost);
        }
    }

    /// Append the pack arts' clips to party member `member`'s art bank: for
    /// each segment of `character`'s arts, a copy of the segment's retail
    /// clip (bank record `base - 0x10`) with the pack's power bytes and beats,
    /// at record `byte - 0x10`. No-op without pack arts for the character.
    pub(crate) fn expansion_extend_art_bank(
        &mut self,
        member: usize,
        character: legaia_art::Character,
    ) {
        let Some(arts) = self.expansion.arts.compiled.clone() else {
            return;
        };
        let ch = pack_character(character);
        if arts.of(ch).next().is_none() {
            return;
        }
        let Some(bank) = self
            .actors
            .get(member)
            .and_then(|a| a.battle_art_bank.clone())
        else {
            return;
        };
        let mut bank: Vec<_> = bank.as_ref().clone();
        for a in arts.of(ch) {
            for s in &a.segments {
                let src = usize::from(s.base - 0x10);
                let dst = usize::from(s.byte - 0x10);
                let Some(mut clip) = bank.get(src).cloned().flatten() else {
                    log::warn!(
                        "expansion: art `{}` borrows constant {:#04x}, whose clip this battle form lacks",
                        a.key,
                        s.base
                    );
                    continue;
                };
                if let Some(run) = clip.entry_power_run() {
                    clip.set_entry_power_run(s.power_run(run));
                }
                if let Some(beats) = clip.entry_event_frames() {
                    clip.set_entry_event_frames(s.beat_run(beats));
                }
                if bank.len() <= dst {
                    bank.resize(dst + 1, None);
                }
                bank[dst] = Some(clip);
            }
        }
        self.set_actor_battle_art_bank(member, Arc::new(bank));
    }

    /// The `+0x1DB` latch the anim commit stores for staged byte `q` on
    /// `slot`: a pack art clip latches the retail constant it borrows, so
    /// every reader of the latch (the per-art camera, the art record's side
    /// data, the slow-motion arms) treats it as that art; the pack art
    /// itself is remembered in `ArtsRuntime::playing`. Any other byte is its
    /// own latch.
    pub(in crate::world) fn expansion_art_latch(&mut self, slot: usize, q: u8) -> u8 {
        if self.expansion.arts.compiled.is_none() {
            return q;
        }
        let hit = self
            .actors
            .get(slot)
            .filter(|a| a.battle_monster_id.is_none() && slot < 3)
            .and_then(|a| {
                let arts = self.expansion.arts.arts()?;
                let ch = pack_character(a.battle.character);
                let (art, seg) = arts.segment(ch, q)?;
                let ai = arts.arts.iter().position(|x| x.key == art.key)?;
                let si = art.segments.iter().position(|s| s.byte == seg.byte)?;
                Some((ai, si, seg.base))
            });
        match hit {
            Some((ai, si, base)) => {
                self.expansion.arts.playing.insert(slot, (ai, si));
                base
            }
            None => {
                self.expansion.arts.playing.remove(&slot);
                q
            }
        }
    }

    /// The pack art `attacker`'s committed clip belongs to.
    fn playing_pack_art(&self, attacker: u8) -> Option<&CompiledArt> {
        let (ai, _) = *self.expansion.arts.playing.get(&usize::from(attacker))?;
        self.pack_arts()?.arts.get(ai)
    }

    /// Scale one landed hit of a pack art: its `damage_pct`, then its
    /// element through the affinity matrix against the target's element,
    /// capped at 9999. The hit as rolled for any other clip.
    pub(in crate::world) fn expansion_scale_art_hit(
        &self,
        attacker: u8,
        target: u8,
        raw: u16,
    ) -> u16 {
        let Some(art) = self.playing_pack_art(attacker) else {
            return raw;
        };
        let mut v = u32::from(raw) * u32::from(art.damage_pct) / 100;
        if let Some(e) = art.element
            && let Some(aff) = self.tables.element_affinity.as_ref()
            && let Some(def) = self.battle_slot_element(target)
        {
            v = v * u32::from(aff.affinity_pct(e, def).unwrap_or(100)) / 100;
        }
        v.min(9999) as u16
    }

    /// A pack art's status rider on a landed hit: one `rand()` draw per
    /// damaging hit on a monster target, then the status.
    pub(in crate::world) fn expansion_art_hit_rider(&mut self, attacker: u8, target: u8, dmg: u16) {
        let Some(rider) = self.playing_pack_art(attacker).and_then(|a| a.rider) else {
            return;
        };
        if dmg == 0 || target < self.party.party_count {
            return;
        }
        self.expansion_roll_rider(target, rider.status, rider.chance);
    }

    fn expansion_roll_rider(&mut self, target: u8, status: Status, chance: u8) {
        let alive = self
            .actors
            .get(usize::from(target))
            .is_some_and(|a| a.battle.liveness != 0 && a.battle.hp > 0);
        if !alive {
            return;
        }
        if (self.next_rand() % 100) as u8 >= chance {
            return;
        }
        let kind = status_kind(status);
        self.battle.status_effects.apply(target, kind);
        if let Some(a) = self.actors.get_mut(usize::from(target)) {
            a.battle.field_flags |= kind.display_bit();
        }
        if kind == legaia_engine_battle_vm::status_effects::StatusKind::Stone {
            self.stone_cancels_queued_action(target);
        }
    }

    /// The pack art name the battle label shows for queue byte `byte` on
    /// party slot `slot`, `None` for a retail byte.
    pub fn expansion_pack_art_name(&self, slot: u8, byte: u8) -> Option<String> {
        let a = self.actors.get(usize::from(slot))?;
        let arts = self.pack_arts()?;
        let (art, _) = arts.segment(pack_character(a.battle.character), byte)?;
        Some(art.name.clone())
    }

    /// The known pack arts of `character` as the battle arts list draws a
    /// row: name, AP and the command glyphs.
    pub fn expansion_arts_list_rows(
        &self,
        character: legaia_art::Character,
        halved: bool,
    ) -> Vec<legaia_engine_battle::arts_command_input::ArtsListRow> {
        let Some(arts) = self.pack_arts() else {
            return Vec::new();
        };
        arts.of(pack_character(character))
            .filter(|a| self.pack_art_known(&a.key) && !a.commands.is_empty())
            .map(|a| legaia_engine_battle::arts_command_input::ArtsListRow {
                name: a.name.as_bytes().to_vec(),
                ap: if halved { a.ap >> 1 } else { a.ap },
                glyphs: a
                    .commands
                    .iter()
                    .filter_map(|&b| legaia_art::Command::from_byte(b))
                    .map(legaia_art::command_to_glyph)
                    .collect(),
            })
            .collect()
    }

    // ---- Spells ----------------------------------------------------------

    /// Install each pack spell's menu entry in the spell catalog: the base
    /// spell's entry under the menu id, with the pack's name, MP and target
    /// shape. Idempotent; a spell whose base the catalog does not hold yet is
    /// installed the next time this runs (battle entry, menu open).
    pub(in crate::world) fn expansion_install_spell_entries(&mut self) {
        use legaia_engine_battle::spells::SpellTarget;
        use legaia_expansion_format::spells::TargetShape;
        let Some(spells) = self.expansion.spells.compiled.clone() else {
            return;
        };
        for s in &spells.spells {
            if self.tables.spell_catalog.get(s.menu_id).is_some() {
                continue;
            }
            let Some(base) = self.tables.spell_catalog.get(s.base()).cloned() else {
                continue;
            };
            let mut def = base;
            def.id = s.menu_id;
            def.name = s.name().to_string();
            if let Some(mp) = s.def.mp {
                def.mp_cost = mp;
            }
            if let Some(t) = s.target() {
                def.target = match t {
                    TargetShape::OneEnemy => SpellTarget::OneEnemy,
                    TargetShape::AllEnemies => SpellTarget::AllEnemies,
                    TargetShape::OneAlly => SpellTarget::OneAlly,
                    TargetShape::AllAllies => SpellTarget::AllAllies,
                };
            }
            self.tables.spell_catalog.insert(def);
        }
    }

    /// The menu ids of the pack spells roster slot `roster` knows.
    pub fn expansion_pack_spell_ids(&self, roster: u8) -> Vec<u8> {
        match self.pack_spells() {
            Some(s) => self.expansion.quests.progress.spells.menu_ids(s, roster),
            None => Vec::new(),
        }
    }

    /// The pack spell listed under menu id `id`.
    pub fn expansion_pack_spell(&self, id: u8) -> Option<&CompiledSpell> {
        self.pack_spells()?.by_menu_id(id)
    }

    /// Begin a pack cast from menu id `menu_id` by battle ordinal `caster`:
    /// the base spell's catalog entry with the pack's MP and target shape
    /// under the **base** id, and the cast recorded as in flight. `None` for
    /// an id that is not a pack spell's.
    pub(in crate::world) fn expansion_begin_pack_cast(
        &mut self,
        caster: u8,
        menu_id: u8,
    ) -> Option<legaia_engine_battle::spells::SpellDef> {
        let spells = self.expansion.spells.compiled.clone()?;
        let index = spells.spells.iter().position(|s| s.menu_id == menu_id)?;
        let sp = &spells.spells[index];
        let mut def = self.tables.spell_catalog.get(menu_id).cloned()?;
        def.id = sp.base();
        let roster = self.party_roster_slot(caster as usize) as u8;
        let level = self
            .expansion
            .quests
            .progress
            .spells
            .level(roster, &sp.key)
            .map_or(1, |l| l.level);
        self.expansion.spells.casting = Some(PackCast {
            caster,
            roster,
            spell: index,
            level,
        });
        log::info!(
            "expansion: {} casts pack spell `{}` (Lv {level}) as spell {:#04x}",
            HERO_NAMES.get(usize::from(roster)).unwrap_or(&"?"),
            sp.key,
            sp.base()
        );
        Some(def)
    }

    /// The pack cast in flight for `caster` on base spell `spell_id`.
    fn pack_cast_for(
        &self,
        caster: Option<u8>,
        spell_id: u8,
    ) -> Option<(&PackCast, &CompiledSpell)> {
        let (c, s) = self.expansion.spells.cast()?;
        (s.base() == spell_id && caster.is_none_or(|x| x == c.caster)).then_some((c, s))
    }

    /// The MP the cast in flight debits for base spell `spell_id`.
    pub(in crate::world) fn expansion_cast_mp(&self, spell_id: u8) -> Option<u8> {
        let (_, s) = self.pack_cast_for(None, spell_id)?;
        s.def.mp
    }

    /// The element the cast in flight strikes with, in place of the summon
    /// creature's.
    pub(in crate::world) fn expansion_cast_element(&self, spell_id: u8) -> Option<u8> {
        let (_, s) = self.pack_cast_for(None, spell_id)?;
        s.def.element
    }

    /// The magic level the cast in flight rolls at (its own, not the base
    /// spell's).
    pub(in crate::world) fn expansion_cast_level(&self, caster: u8, spell_id: u8) -> Option<u8> {
        self.pack_cast_for(Some(caster), spell_id)
            .map(|(c, _)| c.level)
    }

    /// The pack spell name the label shows while its cast is in flight.
    pub fn expansion_cast_name(&self, spell_id: u8) -> Option<String> {
        self.pack_cast_for(None, spell_id)
            .map(|(_, s)| s.name().to_string())
    }

    /// Scale a folded magnitude of the cast in flight by the pack spell's
    /// power at its level.
    pub(in crate::world) fn expansion_scale_cast(
        &self,
        caster: u8,
        spell_id: u8,
        amount: u16,
    ) -> u16 {
        match self.pack_cast_for(Some(caster), spell_id) {
            Some((c, s)) => xs::scale_magnitude(amount, &s.def, c.level),
            None => amount,
        }
    }

    /// Whether the cast in flight is a pack spell's (it then takes no shiny
    /// bonus of the base Seru's).
    pub(in crate::world) fn expansion_casting_pack(&self, caster: u8, spell_id: u8) -> bool {
        self.pack_cast_for(Some(caster), spell_id).is_some()
    }

    /// The cast in flight's status rider on each struck target.
    pub(in crate::world) fn expansion_cast_rider(
        &mut self,
        caster: u8,
        spell_id: u8,
        struck: &[u8],
    ) {
        let Some(rider) = self
            .pack_cast_for(Some(caster), spell_id)
            .and_then(|(_, s)| s.rider())
        else {
            return;
        };
        for &t in struck {
            if t >= self.party.party_count {
                self.expansion_roll_rider(t, rider.status, rider.chance);
            }
        }
    }

    /// Bank a cast's spell XP on the pack spell instead of the base spell's
    /// record slot. `true` when the cast was a pack spell's (the XP is
    /// taken).
    pub(in crate::world) fn expansion_cast_xp(
        &mut self,
        caster: u8,
        spell_id: u8,
        gain: u32,
    ) -> bool {
        let Some((roster, key)) = self
            .pack_cast_for(Some(caster), spell_id)
            .map(|(c, s)| (c.roster, s.key.clone()))
        else {
            return false;
        };
        let thresholds = self.tables.magic_xp_thresholds;
        let up = self
            .expansion
            .quests
            .progress
            .spells
            .accrue(roster, &key, gain, |level, xp| {
                thresholds.as_ref().is_some_and(|t| {
                    legaia_engine_battle_vm::battle_formulas::summon_magic_levels_up(
                        spell_id,
                        level,
                        xp,
                        t.as_slice(),
                    )
                })
            });
        if let Some(level) = up {
            let name = self
                .pack_spells()
                .and_then(|s| s.get(&key))
                .map(|s| s.name().to_string())
                .unwrap_or_default();
            self.expansion
                .toasts
                .push(format!("{name}'s magic level rose to {level}."));
        }
        true
    }

    /// Scale a field (pause-menu) cast of pack spell menu id `menu_id` by
    /// roster slot `roster`: its restore times the pack power at its level.
    /// Identity for any retail id.
    pub fn expansion_scale_field_cast(
        &self,
        roster: u8,
        menu_id: u8,
        outcome: &mut legaia_engine_battle::spells::SpellOutcome,
    ) {
        use legaia_engine_battle::spells::SpellOutcome as O;
        let Some(s) = self.expansion_pack_spell(menu_id) else {
            return;
        };
        let level = self
            .pack_spell_progress()
            .level(roster, &s.key)
            .map_or(1, |l| l.level);
        let scale = |a: &mut u16| *a = xs::scale_magnitude(*a, &s.def, level);
        match outcome {
            O::Heal { amount, .. } | O::Damage { amount, .. } => scale(amount),
            O::MultiHeal { targets } | O::MultiDamage { targets, .. } => {
                for (_, a) in targets.iter_mut() {
                    scale(a);
                }
            }
            _ => {}
        }
    }

    /// End the pack cast in flight.
    pub(in crate::world) fn expansion_end_pack_cast(&mut self) {
        self.expansion.spells.casting = None;
    }

    /// Seru capture of pack monsters: each captured monster id the fight
    /// seats as a pack monster that some pack spell's `learn.capture` names
    /// teaches that spell to every present member who may learn it (and
    /// whose conditions hold), and is taken out of `captures` so the retail
    /// registry does not also bank it. Returns the captures left.
    pub(in crate::world) fn expansion_capture_spells(&mut self, captures: Vec<u16>) -> Vec<u16> {
        let Some(spells) = self.expansion.spells.compiled.clone() else {
            return captures;
        };
        let Some(pack) = self.expansion.pack.clone() else {
            return captures;
        };
        let variants = self
            .battle
            .active_formation
            .as_ref()
            .and_then(|f| {
                self.expansion
                    .battle
                    .compiled
                    .as_ref()?
                    .formation(f.formation_id)
                    .map(|cf| cf.variants.clone())
            })
            .unwrap_or_default();
        let present: Vec<u8> = (0..self.party.party_count.clamp(1, 3))
            .map(|i| self.party_roster_slot(i as usize) as u8)
            .collect();
        let mut left = Vec::new();
        for id in captures {
            let Some(monster) = variants.get(&id) else {
                left.push(id);
                continue;
            };
            let taught: Vec<CompiledSpell> = spells
                .taught_by_capture(&pack.manifest.id, monster)
                .cloned()
                .collect();
            if taught.is_empty() {
                left.push(id);
                continue;
            }
            for s in &taught {
                for &roster in &present {
                    let Some(ch) = xs::character_of(roster) else {
                        continue;
                    };
                    if !s.def.learnable_by(ch) || !self.learn_holds(roster, &s.learn().when()) {
                        continue;
                    }
                    if self.expansion.quests.progress.spells.learn(roster, &s.key) {
                        self.expansion.toasts.push(format!(
                            "{} learned {}!",
                            HERO_NAMES[usize::from(roster)],
                            s.name()
                        ));
                    }
                }
            }
            log::info!("expansion: captured pack monster `{monster}` (base {id})");
        }
        left
    }
}
