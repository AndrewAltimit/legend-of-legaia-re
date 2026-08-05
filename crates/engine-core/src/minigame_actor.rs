//! The **minigame actor record** - the per-entity struct the hub-band overlays
//! (dance, Baka Fighter, fishing, the slot machine) spawn through the shared
//! part-spawn API and then read every frame.
//!
//! # Why this exists as its own type
//!
//! Three minigames' draw kernels are written against *one* record shape, and
//! the port had no equivalent for it. [`crate::dance::sprite_part_emit`],
//! [`crate::dance::sprite_part_fade_weight`],
//! [`crate::dance::dance_clip_driver_gate`] and
//! [`crate::baka_fighter_chrome::mirrored_sprite_pass`] each take a handful of
//! loose integers because that is all a caller could hand them; every one of
//! those integers is a field of this record in retail, and the reason those
//! kernels were inert was that nothing in the engine held the record they read.
//!
//! The field names below carry their retail byte offsets, because the offsets
//! are what the disassembly names and what the kernels' doc comments cite.
//! Where two overlays read the same slot with different meanings the field is
//! named for the slot, not for one overlay's reading - see
//! [`MinigameActor::field_5c`].
//!
//! # What this is *not*
//!
//! It is not the field actor. `crate::field_actor_program` ports the field
//! overlay's actor program and `crate::world` holds the field actor list; this
//! record is the **minigame** pool, which the hub overlays allocate out of
//! their own arrays (`&DAT_801dbfac[slot * 0x50]` in the duel,
//! `DAT_801d55cc`-relative in the dance hall) and which never enters the field
//! actor list. Keeping them apart is deliberate: the field actor carries a
//! script pointer, a collision cell and a motion VM this pool has no analogue
//! for, and a shared type would imply an interchange retail does not have.
//!
//! REF: FUN_80021B04 (the shared part-spawn API every minigame spawn goes
//! through: `(pos, rot, record, scale)`)
//! REF: FUN_801d387c, FUN_801d4098 (dance: the emit dispatch and the clip
//! gate), FUN_801d49e8 (duel: the mirrored sprite pass)

/// Fixed-point scale every minigame part spawn passes to the shared spawn API
/// (`a3 = 0x1000`, i.e. 1.0).
pub const SPAWN_SCALE: i32 = 0x1000;

/// Actor flag bit that forces the shared clip driver to run even with a
/// non-positive `+0x5C` (`FUN_801d4098`'s second arm; the same bit the field
/// motion driver tests before calling `FUN_800204F8`).
pub const FLAG_DRIVE_CLIP: u32 = 0x1000;

/// Actor flag bit meaning "retired" - the shared actor teardown's killed bit.
pub const FLAG_KILLED: u32 = 0x8;

/// Actor flag bit set while the bound clip asked for a translucent draw (the
/// dance kind descriptor's anim-word bit `0x200`, folded into `+0x10` by
/// `FUN_801d1358`).
pub const FLAG_TRANSLUCENT: u32 = 0x0100_0000;

/// Beat / yaw value at or above which [`crate::dance::sprite_part_fade_weight`]
/// collapses the sprite weight to zero outright.
pub const BEAT_FADE_CEILING: u16 = 0x4000;

