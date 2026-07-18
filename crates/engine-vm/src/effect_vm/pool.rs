//! Effect-VM slot pool: constants, master/child slot layout, script header,
//! and the [`Pool`] runtime (`FUN_801DE914` init, `FUN_801DFDF8` spawn,
//! `FUN_801E0088` per-frame walker). Split out of `effect_vm.rs`.
//!
//! The faithful per-frame algebra lives in [`Pool::tick_retail`] (pass 1:
//! master spawn cadence + child anim/motion) and [`Pool::child_billboards`]
//! (pass 2: brightness envelope + UV mirror + sprite sizing). The older
//! host-delegating [`Pool::tick`] is kept as a compatibility shim for engines
//! that model the lifecycle through [`EffectHost::advance_state`].

use super::*;
// The 4096-entry 4.12 trig tables the walker reaches through the pointers
// `_DAT_8007B81C` (sine) / `_DAT_8007B7F8` (cosine) are the same SCUS tables
// the slot-machine reel renderer reads; reuse the reproduced lookups.
// REF: FUN_801d0fa8 (independent consumer that pinned the two tables)
use legaia_asset::minigame_slot_scene::{cos_4096, sin_4096};

/// Default per-effect lifetime in frames, for hosts that model the effect
/// lifecycle as a fixed countdown through [`EffectHost::advance_state`]
/// instead of running the faithful walker.
///
/// Superseded stand-in: the retail cadence (spawn-record delays + pack0 frame
/// delays) is fully ported in [`Pool::tick_retail`]; this budget exists only
/// for the legacy [`Pool::tick`] host path.
pub const DEFAULT_EFFECT_LIFETIME_FRAMES: u32 = 30;

/// Maximum simultaneous effects (master slots).
pub const MAX_MASTER_SLOTS: usize = 32;

/// Maximum simultaneous child sprites pooled across all effects.
pub const MAX_CHILD_SLOTS: usize = 128;

/// Width of one master slot in retail RAM bytes (`+0x1010` stride).
pub const MASTER_SLOT_BYTES: usize = 28;

/// Width of one child slot in retail RAM bytes (`+0x10` stride).
pub const CHILD_SLOT_BYTES: usize = 32;

/// Neutral (maximum) pass-2 brightness modulation - `0x80` is "texel * 1.0".
pub const BRIGHTNESS_NEUTRAL: u8 = 0x80;

/// Per-effect-instance state. Retail layout: 28 bytes at `_DAT_8007BD30 +
/// 0x1010 + slot * 28`. Field names match the byte offsets the retail walker
/// reads/writes; the doc lists the canonical mapping.
///
/// `active == 0` means the slot is empty. The spawn API allocates by linear
/// scan for the first slot where `active == 0`.
#[derive(Debug, Clone, Default)]
pub struct MasterSlot {
    /// `+0x00` - total spawn records for this effect (copied from
    /// `pack1_record[0]`). Doubles as the active flag: zero = unused slot.
    pub child_count: u8,
    /// `+0x01` - flags byte. Bit `0x01` = "randomize the planar spawn
    /// offsets" (consumed at spawn time, when [`Pool::spawn`] rewrites the
    /// per-record offset legs). Copied from `pack1_record[1]`.
    pub flags: u8,
    /// `+0x02` - spawn cursor: spawn records consumed so far. When it
    /// reaches `child_count` the master frees itself.
    pub spawn_cursor: u8,
    /// `+0x03` - the 5.3 fixed-point **wait counter** (not an opcode).
    /// Non-zero: the walker decrements it by 8 (values `< 8` clamp to 0) and
    /// skips the slot. Zero: the spawn loop runs.
    pub state: u8,
    /// `+0x04` - angle (12-bit, masked `& 0xFFF` from the spawn arg).
    pub angle: u16,
    /// `+0x06` - short padding / reserved (one slot).
    pub field_06: u16,
    /// `+0x08` - world X. Spawn writes `(caller_x as i16) << 8` (16.8 fixed).
    pub pos_x: i32,
    /// `+0x0C` - world Y. Same encoding as `pos_x`.
    pub pos_y: i32,
    /// `+0x10` - world Z. Same encoding as `pos_x`.
    pub pos_z: i32,
    /// `+0x14` - dead lane in retail: never written by the spawn API or the
    /// walker; its copy into `child[+0x18]` at seed time carries stale bytes.
    /// The port repurposes it as an **age counter** ([`Pool::tick_retail`]
    /// bumps it once per call per active master) so age-based render fades
    /// keep working; this write is port-side, not retail behaviour.
    pub field_14: i32,
    /// `+0x18` - pointer into the script body. Spawn writes
    /// `pack1_record_offset + 4` (skips the 4-byte header); the walker
    /// advances it by 14 per record consumed. The port keeps it as a byte
    /// offset in lockstep with `spawn_cursor` (`spawn_cursor * 14`); record
    /// resolution goes through the [`EffectCatalog`] instead.
    pub script_offset: u32,

