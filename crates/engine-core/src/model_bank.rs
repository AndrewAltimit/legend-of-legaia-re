//! The retail **model pool** - `DAT_8007C018`, the global array of registered
//! TMD pointers - and the single id space that a placement's model byte and
//! the scripted-motion VM's op `0x0E` operand both index.
//!
//! # The pool
//!
//! `FUN_80026B4C` (`tmd_register`) is the only *registrar*: it validates the
//! TMD magic `0x80000002`, stores the pointer at `DAT_8007C018 + n*4`
//! (`sw a0,0x0(v1)` at `0x80026BA8`), publishes `n` to `0x8007BB38` and
//! returns `n` as the model's id. It is not the only *writer* - both
//! reference scanners agree on ten base-forming sites for `0x8007C018` across
//! SCUS and every extracted overlay, and the one other store is
//! `sw zero,0x0(v0)` at `0x801CF1E0` in PROT 0976 (Baka Fighter), blanking the
//! slot at the current counter. `n` lives at
//! `DAT_8007B774` and is **reset to `*(u32*)0x8007B824` on every stage init**
//! (`FUN_8001E1B4` epilogue, `sw v1,-0x488c(at)` at `0x8001E3AC`), so ids below
//! that watermark survive a scene change and ids at or above it are reissued
//! per stage.
//!
//! Three things register, in this order:
//!
//! | pool range | content | who registers it |
//! |---|---|---|
//! | `[0, 0x8007B824)` | nothing in retail - the watermark is `0` | `FUN_8001F05C`'s `s7 != 0` arm, which never runs |
//! | `[0x8007B824, 0x8007B6F8)` | the five PROT 0874 §0 player meshes | `FUN_8001E890` loop at `0x8001EB4C` |
//! | `[0x8007B6F8, ...)` | the scene's own models | `FUN_8001F05C` type `0x02` / `0x09` arms |
//!
//! `FUN_8001E890` sets `0x8007B6F8 = pack_count + *(u32*)0x8007B824`
//! (`0x8001EB10..0x8001EB20`, a word store), i.e. one past the player pack it just
//! registered. Measured on three field save states (`town01`, `koin1`,
//! `izumi`): `0x8007B824 = 0`, `0x8007B6F8 = 5`, `DAT_8007C018[0..=4]`
//! identical across all three and equal to `*(gp+0x6BC) + 0x18` - the PROT
//! 0874 §0 pack's five members past its `4 + 5*4` header.
//!
//! # The id space
//!
//! Both consumers split the id at `0xF0`, unsigned, against the same two
//! bases:
//!
//! * the placement spawner `FUN_8003A1E4` (`sltiu` at `0x8003A2DC`, the two
//!   `lhu`s at `0x8003A2F0` / `0x8003A314`), and
//! * the scripted-motion VM's op `0x0E` (`sltiu` at `0x800393B8`, the two
//!   `lhu`s at `0x800393D0` / `0x80039418`).
//!
//! Below `0xF0` the id resolves against `*(u16*)0x8007B6F8` - the scene bank.
//! At or above `0xF0` it resolves `id - 0xF0` against `*(u16*)0x8007B824` -
//! the player bank - and raises the translucent-draw bit. What the compare
//! gives is that `0xF0` is *party slot 0*: `FUN_8001E890`'s epilogue walks
//! three entries from the base (`slti v0,s0,0x3` at `0x8001EBA8`) and
//! `FUN_8001EBEC` indexes `pool[*(0x8007B824) + i]` with the same `i` it uses
//! for the per-character equipment bytes (`0x8001EC54`). Naming that slot is
//! the §0 pack's order, not the compare: `0xF0` Vahn,
//! `0xF1` Noa, `0xF2` Gala and `0xF3` / `0xF4` the two auxiliary slots
//! ([`legaia_asset::character_pack`]). The field overlay's own MAIN INIT binds
//! the player actor the same way (`lhu v0,-0x47dc(v0)` into `actor+0x64` at
//! `0x801D6F88..0x801D6F90`), which is the cross-check that `0x8007B824` is
//! the player bank's base and not a scene quantity.
//!
//! # What the scene bank is made of
//!
//! Registration order, and nothing else. Over the 90 `scene_asset_table`
//! bundles on the disc, 618 descriptors, **zero** descriptor `data_offset`s
//! fall outside their own PROT entry, 80 carry a type-`0x02` TMD pack and
//! none carries a type-`0x09` single. Every one of those 80 packs LZS-decodes
//! and every member's first word is `0x80000002`. The ten bundles with no
//! type-`0x02` descriptor take their models from a type-`0x02` **DATA_FIELD
//! chunk** in the block's streaming entry instead (`bubu1` 173 members,
//! `edbubu` 160).
//!
//! `scene_tmd_stream` entries do **not** contribute: `town01` ships four of
//! them, `koin1` and `izumi` one each, and all three scenes' measured pool
//! sizes (`DAT_8007B774 - 5` = 114 / 159 / 50) equal their bundle pack's
//! member count exactly (114 / 159 / 50).
//!
//! # Why this module exists
//!
//! [`crate::scene_resources::SceneResources::tmds`] is a **magic scan** over
//! the scene's raw entries, so it cannot see a TMD inside an LZS-compressed
//! bundle descriptor. For `koin3` it finds 1 model and for `other7` 0, where
//! the registration order holds 77 and 65 - which is what made op `0x0E`'s
//! operands look unresolvable. They are not: all 215 authored operands index
//! their scene's bank.
//!
//! REF: FUN_80026B4C, FUN_8001E890, FUN_8001E1B4, FUN_8001F05C, FUN_80020F88,
//! REF: FUN_8003A1E4, FUN_80038158

