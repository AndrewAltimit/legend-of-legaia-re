//! Built-in 5×7 ASCII bitmap font.
//!
//! Used by [`crate::Font::placeholder`] when the extracted dialog atlas is
//! not available (typical for first-time runs before `font-extract` has
//! been invoked). Without this fallback the placeholder atlas was solid
//! white blocks; with it the engine renders readable English text out of
//! the box.
//!
//! No Sony bytes - this is hand-rolled.

/// One glyph: 5 wide × 7 tall, packed as 7 bytes (one per row, low 5 bits
/// set = pixel on). Bit 4 is leftmost.
pub type Glyph = [u8; 7];

/// How many rows below the normal 7-row box `byte`'s glyph is drawn.
///
/// `g`, `p`, `q` and `y` hang their tails under the baseline, and `j` its
/// hook. Squeezed inside the 7 rows they closed up into other letters: the
/// launcher's first screen read "Lesaia" and "Disrlay".
pub fn descent(byte: u8) -> u32 {
    match byte {
        b'g' | b'p' | b'q' | b'y' => 2,
        b'j' => 1,
        _ => 0,
    }
}

/// Returns the 5×7 glyph for `byte`, or `UNKNOWN` for unmapped bytes.
pub fn glyph(byte: u8) -> Glyph {
    match byte {
        b' ' => SPACE,
        b'!' => EXCL,
        b'"' => DQUOTE,
        b'#' => HASH,
        b'$' => DOLLAR,
        b'%' => PERCENT,
        b'&' => AMP,
        b'\'' => SQUOTE,
        b'(' => LPAREN,
        b')' => RPAREN,
        b'*' => STAR,
        b'+' => PLUS,
        b',' => COMMA,
        b'-' => DASH,
        b'.' => DOT,
        b'/' => SLASH,
        b'0'..=b'9' => DIGITS[(byte - b'0') as usize],
        b':' => COLON,
        b';' => SEMI,
        b'<' => LT,
        b'=' => EQ,
        b'>' => GT,
        b'?' => QMARK,
        b'@' => AT,
        b'A'..=b'Z' => UPPER[(byte - b'A') as usize],
        b'[' => LBRACK,
        b'\\' => BSLASH,
        b']' => RBRACK,
        b'^' => CARET,
        b'_' => UNDERSCORE,
        b'`' => BACKTICK,
        b'a'..=b'z' => LOWER[(byte - b'a') as usize],
        b'{' => LCURLY,
        b'|' => PIPE,
        b'}' => RCURLY,
        b'~' => TILDE,
        _ => UNKNOWN,
    }
}

