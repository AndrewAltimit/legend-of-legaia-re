//! The field follow camera's **per-scene / per-tile parameters**: the
//! camera-region record loader, the camera composer that turns the loaded
//! parameter block plus the player's position into one target pose, and the
//! per-frame ease that walks the live camera globals toward that pose.
//!
//! PORT: FUN_801DBC20, FUN_801DAB90, FUN_801DB510, FUN_801DB8EC, FUN_801DE3E0
//! PORT: FUN_80019B28, FUN_8005B0B8
//! REF: FUN_801DBA20, FUN_80019278, FUN_801DBE9C, FUN_801DE840
//!
//! # The retail pipeline (field overlay, PROT 0897)
//!
//! Retail keeps a **camera parameter block** at `0x8007B606..0x8007B627`
//! ([`CameraZoneConfig`]) and rebuilds the camera from it every frame the
//! player moves:
//!
//! 1. **Load** - `FUN_801DBC20(record)` splits one 18-byte MAN section-3
//!    camera-region record into the block (three layouts, keyed on the high
//!    nibble of `record[5]`; see [`CameraZoneConfig::load_record`]). The
//!    callers are the field VM: op `0x45` LOAD hands it an inline record, and
//!    the tile re-query helper `FUN_801DE3E0(tile_x, tile_z)` runs
//!    `FUN_801DBA20` ([`crate::field_regions::zone_query`]) and loads the hit
//!    or else the fixed [`CameraZoneConfig::ZONE_MISS`] set when no record
//!    covers the tile. `FUN_801DE3E0` is reached from three field-VM arms
//!    (`[4C 38]`, `[4C 39]`, `[4C C4 x z]`), so **in retail the zone query is
//!    script-driven**. The arrival actor `FUN_801DBE9C` only queries on its
//!    `_DAT_8007B868 != 0` leg, and that word is the dev/dual-mode gate
//!    (`0` in retail); its retail leg re-pins the focus and snaps.
//! 2. **Compose** - `FUN_801DAB90(player, staging)` ([`compose`]) reads the
//!    block, the player's position, the player's floor height sampled
//!    through the MAN's own elevation LUT (`FUN_80019278` with the static
//!    table swapped in, so a scripted floor-tier bob never shakes the
//!    camera) and the walk-region attribute box at scratchpad
//!    `0x1F800384..87`, and writes a **staging descriptor** at `0x801F3580`:
//!    pitch `+0x02`, yaw `+0x06`, roll `+0x0A` (always `0`), the eye-space
//!    translation trio `+0x0E/+0x12/+0x16`, focus `+0x1A/+0x1E/+0x22` and
//!    GTE `H` `+0x26`.
//! 3. **Ease** - `FUN_801DB510(player)` ([`ease_step`]) runs from the player
//!    actor's per-frame handler (`FUN_801D2298`). While `DAT_8007B606` is
//!    set (retail boots it to `1`: `FUN_80034A6C` stores `B868 == 0`) and the
//!    player's `(X, footing, Z)` changed since the previous frame, it
//!    composes and then walks the six-entry descriptor list at `0x801F2798`
//!    (`(0x8007B790 pitch, +0x02, 2)`, `(0x8007B792 yaw, +0x06, 2)`,
//!    `(0x800840B8/BC/C0 eye trio, +0x0E/+0x12/+0x16, 4)`, `(0x8007B6F4 H,
//!    +0x26, 2)`), stepping each live global toward its staging field by
//!    `delta >> shift` (plus `delta >> (shift+1)` for the "two-shift" codes)
//!    plus `sign(delta)`, the shift coming from [`EASE_SHIFT_TABLE`] indexed
//!    by `B60B >> 4`. Roll is not in the list: the follow camera never rolls.
//!    A mode-5 shot additionally eases the focus X/Z toward its anchor tile.
//!    A player who stops mid-glide leaves the live camera where the ease got
//!    to - retail does not finish the glide until he moves again, unless
//!    scratchpad `0x1F800394 & 0x40000` (field-VM `2E 12`) forces the ease
//!    on a standing player (`0x801DB578..0x801DB5A4`). With `DAT_8007B606`
//!    clear or `0x1F800394 & 0x400` set, the routine takes its pin leg
//!    (`0x801DB820`) instead: focus = `-player`, no compose, no ease. The
//!    three gates are applied by `Camera::zone_follow_tick`
//!    (`crate::world::CAMERA_HOLD_FLAG`, `crate::world::CAMERA_FORCE_EASE_FLAG`,
//!    `ZoneFollow::follow_enabled`).
//! 4. **Snap** - `FUN_801DB8EC(player)` ([`snap`]) is the same compose + list
//!    walk with a plain copy instead of the ease, then `FUN_8003D254(H)`;
//!    the arrival actor, `[4C 39]` / `[4C 3E]`, and the leader-swap flow
//!    call it.
//!
//! # Units
//!
//! Angles are PSX 12-bit (`0x1000` = one turn); world positions are the
//! actor's `s16` coordinates (one tile = `0x80`); the attribute box is in
//! tiles; the eye trio and `H` are the GTE `TR` / `H` registers' own units.
//! Every halfword store in the composer is a `sh`, so the target fields are
//! [`i16`]; the live eye trio is a word global, so its ease runs in [`i32`].
//!
//! # Provenance
//!
//! `ghidra/scripts/funcs/overlay_0897_801dbc20.txt` (loader),
//! `overlay_cutscene_dialogue_801dab90.txt` (composer - byte-identical to the
//! 0897 copy at the same VA), `overlay_0897_801db510.txt` (ease),
//! `overlay_0897_801db8ec.txt` (snap), `overlay_0897_801de3e0.txt` (re-query
//! and miss defaults), `80019b28.txt` (bearing), `8005b0b8.txt` (square root),
//! and the two data tables read out of the 0897 image at `0x801F2798`
//! (descriptor list) and `0x801F2804` (shift table). The two LUT
//! reproductions here ([`atan_q11`], [`sqrt0`]) are pinned entry-for-entry
//! against `SCUS_942.54` by the disc-gated oracle in
//! `crates/engine-shell/tests/field_camera_zone_oracle.rs`.

use crate::field_regions::ZONE_RECORD_STRIDE;
use legaia_engine_vm::battle_action::motion::trig12;

/// `*(_DAT_8007B81C)[angle]` - the SCUS sine table (q3.12).
fn sin12(angle: u16) -> i32 {
    i32::from(trig12(angle).0)
}

