//! Battle-effect VM, ported clean-room from the `0898_xxx_dat` battle overlay.
//!
//! PORT: FUN_801DE914, FUN_801DFDF8, FUN_801E0088
//!
//! See [`docs/subsystems/effect-vm.md`](../../../docs/subsystems/effect-vm.md)
//! for the authoritative byte-level reference. This crate ports the high-
//! confidence pieces - the slot pool layout, the per-effect script header
//! parser, and the public spawn API - and exposes a [`EffectHost`] trait that
//! lets the engine extend the per-frame walker incrementally.
//!
//! ## Why no opcode table
//!
//! The retail per-frame walker (`FUN_801E0088`, 600+ instructions) does state
//! transitions inline - there's no central `switch (state)` to translate into
//! a clean Rust dispatch. The port models the **walker as a state-machine
//! frame** (slot iteration + state-byte countdown + child-slot allocation)
//! and delegates per-state logic to the host. Engines wire whatever runtime
//! they have for animation playback, GPU primitive emission, and RNG.
//!
//! ## Three retail entry points
//!
//! | Function | Role | Status |
//! |---|---|---|
//! | `0x801DE914` | Init / pack-fixup | Ported as [`Pool::init`] |
//! | `0x801DFDF8` | Public spawn API: `(byte effect_id, short* world_pos, ushort angle)` | Ported as [`Pool::spawn`] |
//! | `0x801E0088` | Per-frame walker | [`Pool::tick`] (skeleton) + host hooks |
//!
//! ## Clean-room boundary
//!
//! No bytes from `SCUS_942.54` or any overlay live in this crate. The Ghidra
//! decompilation at `ghidra/scripts/funcs/overlay_battle_801de914.txt`,
//! `overlay_battle_801dfdf8.txt`, and `overlay_battle_801e0088.txt` is the
//! *spec*, not source. Tests use hand-authored synthetic scripts (no Sony
//! bytes).
//! REF: FUN_801D8DE8

#![allow(clippy::too_many_arguments)]

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

/// Per-master-slot child-distribution descriptor used by [`Pool::spawn`]
/// when the script's `flags & 0x01` bit is set. One entry per child sprite
/// the script will spawn; the spawn loop reads two random values per child.
///
/// Models the per-child params at `pack1_record[+0x4 + child_idx * 14]`
/// in the retail layout (the retail random-distribution loop reads only
/// `+0x2` and `+0x6` of each - width and depth). Other fields belong to
/// the per-frame walker.
#[derive(Debug, Clone, Copy, Default)]
pub struct ChildSprite {
    /// `+0x00..0x02` - sprite identifier, copied to `MasterSlot.field_18`
    /// downstream.
    pub sprite_id: u16,
    /// `+0x02` - half-width of the random X distribution (in 8.8 fixed).
    pub width: i16,
    /// `+0x04..0x06` - anim / shading flags.
    pub anim_flags: u16,
    /// `+0x06` - half-width of the random Z distribution (in 8.8 fixed).
    pub depth: i16,
    /// `+0x08..0x0E` - opaque tail (animation curves / sound id / etc.).
    pub tail: [u8; 6],
}

/// 32-master / 128-child slot pool. Mirrors the 5008-byte block at
/// `_DAT_8007BD30` in retail. Engines own one of these per scene.
#[derive(Debug, Clone)]
pub struct Pool {
    /// `+0x00..0x10` - pool-head record set by [`Pool::init`].
    /// `param_1`, `param_2` are the two `(id=0x1000, param=0xA00)` immediates
    /// the retail caller passes; their semantics remain opaque pending more
    /// reverse work, so we just retain them.
    pub head: PoolHead,
    pub master_slots: [MasterSlot; MAX_MASTER_SLOTS],
    pub children: [ChildSlot; MAX_CHILD_SLOTS],
}