use legaia_asset::AssetType;

use crate::scene::Scene;
use crate::scene_bundle::find_bundle;

/// `*(u32*)0x8007B824` - the first pool id the player bank occupies, and the
/// value `FUN_8001E1B4` resets the registration counter to. Measured `0` on
/// every field save state; the one writer (`FUN_8001F05C`'s `s7 != 0` arm at
/// `0x8001F2F8`) is unreachable because its destination `*(0x8007B8CC)` has no
/// writer anywhere on the disc.
pub const PLAYER_BANK_BASE: u16 = 0;

/// Members of the PROT 0874 §0 player pack - the `count` word the pack opens
/// with, and [`legaia_asset::character_pack::SLOT_COUNT`].
pub const PLAYER_BANK_LEN: u16 = legaia_asset::character_pack::SLOT_COUNT as u16;

/// `*(u16*)0x8007B6F8` - the first pool id the current scene's own models
/// occupy. `FUN_8001E890` computes it as `player_pack_count + 0x8007B824`
/// and stores it as a **word** (`sw v0,-0x4908(at)` at `0x8001EB20`); every
/// consumer reads the low halfword back with `lhu`.
pub const SCENE_BANK_BASE: u16 = PLAYER_BANK_BASE + PLAYER_BANK_LEN;

/// The unsigned split both consumers apply to a model id.
pub const SPECIAL_MODEL_THRESHOLD: u16 = 0xF0;

/// Which half of the pool an id names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelBank {
    /// `id >= 0xF0` - the PROT 0874 §0 player pack, offset `id - 0xF0`. The
    /// consumers also raise the translucent-draw bit `actor[+0x10] & 0x0100_0000`
    /// on this arm.
    Player,
    /// `id < 0xF0` - the scene's own registered models, offset `id`.
    Scene,
}

/// One resolved model id.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModelRef {
    /// Which bank the id landed in.
    pub bank: ModelBank,
    /// Index within that bank.
    pub index: u16,
    /// The `DAT_8007C018` slot retail would reach, using the measured bases.
    pub pool_index: u16,
    /// `true` on the `>= 0xF0` arm, where retail ORs `0x0100_0000` into
    /// `actor[+0x10]` instead of clearing it.
    pub translucent: bool,
}