    // --- engine-side render aids (not part of the 28-byte retail slot) ---
    /// The effect id this slot was spawned with, so the walker + render
    /// snapshots can resolve the effect's spawn records and animations back
    /// through the [`EffectCatalog`]. Retail re-reads these from the live
    /// script pointer; the port keeps the id since it stores the catalog
    /// separately.
    pub ui_id: u8,
    /// Per-record randomized planar offsets (world units) for the
    /// random-distribution path (`flags & 0x01`). Retail scribbles these
    /// back into the resident script buffer (`record[+2]` / `record[+6]`);
    /// the port stores them here and [`Pool::tick_retail`] reads them in
    /// place of the record's offset legs. Empty when flags bit 0 is clear.
    pub child_offsets: Vec<(i16, i16)>,
    /// Optional index into the host's global TMD pool (`etmd.dat`) for the
    /// effect's 3D model. `Some` for model-driven effects (the flame mesh of a
    /// spell like *Tail Fire* - an `etmd` TMD textured by `etim`); `None` for
    /// the 2D-billboard-only effects. Not part of the 28-byte retail slot: the
    /// production effect-id -> etmd-model selection is driven by the move/art
    /// VM and not yet decoded, so this is currently set only by the host's
    /// model-spawn helper.
    pub model_index: Option<usize>,
}

/// Per-sprite render state. Retail layout: 32 bytes per slot at
/// `_DAT_8007BD30 + 0x10 + slot * 32`. The walker maintains 128 of these
/// (at most ~4 per active effect on average). Field names map the byte
/// offsets the walker reads/writes (`overlay_battle_801e0088.txt`).
#[derive(Debug, Clone, Default)]
pub struct ChildSlot {
    /// `+0x00` - pack0 frame count (batch header byte 0). Doubles as the
    /// active flag: zero = free slot.
    pub frame_count: u8,
    /// `+0x01` - random UV-mirror bits, seeded `rand() % 4` (low byte of the
    /// C remainder). Bit 0 = horizontal corner order, bit 1 = vertical;
    /// consumed by pass 2 ([`Pool::child_billboards`]).
    pub mirror: u8,
    /// `+0x02` - animation frame cursor (frames consumed so far). The
    /// current frame of the child's pack0 batch is `frames[frame_cursor]`.
    pub frame_cursor: u8,
    /// `+0x03` - the 5.3 fixed-point wait counter (current frame's hold
    /// delay `<< 3`, byte-truncated).
    pub wait: u8,
    /// `+0x04 / +0x06 / +0x08` - velocity (i16 x/y/z). The planar legs are
    /// the spawn record's legs rotated by the master angle (`>> 12`); the
    /// vertical component is copied direct.
    pub velocity: [i16; 3],
    /// `+0x0C / +0x10 / +0x14` - world position, 16.8 fixed. Seeded from the
    /// master origin (`y -= height << 8`, x/z offset by the rotated planar
    /// legs `>> 4`), then integrated by `vel * frame.speed * motion_scale`.
    pub pos: [i32; 3],
    /// Port-side stand-in for the retail anim-cursor pointer at `+0x1C`
    /// (which walks the pack0 entry by raw 6-byte strides): the pack0 batch
    /// index, resolved through the [`EffectCatalog`] together with
    /// `frame_cursor`.
    pub anim_id: u16,
}

/// Per-effect-id script record (the unit `pack1[i]` resolves to). Lives
/// in the on-disc effect bundle; spawn copies a few bytes out into a
/// freshly-allocated master slot. The bundle's parser ([`crates/asset`])
/// is responsible for handing the bytes here.
#[derive(Debug, Clone, Default)]
pub struct EffectScript {
    /// Header byte 0: total spawn records (children spawned over the
    /// effect's life).
    pub child_count: u8,
    /// Header byte 1: flags (bit 0 = randomize the planar spawn offsets).
    pub flags: u8,
    /// Header i16 at +2: half-range for the random offset rewrite. The
    /// retail spawn uses `rand() % (2 * spread) - spread`.
    pub spread: u16,
    /// Variable-length body following the 4-byte header. Spawn never reads
    /// it directly; the per-frame walker indexes into it via
    /// `script_offset`. We store the bytes so the host can peek without
    /// going through the asset cache.
    pub body: Vec<u8>,
}