/// `*(_DAT_8007B7F8)[angle]` - the same table a quarter turn on, i.e. the
/// cosine read.
fn cos12(angle: u16) -> i32 {
    i32::from(trig12(angle).1)
}

/// The per-frame ease shift, indexed by `DAT_8007B60B >> 4` (the 16-byte
/// table at `0x801F2804` in the field overlay). A code `>= 0x40` selects the
/// **two-shift** form `delta >> (code - 0x40)` + `delta >> (code - 0x40 + 1)`;
/// a code of `0` is a one-frame snap (the step is the whole delta).
pub const EASE_SHIFT_TABLE: [u8; 16] = [0, 5, 4, 3, 2, 6, 7, 8, 0, 69, 68, 67, 0, 0, 0, 0];

/// The camera parameter block `0x8007B606..0x8007B627`, one field per
/// retail global. Field names carry the global each mirrors.
///
/// `B60C` / `B610` / `B614` / `B618` are also the four registers the field-VM
/// op-`0x43` zone ramps write ([`crate::register_ramp::RampSlot`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CameraZoneConfig {
    /// `DAT_8007B607` - the record's `byte[5]`: high nibble = camera mode
    /// (`1`/`2` anchor-follow with a signed X sweep, `3` look-at anchor, `4`
    /// aim-from-box-centre, `5` fixed shot, anything else = no sweep), low
    /// nibble = the mode's strength.
    pub mode: u8,
    /// `DAT_8007B608` - pitch sweep over the box's Z extent (nibble `1`/`2`
    /// = sign; other high nibbles pin the pitch to `0x1B8`).
    pub b608: u8,
    /// `DAT_8007B609` - eye-depth sweep over the box's Z extent, anchored
    /// `(nibble - 1) / 4` of the span below the box's far Z edge.
    pub b609: u8,
    /// `DAT_8007B60A` - floor-height-coupled pitch: nibble `1..=4` add
    /// `strength * floor / 4`, nibble `5` adds `strength * floor / 8` and a
    /// sin/cos compensation of the eye height and depth by the player's
    /// footing.
    pub b60a: u8,
    /// `DAT_8007B60B` - high nibble = the ease shift code (see
    /// [`EASE_SHIFT_TABLE`]); nibbles `1..=11` also lower the eye by
    /// `floor * (low nibble)`.
    pub b60b: u8,
    /// `DAT_8007B60C` - base pitch (the mode-3 arm ignores it).
    pub pitch: i32,
    /// `DAT_8007B610` - base yaw (modes `3` / `4` derive theirs instead).
    pub yaw: i32,
    /// `DAT_8007B614` - eye-space depth (mode 3: a bias on the anchor
    /// distance; mode 4: its sign flips the orbit side).
    pub depth: i32,
    /// `DAT_8007B618` - GTE `H`, every mode.
    pub h: i32,
    /// `DAT_8007B61C` - anchor / focus tile X (modes 3 and 5).
    pub anchor_x: i32,
    /// `DAT_8007B620` - mode 3: anchor height in `0x20`-unit steps; mode 5:
    /// signed height offset rotated by the pitch.
    pub anchor_h: i32,
    /// `DAT_8007B624` - anchor / focus tile Z (modes 3 and 5).
    pub anchor_z: i32,
}

impl CameraZoneConfig {
    /// The block as the SCUS BSS leaves it at boot: every field zero. A
    /// scene entered before any script loaded a record composes from this
    /// (yaw `0x64`, pitch `0x1B8`, depth `0`, `H` `0`).
    pub const BOOT: Self = Self {
        mode: 0,
        b608: 0,
        b609: 0,
        b60a: 0,
        b60b: 0,
        pitch: 0,
        yaw: 0,
        depth: 0,
        h: 0,
        anchor_x: 0,
        anchor_h: 0,
        anchor_z: 0,
    };

    /// The set the tile re-query installs when **no record covers the
    /// tile** (`FUN_801DE3E0`'s miss arm at `0x801DE408..0x801DE464`, and
    /// the same nine stores in `FUN_801DBE9C`'s dev leg): anchor-follow at
    /// strength `0`, pitch `0x1B8`, yaw `0`, depth `0x4000`, `H` `0x300`, a
    /// nibble-5 floor coupling and the shift-4 ease. The anchor cells are
    /// not written and keep their previous values.
    pub const ZONE_MISS: Self = Self {
        mode: 0x10,
        b608: 0x10,
        b609: 0x30,
        b60a: 0x51,
        b60b: 0x20,
        pitch: 0x1B8,
        yaw: 0,
        depth: 0x4000,
        h: 0x300,
        anchor_x: 0,
        anchor_h: 0,
        anchor_z: 0,
    };

    /// Install the miss defaults, keeping the three anchor cells (the miss
    /// arm never writes them).
    pub fn load_zone_miss(&mut self) {
        let keep = (self.anchor_x, self.anchor_h, self.anchor_z);
        *self = Self::ZONE_MISS;
        (self.anchor_x, self.anchor_h, self.anchor_z) = keep;
    }

    /// The block as it sits in retail RAM: 40 bytes starting at
    /// `0x8007B600` (`+6` enable byte, `+7` mode, `+8..+B` the four sweep
    /// bytes, then the seven word cells `+0xC..+0x27`). For oracles that
    /// read a save state; the enable byte at `+6` is not part of the config.
    pub fn from_retail_block(block: &[u8]) -> Option<Self> {
        if block.len() < 0x28 {
            return None;
        }
        let w = |o: usize| i32::from_le_bytes([block[o], block[o + 1], block[o + 2], block[o + 3]]);
        Some(Self {
            mode: block[7],
            b608: block[8],
            b609: block[9],
            b60a: block[0xA],
            b60b: block[0xB],
            pitch: w(0xC),
            yaw: w(0x10),
            depth: w(0x14),
            h: w(0x18),
            anchor_x: w(0x1C),
            anchor_h: w(0x20),
            anchor_z: w(0x24),
        })
    }

    /// The ease shift code this block selects (`EASE_SHIFT_TABLE[B60B >> 4]`).
    pub fn ease_shift(&self) -> u8 {
        EASE_SHIFT_TABLE[usize::from(self.b60b >> 4)]
    }

    /// The camera mode nibble (`B607 >> 4`).
    pub fn mode_nibble(&self) -> u8 {
        self.mode >> 4
    }

