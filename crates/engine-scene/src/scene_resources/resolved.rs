//! The resolved assets a scene build yields: `ResolvedTmd`, `ResolvedAnm` and the battle boot stats.

use super::*;

/// One TMD model the scene exposes, paired with its parsed structure.
///
/// The retail asset chain registers each TMD into the per-scene mesh pointer
/// table at `0x8007C018 + idx*4` via `FUN_80026B4C`. Until the runtime
/// registration order is reverse-engineered, this module surfaces every TMD
/// hit in scene order (CDNAME entry order, then byte-offset within each
/// entry) - an engine can pick its meshes by `(entry_idx, offset)` and
/// pre-resolve the actor → TMD binding through a side table.
#[derive(Debug, Clone)]
pub struct ResolvedTmd {
    /// PROT entry index this TMD came from.
    pub entry_idx: u32,
    /// Byte offset of the TMD magic within the entry's bytes.
    pub offset: usize,
    /// Length of the TMD payload (bytes from magic to end of last primitive).
    pub byte_len: usize,
    /// Parsed TMD ready for primitive walking + render upload.
    pub tmd: legaia_tmd::Tmd,
    /// Raw bytes of the TMD slice (same buffer passed to [`legaia_tmd::parse`]).
    /// Required by [`legaia_tmd::mesh::tmd_to_vram_mesh`] for primitive data reads.
    pub raw: Vec<u8>,
}

impl ResolvedTmd {
    /// Build a renderable VRAM mesh, dropping primitives whose CBA / TSB
    /// would sample VRAM regions the scene's TIM uploads didn't populate.
    /// Mirrors the asset-viewer's filter so engine-side scene rendering
    /// inherits the same "skip prims that would render as flat green" /
    /// "skip prims with palette-depth mismatches" cleanup.
    ///
    /// Use [`legaia_tmd::mesh::tmd_to_vram_mesh`] directly instead when
    /// the caller wants every prim regardless of VRAM state (e.g. the
    /// flat-shaded pre-VRAM path).
    /// Clone with the VDF morph deltas staged onto object `group`'s
    /// vertices (`vertex[i] += deltas[i]`, wrapping i16 per component -
    /// the `FUN_8005B038` add). The clone is what the mesh builders read
    /// (`Object::vertices`), so rebuilding a mesh from it yields the same
    /// vertex-stream order as the rest-pose build with only positions
    /// moved - the retail render substitution (`FUN_8001C604` staging a
    /// scratch copy and retargeting the group's vertex pointer for the
    /// draw) expressed as data. Deltas past the object's vertex count are
    /// ignored.
    ///
    /// REF: FUN_8001C604, FUN_8005B038
    pub fn with_group_deltas(&self, group: u32, deltas: &[[i16; 3]]) -> ResolvedTmd {
        let mut out = self.clone();
        if let Some(obj) = out.tmd.objects.get_mut(group as usize) {
            for (v, d) in obj.vertices.iter_mut().zip(deltas.iter()) {
                v.x = v.x.wrapping_add(d[0]);
                v.y = v.y.wrapping_add(d[1]);
                v.z = v.z.wrapping_add(d[2]);
            }
        }
        out
    }

    pub fn build_filtered_vram_mesh(&self, vram: &Vram) -> legaia_tmd::mesh::VramMesh {
        legaia_tmd::mesh::tmd_to_vram_mesh_filtered(&self.tmd, &self.raw, |cba, tsb, uvs| {
            vram.prim_has_texture_data(cba, tsb, uvs)
        })
    }

    /// [`ResolvedTmd::build_filtered_vram_mesh`] plus the per-vertex
    /// lit-row mask ([`legaia_tmd::mesh::tmd_to_vram_mesh_filtered_lit`]):
    /// which vertices carry no baked colour word. A baked word authored at
    /// exactly `0x80` reads the same colour, so only the mask tells them
    /// apart.
    pub fn build_filtered_vram_mesh_lit(
        &self,
        vram: &Vram,
    ) -> (legaia_tmd::mesh::VramMesh, Vec<bool>) {
        legaia_tmd::mesh::tmd_to_vram_mesh_filtered_lit(&self.tmd, &self.raw, |cba, tsb, uvs| {
            vram.prim_has_texture_data(cba, tsb, uvs)
        })
    }

