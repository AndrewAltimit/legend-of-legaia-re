/// Battle overlay residency window (post-trigger). The pre/post diff
/// surfaces 133 KB of changed bytes inside this range, with no changes
/// outside it (within the wider `0x801C0000..0x80200000` overlay
/// region after stripping the actor-pool / scene-bundle deltas).
pub const OVERLAY_WINDOW: (u32, u32) = (0x801CE800, 0x801F4000);

/// 8-slot battle actor pointer table; populated post-trigger. Each
/// slot is a `0x60`-byte header (the lower bits of `start_addr` align
/// to the stride) carrying actor pointer + control word at offset 0.
pub const ACTOR_POOL_WINDOW: (u32, u32) = (0x801C9370, 0x801C9900);

/// Active scene-name table. Encounter trigger does NOT change this -
/// the scene index stays equal to the field scene that triggered.
pub const SCENE_NAME_TABLE_ADDR: u32 = 0x80084540;

/// Approximate byte-count change in the overlay window between an
/// equivalent pre-encounter / post-encounter save pair. Used for
/// scoping assertions; tolerate ±10% drift across captures.
pub const OVERLAY_BYTES_CHANGED_REF: usize = 133_086;

/// Approximate byte-count change in the actor-pool window between an
/// equivalent pre-encounter / post-encounter save pair. Captured from
/// the wider `0x801C9300..0x801CA000` window; the narrower
/// `ACTOR_POOL_WINDOW` captures a subset.
pub const ACTOR_POOL_BYTES_CHANGED_REF: usize = 200;

/// Slot stride between adjacent battle-actor pool entries.
pub const ACTOR_POOL_SLOT_STRIDE: u32 = 0x60;

/// Number of slots in the battle-actor pointer table.
pub const ACTOR_POOL_SLOT_COUNT: usize = 8;