    /// Load one 18-byte camera-region record - the port of `FUN_801DBC20`.
    ///
    /// Returns the **visible tile window** side-write when the record is a
    /// mask-kind record (`2 <= record[0] < 0x20`) with `record[1] != 0`: the
    /// four bytes `[record[3], record[4], record[1], record[2]]` retail
    /// stores to scratchpad `0x1F8003E8..EB` before the split (see
    /// `docs/formats/encounter.md`). `None` for a kind-0/1 record, and for a
    /// mode-6 record, which returns before writing anything ("keep the
    /// current camera").
    ///
    /// The split, from the disassembly (`s16` = the sign-extended little-
    /// endian halfword `FUN_8003CE9C` reads):
    ///
    /// | field | every other mode | mode 3 | mode 5 |
    /// |---|---|---|---|
    /// | `mode` | `[5]` | `[5]` | `[5]` |
    /// | `b608` | `[6]` | - | - |
    /// | `b609` | `[7]` | - | - |
    /// | `b60a` | `[8]` | `[8]` | - |
    /// | `b60b` | `[9]` | `[9]` | `[9]` |
    /// | `yaw` | s16 `[10]` | - | s16 `[10]` |
    /// | `pitch` | s16 `[12]` | - | s16 `[12]` |
    /// | `depth` | s16 `[14]` | s16 `[14]` | s16 `[14]` |
    /// | `h` | s16 `[16]` | s16 `[16]` | s16 `[16]` |
    /// | `anchor_x` | - | s16 `[6]` | u8 `[6]` |
    /// | `anchor_h` | - | s16 `[10]` | s8 `[8]` |
    /// | `anchor_z` | - | s16 `[12]` | u8 `[7]` |
    ///
    /// (Mode 5's `anchor_h` is a byte the loader ORs `0xFFFF_FF00` into when
    /// it is `>= 0x80`.)
    // PORT: FUN_801DBC20
    pub fn load_record(&mut self, rec: &[u8; ZONE_RECORD_STRIDE]) -> Option<[u8; 4]> {
        if rec[5] >> 4 == 6 {
            return None;
        }
        let s16 = |o: usize| i32::from(i16::from_le_bytes([rec[o], rec[o + 1]]));
        let window = (rec[0].wrapping_sub(2) < 0x1E && rec[1] != 0)
            .then_some([rec[3], rec[4], rec[1], rec[2]]);
        self.mode = rec[5];
        match rec[5] >> 4 {
            3 => {
                self.anchor_x = s16(6);
                self.anchor_z = s16(12);
                self.b60a = rec[8];
                self.b60b = rec[9];
                self.anchor_h = s16(10);
                self.depth = s16(14);
                self.h = s16(16);
            }
            5 => {
                self.anchor_x = i32::from(rec[6]);
                self.anchor_z = i32::from(rec[7]);
                self.anchor_h = i32::from(rec[8]);
                self.b60b = rec[9];
                self.yaw = s16(10);
                self.pitch = s16(12);
                self.depth = s16(14);
                self.h = s16(16);
                if self.anchor_h >= 0x80 {
                    self.anchor_h |= -0x100;
                }
            }
            _ => {
                self.b608 = rec[6];
                self.b609 = rec[7];
                self.b60a = rec[8];
                self.b60b = rec[9];
                self.yaw = s16(10);
                self.pitch = s16(12);
                self.depth = s16(14);
                self.h = s16(16);
            }
        }
        window
    }
}

impl Default for CameraZoneConfig {
    fn default() -> Self {
        Self::BOOT
    }
}

/// What the composer reads besides the parameter block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ComposeInputs {
    /// The player actor's `(+0x14 X, +0x16 footing, +0x18 Z)`.
    pub player: [i32; 3],
    /// `FUN_80019278(player)` - the floor height under the player, sampled
    /// through the scene's elevation LUT.
    pub floor_y: i32,
    /// The walk-region attribute box at scratchpad `0x1F800384..87`, in the
    /// order the composer indexes it: `[x_lo, z_lo, x_hi, z_hi]` tiles
    /// ([`crate::field_regions::RegionAttributes::box_bytes`]).
    pub attr_box: [u8; 4],
    /// The live `_DAT_8007B790` pitch - the composer seeds the staging
    /// pitch from it before the mode arms overwrite it.
    pub live_pitch: i32,
    /// The live `_DAT_8007B792` yaw. Mode 4 rewrites it in place while
    /// unwrapping the ease's shortest path - see [`Composed::live_yaw`].
    pub live_yaw: i32,
    /// `DAT_8007B6A8 != 0` - the scene MAN's low header bit, which halves
    /// the eye height in the `B60A` nibble-5 arm.
    pub half_eye_y: bool,
}

/// The composed target pose - the staging descriptor's camera fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CameraTarget {
    /// Staging `+0x02` -> `_DAT_8007B790`.
    pub pitch: i16,
    /// Staging `+0x06` -> `_DAT_8007B792`.
    pub yaw: i16,
    /// Staging `+0x0E/+0x12/+0x16` -> `_DAT_800840B8/BC/C0` (sign-extended
    /// into the word globals by the snap and the ease alike).
    pub eye: [i16; 3],
    /// Staging `+0x26` -> `_DAT_8007B6F4`.
    pub h: i16,
}

/// A [`compose`] result: the target plus the one side effect the composer
/// has on the live globals.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Composed {
    pub target: CameraTarget,
    /// `Some(v)` when the composer rewrote `_DAT_8007B792` (the mode-4 arm
    /// masks the live yaw to `0xFFF` and may add a turn so the ease takes
    /// the short way round); the caller stores it back.
    pub live_yaw: Option<i16>,
}

fn clamp_i16(v: i32) -> i16 {
    v.clamp(-0x8000, 0x7FFF) as i16
}

/// MIPS `div` (truncating), with the divide-by-zero trap read as `0` - a
/// degenerate attribute box would raise `break 0x1C00` in retail.
fn div(a: i32, b: i32) -> i32 {
    if b == 0 { 0 } else { a.wrapping_div(b) }
}

