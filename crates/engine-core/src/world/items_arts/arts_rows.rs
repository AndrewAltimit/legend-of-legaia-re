//! The battle Arts list: per-caster rows, the Miracle and Super Art strike
//! profiles, the chain library and the art-used notification.

use super::*;

impl World {
    /// Resolve a party slot to the [`legaia_art::Character`] whose art tables
    /// apply. Party slots 0/1/2 are Vahn/Noa/Gala; out-of-range slots (story
    /// guests, monsters) fall back to Vahn so the lookup never panics.
    pub fn caster_character(&self, slot: u8) -> legaia_art::Character {
        legaia_engine_battle::battle_arts::character_for_slot(slot)
    }

    /// Build the Arts submenu rows for `caster` from their saved chains. For
    /// each chain, the longest staged art record whose command string the
    /// chain ends with ([`legaia_engine_battle::battle_arts::chain_matches_record`]) supplies
    /// the real power profile; chains with no matching record fall back to a
    /// synthetic profile derived from the directional commands.
    pub fn build_battle_arts_rows(
        &self,
        caster: u8,
    ) -> Vec<legaia_engine_battle::battle_arts::ArtRow> {
        use legaia_engine_battle::battle_arts::{
            ArtRow, chain_matches_record, miracle_for_chain, power_from_record, super_for_chain,
            synthetic_power,
        };
        // `caster` is the battle ordinal; chains + art catalog belong to the
        // occupying CHARACTER (roster slot per the present-party composition).
        let char_slot = self.party_roster_slot(caster as usize) as u8;
        let character = self.caster_character(char_slot);
        self.party
            .saved_chains
            .iter()
            .filter(|c| c.char_slot == char_slot)
            .map(|c| {
                // Miracle Arts win over a plain art-record match: a chain whose
                // directional string is the caster's Miracle Art replaces the
                // whole queue with the finisher sequence (the retail order:
                // Miracle replacement runs before any tail Super expansion).
                if let Some(miracle) = miracle_for_chain(character, &c.sequence) {
                    let (power, enemy_effect) = self.miracle_strike_profile(character, miracle);
                    // Shout key: the finisher constant of the resolved
                    // Miracle queue (the last art the materialiser runs).
                    let action = legaia_engine_battle_vm::battle_action::resolve_action_queue(
                        character,
                        miracle.commands,
                        &[],
                    )
                    .actions()
                    .iter()
                    .rev()
                    .copied()
                    .find(|a| a.is_art());
                    return ArtRow {
                        name: c.name.clone(),
                        sequence: c.sequence.clone(),
                        power,
                        enemy_effect,
                        miracle: Some(miracle.name),
                        super_art: None,
                        action,
                    };
                }
                // Super Arts next (after Miracle, matching the retail order):
                // recognize the chain's named-art sequence from the caster's
                // art catalog and tail-match it against the caster's Super art
                // sequences (connectors abstracted - see `super_for_chain`).
                let caster_records = || {
                    self.tables
                        .art_records
                        .iter()
                        .filter(|((ch, _), _)| *ch == character)
                        .map(|(_, rec)| rec)
                };
                if let Some(sa) = super_for_chain(character, &c.sequence, caster_records()) {
                    let (power, enemy_effect) = self.super_strike_profile(character, sa);
                    // Shout key: the Super's finisher constant (last art of
                    // the replacement queue).
                    let action = sa
                        .replace
                        .iter()
                        .rev()
                        .filter_map(|&b| legaia_art::ActionConstant::from_byte(b))
                        .find(|a| a.is_art());
                    return ArtRow {
                        name: c.name.clone(),
                        sequence: c.sequence.clone(),
                        power,
                        enemy_effect,
                        miracle: None,
                        super_art: Some(sa.name),
                        action,
                    };
                }
                let best = self
                    .tables
                    .art_records
                    .iter()
                    .filter(|((ch, _), _)| *ch == character)
                    .filter(|(_, rec)| chain_matches_record(&c.sequence, rec))
                    .max_by_key(|(_, rec)| rec.commands.len());
                match best {
                    Some(((_, action), rec)) => {
                        let (power, enemy_effect) = power_from_record(rec);
                        ArtRow {
                            name: c.name.clone(),
                            sequence: c.sequence.clone(),
                            power,
                            enemy_effect,
                            miracle: None,
                            super_art: None,
                            // Shout key: the matched record's own constant.
                            action: Some(*action),
                        }
                    }
                    None => ArtRow {
                        name: c.name.clone(),
                        sequence: c.sequence.clone(),
                        power: synthetic_power(&c.sequence),
                        enemy_effect: legaia_art::EnemyEffect::None,
                        miracle: None,
                        super_art: None,
                        // No matched record: the art executes silent.
                        action: None,
                    },
                }
            })
            .collect()
    }