/// One 14-byte pack1 spawn record (`pack1_entry[+0x4 + child_idx * 14]`).
/// Consumed once by the retail walker (`FUN_801E0088` pass 1) when its child
/// spawns; the field roles below are pinned from the spawn block
/// `0x801E0184..0x801E03F0`. [`Pool::spawn`] additionally rewrites the two
/// planar offset fields with random values when the script's `flags & 0x01`
/// bit is set (retail scribbles them back into the resident script buffer;
/// the port stores the rewrites in [`MasterSlot::child_offsets`]).
#[derive(Debug, Clone, Copy, Default)]
pub struct ChildSprite {
    /// `+0x00` - pack0 anim-batch index (a single byte in retail; kept as
    /// `u16` for the existing consumer surface). Selects the child's sprite
    /// animation - the batch's frame count seeds the child slot's lifetime.
    pub sprite_id: u16,
    /// `+0x01` - frames the master waits after this spawn before consuming
    /// the next record (`<<3` into the master's 5.3 wait counter).
    pub delay: u8,
    /// `+0x02` - planar spawn offset, leg A (rotated into world space by the
    /// master angle, `>>4`). Rewritten with `rand % (2*spread) - spread`
    /// when the script's flags bit 0 is set.
    pub width: i16,
    /// `+0x04` - vertical spawn offset (`<<8`, subtracted from Y).
    pub height: i16,
    /// `+0x06` - planar spawn offset, leg B (rotated, `>>4`). Randomized
    /// alongside `width` under flags bit 0.
    pub depth: i16,
    /// `+0x08 / +0x0A / +0x0C` - initial velocity `(leg A, vertical, leg B)`.
    /// The planar legs are rotated by the master angle (`>>12`); the
    /// vertical component is copied direct to the child slot.
    pub velocity: [i16; 3],
}

/// One pass-2 billboard: everything the retail sprite emit
/// (`FUN_801E0088` pass 2, ~`0x801E07A8..0x801E09A8`) computes per live
/// child before the GTE projection. The renderer projects `pos`, sizes the
/// quad `world_w x world_h`, samples the atlas rect with the mirror-resolved
/// corner order, and modulates by `brightness` (`r = g = b`, `0x80` =
/// neutral).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChildBillboard {
    /// World position, integer units (the pool's 16.8 coordinates `>> 8`,
    /// truncated to i16 exactly as the retail projection input is).
    pub pos: [i16; 3],
    /// Brightness modulation `0..=0x80` from the ramp-in / ramp-out envelope
    /// ([`pass2_brightness`]). Written as `r = g = b` on the retail packet.
    pub brightness: u8,
    /// The current frame's sprite-atlas index (pack0 frame byte 0).
    pub atlas_index: u8,
    /// The resolved 8-byte atlas entry (texel rect + CLUT + tpage).
    pub entry: SpriteAtlasEntry,
    /// On-screen quad width before projection: `atlas.w * sprite_scale >> 8`
    /// (retail init scalar `0xA00` makes this x10 the texel size).
    pub world_w: i32,
    /// On-screen quad height before projection (same scaling as `world_w`).
    pub world_h: i32,
    /// Horizontal texel-corner swap. The retail emit gives the *base* U to
    /// the right-hand vertices when the child's mirror bit 0 is **clear**
    /// (`0x801E08C8`), so `mirror & 1 == 0` reads as flipped here.
    pub flip_h: bool,
    /// Vertical texel-corner swap - mirror bit 1 clear (`0x801E0944`).
    pub flip_v: bool,
}

/// 32-master / 128-child slot pool. Mirrors the 5008-byte block at
/// `_DAT_8007BD30` in retail. Engines own one of these per scene.
#[derive(Debug, Clone)]
pub struct Pool {
    /// `+0x00..0x10` - pool-head record set by [`Pool::init`].
    pub head: PoolHead,
    pub master_slots: [MasterSlot; MAX_MASTER_SLOTS],
    pub children: [ChildSlot; MAX_CHILD_SLOTS],
}

/// 16-byte pool-head record (`_DAT_8007BD30 + 0`). Retail layout:
/// `[i16 motion_scale][i16 sprite_scale][u32 atlas_base][u32 pack0_base]
/// [u32 pack1_base]` - the two init immediates followed by the three
/// (fixup-rebased) table pointers into the resident `efect.dat` buffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PoolHead {
    /// i16 at `+0`: the walker's global child **motion scale** - the first
    /// `FUN_801DE914` immediate (retail `0x1000`). A child motion step is
    /// `pos += vel * frame.speed * motion_scale * 8 >> 15`, which at the
    /// retail value reduces exactly to `pos += vel * frame.speed`.
    pub motion_scale: u16,
    /// i16 at `+2`: the pass-2 sprite **world scale** - the second init
    /// immediate (retail `0xA00`): quad size = `atlas w/h * sprite_scale >>
    /// 8` (x10 the texel size).
    pub sprite_scale: u16,
    /// u32 at `+4`: inline sprite-atlas base (8-byte entries at `buffer+8`).
    /// Opaque token in the port - atlas resolution goes through the
    /// [`EffectCatalog`].
    pub atlas_base: u32,
    /// u32 at `+8`: pack0 pointer-table base (frame-batch animations,
    /// indexed by a spawn record's `anim_batch * 4`). Opaque token.
    pub pack0_base: u32,
    /// u32 at `+0xC`: pack1 pointer-table base (effect-id scripts, indexed
    /// by `effect_id * 4` in the spawn API). Opaque token.
    pub pack1_base: u32,
}

