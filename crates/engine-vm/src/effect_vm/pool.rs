//! Effect-VM slot pool: constants, master/child slot layout, script header,
//! and the [`Pool`] runtime (`FUN_801DE914` init, `FUN_801DFDF8` spawn,
//! `FUN_801E0088` per-frame walker skeleton). Split out of `effect_vm.rs`.

use super::*;

/// Default per-effect lifetime in frames, for hosts that model the effect
/// lifecycle as a fixed countdown rather than walking the inlined per-state
/// token stream of the retail walker.
///
/// Clean-room stand-in: retail (`FUN_801E0088` pass 1) derives each effect's
/// duration from the per-frame state tokens in its script body. That
/// per-state algebra is inlined across 600+ instructions and not yet
/// extracted; until it is, a host can keep a spawned effect visible for a
/// sensible fixed budget and then retire it. The faithful token-driven
/// lifetime lands alongside the textured-sprite render path.
pub const DEFAULT_EFFECT_LIFETIME_FRAMES: u32 = 30;

/// Maximum simultaneous effects (master slots).
pub const MAX_MASTER_SLOTS: usize = 32;

/// Maximum simultaneous child sprites pooled across all effects.
pub const MAX_CHILD_SLOTS: usize = 128;

/// Width of one master slot in retail RAM bytes (`+0x1010` stride).
pub const MASTER_SLOT_BYTES: usize = 28;

/// Width of one child slot in retail RAM bytes (`+0x10` stride).
pub const CHILD_SLOT_BYTES: usize = 32;

/// Per-effect-instance state. Retail layout: 28 bytes at `_DAT_8007BD30 +
/// 0x1010 + slot * 28`. Field names match the byte offsets the retail walker
/// reads/writes; the doc lists the canonical mapping.
///
/// `active == 0` means the slot is empty. The spawn API allocates by linear
/// scan for the first slot where `active == 0`.
#[derive(Debug, Clone, Default)]
pub struct MasterSlot {
    /// `+0x00` - child sprite count for this effect (copied from
    /// `pack1_record[0]`). Zero means the slot is unused.
    pub child_count: u8,
    /// `+0x01` - flags byte. Bit `0x01` = "use random child position
    /// distribution" (drives the child-slot setup loop). Bit `0x02` related
    /// to alternate child-axis layout. Copied from `pack1_record[1]`.
    pub flags: u8,
    /// `+0x02` - counter / sub-state.
    pub field_02: u8,
    /// `+0x03` - primary state byte. Each frame the walker reads it; values
    /// `< 8` decrement to zero and "skip-this-slot", values `>= 8` get
    /// rebased by `-= 8` before continuing into the work path. Cleared when
    /// the effect completes.
    pub state: u8,
    /// `+0x04` - angle (12-bit, masked `& 0xFFF` from the spawn arg).
    pub angle: u16,
    /// `+0x06` - short padding / reserved (one slot).
    pub field_06: u16,
    /// `+0x08` - world X. Spawn writes `(caller_x as i16) << 8` (8.8 fixed).
    pub pos_x: i32,
    /// `+0x0C` - world Y. Same encoding as `pos_x`.
    pub pos_y: i32,
    /// `+0x10` - world Z. Same encoding as `pos_x`.
    pub pos_z: i32,
    /// `+0x14` - generic word (set by the walker during state advance).
    pub field_14: i32,
    /// `+0x18` - pointer (or index) into the script body. Spawn writes
    /// `pack1_record_offset + 4` (skips the 4-byte header).
    ///
    /// We store this as a `u32` "offset" so it survives moves; the host
    /// resolves it to a real Rust slice during [`Pool::tick`].
    pub script_offset: u32,