/// 16-byte pool-head record (`_DAT_8007BD30 + 0`). Retail layout (all u32):
/// `[id_param_packed, pack0_base, pack1_offsets_base, pack1_body_base]`.
/// We model the two 16-bit halves as separate fields for clarity.
#[derive(Debug, Clone, Copy, Default)]
pub struct PoolHead {
    /// Lower half of word 0: `param_1` from `init`.
    pub param_id: u16,
    /// Upper half of word 0: `param_2` from `init`.
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
    /// in the retail code; the port exposes [`EffectHost::advance_state`]
    /// for engines to plug their state-transition logic into. This means
    /// the port runs the per-frame slot iteration and the gating logic,
    /// but the actual per-state work lives in the engine.
    pub fn tick<H: EffectHost + ?Sized>(&mut self, host: &mut H) {
        for slot in 0..MAX_MASTER_SLOTS {
            // Snapshot the activeness up front; the host may compact the
            // pool during `advance_state` and we want a stable view.
            let m = &mut self.master_slots[slot];
            if m.child_count == 0 {
                continue;
            }

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

/// Outcome of one master-slot state advance, returned by
/// [`EffectHost::advance_state`]. The pool uses this to update the slot's
/// state byte / lifecycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StateOutcome {
    /// Stay active, run again next frame (state byte stays at 0).
    Continue,
    /// Wait `frames` frames before the next advance. The pool encodes this
    /// using the retail `state = frames + 8` convention so the countdown
    /// path picks it up next tick.
    Wait { frames: u8 },
    /// Effect is done - the pool zeroes the master slot.
    Terminate,
}

/// Engine-side callbacks the effect VM dispatches into.
///
/// All methods have default impls so a minimal host (only RNG) compiles.
/// Each method documents which retail function it stands in for.
pub trait EffectHost {
    /// Equivalent of `func_0x80056798` - uniform random `i32`. The retail
    /// PRNG is an LCG seeded by `_DAT_8007AB80`; engines plug whatever RNG
    /// they have. Default impl returns `0` (deterministic for tests).
    fn next_random(&mut self) -> i32 {
        0
    }

    /// Returns `true` if `effect_id` should be routed to the streaming-
    /// summon handler (`func_0x80050ed4`) instead of the generic spawn
    /// path. Retail special-cases `id == 4` and `id == 0x13`. Engines
    /// override to route their summon IDs.
    fn is_summon_effect(&self, _effect_id: u8) -> bool {
        false
    }

    /// Equivalent of `func_0x80050ed4(world_pos, &stack_buf, summon_table,
    /// 0x1000)` - the streaming-summon handler. Buffer size per slot is
    /// `0x10800 = 67584` bytes. Default no-op.
    fn handle_summon(&mut self, _effect_id: u8, _world_pos: [i16; 3], _angle: u16) {}

    /// Per-child-sprite random offset, computed by [`Pool::spawn`] when
    /// `flags & 0x01` is set. The retail code scribbles these back into
    /// the script bytes; the port exposes them to the host so engines
    /// store them next to their per-child render state. Default no-op.
    fn assign_child_random_offset(
        &mut self,
        _slot: usize,
        _child_idx: u8,
        _dx_world: i16,
        _dz_world: i16,
    ) {
    }

    /// Per-frame state advance for one active master slot. Engines do
    /// whatever per-effect work they have (read the next state byte,
    /// interpolate child positions, emit GPU primitives, decrement
    /// counters) and return [`StateOutcome`] describing the lifecycle.
    /// Default impl just terminates the slot - useful for engines that
    /// haven't wired the renderer yet.
    fn advance_state(&mut self, _slot: usize, _master: &mut MasterSlot) -> StateOutcome {
        StateOutcome::Terminate
    }
}

/// One inline sprite-atlas entry from the runtime effect buffer (the 8-byte
/// records between `buffer+8` and `pack0`). This is the PSX sprite UV packet
/// the per-frame walker (`FUN_801E0088` pass 2) reads to build each child
/// sprite's GPU primitive. The exact byte layout is pinned from that consumer
/// (dump `overlay_battle_801e0088.txt`, the sprite-emit block ~0x801e0840):
/// it reads `atlas[0]=u`, `atlas[1]=v`, `atlas[2]=w`, `atlas[3]=h` as bytes,
/// copies the u16 at `atlas+4` straight into the primitive's `tpage` field,
/// and the byte at `atlas+6` into the CLUT field. The texel rectangle is
/// `(u, v)..(u+w-1, v+h-1)`; the pixels live in VRAM, uploaded from the
/// sibling TIM pack (PROT 0872).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SpriteAtlasEntry {
    /// `+0` source texel U within the texture page.
    pub u: u8,
    /// `+1` source texel V within the texture page.
    pub v: u8,
    /// `+2` sprite width in texels.
    pub w: u8,
    /// `+3` sprite height in texels.
    pub h: u8,
    /// `+4` PSX `tpage` descriptor (texture-page X/Y base + colour mode +
    /// semi-transparency), used verbatim as the GPU primitive's tpage.
    pub page: u16,
    /// `+6` CLUT (CBA) id.
    pub clut: u8,
    /// `+7` unknown / reserved byte.
    pub unk: u8,
}

/// One frame of a pack0 animation batch. The first byte indexes the sprite
/// atlas (which texel rect to draw this frame); the remaining 5 bytes are
/// timing / direction bits the walker advances per frame.
#[derive(Debug, Clone, Copy, Default)]
pub struct AnimFrame {
    pub atlas_index: u8,
    pub timing: [u8; 5],
}

/// One pack0 entry: a frame-batched sprite animation. A child sprite's
/// `sprite_id` indexes this list; the batch's frames drive its on-screen
/// texel over the effect's lifetime.
#[derive(Debug, Clone, Default)]
pub struct AnimBatch {
    pub flags: u8,
    pub frames: Vec<AnimFrame>,
}

/// Script catalog loaded from the runtime effect buffer (`efect.dat`, PROT
/// 0873). Holds the pack1 effect scripts (one `EffectScript` + its per-child
/// descriptors per effect id), plus the pack0 animation batches and the inline
/// sprite atlas the render path needs to turn a spawned child into a textured
/// billboard.
///
/// Built by [`EffectCatalog::from_efect_dat_bytes`] on the whole PROT 0873
/// buffer. An empty catalog is safe - all `spawn_by_ui_id` calls simply return
/// `None` and there is nothing to draw.
#[derive(Debug, Clone, Default)]
pub struct EffectCatalog {
    entries: Vec<(EffectScript, Vec<ChildSprite>)>,
    atlas: Vec<SpriteAtlasEntry>,
    anims: Vec<AnimBatch>,
}

impl EffectCatalog {
    /// Construct from pre-parsed `(script, children)` pairs. Index 0 = effect
    /// id 0, index 1 = effect id 1, etc. (atlas + anims empty - test helper).
    pub fn new(entries: Vec<(EffectScript, Vec<ChildSprite>)>) -> Self {
        Self {
            entries,
            ..Self::default()
        }
    }