const UNKNOWN: Glyph = [
    0b11111, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b11111,
];
const SPACE: Glyph = [0; 7];
const EXCL: Glyph = [
    0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00000, 0b00100,
];
const DQUOTE: Glyph = [0b01010, 0b01010, 0b00000, 0, 0, 0, 0];
const HASH: Glyph = [
    0b01010, 0b01010, 0b11111, 0b01010, 0b11111, 0b01010, 0b01010,
];
const DOLLAR: Glyph = [
    0b00100, 0b01111, 0b10100, 0b01110, 0b00101, 0b11110, 0b00100,
];
const PERCENT: Glyph = [
    0b11001, 0b11010, 0b00100, 0b00100, 0b01011, 0b10011, 0b00000,
];
const AMP: Glyph = [
    0b01000, 0b10100, 0b10100, 0b01000, 0b10101, 0b10010, 0b01101,
];
const SQUOTE: Glyph = [0b00100, 0b00100, 0b00000, 0, 0, 0, 0];
const LPAREN: Glyph = [
    0b00010, 0b00100, 0b01000, 0b01000, 0b01000, 0b00100, 0b00010,
];
const RPAREN: Glyph = [
    0b01000, 0b00100, 0b00010, 0b00010, 0b00010, 0b00100, 0b01000,
];
const STAR: Glyph = [
    0b00000, 0b00100, 0b10101, 0b01110, 0b10101, 0b00100, 0b00000,
];
const PLUS: Glyph = [
    0b00000, 0b00100, 0b00100, 0b11111, 0b00100, 0b00100, 0b00000,
];
const COMMA: Glyph = [0, 0, 0, 0, 0b00100, 0b00100, 0b01000];
const DASH: Glyph = [0, 0, 0, 0b11111, 0, 0, 0];
const DOT: Glyph = [0, 0, 0, 0, 0, 0b00100, 0b00100];
const SLASH: Glyph = [
    0b00001, 0b00010, 0b00010, 0b00100, 0b01000, 0b01000, 0b10000,
];
const DIGITS: [Glyph; 10] = [
    // 0
    [
        0b01110, 0b10001, 0b10011, 0b10101, 0b11001, 0b10001, 0b01110,
    ],
    // 1
    [
        0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110,
    ],
    // 2
    [
        0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0b01000, 0b11111,
    ],
    // 3
    [
        0b11111, 0b00010, 0b00100, 0b00010, 0b00001, 0b10001, 0b01110,
    ],
    // 4
    [
        0b00010, 0b00110, 0b01010, 0b10010, 0b11111, 0b00010, 0b00010,
    ],
    // 5
    [
        0b11111, 0b10000, 0b11110, 0b00001, 0b00001, 0b10001, 0b01110,
    ],
    // 6
    [
        0b00110, 0b01000, 0b10000, 0b11110, 0b10001, 0b10001, 0b01110,
    ],
    // 7
    [
        0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b01000, 0b01000,
    ],
    // 8
    [
        0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110,
    ],
    // 9
    [
        0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b00010, 0b01100,
    ],
];
const COLON: Glyph = [0, 0, 0b00100, 0, 0b00100, 0, 0];
const SEMI: Glyph = [0, 0, 0b00100, 0, 0b00100, 0b00100, 0b01000];
const LT: Glyph = [
    0b00010, 0b00100, 0b01000, 0b10000, 0b01000, 0b00100, 0b00010,
];
const EQ: Glyph = [0, 0, 0b11111, 0, 0b11111, 0, 0];
const GT: Glyph = [
    0b01000, 0b00100, 0b00010, 0b00001, 0b00010, 0b00100, 0b01000,
];
const QMARK: Glyph = [
    0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0b00000, 0b00100,
];
const AT: Glyph = [
    0b01110, 0b10001, 0b10111, 0b10101, 0b10111, 0b10000, 0b01110,
];
const UPPER: [Glyph; 26] = [
    // A
    [
        0b01110, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001,
    ],
    // B
    [
        0b11110, 0b10001, 0b10001, 0b11110, 0b10001, 0b10001, 0b11110,
    ],
    // C
    [
        0b01110, 0b10001, 0b10000, 0b10000, 0b10000, 0b10001, 0b01110,
    ],
    // D
    [
        0b11110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b11110,
    ],
    // E
    [
        0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b11111,
    ],
    // F
    [
        0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b10000,
    ],
    // G
    [
        0b01110, 0b10001, 0b10000, 0b10111, 0b10001, 0b10001, 0b01110,
    ],
    // H
    [
        0b10001, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001,
    ],
    // I
    [
        0b01110, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110,
    ],
    // J
    [
        0b00111, 0b00010, 0b00010, 0b00010, 0b00010, 0b10010, 0b01100,
    ],
    // K
    [
        0b10001, 0b10010, 0b10100, 0b11000, 0b10100, 0b10010, 0b10001,
    ],
    // L
    [
        0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b11111,
    ],
    // M
    [
        0b10001, 0b11011, 0b10101, 0b10101, 0b10001, 0b10001, 0b10001,
    ],
    // N
    [
        0b10001, 0b10001, 0b11001, 0b10101, 0b10011, 0b10001, 0b10001,
    ],
    // O
    [
        0b01110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110,
    ],
    // P
    [
        0b11110, 0b10001, 0b10001, 0b11110, 0b10000, 0b10000, 0b10000,
    ],
    // Q
    [
        0b01110, 0b10001, 0b10001, 0b10001, 0b10101, 0b10010, 0b01101,
    ],
    // R
    [
        0b11110, 0b10001, 0b10001, 0b11110, 0b10100, 0b10010, 0b10001,
    ],
    // S
    [
        0b01111, 0b10000, 0b10000, 0b01110, 0b00001, 0b00001, 0b11110,
    ],
    // T
    [
        0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100,
    ],
    // U
    [
        0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110,
    ],
    // V
    [
        0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01010, 0b00100,
    ],
    // W
    [
        0b10001, 0b10001, 0b10001, 0b10101, 0b10101, 0b10101, 0b01010,
    ],
    // X
    [
        0b10001, 0b10001, 0b01010, 0b00100, 0b01010, 0b10001, 0b10001,
    ],
    // Y
    [
        0b10001, 0b10001, 0b10001, 0b01010, 0b00100, 0b00100, 0b00100,
    ],
    // Z
    [
        0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b10000, 0b11111,
    ],
];
const LBRACK: Glyph = [
    0b01110, 0b01000, 0b01000, 0b01000, 0b01000, 0b01000, 0b01110,
];
const BSLASH: Glyph = [
    0b10000, 0b01000, 0b01000, 0b00100, 0b00010, 0b00010, 0b00001,
];
const RBRACK: Glyph = [
    0b01110, 0b00010, 0b00010, 0b00010, 0b00010, 0b00010, 0b01110,
];
const CARET: Glyph = [0b00100, 0b01010, 0b10001, 0, 0, 0, 0];
const UNDERSCORE: Glyph = [0, 0, 0, 0, 0, 0, 0b11111];
const BACKTICK: Glyph = [0b01000, 0b00100, 0, 0, 0, 0, 0];
const LOWER: [Glyph; 26] = [
    // a
    [0, 0, 0b01110, 0b00001, 0b01111, 0b10001, 0b01111],
    // b
    [
        0b10000, 0b10000, 0b11110, 0b10001, 0b10001, 0b10001, 0b11110,
    ],
    // c
    [0, 0, 0b01110, 0b10001, 0b10000, 0b10001, 0b01110],
    // d
    [
        0b00001, 0b00001, 0b01111, 0b10001, 0b10001, 0b10001, 0b01111,
    ],
    // e
    [0, 0, 0b01110, 0b10001, 0b11111, 0b10000, 0b01110],
    // f
    [
        0b00110, 0b01001, 0b01000, 0b11110, 0b01000, 0b01000, 0b01000,
    ],
    // g
    [
        0b01111, 0b10001, 0b10001, 0b01111, 0b00001, 0b00001, 0b01110,
    ],
    // h
    [
        0b10000, 0b10000, 0b11110, 0b10001, 0b10001, 0b10001, 0b10001,
    ],
    // i
    [
        0b00100, 0b00000, 0b01100, 0b00100, 0b00100, 0b00100, 0b01110,
    ],
    // j
    [
        0b00010, 0b00000, 0b00110, 0b00010, 0b00010, 0b10010, 0b01100,
    ],
    // k
    [
        0b10000, 0b10000, 0b10010, 0b10100, 0b11000, 0b10100, 0b10010,
    ],
    // l
    [
        0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110,
    ],
    // m
    [0, 0, 0b11010, 0b10101, 0b10101, 0b10001, 0b10001],
    // n
    [0, 0, 0b11110, 0b10001, 0b10001, 0b10001, 0b10001],
    // o
    [0, 0, 0b01110, 0b10001, 0b10001, 0b10001, 0b01110],
    // p
    [
        0b11110, 0b10001, 0b10001, 0b11110, 0b10000, 0b10000, 0b10000,
    ],
    // q
    [
        0b01111, 0b10001, 0b10001, 0b01111, 0b00001, 0b00001, 0b00001,
    ],
    // r
    [0, 0, 0b10110, 0b11001, 0b10000, 0b10000, 0b10000],
    // s
    [0, 0, 0b01111, 0b10000, 0b01110, 0b00001, 0b11110],
    // t
    [
        0b01000, 0b01000, 0b11110, 0b01000, 0b01000, 0b01001, 0b00110,
    ],
    // u
    [0, 0, 0b10001, 0b10001, 0b10001, 0b10011, 0b01101],
    // v
    [0, 0, 0b10001, 0b10001, 0b10001, 0b01010, 0b00100],
    // w
    [0, 0, 0b10001, 0b10001, 0b10101, 0b10101, 0b01010],
    // x
    [0, 0, 0b10001, 0b01010, 0b00100, 0b01010, 0b10001],
    // y
    [
        0b10001, 0b10001, 0b10001, 0b01111, 0b00001, 0b00001, 0b01110,
    ],
    // z
    [0, 0, 0b11111, 0b00010, 0b00100, 0b01000, 0b11111],
];
const LCURLY: Glyph = [
    0b00010, 0b00100, 0b00100, 0b01000, 0b00100, 0b00100, 0b00010,
];
const PIPE: Glyph = [
    0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100,
];
const RCURLY: Glyph = [
    0b01000, 0b00100, 0b00100, 0b00010, 0b00100, 0b00100, 0b01000,
];
const TILDE: Glyph = [0, 0, 0b01001, 0b10110, 0, 0, 0];

