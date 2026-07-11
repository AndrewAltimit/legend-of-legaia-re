//! Inn UI state - rest confirmation and party HP / MP restore.
//!
//! `InnSession` is installed on [`crate::menu_runtime::MenuRuntime`] by
//! `open_inn` before the menu VM enters `InnConfirm`. On confirmation the
//! runtime deducts gold and restores every active party member's HP and MP to
//! their current maximums via the world's live `BattleActor` mirrors.
//!
//! Retail has no inn cost table: each cost is a field-VM script literal in
//! the scene MAN (op-`0x4E` gold gate + `0x3A` debit pair - see
//! `docs/subsystems/inn.md`). The scene host scans those pairs at load
//! (`SceneHost::scene_inn_cost`, via [`legaia_asset::inn_costs`]) and
//! `MenuRuntime::open_scene_inn` opens the prompt with the scanned cost;
//! `open_inn(cost)` remains the direct entry for tests and tooling.

/// Mutable session state for an open inn interaction.
#[derive(Debug, Clone)]
pub struct InnSession {
    /// Gold cost per stay.
    pub cost: u32,
}

impl InnSession {
    pub fn new(cost: u32) -> Self {
        Self { cost }
    }

    /// `true` when the player can afford the rest.
    pub fn can_afford(&self, world_money: i32) -> bool {
        world_money >= self.cost as i32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn can_afford_exact_amount() {
        let s = InnSession::new(200);
        assert!(s.can_afford(200));
    }

    #[test]
    fn cannot_afford_one_short() {
        let s = InnSession::new(200);
        assert!(!s.can_afford(199));
    }

    #[test]
    fn free_inn_always_affordable() {
        let s = InnSession::new(0);
        assert!(s.can_afford(0));
    }
}