/// The composer - the port of `FUN_801DAB90(player, staging)`.
///
/// Every arithmetic step keeps the retail width: `sh` stores truncate to
/// 16 bits, `lhu` re-reads are zero-extended, `lh` re-reads sign-extended,
/// `mult`/`div` are 32-bit wrapping / truncating. See the module docs for
/// the arms; the per-nibble formulas are spelled out inline.
// PORT: FUN_801DAB90
pub fn compose(cfg: &CameraZoneConfig, inp: &ComposeInputs) -> Composed {
    let [px, footing, pz] = inp.player;
    let floor = inp.floor_y;
    let [bx0, bz0, bx1, bz1] = inp.attr_box.map(i32::from);
    let lo16 = |v: i32| v & 0xFFFF; // `lhu` of a word global / staging field
    let sx16 = |v: i32| i32::from(v as i16); // `lh`

    // The staging pose is seeded from the live globals in retail, but every
    // arm overwrites all five fields before they are read (mode 4's unwrap
    // reads the live yaw from `inp` directly), so the seed is not modelled.
    let (mut pitch, mut yaw, mut eye_x, mut eye_y, mut eye_z): (i32, i32, i32, i32, i32);
    let mut live_yaw: Option<i16> = None;
    let mode = cfg.mode >> 4;

    match mode {
        4 => {
            // Aim from the attribute box's centre at the player.
            eye_x = 0;
            pitch = lo16(cfg.pitch);
            eye_z = lo16(cfg.depth);
            let cx = (bx1 + bx0) << 6;
            let cz = (bz1 + bz0) << 6;
            let ang = i32::from(bearing(cx, cz, px, pz));
            yaw = lo16(ang - 0x400);
            if cfg.depth < 0 {
                yaw = lo16(ang - 0xC00);
                eye_z = lo16(-lo16(cfg.depth));
            }
            // Shortest-path unwrap between the live yaw and the target, both
            // masked to one turn; the live global is rewritten in place.
            let mut lv = inp.live_yaw & 0xFFF;
            yaw &= 0xFFF;
            if sx16(lv) < 0x400 && yaw >= 0xC01 {
                lv = lo16(lv + 0x1000);
            }
            if sx16(yaw) < 0x400 && sx16(lv) >= 0xC01 {
                yaw = lo16(yaw + 0x1000);
            }
            live_yaw = Some(lv as i16);
            // -> the B60B stage (the `0x200` retail stores first is what the
            // stage rewrites).
            eye_y = dy_stage(cfg, floor);
        }
        3 => {
            // Look-at anchor: yaw and pitch aimed from the anchor at the
            // player, depth from the 3D distance.
            let ax = (cfg.anchor_x << 7) + 0x40;
            let az = (cfg.anchor_z << 7) + 0x40;
            let ah = cfg.anchor_h << 5;
            yaw = lo16(i32::from(bearing(ax, az, px, pz)) - 0x400);
            pitch = lo16(i32::from(bearing(ah, az, floor, pz)) - 0x400);
            eye_x = 0;
            eye_y = 0x200;
            let dx = ax.wrapping_sub(px);
            let dz = az.wrapping_sub(pz);
            let dh = ah.wrapping_add(footing);
            let sum = dx
                .wrapping_mul(dx)
                .wrapping_add(dz.wrapping_mul(dz))
                .wrapping_add(dh.wrapping_mul(dh));
            let k = i32::from(cfg.mode & 0xF) + 1;
            let v = sqrt0(sum).wrapping_mul(k);
            let v = v.wrapping_mul(6) >> 10;
            let d = v.wrapping_add(cfg.depth).wrapping_sub(0x4000);
            eye_z = i32::from(clamp_i16(d));
            // -> the B60A stage (the dy stage is skipped: eye Y stays 0x200).
        }
        5 => {
            // Fixed shot: the block's own pitch / yaw / depth, the height
            // offset rotated by the pitch into (dy, depth).
            eye_y = 0x200;
            pitch = lo16(cfg.pitch);
            yaw = lo16(cfg.yaw);
            eye_x = 0;
            eye_z = lo16(cfg.depth);
            let s = cos12((pitch & 0xFFF) as u16).wrapping_mul(cfg.anchor_h);
            let v = s.wrapping_mul(3).wrapping_shl(6) >> 12;
            eye_y = lo16(eye_y - v);
            let c = sin12((pitch & 0xFFF) as u16).wrapping_mul(cfg.anchor_h);
            let v = c.wrapping_shl(5) >> 12;
            eye_z = i32::from(clamp_i16(sx16(eye_z) - v));
            return Composed {
                target: CameraTarget {
                    pitch: pitch as i16,
                    yaw: yaw as i16,
                    eye: [eye_x as i16, eye_y as i16, eye_z as i16],
                    h: lo16(cfg.h) as i16,
                },
                live_yaw: None,
            };
        }
        _ => {
            // Anchor-follow with the position-proportional sweeps. Eye X
            // keeps the live value until the depth fix-up below rewrites it.
            eye_x = 0;
            let half_x = (bx1 - bx0) << 6;
            let strength = i32::from(cfg.mode & 0xF);
            yaw = match mode {
                1 | 2 => {
                    let cx = (bx0 + bx1 + 1) << 6;
                    let v = strength.wrapping_mul((cx - px) << 6);
                    let v = div(v, half_x);
                    if mode == 1 {
                        lo16(lo16(cfg.yaw) + v)
                    } else {
                        lo16(lo16(cfg.yaw) - v)
                    }
                }
                _ => 0x64,
            };
            let s8 = i32::from(cfg.b608 & 0xF);
            let span_z = (bz1 - bz0) << 7;
            let cz = (bz0 + bz1) << 6;
            pitch = match cfg.b608 >> 4 {
                1 => lo16(lo16(cfg.pitch) + div(s8.wrapping_mul((cz - pz) << 7), span_z)),
                2 => lo16(lo16(cfg.pitch) + div(s8.wrapping_mul((pz - cz) << 7), span_z)),
                _ => 0x1B8,
            };
            let s9 = i32::from(cfg.b609 & 0xF);
            let n9 = i32::from(cfg.b609 >> 4);
            eye_z = if (1..6).contains(&n9) {
                let mut q = span_z.wrapping_mul(n9 - 1);
                if q < 0 {
                    q += 3;
                }
                q >>= 2;
                let zref = (bz1 << 7) - q;
                let v = div(s9.wrapping_mul((pz - zref) << 10), span_z);
                lo16(lo16(cfg.depth) + v)
            } else {
                lo16(cfg.depth)
            };
            eye_y = dy_stage(cfg, floor);
        }
    }

    // The B60A stage: floor-height-coupled pitch (+ footing compensation).
    let na = cfg.b60a >> 4;
    let sa = i32::from(cfg.b60a & 0xF);
    match na {
        0 => {}
        1..=4 => {
            let v = div(sa.wrapping_mul(floor), 4);
            pitch = lo16(pitch + v);
        }
        5 => {
            let v = div(sa.wrapping_mul(floor), 8);
            pitch = lo16(pitch + v);
            let s = cos12((pitch & 0xFFF) as u16).wrapping_mul(footing);
            let v = s.wrapping_mul(6) >> 12;
            eye_y = lo16(eye_y - v);
            if inp.half_eye_y {
                eye_y = lo16(sx16(eye_y) >> 1);
            }
            let c = sin12((pitch & 0xFFF) as u16).wrapping_mul(footing);
            let v = c >> 12;
            eye_z = i32::from(clamp_i16(sx16(eye_z) - v));
        }
        _ => {}
    }

    let h = lo16(cfg.h);
    if mode != 4 {
        // Eye X / Y ride the depth: `-(depth >> 7)` and `+ depth >> 8`.
        let z = sx16(eye_z);
        eye_x = lo16(-(z >> 7));
        eye_y = lo16(eye_y + (z >> 8));
    }
    Composed {
        target: CameraTarget {
            pitch: pitch as i16,
            yaw: yaw as i16,
            eye: [eye_x as i16, eye_y as i16, eye_z as i16],
            h: h as i16,
        },
        live_yaw,
    }
}

