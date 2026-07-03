use super::ByteDelta;

/// 168 KB battle-bundle residency window: the field-scene payload
/// is overwritten here when battle scene-init runs. Computed from
/// the captured `mednafen-state diff` extent.
pub const BATTLE_BUNDLE_WINDOW: (u32, u32) = (0x80124690, 0x801503C4);

/// 16 KB battle-overlay scratch slice. Resets on battle entry;
/// distinct from the broader battle-action overlay residency at
/// `0x801CE800..0x801F4000`.
pub const OVERLAY_SCRATCH_WINDOW: (u32, u32) = (0x801CE808, 0x801D3018);

/// 8-slot battle actor pointer table; populated post-trigger.
pub const ACTOR_POOL_BASE: u32 = 0x801C9370;
/// Stride between adjacent actor-pointer slots (header bytes).
pub const ACTOR_POOL_SLOT_STRIDE: u32 = 0x60;
/// 8 slots: 0..2 party, 3..7 monsters (per the existing battle
/// pointer-table doc).
pub const ACTOR_POOL_SLOT_COUNT: u32 = 8;

/// Bundle-pool extension that picks up the per-frame actor tick
/// pointer when battle scene-init completes.
pub const BUNDLE_POOL_EXTENSION_BASE: u32 = 0x80083680;
/// Address inside the extension where the per-frame actor tick
/// pointer (`FUN_80021DF4 = 0x80021DF4`) lands once battle is up.
/// The slot holds a non-battle handler (`0x80024C50`) before
/// scene-init runs.
pub const ACTOR_TICK_FN_PTR_ADDR: u32 = 0x800836C8;
/// Expected value once battle scene-init completes.
pub const ACTOR_TICK_FN_PTR_VALUE: u32 = 0x80021DF4;

/// CD I/O state slice that re-wires while the battle bundle is
/// paged in. A non-zero diff over this window plus a stable
/// scene-name table is a reliable "battle scene-init in flight"
/// signature.
pub const CD_IO_STATE_WINDOW: (u32, u32) = (0x801FFCA0, 0x801FFFFE);

/// Formation cell address. Pre/post deltas: `00 00 00 00` →
/// `04 04 00 00` for the captured pair.
pub const FORMATION_CELL_ADDR: u32 = 0x8007BD0C;

/// Encounter delta against the captured pair (count=0 → count=2,
/// monster id 4 in slots 0..1). Independent of which encounter
/// the user captured - if a different formation is captured this
/// constant becomes documentation rather than an assertion.
pub const FORMATION_CELL_DELTAS: [ByteDelta; 2] = [
    ByteDelta {
        addr: FORMATION_CELL_ADDR,
        before: 0x00,
        after: 0x04,
    },
    ByteDelta {
        addr: FORMATION_CELL_ADDR + 1,
        before: 0x00,
        after: 0x04,
    },
];