/// Apply the `0xF0` split exactly the way `FUN_8003A1E4` and op `0x0E` do.
///
/// Both read the operand as a **signed halfword** and then compare it
/// `sltiu`-style against `0xF0`, so a negative operand takes the `>= 0xF0`
/// arm; retail's `operand - 0xF0` then wraps in 16 bits, which is reproduced
/// here with `wrapping_sub`.
///
/// The split itself is **caller-side**. `FUN_80024E08` receives an already
/// resolved pool index in `a1` and only re-binds with it (`sh zero,0x5c(s0)`,
/// `sh a1,0x64(s0)`, reload); its body holds no `sltiu ...,0xf0`. The compare
/// and the two `lhu` bank bases live in the two callers - `0x800393B8` /
/// `0x800393D0` / `0x80039418` in the scripted-motion VM's op `0x0E`, and
/// `0x8003A2DC` / `0x8003A2F0` / `0x8003A314` in the placement spawner
/// `FUN_8003A1E4`. Op `0x0E` sign-extends a halfword operand; the spawner
/// reads a plain `lbu` byte, so only the former can present an id `>= 0x8000`.
///
/// PORT: FUN_8003A1E4 - the placement spawner's bank select at
/// `0x8003A2CC..0x8003A328`: `sltiu ...,0xF0` at `0x8003A2DC`, then either the
/// scene base `lhu` at `0x8003A2F0` or, on the other arm, `+0xFF10` (`-0xF0`)
/// with the player base `lhu` at `0x8003A314` and the translucent bit
/// materialised at `0x8003A304`. The scripted-motion VM's op-`0x0E` arm at
/// `0x800393B8..0x8003942C` is the same select instruction for instruction,
/// over a sign-extended halfword instead of the spawner's `lbu` byte.
/// REF: FUN_80024E08 - what both arms then *call*, with the resolved pool
/// index already in `a1`. That body holds no `0xF0` compare and no bank base;
/// it re-binds the actor (`+0x5C = 0`, `+0x64 = id`, reload), and the `+0x60`
/// mirror it writes under `_DAT_8007B83C == 0xF` is a game-mode branch the
/// engine has no seat for.
pub fn resolve_model_id(id: i16) -> ModelRef {
    let raw = id as u16;
    if raw < SPECIAL_MODEL_THRESHOLD {
        ModelRef {
            bank: ModelBank::Scene,
            index: raw,
            pool_index: SCENE_BANK_BASE.wrapping_add(raw),
            translucent: false,
        }
    } else {
        let index = raw.wrapping_sub(SPECIAL_MODEL_THRESHOLD);
        ModelRef {
            bank: ModelBank::Player,
            index,
            pool_index: PLAYER_BANK_BASE.wrapping_add(index),
            translucent: true,
        }
    }
}

/// Where one scene-bank model's bytes come from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelSource {
    /// A member of a type-`0x02` pack sitting in a bundle descriptor. The
    /// payload is LZS-compressed at `bundle.table_offset() + data_offset`
    /// within the entry.
    BundlePack {
        /// PROT entry the bundle lives in.
        entry_idx: u32,
        /// Descriptor slot (`0..count`).
        descriptor: usize,
        /// Member index within the pack.
        member: usize,
    },
    /// A type-`0x09` single TMD in a bundle descriptor. No retail scene ships
    /// one, but the dispatcher arm exists and registers exactly one model.
    BundleSingle { entry_idx: u32, descriptor: usize },
    /// A member of a type-`0x02` pack carried as an uncompressed DATA_FIELD
    /// chunk in one of the block's streaming entries.
    StreamPack {
        entry_idx: u32,
        /// Byte offset of the chunk's 4-byte header within the entry.
        chunk_header: usize,
        member: usize,
    },
    /// A type-`0x09` single TMD carried as a DATA_FIELD chunk.
    StreamSingle { entry_idx: u32, chunk_header: usize },
    /// A member of a type-`0x02` pack in a descriptor table the strict
    /// bundle detector does not accept - the MAN-less count-`5` table a
    /// v12-family scene such as `balden2` carries in its `lzs_container`
    /// entry, read with the retail walk
    /// ([`legaia_asset::scene_asset_table::descriptor_bundle_walk`]).
    WalkedPack {
        entry_idx: u32,
        descriptor: usize,
        member: usize,
    },
}