    // --- engine-side render aids (not part of the 28-byte retail slot) ---
    /// The effect id this slot was spawned with, so a render snapshot can
    /// resolve the effect's child descriptors + animations back through the
    /// [`EffectCatalog`]. Retail re-reads these from the live script pointer;
    /// the port keeps the id since it stores the catalog separately.
    pub ui_id: u8,
    /// Per-child `(dx, dz)` spawn offsets (8.8 fixed world units) for the
    /// random-distribution path (`flags & 0x01`). Empty when the walker
    /// populates child positions itself. Used by the render snapshot to place
    /// each child billboard around the effect origin.
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

/// Per-sprite render state. Retail layout: 32 bytes per slot at offset
/// `_DAT_8007BD30 + 0x10 + slot * 32`. The walker maintains 128 of these
/// (at most ~4 per active effect on average).
#[derive(Debug, Clone, Default)]
pub struct ChildSlot {
    /// `+0x00..0x10` - animation / sprite descriptor. Modeled as opaque
    /// bytes; engines typed-deserialize what they need from these 16 bytes.
    pub head: [u8; 16],
    /// `+0x10` - interpolation source X (`(caller_x as i16) << 8`).
    pub src_x: i32,
    /// `+0x14` - interpolation source Y.
    pub src_y: i32,
    /// `+0x18` - interpolation source Z.
    pub src_z: i32,
    /// `+0x1C` - packed control / sprite metadata.
    pub field_1c: u32,
}

/// Per-effect-id script record (the unit `pack1[i]` resolves to). Lives
/// in the on-disc effect bundle; spawn copies a few bytes out into a
/// freshly-allocated master slot. The bundle's parser ([`crates/asset`])
/// is responsible for handing the bytes here.
#[derive(Debug, Clone, Default)]
pub struct EffectScript {
    /// Header byte 0: child sprite count to spawn.
    pub child_count: u8,
    /// Header byte 1: flags (bit 0 = use random child distribution).
    pub flags: u8,
    /// Header u16 at +2: spread / range used for the random child position.
    /// The retail caller uses this as a modulo with `func_0x80056798()` (RNG).
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
/// bit is set (retail scribbles them back into the resident script buffer).
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

/// 32-master / 128-child slot pool. Mirrors the 5008-byte block at
/// `_DAT_8007BD30` in retail. Engines own one of these per scene.
#[derive(Debug, Clone)]
pub struct Pool {
    /// `+0x00..0x10` - pool-head record set by [`Pool::init`].
    /// `param_1`, `param_2` are the two `(id=0x1000, param=0xA00)` immediates
    /// the retail caller passes: the walker consumes the i16 at pool `+0`
    /// (`0x1000`) as the global child **motion scale** (its
    /// `* scale * 8 >> 15` reduces to unity) and the i16 at pool `+2`
    /// (`0xA00`) as the sprite **world scale** (`atlas w/h * 0xA00 >> 8` =
    /// x10 texel size before projection).
    pub head: PoolHead,
    pub master_slots: [MasterSlot; MAX_MASTER_SLOTS],
    pub children: [ChildSlot; MAX_CHILD_SLOTS],
}

/// 16-byte pool-head record (`_DAT_8007BD30 + 0`). Retail layout (all u32):
/// `[id_param_packed, pack0_base, pack1_offsets_base, pack1_body_base]`.
/// We model the two 16-bit halves as separate fields for clarity.
#[derive(Debug, Clone, Copy, Default)]
pub struct PoolHead {
    /// Lower half of word 0: `param_1` from `init` (retail `0x1000` - the
    /// walker's global child motion scale, unity at `0x1000`).
    pub param_id: u16,
    /// Upper half of word 0: `param_2` from `init` (retail `0xA00` - the
    /// pass-2 sprite world scale, `atlas w/h * param >> 8`).
    pub param_extra: u16,
    /// Word 1: pack0 record table base (frame-batch animations). Engines
    /// resolve this to a Rust slice; we keep an `u32` opaque token so the
    /// pool stays `Copy`-friendly.
    pub pack0_base: u32,
    /// Word 2: pack1 offset-table base (effect-id index). Indexed by
    /// `effect_id * 4`.
    pub pack1_index_base: u32,
    /// Word 3: pack1 body base (after the offset table). Effects with
    /// `pack1_record[+1] & 0x01` set walk this region.
    pub pack1_body_base: u32,
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

impl Pool {
    /// Construct an empty pool. Equivalent to a freshly-zeroed
    /// `_DAT_8007BD30` block before [`Pool::init`].
    pub fn new() -> Self {
        Self::default()
    }

    /// Port of `FUN_801DE914` - pack-fixup + pool init.
    ///
    /// Retail behavior: zeros the entire 5008-byte pool, then writes the
    /// `(param_1, param_2)` and the three pack pointers into the head
    /// record. The pack-fixup half (rebasing pack0 and pack1's offset
    /// tables to absolute addresses) is moved to the asset layer in this
    /// port - by the time [`Pool::init`] is called, the script catalog
    /// has already been resolved to in-memory offsets.
    ///
    /// Safe to call multiple times - re-initing the pool drops every
    /// active effect.
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
        m.field_02 = 0;
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

        // If the script's flags bit 0 is clear, the child slots will be
        // populated by the per-frame walker rather than upfront. Done.
        if script.flags & 0x01 == 0 {
            return Some(slot);
        }

        // Random child-distribution: for each child sprite, write a random
        // (x_offset, z_offset) into a per-master child-record-array.
        // Retail stores these into `master.script_data[child_idx * 0xE +
        // {2, 6}]` - i.e., it scribbles back into the *script* memory.
        // That's a memory-aliasing hazard in the retail design; in the
        // port we expose the per-child random offsets via a host hook so
        // engines store them next to their per-child render state.
        let _ = children; // child sprite descriptors are consumed by host.
        let spread = script.spread.max(1);
        for child_idx in 0..script.child_count {
            // `iVar2 % iVar4 - half` where iVar4 = `script.spread << 1`.
            // The retail wraparound is `(rand mod (2*spread)) - spread`.
            let modulus = (spread as i32) << 1;
            let raw_x = host.next_random();
            let raw_z = host.next_random();
            let dx = (raw_x.rem_euclid(modulus) - (spread as i32)) as i16;
            let dz = (raw_z.rem_euclid(modulus) - (spread as i32)) as i16;
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

    /// Port of `FUN_801E0088` (skeleton) - per-frame walker.
    ///
    /// The retail walker iterates 32 master slots; for each active one it:
    ///   1. Reads `master.state` (byte +0x03):
    ///      - `state == 0` → run the work path (state advance, child
    ///        emission, GPU primitive build).
    ///      - `state in 1..=7` → decrement and skip this slot.
    ///      - `state >= 8` → write back `state - 8` and skip this slot.
    ///   2. Walks `master.script_offset` to read the next state token.
    ///   3. Updates per-child interpolation state, emits OT primitives.
    ///
    /// Step 1 is high-confidence and ported here. Step 2 + 3 are inline
    /// in the retail code; the port exposes two host hooks. The per-child
    /// position integration (retail's drift that runs every frame
    /// regardless of `state`) is [`EffectHost::accumulate_child_motion`],
    /// called for every active slot before the state gate. The
    /// `state == 0` script-transition work is [`EffectHost::advance_state`].
    /// So the port runs the per-frame slot iteration + gating, motion drifts
    /// every frame, and the script-state work lives in the engine.
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

            // State == 0: work path. The retail body reads the next state
            // token from script_offset, runs interpolation, possibly emits
            // GPU primitives, possibly increments script_offset, and may
            // clear `master.child_count` (terminating the effect).
            //
            // We delegate this to the host to avoid hard-coding the
            // (still inlined-only) state-transition algebra.
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