    /// Number of effect scripts in the catalog.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Look up `effect_id`. Returns `None` when the id is out of range.
    pub fn entry(&self, effect_id: u8) -> Option<(&EffectScript, &[ChildSprite])> {
        let (s, c) = self.entries.get(effect_id as usize)?;
        Some((s, c.as_slice()))
    }

    /// The inline sprite atlas (PSX UV packets). Indexed by an [`AnimFrame`]'s
    /// `atlas_index`.
    pub fn atlas(&self) -> &[SpriteAtlasEntry] {
        &self.atlas
    }

    /// The pack0 animation batches. A [`ChildSprite`]'s `sprite_id` indexes
    /// this list (`None` when out of range).
    pub fn anim(&self, sprite_id: u16) -> Option<&AnimBatch> {
        self.anims.get(sprite_id as usize)
    }

    /// Number of pack0 animation batches.
    pub fn anim_count(&self) -> usize {
        self.anims.len()
    }

    /// Parse the whole runtime effect buffer - the `efect.dat` 2-pack wrapper
    /// (PROT 0873). This is the format battle code actually consumes (see
    /// `docs/formats/effect.md`):
    ///
    /// ```text
    /// +0  u32 pack0_offset    +4  u32 pack1_offset
    /// +8  [inline 8-byte sprite-atlas entries up to pack0_offset]
    /// pack0: u32 count, u32 abs_offsets[count], frame-batch anim records
    /// pack1: u32 count, u32 abs_offsets[count], 4-byte-header effect scripts
    /// ```
    ///
    /// The pack tables hold **absolute file offsets** (not the `word*4`
    /// offsets of `asset::pack`). Returns an empty catalog on any structural
    /// failure so a malformed buffer just yields nothing to spawn or draw.
    pub fn from_efect_dat_bytes(buf: &[u8]) -> Self {
        Self::try_parse_efect_dat(buf).unwrap_or_default()
    }

    fn try_parse_efect_dat(buf: &[u8]) -> Option<Self> {
        let rd_u32 = |off: usize| -> Option<u32> {
            buf.get(off..off + 4)
                .map(|s| u32::from_le_bytes(s.try_into().unwrap()))
        };
        let pack0_off = rd_u32(0)? as usize;
        let pack1_off = rd_u32(4)? as usize;
        if pack0_off < 8 || pack0_off > buf.len() || pack1_off > buf.len() {
            return None;
        }

        // Inline sprite atlas: 8-byte records from +8 up to pack0.
        let mut atlas = Vec::new();
        let atlas_bytes = pack0_off - 8;
        for i in 0..atlas_bytes / 8 {
            let p = 8 + i * 8;
            atlas.push(SpriteAtlasEntry {
                u: buf[p],
                v: buf[p + 1],
                w: buf[p + 2],
                h: buf[p + 3],
                page: u16::from_le_bytes([buf[p + 4], buf[p + 5]]),
                clut: buf[p + 6],
                unk: buf[p + 7],
            });
        }

        // pack0 - animation batches.
        let mut anims = Vec::new();
        for entry in Self::pack_entries(buf, pack0_off)? {
            if entry.len() < 2 {
                anims.push(AnimBatch::default());
                continue;
            }
            let frame_count = entry[0] as usize;
            let flags = entry[1];
            let mut frames = Vec::with_capacity(frame_count);
            for f in 0..frame_count {
                let fb = 2 + f * 6;
                let Some(rec) = entry.get(fb..fb + 6) else {
                    break;
                };
                frames.push(AnimFrame {
                    atlas_index: rec[0],
                    timing: [rec[1], rec[2], rec[3], rec[4], rec[5]],
                });
            }
            anims.push(AnimBatch { flags, frames });
        }

        // pack1 - effect scripts (header + per-child descriptors).
        let mut entries = Vec::new();
        for entry in Self::pack_entries(buf, pack1_off)? {
            entries.push(Self::parse_script_entry(entry));
        }

        Some(Self {
            entries,
            atlas,
            anims,
        })
    }

    /// Read a `[u32 count][u32 abs_offset[count]]` table at `base` and return
    /// each entry as a byte slice. Entry `i` runs from `offset[i]` to
    /// `offset[i+1]` (last entry to end-of-buffer). Offsets are absolute file
    /// offsets and must be non-decreasing and in-bounds.
    fn pack_entries(buf: &[u8], base: usize) -> Option<Vec<&[u8]>> {
        let count = buf
            .get(base..base + 4)
            .map(|s| u32::from_le_bytes(s.try_into().unwrap()))? as usize;
        if count == 0 || count > 4096 {
            return None;
        }
        let table = base + 4;
        let mut offs = Vec::with_capacity(count + 1);
        for i in 0..count {
            let p = table + i * 4;
            let o = buf
                .get(p..p + 4)
                .map(|s| u32::from_le_bytes(s.try_into().unwrap()))? as usize;
            if o > buf.len() {
                return None;
            }
            offs.push(o);
        }
        for w in offs.windows(2) {
            if w[0] > w[1] {
                return None;
            }
        }
        offs.push(buf.len());
        Some((0..count).map(|i| &buf[offs[i]..offs[i + 1]]).collect())
    }