/// One scene's model bank: the pool ids `SCENE_BANK_BASE..` in registration
/// order.
#[derive(Debug, Clone, Default)]
pub struct SceneModelBank {
    sources: Vec<ModelSource>,
}

impl SceneModelBank {
    /// Walk a loaded scene the way the retail loader registers its models:
    /// the bundle's descriptors in table order, then every streaming entry's
    /// DATA_FIELD chunks in entry order.
    ///
    /// PORT: FUN_80026B4C
    ///
    /// `SceneHost::model_bank` holds one per loaded scene, rebuilt on every
    /// field entry; both hosts resolve a scripted mesh re-bind's operand
    /// through it ([`World::field_npc_live_model`] then [`Self::tmd_bytes`]) -
    /// the native window in `upload_assets`, the browser through the
    /// `play_npc_live_model` export its NPC draw consults.
    ///
    /// [`World::field_npc_live_model`]: crate::world::World::field_npc_live_model
    pub fn build(scene: &Scene) -> Self {
        let mut sources = Vec::new();
        let bundle_entry = if let Some(bundle) = find_bundle(scene) {
            let entry_idx = bundle.entry_idx();
            let bytes = bundle.bytes();
            let table_offset = bundle.table_offset();
            for (i, d) in bundle.descriptors().iter().enumerate() {
                match AssetType::from_byte(d.type_byte) {
                    AssetType::Tmd => {
                        let start = table_offset.saturating_add(d.data_offset as usize);
                        let Some(body) = bytes.get(start..) else {
                            continue;
                        };
                        let Ok(decoded) = legaia_lzs::decompress(body, d.size as usize) else {
                            continue;
                        };
                        let Ok(entries) = legaia_asset::pack::parse_pack(&decoded) else {
                            continue;
                        };
                        for member in 0..entries.len() {
                            sources.push(ModelSource::BundlePack {
                                entry_idx,
                                descriptor: i,
                                member,
                            });
                        }
                    }
                    AssetType::Tmd2 => sources.push(ModelSource::BundleSingle {
                        entry_idx,
                        descriptor: i,
                    }),
                    _ => {}
                }
            }
            Some(entry_idx)
        } else {
            // No bundle the strict detector accepts. A v12-family scene
            // (`balden2`) still ships its models in a descriptor table - the
            // count-5, MAN-less one its `lzs_container` entry opens with,
            // since its MAN rides the streaming variant - and `FUN_80020224`
            // walks it with no count constraint. Read it the same way.
            let mut walked = None;
            for entry in &scene.entries {
                if entry.class != legaia_asset::categorize::Class::LzsContainer {
                    continue;
                }
                let Some(descs) =
                    legaia_asset::scene_asset_table::descriptor_bundle_walk(&entry.bytes)
                else {
                    continue;
                };
                let before = sources.len();
                for (i, d) in descs.iter().enumerate() {
                    if AssetType::from_byte(d.type_byte) != AssetType::Tmd {
                        continue;
                    }
                    let Some(members) = walked_pack(&entry.bytes, d).map(|p| p.len()) else {
                        continue;
                    };
                    for member in 0..members {
                        sources.push(ModelSource::WalkedPack {
                            entry_idx: entry.idx,
                            descriptor: i,
                            member,
                        });
                    }
                }
                if sources.len() > before {
                    walked = Some(entry.idx);
                    break;
                }
            }
            walked
        };

        for entry in &scene.entries {
            if Some(entry.idx) == bundle_entry {
                continue;
            }
            if !matches!(
                entry.class,
                legaia_asset::categorize::Class::DataFieldStreaming
            ) {
                continue;
            }
            let Ok(report) = legaia_asset::parse_streaming(&entry.bytes, MAX_STREAM_CHUNKS) else {
                continue;
            };
            for chunk in &report.chunks {
                let data = chunk.header_offset + 4;
                match AssetType::from_byte(chunk.type_byte) {
                    AssetType::Tmd => {
                        let end = data.saturating_add(chunk.size as usize);
                        let Some(body) = entry.bytes.get(data..end) else {
                            continue;
                        };
                        let Ok(entries) = legaia_asset::pack::parse_pack(body) else {
                            continue;
                        };
                        for member in 0..entries.len() {
                            sources.push(ModelSource::StreamPack {
                                entry_idx: entry.idx,
                                chunk_header: chunk.header_offset,
                                member,
                            });
                        }
                    }
                    AssetType::Tmd2 => sources.push(ModelSource::StreamSingle {
                        entry_idx: entry.idx,
                        chunk_header: chunk.header_offset,
                    }),
                    _ => {}
                }
            }
        }

        SceneModelBank { sources }
    }