    /// [`ResolvedTmd::build_filtered_vram_mesh`] plus each lit-row vertex's
    /// normal + object colour ([`legaia_tmd::mesh::tmd_to_vram_mesh_filtered_lit_vertices`]) -
    /// what [`legaia_engine_field::field_lit_mesh::shade_lit_rows`] shades.
    pub fn build_filtered_vram_mesh_lit_vertices(
        &self,
        vram: &Vram,
    ) -> (
        legaia_tmd::mesh::VramMesh,
        Vec<Option<legaia_tmd::mesh::LitVertex>>,
    ) {
        legaia_tmd::mesh::tmd_to_vram_mesh_filtered_lit_vertices(
            &self.tmd,
            &self.raw,
            |cba, tsb, uvs| vram.prim_has_texture_data(cba, tsb, uvs),
        )
    }

    /// Same as [`ResolvedTmd::build_filtered_vram_mesh`] but also
    /// returns [`legaia_tmd::mesh::FilterStats`] so callers can report
    /// how many prims fell through the VRAM-coverage filter. Engines
    /// use this for "town01 mesh K: 92% prims kept" diagnostics.
    pub fn build_filtered_vram_mesh_stats(
        &self,
        vram: &Vram,
    ) -> (legaia_tmd::mesh::VramMesh, legaia_tmd::mesh::FilterStats) {
        legaia_tmd::mesh::tmd_to_vram_mesh_filtered_stats(&self.tmd, &self.raw, |cba, tsb, uvs| {
            vram.prim_has_texture_data(cba, tsb, uvs)
        })
    }

    /// Same shape as [`ResolvedTmd::build_filtered_vram_mesh_stats`] but
    /// surfaces *why* each dropped prim was rejected (MissingClut /
    /// DepthMismatch / MissingTexturePage) via
    /// [`legaia_tmd::mesh::FilterStatsByReason`]. Useful for engine
    /// diagnostics that want to distinguish "the loader skipped the
    /// texture page" from "two TIMs collided on the same CLUT row".
    pub fn build_filtered_vram_mesh_reasoned(
        &self,
        vram: &Vram,
    ) -> (
        legaia_tmd::mesh::VramMesh,
        legaia_tmd::mesh::FilterStatsByReason,
    ) {
        legaia_tmd::mesh::tmd_to_vram_mesh_status_stats(&self.tmd, &self.raw, |cba, tsb, uvs| {
            use legaia_tim::vram::PrimTextureStatus;
            use legaia_tmd::mesh::PrimDecision;
            match vram.prim_texture_status(cba, tsb, uvs) {
                PrimTextureStatus::Ok => PrimDecision::Keep,
                PrimTextureStatus::MissingClut { .. } => PrimDecision::MissingClut,
                PrimTextureStatus::ClutDepthMismatch { .. } => PrimDecision::ClutDepthMismatch,
                PrimTextureStatus::MissingTexturePage { .. } => PrimDecision::MissingTexturePage,
            }
        })
    }
}

/// Diagnostic counts returned alongside [`SceneResources::build_battle_boot_vram`].
#[derive(Debug, Clone, Copy, Default)]
pub struct BattleBootStats {
    /// Number of detected battle_data packs across the input scenes.
    pub packs: usize,
    /// Number of records walked across all detected packs.
    pub records: usize,
    /// Number of standard-PSX-TIM texture pools uploaded to the boot VRAM.
    pub tims_uploaded: usize,
    /// Number of CLUT rows uploaded via `battle_data_pack::clut_uploads`
    /// (currently zero until the descriptor encoding is pinned).
    pub cluts_uploaded: usize,
}

/// One ANM pack found in the scene's CDNAME block.
#[derive(Clone)]
pub struct ResolvedAnm {
    /// PROT entry index this ANM pack came from.
    pub entry_idx: u32,
    /// Byte offset of the ANM payload within the entry.
    pub offset: usize,
    /// Byte length of the ANM payload.
    pub byte_len: usize,
    /// Parsed ANM pack (count + record ranges).
    pub pack: legaia_anm::AnmPack,
    /// Raw payload bytes (no preamble). Index into this with
    /// `pack.records[i].offset .. pack.records[i].offset + pack.records[i].size`
    /// to get the record bytes for `AnimPlayer::new`.
    pub payload: Vec<u8>,
}

impl ResolvedAnm {
    /// Slice the record bytes for record `idx`. Returns `None` if out of range.
    pub fn record_bytes(&self, idx: usize) -> Option<&[u8]> {
        let rec = self.pack.records.get(idx)?;
        self.payload.get(rec.offset..rec.offset + rec.size)
    }
}