    /// Parse one pack1 entry: `[u8 child_count][u8 flags][i16 spread]` then
    /// `child_count × 14-byte` child descriptors, remainder is the body.
    fn parse_script_entry(entry: &[u8]) -> (EffectScript, Vec<ChildSprite>) {
        if entry.len() < 4 {
            return (EffectScript::default(), Vec::new());
        }
        let child_count = entry[0] as usize;
        let flags = entry[1];
        let spread = u16::from_le_bytes([entry[2], entry[3]]);
        let mut children = Vec::with_capacity(child_count);
        for c in 0..child_count {
            let cb = 4 + c * 14;
            let Some(rec) = entry.get(cb..cb + 14) else {
                break;
            };
            children.push(ChildSprite {
                sprite_id: u16::from_le_bytes([rec[0], rec[1]]),
                width: i16::from_le_bytes([rec[2], rec[3]]),
                anim_flags: u16::from_le_bytes([rec[4], rec[5]]),
                depth: i16::from_le_bytes([rec[6], rec[7]]),
                tail: [rec[8], rec[9], rec[10], rec[11], rec[12], rec[13]],
            });
        }
        let body_start = (4 + child_count * 14).min(entry.len());
        (
            EffectScript {
                child_count: child_count as u8,
                flags,
                spread,
                body: entry[body_start..].to_vec(),
            },
            children,
        )
    }

    /// Parse from a raw pack1 byte slice using the abstract `asset::pack`
    /// `word*4` offset convention. Retained for the abstract-pack path; the
    /// runtime `efect.dat` file uses absolute offsets - see
    /// [`Self::from_efect_dat_bytes`].
    pub fn from_pack1_bytes(data: &[u8]) -> Self {
        match Self::try_parse(data) {
            Some(entries) => Self {
                entries,
                ..Self::default()
            },
            None => Self::default(),
        }
    }

    fn try_parse(data: &[u8]) -> Option<Vec<(EffectScript, Vec<ChildSprite>)>> {
        if data.len() < 4 {
            return None;
        }
        let count = u32::from_le_bytes(data[0..4].try_into().unwrap()) as usize;
        if count == 0 || count > 256 {
            return None;
        }
        let table_end = 4 + count * 4;
        if table_end > data.len() {
            return None;
        }

        let mut byte_offsets: Vec<usize> = Vec::with_capacity(count + 1);
        for i in 0..count {
            let w = u32::from_le_bytes(data[4 + i * 4..8 + i * 4].try_into().unwrap()) as usize;
            let byte_off = w.checked_mul(4)?;
            if byte_off > data.len() {
                return None;
            }
            byte_offsets.push(byte_off);
        }
        // Offsets must be monotonically non-decreasing.
        for w in byte_offsets.windows(2) {
            if w[0] > w[1] {
                return None;
            }
        }
        byte_offsets.push(data.len()); // sentinel for last entry's end

        let mut out = Vec::with_capacity(count);
        for i in 0..count {
            let s = byte_offsets[i];
            let e = byte_offsets[i + 1];
            if s > data.len() || e > data.len() || e < s {
                return None;
            }
            let entry = &data[s..e];
            if entry.len() < 4 {
                return None;
            }
            let child_count = entry[0] as usize;
            let flags = entry[1];
            let spread = u16::from_le_bytes([entry[2], entry[3]]);
            let children_bytes = child_count.checked_mul(14)?;
            let header_end = 4usize.checked_add(children_bytes)?;

            let mut children = Vec::with_capacity(child_count);
            if header_end <= entry.len() {
                for c in 0..child_count {
                    let cb = &entry[4 + c * 14..4 + (c + 1) * 14];
                    children.push(ChildSprite {
                        sprite_id: u16::from_le_bytes([cb[0], cb[1]]),
                        width: i16::from_le_bytes([cb[2], cb[3]]),
                        anim_flags: u16::from_le_bytes([cb[4], cb[5]]),
                        depth: i16::from_le_bytes([cb[6], cb[7]]),
                        tail: [cb[8], cb[9], cb[10], cb[11], cb[12], cb[13]],
                    });
                }
            }
            let body_start = header_end.min(entry.len());
            let body = entry[body_start..].to_vec();
            out.push((
                EffectScript {
                    child_count: child_count as u8,
                    flags,
                    spread,
                    body,
                },
                children,
            ));
        }
        Some(out)
    }
}

#[cfg(test)]
#[allow(clippy::field_reassign_with_default)] // tests are clearer with sequential field writes
mod tests {
    use super::*;

    /// Recording host. Captures every callback so tests can assert exact
    /// dispatch order without spinning up a renderer.
    #[derive(Default)]
    struct RecHost {
        rng_seq: Vec<i32>,
        rng_pos: usize,
        summon_ids: std::collections::HashSet<u8>,
        events: Vec<HostEvent>,
        advance_outcomes: Vec<StateOutcome>,
        advance_pos: usize,
    }

    #[derive(Debug, PartialEq, Eq)]
    enum HostEvent {
        Random,
        HandleSummon(u8, [i16; 3], u16),
        ChildOffset(usize, u8, i16, i16),
        AdvanceState(usize),
    }