impl Default for PoolHead {
    /// The retail init immediates (`FUN_800520F0` case `0xE` passes
    /// `(0x1000, 0xA00)` into `FUN_801DE914`), with zero table bases - the
    /// port resolves tables through the [`EffectCatalog`] instead.
    fn default() -> Self {
        Self {
            motion_scale: 0x1000,
            sprite_scale: 0x0A00,
            atlas_base: 0,
            pack0_base: 0,
            pack1_base: 0,
        }
    }
}

impl Default for Pool {
    fn default() -> Self {
        Self {
            head: PoolHead::default(),
            master_slots: std::array::from_fn(|_| MasterSlot::default()),
            children: std::array::from_fn(|_| ChildSlot::default()),
        }
    }
}

/// Pass-2 brightness envelope (`FUN_801E0088`, `0x801E06F4..0x801E07A4`).
///
/// With `n = frame_count >> 3` the modulation ramps in over the first eighth
/// of the animation (`0x80 * (frame_cursor + 1) / n`) then back out over the
/// rest (`0x80 * (frame_count - frame_cursor) / (frame_count - n)`), clamped
/// at `0x80` (neutral). Both divisions are unsigned; the subtraction wraps on
/// out-of-range cursors exactly as retail's u32 arithmetic does (the clamp
/// then floors the wrapped quotient at neutral).
///
/// PORT: FUN_801E0088 (pass-2 brightness envelope)
pub fn pass2_brightness(frame_count: u8, frame_cursor: u8) -> u8 {
    let count = frame_count as u32;
    let cursor = frame_cursor as u32;
    let n = count >> 3;
    let b = if cursor < n {
        // cursor < n implies n >= 1, so the divide is safe.
        ((cursor + 1) * 0x80) / n
    } else {
        let den = count.wrapping_sub(n);
        if den == 0 {
            // Only reachable for frame_count == 0 (an inactive slot, which
            // retail never feeds through pass 2 - its divide would trap).
            return BRIGHTNESS_NEUTRAL;
        }
        (count.wrapping_sub(cursor).wrapping_mul(0x80)) / den
    };
    b.min(BRIGHTNESS_NEUTRAL as u32) as u8
}

impl Pool {
    /// Construct an empty pool: every slot free, head at the retail init
    /// defaults (as if `FUN_801DE914` already ran with the retail
    /// immediates).
    pub fn new() -> Self {
        Self::default()
    }

    /// Port of `FUN_801DE914` - pack-fixup + pool init.
    ///
    /// Retail behavior: zeros the entire 5008-byte pool, then writes the
    /// `(motion_scale, sprite_scale)` immediates and the three pack pointers
    /// into the head record. The pack-fixup half (rebasing pack0 and pack1's
    /// offset tables to absolute addresses) is moved to the asset layer in
    /// this port - by the time [`Pool::init`] is called, the script catalog
    /// has already been resolved to in-memory offsets.
    ///
    /// Safe to call multiple times - re-initing the pool drops every
    /// active effect.
    ///
    /// PORT: FUN_801DE914
    pub fn init(&mut self, head: PoolHead) {
        // Retail clears `_DAT_8007BD30` for `0x4E4 = 1252` u32 words = 5008
        // bytes, which spans the head + child slots + master slots + the
        // unused tail. We just rewrite the typed structs; the result is
        // bytewise equivalent.
        for child in &mut self.children {
            *child = ChildSlot::default();
        }
        for master in &mut self.master_slots {
            *master = MasterSlot::default();
        }
        self.head = head;
    }

    /// Find the first inactive (`child_count == 0`) master slot.
    ///
    /// The retail spawn function uses this as its allocator: linear scan,
    /// first hit wins, give up after `0x20 = 32` tries. Returns `None` if
    /// the pool is full.
    pub fn allocate_master(&self) -> Option<usize> {
        self.master_slots
            .iter()
            .position(|m| m.child_count == 0)
            .filter(|&i| i < MAX_MASTER_SLOTS)
    }