    /// How many models the scene registers - retail's
    /// `DAT_8007B774 - *(u16*)0x8007B6F8` once the scene has loaded.
    pub fn len(&self) -> usize {
        self.sources.len()
    }

    /// `true` when the scene registers no models at all.
    pub fn is_empty(&self) -> bool {
        self.sources.is_empty()
    }

    /// Every source in registration order.
    pub fn sources(&self) -> &[ModelSource] {
        &self.sources
    }

    /// Where the bytes for one **scene-bank** index live, or `None` when the
    /// index is past what the scene registered.
    pub fn scene_source(&self, index: u16) -> Option<ModelSource> {
        self.sources.get(index as usize).copied()
    }

    /// Resolve a raw model id - a placement's `model_index` or an op-`0x0E`
    /// operand - all the way to a byte source.
    ///
    /// Returns `None` for the [`ModelBank::Player`] arm: those five meshes
    /// come from PROT 0874, which is not one of this scene's entries.
    /// [`legaia_asset::character_pack`] is that half.
    pub fn source_for_model_id(&self, id: i16) -> Option<ModelSource> {
        let r = resolve_model_id(id);
        match r.bank {
            ModelBank::Scene => self.scene_source(r.index),
            ModelBank::Player => None,
        }
    }

    /// Materialise one scene-bank model's TMD bytes out of the scene entries.
    pub fn tmd_bytes(&self, scene: &Scene, id: i16) -> Option<Vec<u8>> {
        let source = self.source_for_model_id(id)?;
        match source {
            ModelSource::BundlePack {
                descriptor, member, ..
            } => {
                let bundle = find_bundle(scene)?;
                let d = bundle.descriptors()[descriptor];
                let start = bundle.table_offset().saturating_add(d.data_offset as usize);
                let decoded =
                    legaia_lzs::decompress(bundle.bytes().get(start..)?, d.size as usize).ok()?;
                let entries = legaia_asset::pack::parse_pack(&decoded).ok()?;
                let e = entries.get(member)?;
                decoded
                    .get(e.byte_offset..e.byte_offset + e.size)
                    .map(<[u8]>::to_vec)
            }
            ModelSource::BundleSingle { descriptor, .. } => {
                let bundle = find_bundle(scene)?;
                let d = bundle.descriptors()[descriptor];
                let start = bundle.table_offset().saturating_add(d.data_offset as usize);
                legaia_lzs::decompress(bundle.bytes().get(start..)?, d.size as usize).ok()
            }
            ModelSource::StreamPack {
                entry_idx,
                chunk_header,
                member,
            } => {
                let entry = scene.entries.iter().find(|e| e.idx == entry_idx)?;
                let (data, size) = stream_chunk_body(&entry.bytes, chunk_header)?;
                let body = entry.bytes.get(data..data + size)?;
                let entries = legaia_asset::pack::parse_pack(body).ok()?;
                let e = entries.get(member)?;
                body.get(e.byte_offset..e.byte_offset + e.size)
                    .map(<[u8]>::to_vec)
            }
            ModelSource::StreamSingle {
                entry_idx,
                chunk_header,
            } => {
                let entry = scene.entries.iter().find(|e| e.idx == entry_idx)?;
                let (data, size) = stream_chunk_body(&entry.bytes, chunk_header)?;
                entry.bytes.get(data..data + size).map(<[u8]>::to_vec)
            }
            ModelSource::WalkedPack {
                entry_idx,
                descriptor,
                member,
            } => walked_member(scene, entry_idx, descriptor, member),
        }
    }
}

