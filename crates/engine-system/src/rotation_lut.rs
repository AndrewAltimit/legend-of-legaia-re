//! The retail 12-bit rotation LUT pair: one `SCUS_942.54` sine table read as
//! sine (`_DAT_8007B81C`) and, a quarter turn on, as cosine (`_DAT_8007B7F8`).
//!
//! Every consumer that rotates by a 12-bit angle reads this pair - the action
//! effect-script stepper, the ribbon and part-motion emitters, the battle
//! camera ghost, the field fog particles - so it sits beside the other 12-bit
//! trig ([`crate::trig::sin12`]) where all of them can name
//! it. [`RotationLut`] is the pair as the steppers index it;
//! [`RetailRotationLut`] is the byte-exact table.

/// The 12-bit angle mask the table is indexed under.
const ANGLE_MASK: i32 = 0xFFF;

/// The two `i16` LUTs the rotation indexes, in the pairing the stepper uses.
///
/// Retail dereferences `_DAT_8007B7F8` and `_DAT_8007B81C`. The stepper indexes
/// the first at `0xFFF - a` and the second at `a`, where `a` is the biased
/// facing - the complement is what makes the pair a rotation rather than two
/// independent scalings.
pub trait RotationLut {
    /// The `_DAT_8007B7F8` table, `1 << 12` fixed point.
    fn a(&self, angle: i32) -> i32;
    /// The `_DAT_8007B81C` table, `1 << 12` fixed point.
    fn b(&self, angle: i32) -> i32;
}

/// The retail LUT pair, reconstructed from its mathematical definition.
///
/// PORT: FUN_80026BE0 (installs the pair: `_DAT_8007B81C = 0x80070A2C`,
/// `_DAT_8007B7F8 = 0x80070A2C + 0x800`).
///
/// Both pointers land in **one** static `SCUS_942.54` sine table at
/// `0x80070A2C`: 5120 `i16` entries of `trunc(sin(i * 2pi / 4096) * 4096)`
/// (12-bit angle space, `1 << 12` fixed point, truncation toward zero -
/// disc-verified byte-exact over all 5120 entries). The `+0x800`-**byte**
/// offset between the two pointers is `+0x400` entries = a quarter
/// revolution, and the table's final 1024 entries repeat its first 1024 so
/// the offset read never needs a mask: `b(angle)` is sine, `a(angle)` is
/// cosine. This type generates the table from the same formula (the values
/// are a mathematical function of the index, not creative content), so any
/// engine host can hold a byte-exact pair without disc access.
pub struct RetailRotationLut {
    /// One revolution of `trunc(sin) * 4096`, indexed by 12-bit angle.
    sin: [i16; 4096],
}

impl RetailRotationLut {
    /// Build the table. `trunc` (toward zero), not `round` - the retail
    /// bytes match truncation exactly and differ from rounding in 2510 of
    /// the 5120 entries.
    pub fn new() -> Self {
        let mut sin = [0i16; 4096];
        for (i, slot) in sin.iter_mut().enumerate() {
            let a = (i as f64) * std::f64::consts::TAU / 4096.0;
            *slot = (a.sin() * 4096.0) as i16; // `as` truncates toward zero
        }
        Self { sin }
    }
}

impl RetailRotationLut {
    /// The sine view behind `_DAT_8007B81C`: one revolution, 12-bit angle.
    pub fn sin_table(&self) -> &[i16; 4096] {
        &self.sin
    }

    /// The cosine view behind `_DAT_8007B7F8` (`_DAT_8007B81C + 0x800`
    /// bytes = `+0x400` entries), materialised as its own revolution.
    ///
    /// Retail reads past the first revolution into the table's repeated
    /// tail instead of masking; over the `0..=0xFFF` index domain every
    /// consumer masks to, the two are the same values.
    pub fn cos_table(&self) -> [i16; 4096] {
        std::array::from_fn(|i| self.sin[(i + 0x400) & 0xFFF])
    }
}

impl Default for RetailRotationLut {
    fn default() -> Self {
        Self::new()
    }
}

/// The process-wide [`RetailRotationLut`], built once on first use - the
/// engine analogue of retail's boot-time pointer install (`FUN_80026BE0`).
pub fn retail_rotation_lut() -> &'static RetailRotationLut {
    static LUT: std::sync::OnceLock<RetailRotationLut> = std::sync::OnceLock::new();
    LUT.get_or_init(RetailRotationLut::new)
}

impl RotationLut for RetailRotationLut {
    fn a(&self, angle: i32) -> i32 {
        // The `+0x800`-byte pointer offset = +0x400 entries (cosine). The
        // retail read relies on the table's repeated tail instead of a mask;
        // over the stepper's `0..=0xFFF` domain the two are identical.
        i32::from(self.sin[((angle + 0x400) & ANGLE_MASK) as usize])
    }
    fn b(&self, angle: i32) -> i32 {
        i32::from(self.sin[(angle & ANGLE_MASK) as usize])
    }
}