/// One minigame actor.
///
/// Every field is a retail slot; the doc comment on each names the offset and
/// which kernel reads it. A pool entry is plain data - the *behaviour* stays
/// in the ported kernels, which take the fields they need as arguments.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MinigameActor {
    /// `+0x10` - the actor flag word. [`FLAG_DRIVE_CLIP`] is the bit
    /// [`crate::dance::dance_clip_driver_gate`] tests; [`FLAG_KILLED`] and
    /// [`FLAG_TRANSLUCENT`] are the two other bits the minigame overlays
    /// write.
    pub flags: u32,
    /// `+0x14` / `+0x16` / `+0x18` - the actor's position triple. The dance
    /// emit dispatch reads the first two as its screen pair (rounding toward
    /// zero before a `>> 3`); the duel's impact spawn reads all three as a
    /// world position.
    pub pos: [i16; 3],
    /// `+0x26` - the yaw accumulator the groovy-move spin drives (retail
    /// wraps it at `0x1000` per turn).
    pub yaw: i16,
    /// `+0x4C` - the clip record the mirrored sprite pass caches after
    /// resolving `+0x5C` against a sprite archive. `None` until resolved.
    pub clip_record: Option<u32>,
    /// `+0x50` - the sprite word the emit dispatch ORs its semi-transparency
    /// flags into, and whose low nibble the marker arm stamps as a CLUT byte.
    pub sprite: u16,
    /// `+0x5A` - the live mask. The mirrored sprite pass retires an actor
    /// outright when this is empty, and clears one bit per pass whose cursor
    /// has run past its clip's end.
    pub live_mask: u16,
    /// `+0x5C` - the bound clip id, in both overlays that read it. The dance
    /// spawner writes `kind_desc[0x10] & 0x1FF` here (`FUN_801d0190` at
    /// `801d031c`) and `FUN_801d1358` rewrites it with each judge-returned move
    /// pair, so `FUN_801d4098`'s `> 0` test means "this actor has a clip
    /// bound"; the duel's sprite pool indexes an archive with the same slot's
    /// low bits (`FUN_801d49e8`). The field keeps its offset for a name because
    /// the *magnitude* is an archive-relative id in one overlay and a
    /// placement-space anim id in the other.
    pub field_5c: i16,
    /// `+0x68` - the frame cursor the mirrored sprite pass steps and restores.
    pub cursor: i16,
    /// `+0x78` - the beat / yaw halfword the fade-weight prologue reads.
    /// Retail compares it against [`BEAT_FADE_CEILING`] as a *signed 32-bit*
    /// value, which is why the port keeps it unsigned.
    pub beat: u16,
    /// `+0x90` .. `+0x94` - the transform template the emit dispatch's mode-0
    /// arm copies in and whose `+0x94` its mode-1 arm writes.
    pub template: [i16; 3],
    /// `+0x94` - the template's Z word (mode 1's store).
    pub template_z: i16,
    /// The draw mode the emit dispatch selects on (retail's `a1`, a jump-table
    /// index `0..=4`).
    pub draw_mode: u32,
    /// Rodata VA of the prototype this actor was spawned from, when it came
    /// through [`MinigameActorPool::spawn`]. `0` for a hand-built record.
    pub template_va: u32,
    /// Fixed-point spawn scale ([`SPAWN_SCALE`] for every retail minigame
    /// spawn).
    pub scale: i32,
}

impl MinigameActor {
    /// A live actor at `pos` with draw mode `draw_mode`.
    pub fn at(pos: [i16; 3], draw_mode: u32) -> Self {
        Self {
            pos,
            draw_mode,
            scale: SPAWN_SCALE,
            ..Default::default()
        }
    }

    /// `true` when the actor has been retired ([`FLAG_KILLED`] raised).
    pub fn killed(&self) -> bool {
        self.flags & FLAG_KILLED != 0
    }

    /// `true` when the actor's flag word forces the shared clip driver
    /// ([`FLAG_DRIVE_CLIP`]).
    pub fn drives_clip(&self) -> bool {
        self.flags & FLAG_DRIVE_CLIP != 0
    }

    /// Raise or clear [`FLAG_DRIVE_CLIP`].
    pub fn set_drives_clip(&mut self, on: bool) {
        if on {
            self.flags |= FLAG_DRIVE_CLIP;
        } else {
            self.flags &= !FLAG_DRIVE_CLIP;
        }
    }

    /// Raise or clear [`FLAG_TRANSLUCENT`] - the bit the bound clip's anim
    /// word (`0x200`) folds into `+0x10`.
    pub fn set_translucent(&mut self, on: bool) {
        if on {
            self.flags |= FLAG_TRANSLUCENT;
        } else {
            self.flags &= !FLAG_TRANSLUCENT;
        }
    }

    /// The screen pair the dance emit dispatch reads (`+0x14` / `+0x16`).
    pub fn screen_pair(&self) -> (i16, i16) {
        (self.pos[0], self.pos[1])
    }
}

/// The minigame actor pool: what the hub overlays spawn into and iterate.
///
/// Retail's pools are fixed-size overlay arrays, so the pool is bounded and a
/// spawn into a full pool retires the oldest entry rather than growing - the
/// same bound the play window's effect-part pool already applies.
#[derive(Debug, Clone, Default)]
pub struct MinigameActorPool {
    actors: Vec<MinigameActor>,
    capacity: usize,
}

/// Default pool bound. Retail's largest minigame pool is the free-play dance
/// floor's six dancers plus the floor tiles the per-frame pass spawns; the
/// bound exists to keep a mis-driven host from growing the pool without limit,
/// not to reproduce a specific overlay array's length.
pub const DEFAULT_POOL_CAPACITY: usize = 128;

