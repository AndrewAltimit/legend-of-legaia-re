//! A synthetic `.sstate` payload writer, for tests and examples.
//!
//! It emits exactly the shape this crate's readers walk - a top-level
//! protobuf message whose memory submessage holds RAM, BIOS, parallel-port and
//! 64 KiB hardware blobs, and whose GPU submessage holds `GPUSTAT`, the GP1
//! control log and VRAM - with field numbers matching what PCSX-Redux writes
//! (memory = field 3, GPU = field 5). It is **not** a state PCSX-Redux can
//! load: every other submessage (CPU registers, SPU, CD-ROM, timers) is left
//! out. Its use is to exercise the readers on a state that contains no game
//! bytes at all, which is what lets a non-Legaia test run in CI.

use std::io::Write;

/// RAM blob length PCSX-Redux allocates (8 MiB); the console's 2 MiB is its
/// first part.
pub const PCSX_RAM_BLOB_LEN: usize = 0x80_0000;
/// BIOS ROM blob length (512 KiB).
pub const BIOS_LEN: usize = 0x8_0000;

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

/// The contents of a synthetic state.
pub struct SyntheticState<'a> {
    /// Main RAM, at most 8 MiB; zero-padded to [`PCSX_RAM_BLOB_LEN`].
    pub ram: &'a [u8],
    /// The 64 KiB hardware region (scratchpad first), at most 64 KiB;
    /// zero-padded.
    pub hardware: &'a [u8],
    /// VRAM, at most 1 MiB; zero-padded to [`crate::gpu::VRAM_BYTES`].
    pub vram: &'a [u8],
    /// The GP1 control log (last value per GP1 command).
    pub gp1: [u32; 256],
}

fn padded(src: &[u8], len: usize) -> Vec<u8> {
    let mut v = src[..src.len().min(len)].to_vec();
    v.resize(len, 0);
    v
}

impl SyntheticState<'_> {
    /// The bare (uncompressed) payload, the shape a Lua-probe snapshot writes.
    pub fn to_payload(&self) -> Vec<u8> {
        let mut out = Vec::new();
        // A small header submessage first, as the real state carries one.
        len_field(&mut out, 1, b"\x0a\x09synthetic");

        let mut mem = Vec::new();
        len_field(&mut mem, 1, &padded(self.ram, PCSX_RAM_BLOB_LEN));
        len_field(&mut mem, 2, &vec![0u8; BIOS_LEN]);
        len_field(&mut mem, 3, &vec![0u8; PCSX_RAM_BLOB_LEN]);
        len_field(&mut mem, 4, &padded(self.hardware, 0x1_0000));
        len_field(&mut out, 3, &mem);

        let mut gpu = Vec::new();
        put_varint(&mut gpu, 1 << 3);
        put_varint(&mut gpu, 0x1480_2000);
        let log: Vec<u8> = self.gp1.iter().flat_map(|w| w.to_le_bytes()).collect();
        len_field(&mut gpu, 2, &log);
        len_field(&mut gpu, 3, &padded(self.vram, crate::gpu::VRAM_BYTES));
        len_field(&mut out, 5, &gpu);
        out
    }

    /// The gzipped payload, the shape PCSX-Redux's own save slots write.
    pub fn to_gzip(&self) -> Vec<u8> {
        let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        enc.write_all(&self.to_payload())
            .expect("write to an in-memory gzip encoder");
        enc.finish().expect("finish an in-memory gzip encoder")
    }
}