#[cfg(test)]
mod tests {
    use super::*;

    /// The letters that hang below the baseline are not the letters they
    /// used to read as, and start on the x-height once drawn at their
    /// [`descent`].
    #[test]
    fn descending_letters_are_distinct_and_sit_on_the_x_height() {
        for (a, b) in [(b'g', b's'), (b'p', b'r'), (b'g', b'q'), (b'y', b'v')] {
            assert_ne!(glyph(a), glyph(b));
        }
        let top = |b: u8| glyph(b).iter().position(|&r| r != 0).unwrap() as u32 + descent(b);
        for b in [b'g', b'p', b'q', b'y'] {
            assert_eq!(top(b), top(b'a'), "{}", b as char);
        }
    }

    #[test]
    fn space_is_blank() {
        assert_eq!(glyph(b' '), [0; 7]);
    }

    #[test]
    fn capital_a_starts_with_top_arc() {
        let a = glyph(b'A');
        assert_eq!(a[0], 0b01110);
        assert_eq!(a[3], 0b11111);
    }

    #[test]
    fn digit_zero_full_outline() {
        let z = glyph(b'0');
        assert_eq!(z[0], 0b01110);
        assert_eq!(z[6], 0b01110);
    }

    #[test]
    fn unknown_byte_returns_block() {
        let g = glyph(0xFE);
        assert_eq!(g[0], 0b11111);
        assert_eq!(g[6], 0b11111);
    }
}