    /// Resolve a Miracle Art's per-strike power profile from its
    /// finisher-replacement queue. Runs the canonical command resolution
    /// ([`legaia_engine_battle_vm::battle_action::resolve_action_queue`]), which
    /// replaces the directional input with the Miracle's component-art queue,
    /// then turns each art constant in that queue into strikes:
    ///
    /// - if the `(character, art)` record is staged ([`Self::set_art_record`]),
    ///   the art contributes its real damage power bytes + status effect;
    /// - otherwise it contributes one tier-0 (`x12`) synthetic strike, the same
    ///   graceful-degradation profile [`legaia_engine_battle::battle_arts::synthetic_power`]
    ///   uses when no disc art data is loaded.
    ///
    /// The first staged component art's status effect is adopted for the whole
    /// finisher. Result is clamped to [`legaia_engine_battle::battle_arts::MAX_ART_HITS`] and
    /// floored at one strike.
    pub(in crate::world) fn miracle_strike_profile(
        &self,
        character: legaia_art::Character,
        miracle: &legaia_art::MiracleArt,
    ) -> (Vec<legaia_art::power::PowerByte>, legaia_art::EnemyEffect) {
        let queue = legaia_engine_battle_vm::battle_action::resolve_action_queue(
            character,
            miracle.commands,
            &[],
        );
        self.art_actions_strike_profile(character, queue.actions().iter().copied())
    }

    /// Resolve a **Super Art**'s per-strike power profile from its
    /// finisher-replacement queue ([`legaia_art::SuperArt::replace`]). The
    /// replacement keeps the leading component arts and ends in the Super
    /// finisher constant(s) (e.g. Tri-Somersault → `… 1A 2B 2B 2B`), so each art
    /// constant in it contributes a strike via the shared resolver
    /// ([`Self::art_actions_strike_profile`]) - real [`ArtRecord`] power where the
    /// `(character, art)` record is staged, else a tier-0 synthetic strike.
    ///
    /// [`ArtRecord`]: legaia_art::ArtRecord
    pub(in crate::world) fn super_strike_profile(
        &self,
        character: legaia_art::Character,
        sa: &legaia_art::SuperArt,
    ) -> (Vec<legaia_art::power::PowerByte>, legaia_art::EnemyEffect) {
        let actions = sa
            .replace
            .iter()
            .filter_map(|&b| legaia_art::ActionConstant::from_byte(b));
        self.art_actions_strike_profile(character, actions)
    }