    /// Port of `FUN_801DFDF8` - spawn an effect at `world_pos` with `angle`
    /// (12 bits used).
    ///
    /// Retail signature: `void(byte effect_id, short* world_pos, ushort
    /// angle)`. The `world_pos` arg is an `[x, y, z]` triple of i16s; we
    /// take it as a typed array.
    ///
    /// Returns `Some(slot)` on success, `None` if the pool is full or the
    /// global guard `_DAT_8007BD71 != -1` would fire (callers gate this).
    /// The retail dispatch on `effect_id == 4` / `effect_id == 0x13` -
    /// which calls the alternate handler `func_0x80050ed4` for "summon"
    /// effects - is delegated to [`EffectHost::handle_summon`]; this port
    /// handles only the generic case.
    ///
    /// PORT: FUN_801DFDF8
    pub fn spawn<H: EffectHost + ?Sized>(
        &mut self,
        host: &mut H,
        effect_id: u8,
        world_pos: [i16; 3],
        angle: u16,
        script: &EffectScript,
        children: &[ChildSprite],
    ) -> Option<usize> {
        // Special-case effect IDs route to the streaming-summon handler,
        // which has its own buffer pool. We consult the host instead of
        // hard-wiring the constants, so engines can route summon IDs by
        // table.
        if host.is_summon_effect(effect_id) {
            host.handle_summon(effect_id, world_pos, angle);
            return None;
        }

        let slot = self.allocate_master()?;
        let m = &mut self.master_slots[slot];

        m.child_count = script.child_count;
        m.flags = script.flags;
        m.spawn_cursor = 0;
        m.state = 0;
        m.angle = angle & 0x0FFF;
        m.field_06 = 0;
        m.pos_x = (world_pos[0] as i32) << 8;
        m.pos_y = (world_pos[1] as i32) << 8;
        m.pos_z = (world_pos[2] as i32) << 8;
        m.field_14 = 0;
        // Retail: `*piVar8 + 4` - pointer past the 4-byte script header.
        // We store offset-into-body since we keep the header separately.
        m.script_offset = 0;
        m.ui_id = effect_id;
        m.child_offsets = Vec::new();

        // If the script's flags bit 0 is clear, the spawn records keep their
        // authored offset legs; the walker consumes them as-is. Done.
        if script.flags & 0x01 == 0 {
            return Some(slot);
        }

        // Random spawn-offset distribution: for each spawn record, rewrite
        // its two planar offset legs with `rand % (2*spread) - spread`.
        // Retail stores these into `record[+2]` / `record[+6]` - i.e., it
        // scribbles back into the resident *script* buffer. The port keeps
        // the catalog immutable: the rewrites land in
        // `master.child_offsets` (consumed by [`Pool::tick_retail`]) and are
        // also surfaced through a host hook so engines can mirror them into
        // per-child render state.
        //
        // The remainder is the C-style `%` (truncated toward zero) the MIPS
        // `div` produces - negative RNG samples yield negative remainders.
        // Retail traps on `spread == 0` (a zero divisor); the port clamps to
        // 1 instead of crashing.
        let _ = children; // record fields are read back through the catalog.
        let spread = script.spread.max(1);
        for child_idx in 0..script.child_count {
            let modulus = (spread as i32) << 1;
            let raw_x = host.next_random();
            let raw_z = host.next_random();
            let dx = (raw_x % modulus - spread as i32) as i16;
            let dz = (raw_z % modulus - spread as i32) as i16;
            self.master_slots[slot].child_offsets.push((dx, dz));
            host.assign_child_random_offset(slot, child_idx, dx, dz);
        }

        Some(slot)
    }

    /// Count of currently active master slots (slots where `child_count > 0`).
    /// Useful in tests and integration checks to verify effects are spawning.
    pub fn active_count(&self) -> usize {
        self.master_slots
            .iter()
            .filter(|m| m.child_count > 0)
            .count()
    }

    /// Count of currently active child slots (slots where `frame_count > 0`).
    pub fn active_child_count(&self) -> usize {
        self.children.iter().filter(|c| c.frame_count > 0).count()
    }

    /// Look up `ui_id` in `catalog` and spawn the effect at `world_pos` /
    /// `angle`. Returns `None` if the id is out of range or the pool is full.
    /// Mirrors the retail `FUN_801D8DE8(ui_id, mode)` → `FUN_801DFDF8` path.
    pub fn spawn_by_ui_id<H: EffectHost + ?Sized>(
        &mut self,
        host: &mut H,
        ui_id: u8,
        world_pos: [i16; 3],
        angle: u16,
        catalog: &EffectCatalog,
    ) -> Option<usize> {
        let (script, children) = catalog.entry(ui_id)?;
        self.spawn(host, ui_id, world_pos, angle, script, children)
    }

