//! The cabinet mesh carried as a TMD in PROT 1200 descriptor 1.

use super::*;

// ---------------------------------------------------------------------------
// The cabinet mesh (PROT 1200 descriptor 1)
// ---------------------------------------------------------------------------

/// One triangle of the cabinet mesh, in the machine's model space.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CabinetTri {
    /// The three corners.
    pub pos: [Pos3; 3],
    /// Per-corner packet colour (a flat prim repeats its one colour).
    pub rgb: [[u8; 3]; 3],
    /// The group's ABE bit: the prim blends.
    pub semi: bool,
}

/// The machine's body: every prim of the untextured cabinet TMD the slot
/// init installs into the model bank and draws as an ordinary actor (see
/// `docs/subsystems/minigame-slot-machine.md`, "The cabinet is a mesh"). Quads
/// are split into two triangles with the TMD walker's `[0,1,2, 1,3,2]`
/// winding.
#[derive(Debug, Clone, Default)]
pub struct SlotCabinetMesh {
    pub tris: Vec<CabinetTri>,
}

/// Asset type byte of a TMD descriptor.
const TYPE_TMD: u8 = 0x02;

/// Decode the cabinet mesh out of a **raw** PROT entry 1200 - the art pack's
/// container, whose first `TMD` descriptor is the machine's body.
pub fn parse_cabinet(entry: &[u8]) -> Result<SlotCabinetMesh> {
    use anyhow::Context;
    if entry.len() < 8 {
        bail!("art entry too small ({}b) for a container", entry.len());
    }
    let count = u32::from_le_bytes(entry[0..4].try_into().unwrap()) as usize;
    if count == 0 || count > 16 {
        bail!("implausible descriptor count {count} in art entry");
    }
    let container = legaia_container_formats::parse_player_lzs(entry, count)?;
    let desc = container
        .descriptors
        .iter()
        .find(|d| d.type_byte == TYPE_TMD)
        .context("art entry carries no TMD descriptor")?;
    // The descriptor LZS-decodes to a one-member `container-formats::pack` whose member is
    // the TMD (`[count = 1][word offset 2]`, magic at byte 8).
    let is_tmd = |b: &[u8]| b.len() >= 4 && b[0..4] == 0x8000_0002u32.to_le_bytes();
    let decoded =
        legaia_container_formats::decode(entry, desc, legaia_container_formats::DecodeMode::Lzs)?;
    let body = if is_tmd(&decoded) {
        decoded
    } else {
        legaia_container_formats::pack::extract_pack(&decoded)
            .ok()
            .and_then(|members| members.into_iter().find(|m| is_tmd(m)))
            .map(|m| m.to_vec())
            .context("cabinet descriptor does not decode to a Legaia TMD")?
    };
    let tmd = legaia_tmd::parse(&body).context("parsing the cabinet TMD")?;
    let mesh = legaia_tmd::mesh::tmd_to_color_mesh(&tmd, &body);
    let pos = |i: u32| {
        let p = mesh.positions[i as usize];
        Pos3 {
            x: p[0] as i16,
            y: p[1] as i16,
            z: p[2] as i16,
        }
    };
    let tris = mesh
        .indices
        .as_chunks::<3>()
        .0
        .iter()
        .map(|t| CabinetTri {
            pos: [pos(t[0]), pos(t[1]), pos(t[2])],
            rgb: [
                mesh.colors[t[0] as usize],
                mesh.colors[t[1] as usize],
                mesh.colors[t[2] as usize],
            ],
            semi: mesh.blend[t[0] as usize] & 0x8000 != 0,
        })
        .collect();
    Ok(SlotCabinetMesh { tris })
}
