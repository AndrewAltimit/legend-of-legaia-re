//! Effect sprite-atlas + animation catalog: [`SpriteAtlasEntry`],
//! [`AnimFrame`], [`AnimBatch`], and the [`EffectCatalog`]. Split out of
//! `effect_vm.rs`.

use super::*;

/// One inline sprite-atlas entry from the runtime effect buffer (the 8-byte
/// records between `buffer+8` and `pack0`). This is the PSX sprite UV packet
/// the per-frame walker (`FUN_801E0088` pass 2) reads to build each child
/// sprite's GPU primitive. The exact byte layout is pinned from that consumer
/// (dump `overlay_battle_801e0088.txt`, the sprite-emit block ~0x801e0840):
/// it reads `atlas[0]=u`, `atlas[1]=v`, `atlas[2]=w`, `atlas[3]=h` as bytes,
/// copies the **u16 at `atlas+4`** into the primitive's **CLUT** field
/// (`POLY_FT4` word3 high half), and the **byte at `atlas+6`** into the
/// primitive's **tpage** field (`POLY_FT4` word5 high half). (The fields are
/// the reverse of an earlier reading: `atlas+4` is the CBA, not the tpage -
/// `0x7680` decodes as CLUT `(0, 474)`, an effect-CLUT row, not page `(0,0)`.)
/// The texel rectangle is `(u, v)..(u+w-1, v+h-1)`; the pixels live in VRAM,
/// uploaded by the effect-texture loaders (PROT 870 / `etim`).
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
    /// `+4` CLUT (CBA) id - the u16 the emit writes into the primitive's CLUT
    /// field. Effect sprites point into the effect-CLUT rows (473..=480).
    pub clut: u16,
    /// `+6` PSX `tpage` descriptor byte (texture-page X/Y base + colour mode +
    /// semi-transparency), zero-extended into the GPU primitive's tpage field.
    /// Effect sprites select the loaded effect pages (e.g. `0x25` = page
    /// `(320,0)` 4bpp, a PROT 870 flame-atlas page).
    pub page: u16,
    /// `+7` unknown / reserved byte.
    pub unk: u8,
}

/// One frame of a pack0 animation batch. The first byte indexes the sprite
/// atlas (which texel rect to draw this frame). Of the trailing bytes the
/// retail walker reads exactly two: `timing[0]` is the frame's hold delay
/// (frames, `<<3` into the child's 5.3 wait counter) and `timing[1]` is the
/// per-frame motion speed scalar multiplying the child velocity;
/// `timing[2..=4]` are never read (`overlay_battle_801e0088.txt`).
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
                clut: u16::from_le_bytes([buf[p + 4], buf[p + 5]]),
                page: buf[p + 6] as u16,
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
                // Retail reads a single byte here (pack0 anim-batch index);
                // rec[1] is the master's post-spawn delay, NOT the high byte
                // of a u16 id (see docs/formats/effect.md).
                sprite_id: rec[0] as u16,
                delay: rec[1],
                width: i16::from_le_bytes([rec[2], rec[3]]),
                height: i16::from_le_bytes([rec[4], rec[5]]),
                depth: i16::from_le_bytes([rec[6], rec[7]]),
                velocity: [
                    i16::from_le_bytes([rec[8], rec[9]]),
                    i16::from_le_bytes([rec[10], rec[11]]),
                    i16::from_le_bytes([rec[12], rec[13]]),
                ],
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
                        sprite_id: cb[0] as u16,
                        delay: cb[1],
                        width: i16::from_le_bytes([cb[2], cb[3]]),
                        height: i16::from_le_bytes([cb[4], cb[5]]),
                        depth: i16::from_le_bytes([cb[6], cb[7]]),
                        velocity: [
                            i16::from_le_bytes([cb[8], cb[9]]),
                            i16::from_le_bytes([cb[10], cb[11]]),
                            i16::from_le_bytes([cb[12], cb[13]]),
                        ],
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