    impl EffectHost for RecHost {
        fn next_random(&mut self) -> i32 {
            self.events.push(HostEvent::Random);
            let v = self.rng_seq.get(self.rng_pos).copied().unwrap_or(0);
            self.rng_pos += 1;
            v
        }

        fn is_summon_effect(&self, effect_id: u8) -> bool {
            self.summon_ids.contains(&effect_id)
        }

        fn handle_summon(&mut self, effect_id: u8, world_pos: [i16; 3], angle: u16) {
            self.events
                .push(HostEvent::HandleSummon(effect_id, world_pos, angle));
        }

        fn assign_child_random_offset(&mut self, slot: usize, child_idx: u8, dx: i16, dz: i16) {
            self.events
                .push(HostEvent::ChildOffset(slot, child_idx, dx, dz));
        }

        fn advance_state(&mut self, slot: usize, _m: &mut MasterSlot) -> StateOutcome {
            self.events.push(HostEvent::AdvanceState(slot));
            let outcome = self
                .advance_outcomes
                .get(self.advance_pos)
                .copied()
                .unwrap_or(StateOutcome::Continue);
            self.advance_pos += 1;
            outcome
        }
    }

    #[test]
    fn init_zeros_all_slots() {
        let mut pool = Pool::new();
        // Smudge the pool so init has something to clear.
        pool.master_slots[0].child_count = 1;
        pool.children[5].src_x = 0xDEAD_BEEFu32 as i32;

        pool.init(PoolHead {
            param_id: 0x1000,
            param_extra: 0x0A00,
            pack0_base: 0x1234_0000,
            pack1_index_base: 0x1234_4000,
            pack1_body_base: 0x1234_8000,
        });

        assert_eq!(pool.master_slots[0].child_count, 0);
        assert_eq!(pool.children[5].src_x, 0);
        assert_eq!(pool.head.param_id, 0x1000);
        assert_eq!(pool.head.param_extra, 0x0A00);
        assert_eq!(pool.head.pack0_base, 0x1234_0000);
    }

    #[test]
    fn allocate_master_finds_first_empty() {
        let mut pool = Pool::new();
        assert_eq!(pool.allocate_master(), Some(0));

        // Mark slot 0 active; allocator advances.
        pool.master_slots[0].child_count = 3;
        assert_eq!(pool.allocate_master(), Some(1));

        // Fill all slots; allocator returns None.
        for m in &mut pool.master_slots {
            m.child_count = 1;
        }
        assert_eq!(pool.allocate_master(), None);
    }

    #[test]
    fn spawn_routes_summon_to_handler() {
        let mut pool = Pool::new();
        let mut host = RecHost::default();
        host.summon_ids.insert(4);
        let script = EffectScript::default();

        let r = pool.spawn(&mut host, 4, [10, 20, 30], 0x123, &script, &[]);
        assert_eq!(r, None);
        assert_eq!(
            host.events,
            vec![HostEvent::HandleSummon(4, [10, 20, 30], 0x123)]
        );
        // No master slot consumed.
        assert_eq!(pool.master_slots[0].child_count, 0);
    }

    #[test]
    fn spawn_initializes_master_slot() {
        let mut pool = Pool::new();
        let mut host = RecHost::default();
        let script = EffectScript {
            child_count: 4,
            flags: 0x00, // no random distribution
            spread: 16,
            body: vec![0u8; 16],
        };

        let slot = pool
            .spawn(&mut host, 7, [100, -50, 200], 0x800, &script, &[])
            .expect("slot");
        assert_eq!(slot, 0);

        let m = &pool.master_slots[0];
        assert_eq!(m.child_count, 4);
        assert_eq!(m.flags, 0x00);
        assert_eq!(m.angle, 0x800);
        assert_eq!(m.pos_x, 100i32 << 8);
        assert_eq!(m.pos_y, (-50i32) << 8);
        assert_eq!(m.pos_z, 200i32 << 8);
        assert_eq!(m.state, 0);

        // No random distribution requested, so no host events.
        assert!(host.events.is_empty());
    }

    #[test]
    fn spawn_distributes_random_children_when_flag_set() {
        let mut pool = Pool::new();
        let mut host = RecHost::default();
        // Two children, deterministic RNG sequence.
        host.rng_seq = vec![0, 8, 31, 7];

        let script = EffectScript {
            child_count: 2,
            flags: 0x01,
            spread: 16,
            body: vec![],
        };

        let _ = pool
            .spawn(&mut host, 9, [0, 0, 0], 0, &script, &[])
            .unwrap();

        // Expected per-child math: modulus = 32, raw % 32 - 16.
        // child 0: (0 % 32) - 16 = -16, (8 % 32) - 16 = -8.
        // child 1: (31 % 32) - 16 = 15, (7 % 32) - 16 = -9.
        let want: Vec<HostEvent> = vec![
            HostEvent::Random,
            HostEvent::Random,
            HostEvent::ChildOffset(0, 0, -16, -8),
            HostEvent::Random,
            HostEvent::Random,
            HostEvent::ChildOffset(0, 1, 15, -9),
        ];
        assert_eq!(host.events, want);
    }

