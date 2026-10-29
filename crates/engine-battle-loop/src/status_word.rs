//! Status-word reads over the battle's status tracker: the raw word a
//! seat carries and the blocks it imposes (petrify, confusion, the act and
//! magic gates, Mystic Shield).

use crate::battle_state::BattleState;

impl BattleState {
    /// True if `slot` carries any status that blocks all actions (Sleep /
    /// Stone / Faint), so it loses its turn. The blocking set is defined
    /// by [`legaia_engine_battle_vm::status_effects::StatusKind::blocks_actions`]; the
    /// battle turn loop (`World::advance_battle_mode`) enforces it here.
    pub fn actor_blocked_from_acting(&self, slot: u8) -> bool {
        self.status_effects
            .statuses(slot)
            .iter()
            .any(|s| s.kind.blocks_actions())
    }

    /// True if `slot` carries any status that blocks magic (Curse /
    /// Faint). A blocked caster falls back to a physical strike rather
    /// than casting.
    pub fn actor_blocked_from_magic(&self, slot: u8) -> bool {
        self.status_effects
            .statuses(slot)
            .iter()
            .any(|s| s.kind.blocks_magic())
    }

    /// True if `slot` is petrified (Stone). A petrified actor can't be damaged
    /// (the wiki: it is "no longer able to be damaged") and counts as defeated.
    pub fn actor_is_petrified(&self, slot: u8) -> bool {
        self.status_effects
            .statuses(slot)
            .iter()
            .any(|s| s.kind == legaia_engine_battle_vm::status_effects::StatusKind::Stone)
    }

    /// Whether Cort's **Mystic Shield** is up - retail's `_DAT_8007BD84`,
    /// the effect handle PROT 0940's `0xAC` arm 4 stores (`0x801F7678`) and
    /// the engine mirrors as [`legaia_engine_battle::monster_ai::MonsterAiState::flag_bd84`].
    /// While it is non-null the damage finisher halves every hit on an enemy
    /// defender (`FUN_801DDB30`, `0x801DDB98..0x801DDBC8`).
    pub fn mystic_shield_up(&self) -> bool {
        self.monster_ai_state.flag_bd84 != 0
    }

    /// True if `slot` carries the Confuse status.
    pub fn actor_is_confused(&self, slot: u8) -> bool {
        self.status_effects
            .statuses(slot)
            .iter()
            .any(|s| s.kind == legaia_engine_battle_vm::status_effects::StatusKind::Confuse)
    }
}