impl MinigameActorPool {
    /// A pool bounded at [`DEFAULT_POOL_CAPACITY`].
    pub fn new() -> Self {
        Self::with_capacity(DEFAULT_POOL_CAPACITY)
    }

    /// A pool bounded at `capacity` (minimum 1).
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            actors: Vec::new(),
            capacity: capacity.max(1),
        }
    }

    /// Spawn one actor from a prototype VA at `pos`, mirroring the shared
    /// part-spawn API's argument tuple (`FUN_80021B04(pos, rot, record,
    /// scale)`). Returns its index in the pool.
    pub fn spawn(&mut self, template_va: u32, pos: [i16; 3], draw_mode: u32) -> usize {
        if self.actors.len() >= self.capacity {
            self.actors.remove(0);
        }
        self.actors.push(MinigameActor {
            template_va,
            ..MinigameActor::at(pos, draw_mode)
        });
        self.actors.len() - 1
    }

    /// Push an already-built record (the shape a per-frame rebuild uses).
    pub fn push(&mut self, actor: MinigameActor) -> usize {
        if self.actors.len() >= self.capacity {
            self.actors.remove(0);
        }
        self.actors.push(actor);
        self.actors.len() - 1
    }

    /// Drop every actor.
    pub fn clear(&mut self) {
        self.actors.clear();
    }

    /// Retire every actor whose [`FLAG_KILLED`] bit is up, and every actor
    /// whose live mask has emptied - the two retirement conditions the
    /// mirrored sprite pass and the shared teardown apply.
    pub fn retire_dead(&mut self) {
        self.actors.retain(|a| !a.killed());
    }

    /// Live actors.
    pub fn actors(&self) -> &[MinigameActor] {
        &self.actors
    }

    /// Live actors, mutably - the per-frame update path.
    pub fn actors_mut(&mut self) -> &mut [MinigameActor] {
        &mut self.actors
    }

    /// One actor by index.
    pub fn get(&self, i: usize) -> Option<&MinigameActor> {
        self.actors.get(i)
    }

    /// One actor by index, mutably.
    pub fn get_mut(&mut self, i: usize) -> Option<&mut MinigameActor> {
        self.actors.get_mut(i)
    }

    /// How many actors are live.
    pub fn len(&self) -> usize {
        self.actors.len()
    }

    /// `true` when nothing is spawned.
    pub fn is_empty(&self) -> bool {
        self.actors.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spawn_carries_the_retail_scale_and_template() {
        let mut pool = MinigameActorPool::new();
        let i = pool.spawn(0x801D_42FC, [0x40, -0x80, 0x100], 2);
        let a = pool.get(i).expect("spawned");
        assert_eq!(a.template_va, 0x801D_42FC);
        assert_eq!(a.scale, SPAWN_SCALE);
        assert_eq!(a.pos, [0x40, -0x80, 0x100]);
        assert_eq!(a.draw_mode, 2);
        assert_eq!(a.screen_pair(), (0x40, -0x80));
    }

    #[test]
    fn pool_is_bounded_and_drops_the_oldest() {
        let mut pool = MinigameActorPool::with_capacity(2);
        pool.spawn(1, [0, 0, 0], 0);
        pool.spawn(2, [0, 0, 0], 0);
        pool.spawn(3, [0, 0, 0], 0);
        assert_eq!(pool.len(), 2);
        assert_eq!(pool.actors()[0].template_va, 2);
        assert_eq!(pool.actors()[1].template_va, 3);
    }

    #[test]
    fn killed_actors_retire() {
        let mut pool = MinigameActorPool::new();
        pool.spawn(1, [0, 0, 0], 0);
        pool.spawn(2, [0, 0, 0], 0);
        pool.actors_mut()[0].flags |= FLAG_KILLED;
        pool.retire_dead();
        assert_eq!(pool.len(), 1);
        assert_eq!(pool.actors()[0].template_va, 2);
    }

    #[test]
    fn clip_drive_flag_round_trips() {
        let mut a = MinigameActor::at([0, 0, 0], 0);
        assert!(!a.drives_clip());
        a.set_drives_clip(true);
        assert_eq!(a.flags & FLAG_DRIVE_CLIP, FLAG_DRIVE_CLIP);
        assert!(a.drives_clip());
        a.set_drives_clip(false);
        assert!(!a.drives_clip());
    }
}
