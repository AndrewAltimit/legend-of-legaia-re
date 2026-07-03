//! World-map top-view camera-state RAM-global decode subcommand for
//! `mednafen-state`.

use anyhow::{Result, bail};
use legaia_mednafen::{SaveState, extract::ram_slice};
use std::path::PathBuf;

/// World-map top-view camera-state globals. See `docs/subsystems/world-map.md`
/// section "Globals used". The X/Z scrolls are stored as negated
/// map-origin coordinates; the negation is applied here so `cam_x` /
/// `cam_z` are camera-target world units.
const CAM_X_SCROLL: u32 = 0x80089120;
const CAM_Z_SCROLL: u32 = 0x80089118;
const CAM_AZIMUTH: u32 = 0x8007B794;
const CAM_ZOOM_MODE: u32 = 0x8007B6F4;
const VIEW_MODE_FLAG: u32 = 0x801F2B94;

#[derive(Debug)]
struct CameraState {
    raw_x: i32,
    raw_z: i32,
    raw_az: i32,
    raw_zoom_mode: u32,
    view_mode: u8,
}

impl CameraState {
    fn from_ram(ram: &[u8]) -> Result<Self> {
        let raw_x = read_i32_le(ram, CAM_X_SCROLL)?;
        let raw_z = read_i32_le(ram, CAM_Z_SCROLL)?;
        let raw_az = read_i32_le(ram, CAM_AZIMUTH)?;
        let raw_zoom_mode = read_u32_le(ram, CAM_ZOOM_MODE)?;
        let view_mode = ram_slice(ram, VIEW_MODE_FLAG, VIEW_MODE_FLAG + 1)?[0];
        Ok(Self {
            raw_x,
            raw_z,
            raw_az,
            raw_zoom_mode,
            view_mode,
        })
    }
    fn cam_x(&self) -> i32 {
        -self.raw_x
    }
    fn cam_z(&self) -> i32 {
        -self.raw_z
    }
    fn view_label(&self) -> &'static str {
        match self.view_mode {
            0 => "walk",
            1 => "top",
            _ => "?",
        }
    }
}