/// Decode one walked table's type-`0x02` descriptor and split its pack:
/// `(decoded payload, member spans)`.
fn walked_pack(
    entry: &[u8],
    d: &legaia_asset::scene_asset_table::DescriptorRecord,
) -> Option<Vec<(usize, usize)>> {
    let decoded =
        legaia_lzs::decompress(entry.get(d.data_offset as usize..)?, d.size as usize).ok()?;
    let entries = legaia_asset::pack::parse_pack(&decoded).ok()?;
    Some(entries.iter().map(|e| (e.byte_offset, e.size)).collect())
}

/// One [`ModelSource::WalkedPack`] member's bytes.
fn walked_member(
    scene: &Scene,
    entry_idx: u32,
    descriptor: usize,
    member: usize,
) -> Option<Vec<u8>> {
    let entry = scene.entries.iter().find(|e| e.idx == entry_idx)?;
    let descs = legaia_asset::scene_asset_table::descriptor_bundle_walk(&entry.bytes)?;
    let d = descs.get(descriptor)?;
    let decoded =
        legaia_lzs::decompress(entry.bytes.get(d.data_offset as usize..)?, d.size as usize).ok()?;
    let entries = legaia_asset::pack::parse_pack(&decoded).ok()?;
    let e = entries.get(member)?;
    decoded
        .get(e.byte_offset..e.byte_offset + e.size)
        .map(<[u8]>::to_vec)
}

impl SceneModelBank {
    /// Every scene-bank model's TMD bytes, in registration order - index `i`
    /// is pool slot [`SCENE_BANK_BASE`]` + i`. Each pack is decoded once
    /// (where [`Self::tmd_bytes`] decodes per call), so this is the form to
    /// take when the whole bank is wanted. `None` where a source's bytes do
    /// not materialise.
    pub fn materialise(&self, scene: &Scene) -> Vec<Option<Vec<u8>>> {
        let mut packs: std::collections::HashMap<(u32, usize, bool), Vec<u8>> =
            std::collections::HashMap::new();
        let bundle = find_bundle(scene);
        let member = |body: &[u8], m: usize| -> Option<Vec<u8>> {
            let entries = legaia_asset::pack::parse_pack(body).ok()?;
            let e = entries.get(m)?;
            body.get(e.byte_offset..e.byte_offset + e.size)
                .map(<[u8]>::to_vec)
        };
        self.sources
            .iter()
            .map(|src| match *src {
                ModelSource::BundlePack {
                    entry_idx,
                    descriptor,
                    member: m,
                } => {
                    let key = (entry_idx, descriptor, true);
                    if let std::collections::hash_map::Entry::Vacant(slot) = packs.entry(key) {
                        let b = bundle.as_ref()?;
                        let d = b.descriptors()[descriptor];
                        let start = b.table_offset().saturating_add(d.data_offset as usize);
                        let decoded =
                            legaia_lzs::decompress(b.bytes().get(start..)?, d.size as usize)
                                .ok()?;
                        slot.insert(decoded);
                    }
                    member(packs.get(&key)?, m)
                }
                ModelSource::StreamPack {
                    entry_idx,
                    chunk_header,
                    member: m,
                } => {
                    let entry = scene.entries.iter().find(|e| e.idx == entry_idx)?;
                    let (data, size) = stream_chunk_body(&entry.bytes, chunk_header)?;
                    member(entry.bytes.get(data..data + size)?, m)
                }
                ModelSource::WalkedPack {
                    entry_idx,
                    descriptor,
                    member: m,
                } => {
                    let key = (entry_idx, descriptor, false);
                    if let std::collections::hash_map::Entry::Vacant(slot) = packs.entry(key) {
                        let entry = scene.entries.iter().find(|e| e.idx == entry_idx)?;
                        let descs =
                            legaia_asset::scene_asset_table::descriptor_bundle_walk(&entry.bytes)?;
                        let d = descs.get(descriptor)?;
                        let decoded = legaia_lzs::decompress(
                            entry.bytes.get(d.data_offset as usize..)?,
                            d.size as usize,
                        )
                        .ok()?;
                        slot.insert(decoded);
                    }
                    member(packs.get(&key)?, m)
                }
                ModelSource::BundleSingle { .. } | ModelSource::StreamSingle { .. } => {
                    let index = self.sources.iter().position(|s| s == src)?;
                    self.tmd_bytes(scene, index as i16)
                }
            })
            .collect()
    }
}

