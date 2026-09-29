//! The GPU half of a PCSX-Redux save state: the 1 MiB VRAM and the GP1
//! control-register log, located structurally in the state's protobuf.
//!
//! PCSX-Redux serialises its GPU as one submessage of the top-level state
//! holding three members: the `GPUSTAT` word (a varint), a `0x400`-byte array
//! of 256 little-endian `u32`s - the **last value written for each GP1
//! command**, indexed by command number - and the `0x100000`-byte VRAM image.
//! Nothing here keys on field numbers or file offsets: the submessage is the
//! one whose length-delimited members are exactly a `0x400`-byte blob and a
//! `0x100000`-byte blob, the same structural rule [`crate::SaveState`] uses to
//! find the scratchpad.
//!
//! Because the control log is indexed by GP1 command, the display state reads
//! straight off it: `GP1(0x05)` is the display-area start (`x` in bits `0..10`,
//! `y` in bits `10..19`) and `GP1(0x08)` is the display mode. That is the same
//! pair a mednafen state carries as `DisplayFB_XStart` / `DisplayFB_YStart`
//! and `DisplayMode`, so the two emulators crop the on-screen framebuffer
//! identically - a RAM-and-VRAM image of a frame *is* the frame, and no
//! emulator run is needed to see what retail displayed.

use std::io::Read;
use std::path::Path;

use anyhow::{Context, Result};

/// VRAM size in bytes (1024 x 512 BGR555).
pub const VRAM_BYTES: usize = 1024 * 512 * 2;
/// Size of the GP1 control-register log (256 `u32`s).
const CONTROL_LOG_BYTES: usize = 0x400;

/// The GPU snapshot of one state.
pub struct PcsxGpu {
    /// 1 MiB little-endian BGR555 VRAM, row-major, 1024 pixels per row.
    pub vram: Vec<u8>,
    /// The last value written to each GP1 command, indexed by command.
    pub control: [u32; 256],
}

fn varint(buf: &[u8], mut off: usize) -> Option<(u64, usize)> {
    let mut out: u64 = 0;
    let mut shift = 0u32;
    loop {
        let b = *buf.get(off)?;
        off += 1;
        out |= u64::from(b & 0x7F) << shift;
        if b & 0x80 == 0 {
            return Some((out, off));
        }
        shift += 7;
        if shift > 63 {
            return None;
        }
    }
}

/// Every length-delimited member of the message spanning `[start, end)`, or
/// `None` when the span does not parse as exactly one message.
fn members(buf: &[u8], start: usize, end: usize) -> Option<Vec<(usize, usize)>> {
    let mut out = Vec::new();
    let mut off = start;
    while off < end {
        let (tag, o) = varint(buf, off)?;
        match tag & 7 {
            0 => off = varint(buf, o)?.1,
            1 => off = o.checked_add(8)?,
            2 => {
                let (len, o2) = varint(buf, o)?;
                let len = usize::try_from(len).ok()?;
                let stop = o2.checked_add(len)?;
                if stop > end {
                    return None;
                }
                out.push((o2, len));
                off = stop;
            }
            5 => off = o.checked_add(4)?,
            _ => return None,
        }
    }
    (off == end).then_some(out)
}

fn decompress(bytes: &[u8]) -> Result<std::borrow::Cow<'_, [u8]>> {
    if bytes.starts_with(&[0x1f, 0x8b]) {
        let mut buf = Vec::new();
        flate2::read::GzDecoder::new(bytes)
            .read_to_end(&mut buf)
            .context("gunzip .sstate")?;
        Ok(std::borrow::Cow::Owned(buf))
    } else {
        Ok(std::borrow::Cow::Borrowed(bytes))
    }
}

/// Load a `.sstate` once and return both halves a frame comparison needs: the
/// [`crate::SaveState`] (main RAM + scratchpad) and the GPU snapshot, with the
/// SCUS bytes for the RAM anchor search passed in rather than resolved from
/// the working directory or `LEGAIA_SCUS` - a caller that already knows where
/// its extracted disc lives should not depend on its cwd.
pub fn load_with_scus(path: &Path, scus: &[u8]) -> Result<(crate::SaveState, Option<PcsxGpu>)> {
    let raw = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    let payload = decompress(&raw)?;
    let ram = legaia_mednafen::extract::main_ram_via_anchor_with_scus(&payload, scus)
        .context("locate main RAM in PCSX-Redux payload (anchor search)")?
        .to_vec();
    let hardware = crate::find_hardware(&payload).map(<[u8]>::to_vec);
    let gpu = PcsxGpu::from_payload(&payload);
    Ok((crate::SaveState { ram, hardware }, gpu))
}