fn read_u32_le(ram: &[u8], addr: u32) -> Result<u32> {
    let s = ram_slice(ram, addr, addr + 4)?;
    Ok(u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}

fn read_i32_le(ram: &[u8], addr: u32) -> Result<i32> {
    Ok(read_u32_le(ram, addr)? as i32)
}

pub fn cmd_world_map_camera(saves: &[PathBuf], table: bool) -> Result<()> {
    if saves.is_empty() {
        bail!("at least one save state is required");
    }
    let mut decoded = Vec::with_capacity(saves.len());
    for path in saves {
        let s = SaveState::from_path(path)?;
        let ram = s.main_ram()?;
        decoded.push((path.clone(), CameraState::from_ram(ram)?));
    }
    if table {
        println!(
            "{:<48}  {:>4}  {:>10}  {:>10}  {:>10}  {:>10}  {:>10}",
            "save", "view", "raw_x", "raw_z", "cam_x", "cam_z", "az/zoom"
        );
        println!("{}", "-".repeat(120));
        for (path, c) in &decoded {
            let name = path
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("?")
                .to_string();
            let truncated = if name.len() > 48 {
                format!("…{}", &name[name.len() - 47..])
            } else {
                name
            };
            println!(
                "{:<48}  {:>4}  {:>10}  {:>10}  {:>10}  {:>10}  az=0x{:04X} zoom=0x{:04X}",
                truncated,
                c.view_label(),
                c.raw_x,
                c.raw_z,
                c.cam_x(),
                c.cam_z(),
                (c.raw_az as u32) & 0xFFFF,
                c.raw_zoom_mode & 0xFFFF
            );
        }
        let top_view_count = decoded.iter().filter(|(_, c)| c.view_mode == 1).count();
        println!();
        println!(
            "[info] {}/{} save state(s) captured in top-view mode (DAT_801F2B94 = 1)",
            top_view_count,
            decoded.len()
        );
        if top_view_count == 0 {
            println!(
                "[warn] all captured saves are in walk-view; cam_x/cam_z reflect \
                 load-time map-origin only, not an interactively-scrolled camera \
                 position. Re-capture in top-view debug mode (dev menu) to get \
                 true camera defaults."
            );
        }
    } else {
        for (path, c) in &decoded {
            println!("{}", path.display());
            println!(
                "  view-mode flag (DAT_801F2B94)        = {} ({})",
                c.view_mode,
                c.view_label()
            );
            println!(
                "  _DAT_80089120  raw i32                = {} (0x{:08X})",
                c.raw_x, c.raw_x as u32
            );
            println!(
                "  _DAT_80089118  raw i32                = {} (0x{:08X})",
                c.raw_z, c.raw_z as u32
            );
            println!("  cam_x = -_DAT_80089120                = {}", c.cam_x());
            println!("  cam_z = -_DAT_80089118                = {}", c.cam_z());
            println!(
                "  _DAT_8007B794  azimuth (low u16)      = 0x{:04X} ({})",
                (c.raw_az as u32) & 0xFFFF,
                c.raw_az & 0xFFFF
            );
            println!(
                "  _DAT_8007B6F4  zoom/mode (low u16)    = 0x{:04X} ({})",
                c.raw_zoom_mode & 0xFFFF,
                c.raw_zoom_mode & 0xFFFF
            );
            println!();
        }
    }
    Ok(())
}

#[cfg(test)]
mod camera_decode_tests {
    use super::*;
    use legaia_mednafen::extract::{PSX_RAM_KSEG0, PSX_RAM_SIZE};

    fn synth_ram_with(values: &[(u32, &[u8])]) -> Vec<u8> {
        let mut ram = vec![0u8; PSX_RAM_SIZE];
        for (addr, bytes) in values {
            let off = (*addr - PSX_RAM_KSEG0) as usize;
            ram[off..off + bytes.len()].copy_from_slice(bytes);
        }
        ram
    }

    #[test]
    fn decode_drake_walk_view_capture() {
        // Mirrors a captured world-map walk-view state: raw_x =
        // -8832 (0xFFFFDD80), raw_z = -8832, zoom-mode = 0x0170,
        // view-mode flag = 0.
        let ram = synth_ram_with(&[
            (CAM_X_SCROLL, &(-8832i32).to_le_bytes()),
            (CAM_Z_SCROLL, &(-8832i32).to_le_bytes()),
            (CAM_AZIMUTH, &0u32.to_le_bytes()),
            (CAM_ZOOM_MODE, &0x0170u32.to_le_bytes()),
            (VIEW_MODE_FLAG, &[0u8]),
        ]);
        let c = CameraState::from_ram(&ram).unwrap();
        assert_eq!(c.raw_x, -8832);
        assert_eq!(c.raw_z, -8832);
        assert_eq!(c.cam_x(), 8832);
        assert_eq!(c.cam_z(), 8832);
        assert_eq!(c.raw_zoom_mode & 0xFFFF, 0x0170);
        assert_eq!(c.view_mode, 0);
        assert_eq!(c.view_label(), "walk");
    }

    #[test]
    fn decode_top_view_flag_labels_correctly() {
        let ram = synth_ram_with(&[
            (CAM_X_SCROLL, &0u32.to_le_bytes()),
            (CAM_Z_SCROLL, &0u32.to_le_bytes()),
            (CAM_AZIMUTH, &0u32.to_le_bytes()),
            (CAM_ZOOM_MODE, &0u32.to_le_bytes()),
            (VIEW_MODE_FLAG, &[1u8]),
        ]);
        let c = CameraState::from_ram(&ram).unwrap();
        assert_eq!(c.view_mode, 1);
        assert_eq!(c.view_label(), "top");
    }

    #[test]
    fn cam_negation_matches_overlay_convention() {
        // `_DAT_80089118 = -(int)*(short *)(actor + 0x14)` in
        // overlay_0978 + slot_machine means: cam_z is the negation of
        // the raw cell. A positive raw_z must round-trip to negative
        // cam_z.
        let ram = synth_ram_with(&[
            (CAM_X_SCROLL, &1234i32.to_le_bytes()),
            (CAM_Z_SCROLL, &5678i32.to_le_bytes()),
            (CAM_AZIMUTH, &0u32.to_le_bytes()),
            (CAM_ZOOM_MODE, &0u32.to_le_bytes()),
            (VIEW_MODE_FLAG, &[0u8]),
        ]);
        let c = CameraState::from_ram(&ram).unwrap();
        assert_eq!(c.cam_x(), -1234);
        assert_eq!(c.cam_z(), -5678);
    }
}