    /// The faithful per-frame walker - `FUN_801E0088` pass 1 (master spawn
    /// cadence + child animation/motion), ported operator-for-operator from
    /// `overlay_battle_801e0088.txt`.
    ///
    /// `frame_skip` is the adaptive catch-up factor `DAT_1F800393` (how many
    /// logic frames this call represents; retail passes its frame-skip
    /// counter, a steady 60-fps host passes 1). Every wait counter is 5.3
    /// fixed-point: stored `<< 3`, decremented by 8 per logic frame, values
    /// already `< 8` clamp to 0. A sweep that finds zero active masters and
    /// zero active children adds 4 to the sweep counter, skipping up to four
    /// remaining catch-up iterations exactly as retail does.
    ///
    /// Per sweep:
    ///
    /// - **Master walk** (32 slots): an active master with a non-zero wait
    ///   counts down; at zero it runs the spawn loop - seed the next free
    ///   child slot from the current 14-byte spawn record (the allocation
    ///   cursor scans forward and persists across masters within the sweep;
    ///   on pool exhaustion the record is still consumed with no child),
    ///   advance the cursor, set `wait = record.delay << 3` (byte-truncated,
    ///   so a delay `>= 32` frames wraps), and repeat while the new wait is
    ///   zero (zero-delay records spawn as one burst). Consuming the final
    ///   record frees the master and forces `wait = 8` to exit the loop.
    /// - **Child walk** (128 slots): an active child with a non-zero wait
    ///   counts down and takes one motion step; at zero it loops - advance
    ///   one anim frame (`frame_cursor += 1`, `wait` = new frame's delay
    ///   `<< 3`; reaching `frame_count` retires the slot), then one motion
    ///   step - while the new wait is zero. A motion step is
    ///   `pos += vel * frame.speed * motion_scale * 8 >> 15` per axis
    ///   (wrapping 32-bit, like the MIPS `mflo` chain); the retail
    ///   `motion_scale = 0x1000` reduces it to `pos += vel * frame.speed`.
    ///
    /// Known unobservable divergence: after retiring a child, retail's
    /// frame-advance loop keeps consuming 6-byte strides past the batch end
    /// (on the now-inactive slot) until it hits a non-zero delay byte; the
    /// port breaks at retirement instead. The extra reads/motion touch only
    /// a retired slot, which the next seed fully rewrites.
    ///
    /// The `DAT_8007BD71 == 0xFF` ready-flag gate stays with the caller.
    /// Pass 2 (render) is [`Pool::child_billboards`].
    ///
    /// PORT: FUN_801E0088
    pub fn tick_retail<H: EffectHost + ?Sized>(
        &mut self,
        host: &mut H,
        catalog: &EffectCatalog,
        frame_skip: u8,
    ) {
        let motion_scale = self.head.motion_scale as i16 as i32;

        // Port-side aging aid (retail leaves master +0x14 unwritten): bump
        // once per call per active master so age-based render fades keep
        // working for hosts that read `field_14`.
        for m in &mut self.master_slots {
            if m.child_count != 0 {
                m.field_14 = m.field_14.saturating_add(1);
            }
        }

        let mut sweep: u32 = 0;
        while (sweep & 0xFF) < frame_skip as u32 {
            let mut live_slots = 0usize;
            // Child-slot allocation cursor: scans forward, persists across
            // masters within one sweep (retail `s4`).
            let mut alloc = 0usize;

            // --- master walk: spawn cadence over the pack1 records -------
            for mi in 0..MAX_MASTER_SLOTS {
                if self.master_slots[mi].child_count == 0 {
                    continue;
                }
                live_slots += 1;
                let wait = self.master_slots[mi].state;
                if wait != 0 {
                    // 5.3 countdown: -8, clamping values < 8 to zero.
                    self.master_slots[mi].state = wait.saturating_sub(8);
                    continue;
                }
                // Spawn loop: consume records while the running wait is 0.
                loop {
                    let m = &self.master_slots[mi];
                    let idx = m.spawn_cursor as usize;
                    let rec = catalog
                        .entry(m.ui_id)
                        .and_then(|(_, ch)| ch.get(idx))
                        .copied();
                    // flags bit 0: the randomized planar legs (retail reads
                    // them back out of the scribbled script buffer).
                    let overridden = if m.flags & 0x01 != 0 {
                        m.child_offsets.get(idx).copied()
                    } else {
                        None
                    };
                    if let Some(rec) = rec {
                        let (width, depth) = overridden.unwrap_or((rec.width, rec.depth));
                        // First-fit child slot from the persistent cursor.
                        while alloc < MAX_CHILD_SLOTS && self.children[alloc].frame_count != 0 {
                            alloc += 1;
                        }
                        // A batch missing from the catalog would be a wild
                        // pointer read in retail; the port skips the seed
                        // (the record is still consumed below). On pool
                        // exhaustion (alloc off the end) the record is also
                        // consumed with no child - the effect degrades
                        // rather than stalling.
                        if alloc < MAX_CHILD_SLOTS
                            && let Some(batch) = catalog.anim(rec.sprite_id)
                        {
                            let mirror_rand = host.next_random();
                            let master = &self.master_slots[mi];
                            Self::seed_child(
                                &mut self.children[alloc],
                                mirror_rand,
                                master,
                                &rec,
                                width,
                                depth,
                                batch,
                            );
                        }
                    }
                    // Consume the record: advance the cursors, arm the wait.
                    let m = &mut self.master_slots[mi];
                    let delay = rec.map(|r| r.delay).unwrap_or(0);
                    m.spawn_cursor = m.spawn_cursor.wrapping_add(1);
                    m.script_offset = m.script_offset.wrapping_add(14);
                    m.state = ((delay as u32) << 3) as u8;
                    if m.spawn_cursor == m.child_count {
                        // Final record consumed: free the master; wait = 8
                        // forces the loop exit.
                        m.child_count = 0;
                        m.state = 8;
                    }
                    if m.state != 0 {
                        break;
                    }
                }
            }

            // --- child walk: anim frame advance + motion integration -----
            for ci in 0..MAX_CHILD_SLOTS {
                if self.children[ci].frame_count == 0 {
                    continue;
                }
                live_slots += 1;
                let wait = self.children[ci].wait;
                if wait != 0 {
                    // Countdown branch still takes one motion step - a child
                    // keeps drifting while its frame holds.
                    let c = &mut self.children[ci];
                    c.wait = wait.saturating_sub(8);
                    Self::motion_step(c, catalog, motion_scale);
                } else {
                    loop {
                        let c = &mut self.children[ci];
                        // Advance one anim frame.
                        c.frame_cursor = c.frame_cursor.wrapping_add(1);
                        if c.frame_cursor == c.frame_count {
                            // Retire the slot (retail zeroes wait + count and
                            // keeps scanning; see the divergence note above).
                            c.wait = 0;
                            c.frame_count = 0;
                            break;
                        }
                        let delay = catalog
                            .anim(c.anim_id)
                            .and_then(|b| b.frames.get(c.frame_cursor as usize))
                            .map(|f| f.timing[0])
                            .unwrap_or(0);
                        c.wait = ((delay as u32) << 3) as u8;
                        Self::motion_step(c, catalog, motion_scale);
                        if c.wait != 0 {
                            break;
                        }
                    }
                }
            }

            // Retail: an all-idle sweep adds 4 to the (byte-masked) sweep
            // counter, short-circuiting the remaining catch-up iterations.
            if live_slots == 0 {
                sweep = sweep.wrapping_add(4);
            }
            sweep = sweep.wrapping_add(1);
        }
    }