/// The `B60B` stage: eye Y = `0x200 - floor * (low nibble)` for high
/// nibbles `1..=11`, plain `0x200` otherwise.
fn dy_stage(cfg: &CameraZoneConfig, floor: i32) -> i32 {
    let n = cfg.b60b >> 4;
    if (1..12).contains(&n) {
        let k = i32::from(cfg.b60b & 0xF);
        ((-floor).wrapping_mul(k) + 0x200) & 0xFFFF
    } else {
        0x200
    }
}

/// One ease step of `FUN_801DB510`'s descriptor walk for a word-wide live
/// global: `live + (delta >> s) [+ (delta >> (s + 1))] + sign(delta)`, with
/// `s` and the two-shift flag decoded from the shift `code`
/// ([`EASE_SHIFT_TABLE`]).
// PORT: FUN_801DB510
pub fn ease_step(live: i32, target: i32, code: u8) -> i32 {
    let two = code >= 0x40;
    let s = u32::from(if two { code - 0x40 } else { code });
    let d = target.wrapping_sub(live);
    let mut step = d >> s;
    if two {
        step = step.wrapping_add(d >> (s + 1));
    }
    let mut v = live.wrapping_add(step);
    if d > 0 {
        v = v.wrapping_add(1);
    }
    if d < 0 {
        v = v.wrapping_sub(1);
    }
    v
}

/// [`ease_step`] for a halfword live global (pitch / yaw / `H`): both
/// operands are `lh` reads and the result is a `sh` store.
pub fn ease_step_i16(live: i16, target: i16, code: u8) -> i16 {
    ease_step(i32::from(live), i32::from(target), code) as i16
}

/// The snap (`FUN_801DB8EC`'s list walk) reduces to a copy; this is the one
/// place its width rule lives: a halfword target sign-extends into the word
/// eye globals.
// PORT: FUN_801DB8EC
pub fn snap(target: &CameraTarget) -> (i32, i32, [i32; 3], i32) {
    (
        i32::from(target.pitch),
        i32::from(target.yaw),
        target.eye.map(i32::from),
        i32::from(target.h),
    )
}

/// The **focus edge clamp** - the port of `FUN_801DAA50`, the routine every
/// caller of the ease and the snap runs immediately after them (the field
/// per-frame update at `0x801D183C` / `0x801D22C0`, the player seat at
/// `0x801D2010`, and the `[4C 39]` / `[4C 3E]` arms at `0x801E10CC`).
///
/// It keeps the camera's focus point inside the walk region the attribute
/// refresh latched, widened by the camera's own visible-tile window, so the
/// lens never pans far enough past a room's edge to show the void behind it.
/// Operates on the focus globals **as retail stores them** -
/// `_DAT_80089118` = `-X`, `_DAT_80089120` = `-Z` - which is why the "min"
/// clamps are the far edges and the "max" clamps the near ones.
///
/// ```text
/// if mode_nibble == 5: unchanged            // a fixed shot frames itself
/// pad = half_eye ? -0x14 : 1                // _DAT_8007B6A8
/// if attribute type byte != 0 {             // 0x1F80037C
///   fx = min(fx, (2   - (box[0] - win[0])) * 0x80)
///   fz = min(fz, (4   - (box[1] - win[1])) * 0x80)
///   fx = max(fx, (      win[2] - box[2])   * 0x80)
///   fz = max(fz, (pad - (box[3] - win[3])) * 0x80)
/// }
/// if script_focus.x != 0 { fx = -script_focus.x }   // _DAT_8007B628
/// if script_focus.z != 0 { fz = -script_focus.z }   // _DAT_8007B62A
/// ```
///
/// `box` is the scratchpad attribute box `0x1F800384..87` in the retail
/// store order (`[rec[0], rec[3], rec[2], rec[1]]`, unsigned bytes);
/// `window` is `0x1F8003E8..EB` read as **signed** bytes, whose field
/// default is [`crate::mode_entry_init::FIELD_DEFAULT_VIEW_WINDOW`] and
/// whose per-record override is the mask-kind side-write
/// [`CameraZoneConfig::load_record`] returns.
// PORT: FUN_801DAA50
pub fn clamp_focus(
    focus_stored: [i32; 2],
    mode_nibble: u8,
    attr_kind_latched: bool,
    attr_box: [u8; 4],
    window: [i8; 4],
    half_eye: bool,
    script_focus: [i16; 2],
) -> [i32; 2] {
    let [mut fx, mut fz] = focus_stored;
    if mode_nibble == 5 {
        return [fx, fz];
    }
    let pad = if half_eye { -0x14 } else { 1 };
    let b = attr_box.map(i32::from);
    let w = window.map(i32::from);
    if attr_kind_latched {
        fx = fx.min((2 - (b[0] - w[0])) * 0x80);
        fz = fz.min((4 - (b[1] - w[1])) * 0x80);
        fx = fx.max((w[2] - b[2]) * 0x80);
        fz = fz.max((pad - (b[3] - w[3])) * 0x80);
    }
    if script_focus[0] != 0 {
        fx = -i32::from(script_focus[0]);
    }
    if script_focus[1] != 0 {
        fz = -i32::from(script_focus[1]);
    }
    [fx, fz]
}