    /// Turn a queue of [`ActionConstant`](legaia_art::ActionConstant)s into a
    /// per-strike power profile: each art constant resolves to its staged
    /// [`ArtRecord`](legaia_art::ArtRecord) power bytes + status effect, or one
    /// tier-0 (`x12`) synthetic strike when that art's record isn't loaded (the
    /// graceful-degradation fallback the no-disc-data path uses). The first
    /// staged status effect is adopted for the whole finisher; the result is
    /// clamped to [`legaia_engine_battle::battle_arts::MAX_ART_HITS`] and floored at one strike.
    /// Shared by the Miracle and Super finisher resolvers.
    fn art_actions_strike_profile(
        &self,
        character: legaia_art::Character,
        actions: impl Iterator<Item = legaia_art::ActionConstant>,
    ) -> (Vec<legaia_art::power::PowerByte>, legaia_art::EnemyEffect) {
        use legaia_art::power::PowerByte;
        use legaia_engine_battle::battle_arts::MAX_ART_HITS;
        // Synthetic UDF x12 - the tier-0 high strike a component art with no
        // staged record degrades to.
        const SYNTH_UDF_X12: u8 = 0x16;

        let mut power: Vec<PowerByte> = Vec::new();
        let mut enemy_effect = legaia_art::EnemyEffect::None;
        for action in actions {
            if !action.is_art() {
                continue;
            }
            match self.tables.art_records.get(&(character, action)) {
                Some(rec) => {
                    let (mut bytes, effect) =
                        legaia_engine_battle::battle_arts::power_from_record(rec);
                    if enemy_effect == legaia_art::EnemyEffect::None {
                        enemy_effect = effect;
                    }
                    power.append(&mut bytes);
                }
                None => power.push(PowerByte::from_byte(SYNTH_UDF_X12)),
            }
            if power.len() >= MAX_ART_HITS as usize {
                break;
            }
        }
        power.truncate(MAX_ART_HITS as usize);
        if power.is_empty() {
            power.push(PowerByte::from_byte(SYNTH_UDF_X12));
        }
        (power, enemy_effect)
    }

    /// Pull the cross-character saved-chain library out as a
    /// [`legaia_engine_battle::tactical_arts_editor::ChainLibrary`] - what the field menu's
    /// Tactical Arts editor browses + edits. The editor mutates the returned
    /// library; the engine writes the result back with
    /// [`Self::store_chain_library`] so the edit reaches the next battle's
    /// Arts menu (via `Self::build_battle_arts_rows`) and the next save
    /// (via [`legaia_engine_menus::party_state::PartyState::saved_chains`]).
    pub fn chain_library(&self) -> legaia_engine_battle::tactical_arts_editor::ChainLibrary {
        legaia_engine_battle::tactical_arts_editor::ChainLibrary::from_records(
            &self.party.saved_chains,
        )
    }

    /// Write an edited [`legaia_engine_battle::tactical_arts_editor::ChainLibrary`] back into
    /// [`legaia_engine_menus::party_state::PartyState::saved_chains`], replacing the whole library. This is the bridge
    /// that closes the loop the field menu opens with [`Self::chain_library`]:
    /// once stored, a chain composed in the editor is selectable in battle and
    /// persists across `save_full` / `load_full`.
    pub fn store_chain_library(
        &mut self,
        lib: &legaia_engine_battle::tactical_arts_editor::ChainLibrary,
    ) {
        self.party.saved_chains = lib.to_records();
    }

    /// Record one use of `art_id` by `char_id` (roster index).
    ///
    /// Delegates to [`TacticalArtsTracker::notify_art_used`](legaia_engine_battle::tactical_arts::TacticalArtsTracker::notify_art_used). When the use
    /// count first crosses the learn threshold, this method:
    ///
    /// 1. Pushes [`BattleEvent::TacticalArtLearned`] onto
    ///    [`Self::pending_battle_events`].
    /// 2. Sets [`legaia_engine_menus::party_state::PartyState::current_art_banner`] with a 2-second display window
    ///    so the engine's HUD overlay can show "Learned Art #N!".
    ///
    /// Subsequent calls for the same `(char_id, art_id)` pair are no-ops.
    pub fn notify_art_used(&mut self, char_id: u8, art_id: u8) {
        if let Some(ev) = self.party.tactical_arts.notify_art_used(char_id, art_id) {
            let text = format!("Learned {}!", ev.name);
            self.party.current_art_banner = Some(ArtLearnedBanner {
                text,
                frames_remaining: ArtLearnedBanner::DEFAULT_FRAMES,
            });
            self.pending_battle_events
                .push(BattleEvent::TacticalArtLearned {
                    char_id: ev.char_id,
                    art_id: ev.art_id,
                });
        }
    }
}