    /// Seed one child slot from a spawn record (`FUN_801E0088` pass 1, the
    /// spawn block `0x801E0184..0x801E03F0`).
    fn seed_child(
        child: &mut ChildSlot,
        mirror_rand: i32,
        master: &MasterSlot,
        rec: &ChildSprite,
        width: i16,
        depth: i16,
        batch: &AnimBatch,
    ) {
        // Batch header byte 0 (frame count) doubles as the active flag. The
        // catalog only materializes in-bounds frames, so the port derives the
        // count from the frames it actually holds.
        child.frame_count = batch.frames.len().min(0xFF) as u8;
        // Random UV-mirror bits: `rand % 4`, the low byte of the C-style
        // remainder (negative samples keep their two's-complement bits).
        child.mirror = (mirror_rand % 4) as u8;
        child.frame_cursor = 0;
        child.anim_id = rec.sprite_id;
        // Wait = first frame's hold delay << 3 (byte-truncated).
        child.wait = batch
            .frames
            .first()
            .map(|f| ((f.timing[0] as u32) << 3) as u8)
            .unwrap_or(0);

        // Position: master origin (16.8), minus the vertical leg, plus the
        // planar legs rotated by the master angle through the 4096-entry
        // trig tables (`_DAT_8007B81C` sin / `_DAT_8007B7F8` cos), >> 4.
        let a = (master.angle & 0x0FFF) as i32;
        let (sin_a, cos_a) = (sin_4096(a), cos_4096(a));
        let (sin_na, cos_na) = (sin_4096(0xFFF - a), cos_4096(0xFFF - a));
        let (w, d) = (width as i32, depth as i32);
        child.pos = [
            master
                .pos_x
                .wrapping_add((sin_a * d) >> 4)
                .wrapping_add((cos_na * w) >> 4),
            master.pos_y.wrapping_sub((rec.height as i32) << 8),
            master
                .pos_z
                .wrapping_add((sin_na * w) >> 4)
                .wrapping_add((cos_a * d) >> 4),
        ];

        // Velocity: planar legs rotated by the same angle, >> 12; the
        // vertical component copies direct.
        let (va, vb) = (rec.velocity[0] as i32, rec.velocity[2] as i32);
        child.velocity = [
            (((sin_a * vb) >> 12) + ((cos_na * va) >> 12)) as i16,
            rec.velocity[1],
            (((sin_na * va) >> 12) + ((cos_a * vb) >> 12)) as i16,
        ];
        // Retail also copies master +0x14 into child +0x18 here - a dead
        // lane on both sides (never read); the port drops it.
    }

