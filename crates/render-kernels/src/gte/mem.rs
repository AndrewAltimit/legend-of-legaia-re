/// Memory bridge for the GTE's load / store ops. Engines wire this up to
/// their main-memory implementation (the retail PSX uses 2 MB of physical
/// RAM mirrored to KSEG0/KSEG1; the Rust port can use a simple `Vec<u8>`
/// with bounds-checking, or anything else that produces u32 reads).
///
/// The default impl returns `0` from `cop2_load` and silently drops
/// `cop2_store`; tests that don't need memory can rely on that.
pub trait Cop2Mem {
    fn cop2_load(&mut self, addr: u32) -> u32;
    fn cop2_store(&mut self, addr: u32, val: u32);
}

/// Vec-backed [`Cop2Mem`]. The address is wrapped to the buffer length
/// (PSX RAM mirror), and out-of-bounds reads return zero rather than
/// panicking. Suitable for capturing GTE traces against a recorded RAM
/// snapshot.
pub struct VecMem {
    pub bytes: Vec<u8>,
}

impl VecMem {
    pub fn new(size: usize) -> Self {
        Self {
            bytes: vec![0; size],
        }
    }

    pub fn from_bytes(bytes: Vec<u8>) -> Self {
        Self { bytes }
    }

    pub fn write_u32_at(&mut self, addr: u32, val: u32) {
        let a = addr as usize % self.bytes.len().max(1);
        for (i, b) in val.to_le_bytes().iter().enumerate() {
            if a + i < self.bytes.len() {
                self.bytes[a + i] = *b;
            }
        }
    }
}

impl Cop2Mem for VecMem {
    fn cop2_load(&mut self, addr: u32) -> u32 {
        let n = self.bytes.len();
        if n == 0 {
            return 0;
        }
        let a = (addr as usize) % n;
        let mut buf = [0u8; 4];
        for (i, slot) in buf.iter_mut().enumerate() {
            if a + i < n {
                *slot = self.bytes[a + i];
            }
        }
        u32::from_le_bytes(buf)
    }

    fn cop2_store(&mut self, addr: u32, val: u32) {
        self.write_u32_at(addr, val);
    }
}

/// No-op [`Cop2Mem`]. Loads return `0`, stores are dropped. Useful when
/// instantiating a GTE for unit tests that don't exercise LWC2/SWC2.
pub struct NullMem;

impl Cop2Mem for NullMem {
    fn cop2_load(&mut self, _addr: u32) -> u32 {
        0
    }
    fn cop2_store(&mut self, _addr: u32, _val: u32) {}
}