    #[test]
    fn spawn_angle_is_masked_to_12_bits() {
        let mut pool = Pool::new();
        let mut host = RecHost::default();
        let script = EffectScript {
            child_count: 1,
            ..EffectScript::default()
        };
        let _ = pool
            .spawn(&mut host, 1, [0, 0, 0], 0xF234, &script, &[])
            .unwrap();
        assert_eq!(pool.master_slots[0].angle, 0x0234);
    }

    #[test]
    fn tick_decrements_state_below_8() {
        let mut pool = Pool::new();
        let mut host = RecHost::default();
        // Mark slot 0 active with a low state.
        pool.master_slots[0].child_count = 1;
        pool.master_slots[0].state = 5;

        pool.tick(&mut host);

        // State < 8 → goes to 0 in one tick (retail clears, doesn't decrement).
        assert_eq!(pool.master_slots[0].state, 0);
        // No advance_state - slot was waiting.
        assert!(host.events.is_empty());
    }

    #[test]
    fn tick_rebases_state_at_or_above_8() {
        let mut pool = Pool::new();
        let mut host = RecHost::default();
        pool.master_slots[0].child_count = 1;
        pool.master_slots[0].state = 16;

        pool.tick(&mut host);

        // State >= 8 → state -= 8.
        assert_eq!(pool.master_slots[0].state, 8);
        assert!(host.events.is_empty());
    }

    #[test]
    fn tick_calls_advance_state_when_state_is_zero() {
        let mut pool = Pool::new();
        let mut host = RecHost::default();
        host.advance_outcomes = vec![StateOutcome::Continue];

        pool.master_slots[0].child_count = 1;
        pool.master_slots[0].state = 0;

        pool.tick(&mut host);

        assert_eq!(host.events, vec![HostEvent::AdvanceState(0)]);
        // Slot still active.
        assert_eq!(pool.master_slots[0].child_count, 1);
    }

    #[test]
    fn tick_terminate_clears_slot() {
        let mut pool = Pool::new();
        let mut host = RecHost::default();
        host.advance_outcomes = vec![StateOutcome::Terminate];

        pool.master_slots[0].child_count = 1;
        pool.master_slots[0].state = 0;
        pool.master_slots[0].pos_x = 0xDEAD;

        pool.tick(&mut host);

        assert_eq!(pool.master_slots[0].child_count, 0);
        assert_eq!(pool.master_slots[0].pos_x, 0);
    }

    #[test]
    fn tick_wait_encodes_frames_via_state_byte() {
        let mut pool = Pool::new();
        let mut host = RecHost::default();
        host.advance_outcomes = vec![StateOutcome::Wait { frames: 3 }];

        pool.master_slots[0].child_count = 1;
        pool.master_slots[0].state = 0;

        pool.tick(&mut host);

        // Wait { frames: 3 } → state = 3 + 8 = 11 (so countdown rebase
        // brings it back to 3 next tick).
        assert_eq!(pool.master_slots[0].state, 11);
    }

    #[test]
    fn tick_skips_inactive_slots() {
        let mut pool = Pool::new();
        let mut host = RecHost::default();
        // No slot activated → no advance_state ever called.
        pool.tick(&mut host);
        assert!(host.events.is_empty());
    }

    #[test]
    fn tick_iterates_all_active_slots() {
        let mut pool = Pool::new();
        let mut host = RecHost::default();
        host.advance_outcomes = vec![StateOutcome::Continue; 5];

        // Activate slots 0, 7, 31.
        for &i in &[0usize, 7, 31] {
            pool.master_slots[i].child_count = 1;
        }

        pool.tick(&mut host);

        let want = vec![
            HostEvent::AdvanceState(0),
            HostEvent::AdvanceState(7),
            HostEvent::AdvanceState(31),
        ];
        assert_eq!(host.events, want);
    }

    /// Pool exhaustion: spawning into a fully-occupied pool returns `None`.
    /// Mirrors retail's "no free slot → drop spawn" branch in `FUN_801DFDF8`.
    #[test]
    fn spawn_returns_none_when_pool_exhausted() {
        let mut pool = Pool::new();
        let mut host = RecHost::default();

        // Mark every master slot in use.
        for m in &mut pool.master_slots {
            m.child_count = 1;
        }

        let r = pool.spawn(&mut host, 10, [0, 0, 0], 0, &EffectScript::default(), &[]);
        assert_eq!(r, None);
        // No host event was recorded - the pool returned before any work.
        assert!(host.events.is_empty());
    }

    /// Spawn → tick to completion → slot freed → respawn. Validates the
    /// full lifecycle of a master slot: terminate clears `child_count`,
    /// then the next allocator call returns the same slot index.
    #[test]
    fn spawn_terminate_respawn_reuses_slot() {
        let mut pool = Pool::new();
        let mut host = RecHost::default();
        let script = EffectScript::default();

        // First spawn - slot 0.
        let s0 = pool
            .spawn(&mut host, 10, [1, 2, 3], 0x111, &script, &[])
            .expect("first spawn ok");
        assert_eq!(s0, 0);
        assert_eq!(pool.master_slots[0].child_count, 0); // EffectScript::default() has 0 children
        // child_count == 0 means the slot is "empty" by allocator rules. To
        // simulate a real spawn that activates the slot, mark it manually.
        pool.master_slots[0].child_count = 1;

        // Tick once - host returns Terminate for this slot.
        host.advance_outcomes = vec![StateOutcome::Terminate];
        pool.master_slots[0].state = 0; // ensure work-path runs
        pool.tick(&mut host);
        assert_eq!(pool.master_slots[0].child_count, 0); // freed

        // Second spawn - should reuse slot 0 since it's the first empty.
        let s1 = pool
            .spawn(&mut host, 11, [4, 5, 6], 0x222, &script, &[])
            .expect("respawn ok");
        assert_eq!(s1, 0);
    }