impl PcsxGpu {
    /// Read the GPU snapshot out of a `.sstate` file (gzipped or bare).
    pub fn from_path(path: &Path) -> Result<Self> {
        let raw = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
        Self::from_sstate_bytes(&raw)
    }

    /// Same as [`Self::from_path`] from in-memory `.sstate` bytes.
    pub fn from_sstate_bytes(bytes: &[u8]) -> Result<Self> {
        let payload = decompress(bytes)?;
        Self::from_payload(&payload).context("no GPU submessage (VRAM + GP1 log) in the state")
    }

    fn from_payload(payload: &[u8]) -> Option<Self> {
        for (start, len) in members(payload, 0, payload.len())? {
            let Some(inner) = members(payload, start, start + len) else {
                continue;
            };
            let vram = inner.iter().find(|(_, l)| *l == VRAM_BYTES);
            let log = inner.iter().find(|(_, l)| *l == CONTROL_LOG_BYTES);
            let (Some(&(vo, vl)), Some(&(co, _))) = (vram, log) else {
                continue;
            };
            let mut control = [0u32; 256];
            for (i, w) in control.iter_mut().enumerate() {
                let b = payload.get(co + i * 4..co + i * 4 + 4)?;
                *w = u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
            }
            return Some(Self {
                vram: payload.get(vo..vo + vl)?.to_vec(),
                control,
            });
        }
        None
    }

    /// Display-area start `(x, y)` in VRAM pixels (`GP1(0x05)`).
    pub fn display_start(&self) -> (u32, u32) {
        let w = self.control[5];
        (w & 0x3FF, (w >> 10) & 0x1FF)
    }

    /// Display resolution `(width, height)` decoded from `GP1(0x08)` - the
    /// standard PSX decode, the same one `legaia_mednafen::GpuRegs` applies.
    pub fn display_resolution(&self) -> (u32, u32) {
        let m = self.control[8];
        let width = if m & 0x40 != 0 {
            368
        } else {
            [256u32, 320, 512, 640][(m & 0x3) as usize]
        };
        let height = if (m & 0x20 != 0) && (m & 0x04 != 0) {
            480
        } else {
            240
        };
        (width, height)
    }

    /// The on-screen VRAM rectangle `(x, y, w, h)`, clamped to VRAM.
    pub fn display_crop_rect(&self) -> (u32, u32, u32, u32) {
        let (x, y) = self.display_start();
        let (w, h) = self.display_resolution();
        (x, y, w.min(1024 - x.min(1024)), h.min(512 - y.min(512)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn put_varint(out: &mut Vec<u8>, mut v: u64) {
        loop {
            let b = (v & 0x7F) as u8;
            v >>= 7;
            if v == 0 {
                out.push(b);
                return;
            }
            out.push(b | 0x80);
        }
    }

    fn len_field(out: &mut Vec<u8>, field: u64, body: &[u8]) {
        put_varint(out, (field << 3) | 2);
        put_varint(out, body.len() as u64);
        out.extend_from_slice(body);
    }

    #[test]
    fn finds_the_gpu_submessage_by_shape() {
        let mut control = vec![0u8; CONTROL_LOG_BYTES];
        let d5: u32 = 0x0500_0000 | (244 << 10);
        control[20..24].copy_from_slice(&d5.to_le_bytes());
        control[32..36].copy_from_slice(&0x0800_0001u32.to_le_bytes());
        let mut vram = vec![0u8; VRAM_BYTES];
        vram[0] = 0x1F;
        let mut gpu = Vec::new();
        put_varint(&mut gpu, 1 << 3);
        put_varint(&mut gpu, 0x1234);
        len_field(&mut gpu, 2, &control);
        len_field(&mut gpu, 3, &vram);
        let mut top = Vec::new();
        len_field(&mut top, 1, b"unrelated");
        len_field(&mut top, 5, &gpu);

        let g = PcsxGpu::from_sstate_bytes(&top).expect("gpu");
        assert_eq!(g.vram[0], 0x1F);
        assert_eq!(g.display_start(), (0, 244));
        assert_eq!(g.display_resolution(), (320, 240));
        assert_eq!(g.display_crop_rect(), (0, 244, 320, 240));
    }
}