    /// One child motion step: `pos += vel * frame.speed * motion_scale * 8
    /// >> 15` per axis, on the current frame's speed byte (frame byte +2).
    /// The multiply chain wraps in 32 bits like the MIPS `mflo` sequence.
    fn motion_step(child: &mut ChildSlot, catalog: &EffectCatalog, motion_scale: i32) {
        let speed = catalog
            .anim(child.anim_id)
            .and_then(|b| b.frames.get(child.frame_cursor as usize))
            .map(|f| f.timing[1] as i32)
            .unwrap_or(0);
        for axis in 0..3 {
            let step = (child.velocity[axis] as i32)
                .wrapping_mul(speed)
                .wrapping_mul(motion_scale)
                .wrapping_shl(3)
                >> 15;
            child.pos[axis] = child.pos[axis].wrapping_add(step);
        }
    }

    /// `FUN_801E0088` pass 2 - one billboard per live child, run once per
    /// call in retail (not per catch-up sweep). The retail body additionally
    /// projects through `FUN_800195A8` and inserts the 9-word GPU packet
    /// (tag `0x09000000`, prim code `0x2E` - flat textured semi-transparent
    /// quad) into the OT at `_DAT_1F8003F4 + depth * 4`; both stay with the
    /// renderer. Everything else - the brightness envelope, the atlas
    /// resolution off the current frame, the sprite scaling, and the random
    /// UV-mirror corner order - is computed here.
    ///
    /// PORT: FUN_801E0088 (pass 2)
    pub fn child_billboards(&self, catalog: &EffectCatalog) -> Vec<ChildBillboard> {
        let sprite_scale = self.head.sprite_scale as i16 as i32;
        let mut out = Vec::new();
        for c in &self.children {
            if c.frame_count == 0 {
                continue;
            }
            let Some(frame) = catalog
                .anim(c.anim_id)
                .and_then(|b| b.frames.get(c.frame_cursor as usize))
            else {
                continue;
            };
            let Some(entry) = catalog.atlas().get(frame.atlas_index as usize).copied() else {
                continue;
            };
            out.push(ChildBillboard {
                // 16.8 -> integer world units; the low 16 bits of the
                // shifted value, exactly as retail truncates for projection.
                pos: [
                    (c.pos[0] >> 8) as i16,
                    (c.pos[1] >> 8) as i16,
                    (c.pos[2] >> 8) as i16,
                ],
                brightness: pass2_brightness(c.frame_count, c.frame_cursor),
                atlas_index: frame.atlas_index,
                entry,
                world_w: (entry.w as i32 * sprite_scale) >> 8,
                world_h: (entry.h as i32 * sprite_scale) >> 8,
                flip_h: c.mirror & 0x01 == 0,
                flip_v: c.mirror & 0x02 == 0,
            });
        }
        out
    }

    /// Legacy host-delegating frame - the pre-algebra shim kept for engines
    /// that model the effect lifecycle through
    /// [`EffectHost::advance_state`] / [`EffectHost::accumulate_child_motion`]
    /// (a fixed-lifetime countdown instead of the retail cadence). New hosts
    /// should call [`Pool::tick_retail`], the faithful walker.
    ///
    /// This shim still ports the master-slot iteration + the 5.3 state-byte
    /// countdown (`0x801e0130..0x801e015f`); only the `state == 0` work is
    /// delegated.
    ///
    /// REF: FUN_801E0088
    pub fn tick<H: EffectHost + ?Sized>(&mut self, host: &mut H) {
        for slot in 0..MAX_MASTER_SLOTS {
            // Snapshot the activeness up front; the host may compact the
            // pool during `advance_state` and we want a stable view.
            let m = &mut self.master_slots[slot];
            if m.child_count == 0 {
                continue;
            }

            // Per-frame child-sprite motion runs FIRST, unconditionally, for
            // every active slot - retail FUN_801E0088 integrates each child's
            // position by its velocity in both the work loop and the
            // wait-countdown branch, so billboards keep drifting during a
            // wait state. Only the script advance below is `state`-gated.
            host.accumulate_child_motion(slot, m);

            // State-byte countdown logic. Retail at 801e0130-801e015f:
            //   state = master[+0x03]
            //   if state == 0 → fall through to work
            //   else if state < 8 → master[+0x03] = 0; skip
            //   else            → master[+0x03] = state - 8; skip
            let s = m.state;
            if s != 0 {
                m.state = s.saturating_sub(8);
                continue;
            }

            // State == 0: work path, delegated to the host.
            let outcome = host.advance_state(slot, m);
            match outcome {
                StateOutcome::Continue => {}
                StateOutcome::Wait { frames } => {
                    // Encode frames + 8 so the countdown rebase path picks
                    // up `frames` next tick. saturating_add caps at u8::MAX.
                    m.state = frames.saturating_add(8);
                }
                StateOutcome::Terminate => {
                    *m = MasterSlot::default();
                }
            }
        }
    }
}