    /// Tick a Wait-encoded slot through several frames - each tick subtracts
    /// 8 (saturating) until state hits 0, at which point the next tick fires
    /// `advance_state`. Mirrors retail's `state -= 8` countdown at
    /// `0x801e0130..0x801e015f`.
    #[test]
    fn wait_state_subtracts_8_per_tick_across_multiple_frames() {
        let mut pool = Pool::new();
        let mut host = RecHost::default();

        // Seed slot 0 with state = 24 - three ticks of `-= 8` before reaching 0.
        pool.master_slots[0].child_count = 1;
        pool.master_slots[0].state = 24;

        // After tick: 24 → 16. No advance_state.
        pool.tick(&mut host);
        assert_eq!(pool.master_slots[0].state, 16);
        assert!(host.events.is_empty(), "advance_state called too early");

        // After tick: 16 → 8.
        pool.tick(&mut host);
        assert_eq!(pool.master_slots[0].state, 8);
        assert!(host.events.is_empty());

        // After tick: 8 → 0 (still NOT a work tick - saturates).
        pool.tick(&mut host);
        assert_eq!(pool.master_slots[0].state, 0);
        assert!(host.events.is_empty());

        // After tick: state==0 → advance_state fires.
        host.advance_outcomes = vec![StateOutcome::Continue];
        pool.tick(&mut host);
        assert_eq!(host.events, vec![HostEvent::AdvanceState(0)]);
    }

    #[test]
    fn active_count_zero_on_fresh_pool() {
        let pool = Pool::new();
        assert_eq!(pool.active_count(), 0);
    }

    #[test]
    fn active_count_increments_after_spawn_with_children() {
        let mut pool = Pool::new();
        let mut host = RecHost::default();
        let script = EffectScript {
            child_count: 3,
            flags: 0,
            spread: 0,
            body: vec![],
        };
        pool.spawn(&mut host, 1, [0, 0, 0], 0, &script, &[])
            .unwrap();
        assert_eq!(pool.active_count(), 1);
        // A second slot
        pool.spawn(&mut host, 2, [0, 0, 0], 0, &script, &[])
            .unwrap();
        assert_eq!(pool.active_count(), 2);
    }

    #[test]
    fn spawn_by_ui_id_fills_slot_from_catalog() {
        let mut pool = Pool::new();
        let mut host = RecHost::default();
        let script = EffectScript {
            child_count: 2,
            flags: 0,
            spread: 0,
            body: vec![],
        };
        let catalog = EffectCatalog::new(vec![(script, vec![])]);
        assert_eq!(pool.active_count(), 0);
        let slot = pool.spawn_by_ui_id(&mut host, 0, [10, 20, 30], 0x100, &catalog);
        assert_eq!(slot, Some(0));
        assert_eq!(pool.active_count(), 1);
        assert_eq!(pool.master_slots[0].child_count, 2);
        assert_eq!(pool.master_slots[0].angle, 0x100);
    }

    #[test]
    fn spawn_by_ui_id_out_of_range_returns_none() {
        let mut pool = Pool::new();
        let mut host = RecHost::default();
        let catalog = EffectCatalog::default();
        assert!(
            pool.spawn_by_ui_id(&mut host, 5, [0, 0, 0], 0, &catalog)
                .is_none()
        );
        assert_eq!(pool.active_count(), 0);
    }

    #[test]
    fn catalog_from_pack1_bytes_parses_single_script() {
        // Pack1 with 1 entry:
        // count=1 at [0..4], word_offset[0]=2 at [4..8] (byte 8)
        // entry at [8..14]: child_count=0, flags=0, spread=8 LE, body=[0xAA, 0xBB]
        let mut data = Vec::new();
        data.extend_from_slice(&1u32.to_le_bytes()); // count=1
        data.extend_from_slice(&2u32.to_le_bytes()); // offset[0] = word 2 = byte 8
        data.extend_from_slice(&[0u8, 0, 8, 0, 0xAA, 0xBB]); // entry
        let catalog = EffectCatalog::from_pack1_bytes(&data);
        assert_eq!(catalog.len(), 1);
        let (script, children) = catalog.entry(0).unwrap();
        assert_eq!(script.child_count, 0);
        assert_eq!(script.spread, 8);
        assert_eq!(script.body, vec![0xAA, 0xBB]);
        assert!(children.is_empty());
    }

    #[test]
    fn catalog_from_pack1_bytes_empty_on_bad_data() {
        // Implausible count → empty catalog.
        let data = 0xFFFF_FFFFu32.to_le_bytes();
        let catalog = EffectCatalog::from_pack1_bytes(&data);
        assert!(catalog.is_empty());
    }