/// The retail arctangent table at `0x8006F4C8`: 2049 entries over the ratio
/// `i / 2048` (`0 <= i <= 2048`), each `trunc(atan(i/2048) * 4096 / 2pi)` -
/// the disc-gated oracle pins every entry.
pub fn atan_q11(i: i32) -> i32 {
    let i = i.clamp(0, 2048) as f64;
    ((i / 2048.0).atan() * 4096.0 / std::f64::consts::TAU).trunc() as i32
}

/// The bearing from `(ax, az)` to `(bx, bz)` in PSX 12-bit units, `0` =
/// `+X`, `0x400` = `+Z`, `0x800` = `-X`, `0xC00` = `-Z` - the port of
/// `FUN_80019B28`. Octant folding over the absolute deltas, one table
/// lookup on the smaller-over-larger ratio in q11.
// PORT: FUN_80019B28
pub fn bearing(ax: i32, az: i32, bx: i32, bz: i32) -> u16 {
    let mut dx = bx.wrapping_sub(ax);
    let mut dz = bz.wrapping_sub(az);
    let mut q = 0u8;
    if dx < 0 {
        dx = dx.wrapping_neg();
        q = 2;
    }
    if dz < 0 {
        dz = dz.wrapping_neg();
        q += 1;
    }
    let atan = |num: i32, den: i32| atan_q11(div(num.wrapping_shl(11), den));
    let v = match q {
        0 => {
            if dx < dz {
                if dx == 0 { 0x400 } else { 0x400 - atan(dx, dz) }
            } else if dz == 0 {
                0
            } else {
                atan(dz, dx)
            }
        }
        1 => {
            if dx < dz {
                if dx == 0 { 0xC00 } else { atan(dx, dz) + 0xC00 }
            } else if dz == 0 {
                0x1000
            } else {
                0x1000 - atan(dz, dx)
            }
        }
        2 => {
            if dx < dz {
                if dx == 0 { 0x400 } else { atan(dx, dz) + 0x400 }
            } else if dz == 0 {
                0x800
            } else {
                0x800 - atan(dz, dx)
            }
        }
        _ => {
            if dx < dz {
                if dx == 0 { 0xC00 } else { 0xC00 - atan(dx, dz) }
            } else if dz == 0 {
                0x800
            } else {
                atan(dz, dx) + 0x800
            }
        }
    };
    (v & 0xFFF) as u16
}

/// The 192-entry mantissa table behind `FUN_8005B0B8` (at `0x80078E84`):
/// `trunc(sqrt((64 + i) / 64) * 4096)` for `0 <= i < 0xC0` - pinned
/// entry-for-entry by the disc-gated oracle.
pub fn sqrt0_lut(i: usize) -> i32 {
    (((64 + i) as f64 / 64.0).sqrt() * 4096.0).trunc() as i32
}

