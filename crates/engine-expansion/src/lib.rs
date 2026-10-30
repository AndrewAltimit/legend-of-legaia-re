//! The `World`-free runtime of an expansion pack: what a loaded
//! [`legaia_expansion_format::ExpansionPack`] does to battles, progression
//! and quests, as kernels over plain state. `engine-core` holds one
//! [`ExpansionState`] on the `World` (`World::expansion`) and calls these
//! kernels from its scene-entry, encounter and battle-spoils steps.
//!
//! With no pack loaded every kernel is the identity, so a world without a
//! pack plays exactly as before.

#![forbid(unsafe_code)]

pub mod battle;
pub mod battle_waves;
pub mod boss_script;
pub mod cards;
pub mod cutscene;
pub mod dialog;
pub mod entrance;
pub mod flags;
pub mod item_screen;
pub mod items;
pub mod journal;
pub mod map;
pub mod map_detail;
pub mod mode_stage;
pub mod modes;
pub mod progression;
pub mod quest;
pub mod quest_challenge;
pub mod quest_save;
pub mod toast;

use std::sync::Arc;

use legaia_expansion_format::ExpansionPack;

/// The `World`'s expansion seat.
#[derive(Debug, Clone, Default)]
pub struct ExpansionState {
    /// The active pack, if any.
    pub pack: Option<Arc<ExpansionPack>>,
    /// The pack's compiled battle tables and per-fight state.
    pub battle: battle::BattleRuntime,
    /// The pack's progression state.
    pub progression: progression::ProgressionRuntime,
    /// Quest, challenge, pack-flag and pack-dialogue state
    /// ([`quest::QuestRuntime`]); what the save's expansion block carries.
    pub quests: quest::QuestRuntime,
    /// The pack map the player is in, if any.
    pub map: Option<map::ActiveMap>,
    /// The game-mode seat while a mode screen owns the frame
    /// (`SceneMode::Expansion`).
    pub mode_seat: Option<Box<modes::PackModeSeat>>,
    /// Card grants won in modes, waiting for the card economy.
    pub pending_card_rewards: Vec<modes::PendingCardReward>,
    /// The card economy's notices, screen and portraits; the cards
    /// themselves are `quests.progress.cards`.
    pub cards: cards::CardRuntime,
    /// The retail-scene entrances into the pack's maps
    /// ([`entrance::EntranceWatch`]).
    pub entrances: entrance::EntranceWatch,
    /// A map the pause menu's Travel page picked, `(map id, spawn)`, for the
    /// scene host to enter on its next tick.
    pub travel: Option<(String, String)>,
    /// The Journal's Cards page asked for the card menu, opened once the
    /// pause menu has closed and the player walks again.
    pub open_cards: bool,
    /// The Journal's Items page asked for the pack item screen (`Some(gear
    /// tab)`), opened once the pause menu has closed.
    pub open_items: Option<bool>,
    /// Quest toasts for the hosts to show ([`toast::ToastQueue`]).
    pub toasts: toast::ToastQueue,
    /// The pack's compiled items, this fight's item aliases and the item
    /// screen; the items held are `quests.progress.items`.
    pub items: items::ItemRuntime,
    /// The cutscene in play and the ones queued
    /// ([`cutscene::CutsceneRuntime`]).
    pub cutscene: cutscene::CutsceneRuntime,
}

impl ExpansionState {
    /// Whether a pack is active.
    pub fn is_active(&self) -> bool {
        self.pack.is_some()
    }
}
