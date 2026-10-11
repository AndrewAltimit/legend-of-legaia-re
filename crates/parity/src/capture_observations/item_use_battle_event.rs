//! Capture observation: in-battle item-use event (field-pack base + actor-pool shifts).

/// Field-pack base pointer cell. Flips between pre / post saves
/// when the item-use sub-mode reseats the active scene buffer.
pub const FIELD_PACK_BASE_PTR_ADDR: u32 = 0x8007B8D0;

/// Pre-event value (battle-init residency).
pub const FIELD_PACK_BASE_PTR_PRE: u32 = 0x8014BD30;
/// Post-event value (item-use residency).
pub const FIELD_PACK_BASE_PTR_POST: u32 = 0x800ABA4C;

/// Script-VM context block window. ~660 bytes shift across the
/// pair as the menu / item / target / commit pipeline runs.
pub const SCRIPT_VM_CTX_WINDOW: (u32, u32) = (0x801BA7DC, 0x801BADEC);

/// 8-slot battle actor pool. In the count-2 formation, slots 0..4
/// are populated (3 party + 2 monsters); slots 5..7 are zero in
/// both saves and remain zero across the pair.
pub const ACTOR_POOL_BASE: u32 = 0x801C9370;

/// Number of active actor slots in the count-2 formation.
pub const ACTIVE_SLOTS: u32 = 5;
/// Total slots; trailing entries are zero-armed.
pub const TOTAL_SLOTS: u32 = 8;