/// Bound on the DATA_FIELD chunk walk - the same cap the `asset` CLI uses.
const MAX_STREAM_CHUNKS: usize = 4096;

/// `(data_offset, size)` of the chunk whose header sits at `header_offset`.
fn stream_chunk_body(bytes: &[u8], header_offset: usize) -> Option<(usize, usize)> {
    let raw = bytes.get(header_offset..header_offset + 4)?;
    let header = u32::from_le_bytes(raw.try_into().ok()?);
    let size = (header & 0x00FF_FFFF) as usize;
    if size == 0 {
        return None;
    }
    Some((header_offset + 4, size))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_f0_split_matches_both_consumers() {
        // Scene arm.
        let r = resolve_model_id(0);
        assert_eq!(r.bank, ModelBank::Scene);
        assert_eq!(r.pool_index, SCENE_BANK_BASE);
        assert!(!r.translucent);
        let r = resolve_model_id(0xEF);
        assert_eq!(r.bank, ModelBank::Scene);
        assert_eq!(r.index, 0xEF);
        // Player arm: 0xF0 is pool slot 0, the first PROT 0874 member.
        let r = resolve_model_id(0xF0);
        assert_eq!(r.bank, ModelBank::Player);
        assert_eq!(r.index, 0);
        assert_eq!(r.pool_index, PLAYER_BANK_BASE);
        assert!(r.translucent);
        let r = resolve_model_id(0xF4);
        assert_eq!(r.index, PLAYER_BANK_LEN - 1);
    }

    #[test]
    fn a_negative_operand_takes_the_player_arm() {
        // `sltiu v0,s4,0xf0` on a sign-extended halfword: -1 is 0xFFFF
        // unsigned, so it is >= 0xF0 and resolves `operand - 0xF0`.
        let r = resolve_model_id(-1);
        assert_eq!(r.bank, ModelBank::Player);
        assert_eq!(r.index, 0xFFFFu16.wrapping_sub(0xF0));
    }

    #[test]
    fn the_measured_bases_are_the_constants() {
        // Three field save states (`town01`, `koin1`, `izumi`) all read
        // `0x8007B824 = 0` and `0x8007B6F8 = 5`, and the five
        // `DAT_8007C018[0..=4]` pointers are the PROT 0874 §0 pack members.
        assert_eq!(PLAYER_BANK_BASE, 0);
        assert_eq!(PLAYER_BANK_LEN, 5);
        assert_eq!(SCENE_BANK_BASE, 5);
    }
}