    /// The real `efect.dat` 2-pack: header pointers, an inline sprite atlas,
    /// pack0 anim batches, and pack1 effect scripts - all with **absolute**
    /// file offsets (the shape verified against PROT 0873).
    #[test]
    fn catalog_from_efect_dat_parses_packs_atlas_and_anims() {
        let mut buf = Vec::new();
        // Reserve header (filled at the end).
        buf.extend_from_slice(&[0u8; 8]);
        // Inline atlas: 2 entries (u, v, w, h, u16 tpage, clut, unk).
        buf.extend_from_slice(&[0u8, 0, 32, 32]); // u=0 v=0 w=32 h=32
        buf.extend_from_slice(&0x7680u16.to_le_bytes());
        buf.extend_from_slice(&[0x25u8, 0]); // clut, unk
        buf.extend_from_slice(&[32u8, 0, 32, 32]); // u=32 v=0 w=32 h=32
        buf.extend_from_slice(&0x7680u16.to_le_bytes());
        buf.extend_from_slice(&[0x25u8, 0]);
        let pack0_off = buf.len() as u32; // 8 + 16 = 24

        // pack0: 1 anim batch with 2 frames.
        let p0_table = buf.len();
        buf.extend_from_slice(&1u32.to_le_bytes()); // count
        buf.extend_from_slice(&[0u8; 4]); // offset[0] placeholder
        let anim0 = buf.len() as u32;
        buf.extend_from_slice(&[2u8, 0x00]); // frame_count=2, flags
        buf.extend_from_slice(&[0u8, 1, 4, 0, 0, 0]); // frame 0 (atlas_index 0)
        buf.extend_from_slice(&[1u8, 1, 4, 0, 0, 0]); // frame 1 (atlas_index 1)
        buf[p0_table + 4..p0_table + 8].copy_from_slice(&anim0.to_le_bytes());
        let pack1_off = buf.len() as u32;

        // pack1: 1 effect script with 2 children.
        let p1_table = buf.len();
        buf.extend_from_slice(&1u32.to_le_bytes()); // count
        buf.extend_from_slice(&[0u8; 4]); // offset[0] placeholder
        let script0 = buf.len() as u32;
        buf.extend_from_slice(&[2u8, 0x00]); // child_count=2, flags
        buf.extend_from_slice(&0i16.to_le_bytes()); // spread
        for sid in [514u16, 2u16] {
            buf.extend_from_slice(&sid.to_le_bytes()); // sprite_id
            buf.extend_from_slice(&0i16.to_le_bytes()); // width
            buf.extend_from_slice(&0u16.to_le_bytes()); // anim_flags
            buf.extend_from_slice(&0i16.to_le_bytes()); // depth
            buf.extend_from_slice(&[0u8; 6]); // tail
        }
        buf[p1_table + 4..p1_table + 8].copy_from_slice(&script0.to_le_bytes());

        buf[0..4].copy_from_slice(&pack0_off.to_le_bytes());
        buf[4..8].copy_from_slice(&pack1_off.to_le_bytes());

        let cat = EffectCatalog::from_efect_dat_bytes(&buf);
        assert_eq!(cat.len(), 1, "one effect script");
        assert_eq!(cat.atlas().len(), 2, "two atlas entries");
        assert_eq!(cat.atlas()[1].u, 32);
        assert_eq!(cat.atlas()[0].w, 32);
        assert_eq!(cat.atlas()[0].h, 32);
        assert_eq!(cat.atlas()[0].page, 0x7680);
        assert_eq!(cat.atlas()[0].clut, 0x25);
        assert_eq!(cat.anim_count(), 1);
        let batch = cat.anim(0).expect("anim batch 0");
        assert_eq!(batch.frames.len(), 2);
        assert_eq!(batch.frames[1].atlas_index, 1);
        let (script, children) = cat.entry(0).unwrap();
        assert_eq!(script.child_count, 2);
        assert_eq!(children[0].sprite_id, 514);
        assert_eq!(children[1].sprite_id, 2);
    }

    #[test]
    fn catalog_from_efect_dat_empty_on_truncated() {
        assert!(EffectCatalog::from_efect_dat_bytes(&[0u8; 4]).is_empty());
        // pack0_offset past EOF.
        let mut buf = vec![0u8; 8];
        buf[0..4].copy_from_slice(&0xFFFFu32.to_le_bytes());
        assert!(EffectCatalog::from_efect_dat_bytes(&buf).is_empty());
    }

    /// `is_summon_effect` short-circuits BEFORE consuming a master slot.
    /// Verifies that a summon dispatch leaves the pool fully empty (no
    /// allocator call, no child population). Guards against accidentally
    /// committing pool state on the summon path.
    #[test]
    fn summon_path_does_not_consume_master_slot() {
        let mut pool = Pool::new();
        let mut host = RecHost::default();
        host.summon_ids.insert(4);

        let r = pool.spawn(
            &mut host,
            4,
            [10, 20, 30],
            0x123,
            &EffectScript::default(),
            &[],
        );
        assert_eq!(r, None);

        // Every slot must remain empty.
        for m in &pool.master_slots {
            assert_eq!(m.child_count, 0);
            assert_eq!(m.pos_x, 0);
        }
        // Allocator should still hand out slot 0 on a non-summon spawn.
        assert_eq!(pool.allocate_master(), Some(0));
    }
}