/// The PsyQ-shaped square root the mode-3 depth uses - the port of
/// `FUN_8005B0B8`. Normalises the argument to a 7/8-bit mantissa with the
/// GTE's leading-zero count, looks it up, and shifts back: the result is
/// `sqrt(a) * 64` (q26.6), e.g. `sqrt0(0x4000) == 8192`. `0` for `a <= 0`
/// (the count of a non-positive word is what the table cannot index).
// PORT: FUN_8005B0B8
pub fn sqrt0(a: i32) -> i32 {
    if a <= 0 {
        return 0;
    }
    let lz = a.leading_zeros() as i32;
    let t2 = lz & !1;
    let t1 = (0x13 - t2) >> 1;
    let t3 = t2 - 0x18;
    let t4 = if t3 >= 0 { a << t3 } else { a >> (0x18 - t2) };
    let idx = (t4 - 0x40).clamp(0, 0xBF) as usize;
    let m = sqrt0_lut(idx);
    if t1 >= 0 { m << t1 } else { m >> (-t1) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(bytes: &[u8]) -> [u8; ZONE_RECORD_STRIDE] {
        let mut r = [0u8; ZONE_RECORD_STRIDE];
        r[..bytes.len()].copy_from_slice(bytes);
        r
    }

    #[test]
    fn loader_sweep_split_lands_every_field_and_side_writes_the_window() {
        let mut cfg = CameraZoneConfig::BOOT;
        // kind 2 (mask kind) with byte[1] != 0 -> window side-write.
        let r = rec(&[
            2, 0x11, 0x22, 0x33, 0x44, 0x1A, 0x21, 0x32, 0x51, 0x20, 0x60, 0xFF, 0xB8, 0x01, 0x00,
            0x40, 0x00, 0x02,
        ]);
        let win = cfg.load_record(&r);
        assert_eq!(win, Some([0x33, 0x44, 0x11, 0x22]));
        assert_eq!(cfg.mode, 0x1A);
        assert_eq!(
            (cfg.b608, cfg.b609, cfg.b60a, cfg.b60b),
            (0x21, 0x32, 0x51, 0x20)
        );
        assert_eq!(cfg.yaw, -160, "s16 at +10 sign-extends");
        assert_eq!(cfg.pitch, 0x1B8);
        assert_eq!(cfg.depth, 0x4000);
        assert_eq!(cfg.h, 0x200);
        // A kind-1 record never side-writes.
        let mut r1 = r;
        r1[0] = 1;
        assert_eq!(cfg.load_record(&r1), None);
    }

    #[test]
    fn loader_mode_three_and_five_take_their_own_splits() {
        let mut cfg = CameraZoneConfig::BOOT;
        let r3 = rec(&[
            1, 0, 0, 9, 9, 0x32, 0x10, 0x00, 0x51, 0x20, 0x05, 0x00, 0x20, 0x00, 0x00, 0x30, 0x00,
            0x03,
        ]);
        assert_eq!(cfg.load_record(&r3), None);
        assert_eq!(cfg.mode, 0x32);
        assert_eq!(cfg.anchor_x, 0x10);
        assert_eq!(cfg.anchor_h, 5);
        assert_eq!(cfg.anchor_z, 0x20);
        assert_eq!((cfg.b60a, cfg.b60b), (0x51, 0x20));
        assert_eq!(cfg.depth, 0x3000);
        assert_eq!(cfg.h, 0x300);
        // b608 / b609 / pitch / yaw untouched by the mode-3 split.
        assert_eq!((cfg.b608, cfg.b609, cfg.pitch, cfg.yaw), (0, 0, 0, 0));

        let r5 = rec(&[
            1, 0, 0, 9, 9, 0x50, 0x12, 0x34, 0xF0, 0x20, 0x64, 0x00, 0xB8, 0x01, 0x00, 0x40, 0x00,
            0x02,
        ]);
        cfg.load_record(&r5);
        assert_eq!(cfg.mode, 0x50);
        assert_eq!((cfg.anchor_x, cfg.anchor_z), (0x12, 0x34));
        assert_eq!(cfg.anchor_h, -16, "a byte >= 0x80 sign-extends");
        assert_eq!(
            (cfg.yaw, cfg.pitch, cfg.depth, cfg.h),
            (0x64, 0x1B8, 0x4000, 0x200)
        );
    }

    #[test]
    fn mode_six_keeps_the_current_block() {
        let mut cfg = CameraZoneConfig::ZONE_MISS;
        let r = rec(&[1, 0, 0, 9, 9, 0x60, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12]);
        assert_eq!(cfg.load_record(&r), None);
        assert_eq!(cfg, CameraZoneConfig::ZONE_MISS);
    }

    #[test]
    fn zone_miss_keeps_the_anchor_cells() {
        let mut cfg = CameraZoneConfig {
            anchor_x: 7,
            anchor_h: 8,
            anchor_z: 9,
            ..CameraZoneConfig::BOOT
        };
        cfg.load_zone_miss();
        assert_eq!((cfg.anchor_x, cfg.anchor_h, cfg.anchor_z), (7, 8, 9));
        assert_eq!(cfg.mode, 0x10);
        assert_eq!(cfg.ease_shift(), 4, "B60B = 0x20 -> shift 4");
    }

    #[test]
    fn retail_block_reader_round_trips_the_cells() {
        let mut block = [0u8; 0x28];
        block[7] = 0x1A;
        block[8] = 0x21;
        block[0xC..0x10].copy_from_slice(&0x1B8i32.to_le_bytes());
        block[0x10..0x14].copy_from_slice(&(-160i32).to_le_bytes());
        block[0x18..0x1C].copy_from_slice(&0x200i32.to_le_bytes());
        let cfg = CameraZoneConfig::from_retail_block(&block).unwrap();
        assert_eq!(
            (cfg.mode, cfg.b608, cfg.pitch, cfg.yaw, cfg.h),
            (0x1A, 0x21, 0x1B8, -160, 0x200)
        );
        assert!(CameraZoneConfig::from_retail_block(&block[..10]).is_none());
    }

    fn inputs(px: i32, pz: i32, floor: i32) -> ComposeInputs {
        ComposeInputs {
            player: [px, floor, pz],
            floor_y: floor,
            attr_box: [0, 0, 0x7F, 0x7F],
            live_pitch: 0x1B8,
            live_yaw: 0x64,
            half_eye_y: false,
        }
    }

    /// The zone-miss set on flat ground: pitch `0x1B8`, yaw `0`, depth
    /// `0x4000`, `H` `0x300`, eye X / Y riding the depth.
    #[test]
    fn zone_miss_on_flat_ground_composes_the_documented_defaults() {
        let c = compose(&CameraZoneConfig::ZONE_MISS, &inputs(0x1000, 0x1000, 0));
        assert_eq!(c.live_yaw, None);
        let t = c.target;
        assert_eq!(t.pitch, 0x1B8);
        assert_eq!(t.yaw, 0);
        assert_eq!(t.h, 0x300);
        assert_eq!(t.eye[2], 0x4000);
        assert_eq!(t.eye[0], -(0x4000 >> 7));
        assert_eq!(t.eye[1], 0x200 + (0x4000 >> 8));
    }

    /// The `B60A` nibble-5 arm: a raised floor tilts the camera down by
    /// `floor / 8` and the footing compensation moves eye Y and depth.
    #[test]
    fn nibble_five_floor_coupling_tilts_by_an_eighth() {
        let flat = compose(&CameraZoneConfig::ZONE_MISS, &inputs(0x1000, 0x1000, 0)).target;
        let raised = compose(&CameraZoneConfig::ZONE_MISS, &inputs(0x1000, 0x1000, 80)).target;
        assert_eq!(raised.pitch, flat.pitch + 10);
        assert_ne!(raised.eye[1], flat.eye[1]);
        assert_ne!(raised.eye[2], flat.eye[2]);
    }

    /// Mode 1 sweeps the yaw with the player's X across the box, mode 2 the
    /// other way; the sweep is zero at the box centre.
    #[test]
    fn x_sweep_is_signed_by_the_mode_nibble_and_zero_at_the_centre() {
        let mut cfg = CameraZoneConfig::ZONE_MISS;
        cfg.mode = 0x14;
        let mut inp = inputs(0, 0, 0);
        inp.attr_box = [10, 10, 20, 20];
        // Box centre X in world units: (10 + 20 + 1) << 6.
        inp.player[0] = (10 + 20 + 1) << 6;
        assert_eq!(compose(&cfg, &inp).target.yaw, 0);
        inp.player[0] = 10 << 7;
        let left = compose(&cfg, &inp).target.yaw;
        assert!(
            left > 0,
            "mode 1: yaw {left} swings positive west of centre"
        );
        cfg.mode = 0x24;
        assert_eq!(compose(&cfg, &inp).target.yaw, -left);
        // Neither 1 nor 2: the yaw is the fixed 0x64.
        cfg.mode = 0x04;
        assert_eq!(compose(&cfg, &inp).target.yaw, 0x64);
    }

    #[test]
    fn mode_five_is_the_fixed_shot_with_a_rotated_height_offset() {
        let cfg = CameraZoneConfig {
            mode: 0x50,
            b60b: 0x20,
            pitch: 0x200,
            yaw: 0x300,
            depth: 0x3000,
            h: 0x280,
            anchor_h: 0,
            ..CameraZoneConfig::BOOT
        };
        let t = compose(&cfg, &inputs(5, 6, 7)).target;
        assert_eq!(
            (t.pitch, t.yaw, t.eye, t.h),
            (0x200, 0x300, [0, 0x200, 0x3000], 0x280)
        );
        // A height offset lifts the eye by cos(pitch) * h * 3 / 64 and pulls
        // the depth in by sin(pitch) * h / 128.
        let cfg2 = CameraZoneConfig {
            anchor_h: 64,
            ..cfg
        };
        let t2 = compose(&cfg2, &inputs(5, 6, 7)).target;
        let s = cos12(0x200) * 64;
        let c = sin12(0x200) * 64;
        assert_eq!(i32::from(t2.eye[1]), 0x200 - ((s * 3) << 6 >> 12));
        assert_eq!(i32::from(t2.eye[2]), 0x3000 - (c << 5 >> 12));
    }

    #[test]
    fn mode_four_aims_from_the_box_centre_and_unwraps_the_live_yaw() {
        let cfg = CameraZoneConfig {
            mode: 0x40,
            b60b: 0x20,
            pitch: 0x1C0,
            depth: 0x2000,
            h: 0x200,
            ..CameraZoneConfig::BOOT
        };
        let mut inp = inputs(0, 0, 0);
        inp.attr_box = [0, 0, 8, 8];
        // Player east and a little north of the centre: bearing ~0x41 ->
        // yaw = bearing - 0x400, masked into the top quadrant.
        inp.player = [(8 << 6) + 1000, 0, (8 << 6) + 100];
        inp.live_yaw = 0x10;
        let c = compose(&cfg, &inp);
        let want =
            (i32::from(bearing(8 << 6, 8 << 6, inp.player[0], inp.player[2])) - 0x400) & 0xFFF;
        assert!(want > 0xC00, "test geometry: {want:#x}");
        assert_eq!(i32::from(c.target.yaw), want);
        // Live yaw below 0x400 with a target above 0xC00: the live global
        // gains a turn so the ease runs the short way.
        assert_eq!(c.live_yaw, Some(0x1010));
        // Exactly 0xC00 is NOT above 0xC00 (`slti 0xC01`): due east keeps
        // the live yaw as it was.
        let mut east = inp;
        east.player = [(8 << 6) + 1000, 0, 8 << 6];
        let e = compose(&cfg, &east);
        assert_eq!(e.target.yaw, 0xC00);
        assert_eq!(e.live_yaw, Some(0x10));
        assert_eq!(c.target.pitch, 0x1C0);
        assert_eq!(
            c.target.eye,
            [0, 0x200, 0x2000],
            "mode 4 skips the depth fix-up"
        );
        // A negative depth flips the orbit side and the depth's sign.
        let flipped = CameraZoneConfig {
            depth: -0x2000,
            ..cfg
        };
        let f = compose(&flipped, &east);
        assert_eq!(f.target.eye[2], 0x2000);
        assert_eq!(f.target.yaw, 0x400);
    }

    #[test]
    fn mode_three_looks_from_the_anchor_at_the_player() {
        let cfg = CameraZoneConfig {
            mode: 0x30,
            b60a: 0,
            anchor_x: 4,
            anchor_z: 4,
            anchor_h: 2,
            depth: 0x4000,
            h: 0x200,
            ..CameraZoneConfig::BOOT
        };
        // Player due +Z of the anchor at ground level.
        let ax = (4 << 7) + 0x40;
        let az = (4 << 7) + 0x40;
        let c = compose(&cfg, &inputs(ax, az + 640, 0));
        assert_eq!(c.target.yaw, 0, "bearing +Z (0x400) - 0x400");
        // Distance 640 * ((0 & 0xF) + 1) * 6 / 1024 * 64 -> 240, plus depth - 0x4000.
        assert_eq!(
            i32::from(c.target.eye[2]),
            (sqrt0(640 * 640 + 64 * 64) * 6) >> 10
        );
        assert_eq!(c.target.h, 0x200);
    }

    #[test]
    fn ease_converges_and_snaps_on_code_zero() {
        // Shift 4 from 0 toward 1000: first step 1000 >> 4 + 1 = 63.
        assert_eq!(ease_step(0, 1000, 4), 63);
        // Two-shift 0x45: 1000 >> 5 + 1000 >> 6 + 1 = 31 + 15 + 1.
        assert_eq!(ease_step(0, 1000, 0x45), 47);
        // Negative deltas floor the shift and step one past.
        assert_eq!(ease_step(0, -1, 4), -2);
        // Code 0 lands one past the target - the retail overshoot.
        assert_eq!(ease_step(0, 1000, 0), 1001);
        let mut v = 0i16;
        for _ in 0..400 {
            v = ease_step_i16(v, -160, 4);
        }
        assert_eq!(v, -160, "a moving player converges exactly");
        assert_eq!(ease_step(5, 5, 4), 5, "no delta, no motion");
    }

    #[test]
    fn bearing_hits_the_four_cardinals_and_is_monotone_in_between() {
        assert_eq!(bearing(0, 0, 10, 0), 0);
        assert_eq!(bearing(0, 0, 0, 10), 0x400);
        assert_eq!(bearing(0, 0, -10, 0), 0x800);
        assert_eq!(bearing(0, 0, 0, -10), 0xC00);
        assert_eq!(bearing(0, 0, 0, 0), 0);
        assert_eq!(bearing(0, 0, 10, 10), 0x200);
        // Against a float atan2 in the same convention, within one unit.
        for (dx, dz) in [(100, 37), (-53, 91), (-77, -12), (9, -400)] {
            let want = (dz as f64).atan2(dx as f64) * 4096.0 / std::f64::consts::TAU;
            let want = want.rem_euclid(4096.0);
            let got = f64::from(bearing(0, 0, dx, dz));
            assert!((got - want).abs() < 1.5, "({dx},{dz}): {got} vs {want}");
        }
    }

    #[test]
    fn sqrt0_is_the_square_root_in_q6() {
        assert_eq!(sqrt0(0), 0);
        assert_eq!(sqrt0(1), 64);
        assert_eq!(sqrt0(4), 128);
        assert_eq!(sqrt0(100), 640);
        assert_eq!(sqrt0(0x4000), 8192);
        assert_eq!(sqrt0(0x10000), 16384);
        for a in [123_456i32, 2_000_000, 50_000_000, i32::MAX] {
            let exact = (a as f64).sqrt() * 64.0;
            let got = f64::from(sqrt0(a));
            assert!((got - exact).abs() / exact < 0.01, "{a}: {got} vs {exact}");
        }
    }
}
