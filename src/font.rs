//! A tiny built-in bitmap font, so games can draw text without loading files.
//!
//! Every character is 3 pixels wide. Capitals and digits are 5 pixels
//! tall; lowercase letters are 4 tall (with a 5th for the tall ones like
//! `b` and `t`), and `g j p q y` hang one pixel below the line, into the
//! gap between lines. A glyph is stored as six rows (the sixth is that
//! hanging row); in each row the three lowest bits say which pixels are
//! lit, left to right. For example the letters `A` and `g`:
//!
//! ```text
//! 0b010   . # .        0b000   . . .
//! 0b101   # . #        0b011   . # #
//! 0b111   # # #        0b101   # . #
//! 0b101   # . #        0b011   . # #
//! 0b101   # . #        0b001   . . #
//! 0b000   . . .        0b110   # # .
//! ```
//!
//! Every printable ASCII character, from space to `~`, has a glyph.
//! Anything else is drawn as `?`.

pub const GLYPH_WIDTH: usize = 3;
/// The height of capitals and digits.
pub const GLYPH_HEIGHT: usize = 5;
/// Rows stored per glyph: the 5 rows, plus one below the line for `g j p q y`.
pub const GLYPH_ROWS: usize = GLYPH_HEIGHT + 1;

/// Pixels from the start of one character to the start of the next (glyph + 1 px gap).
pub const ADVANCE: usize = GLYPH_WIDTH + 1;
/// Pixels from the top of one line of text to the top of the next.
pub const LINE_HEIGHT: usize = GLYPH_HEIGHT + 2;

type Glyph = [u8; GLYPH_ROWS];

const UNKNOWN: Glyph = [0b111, 0b001, 0b010, 0b000, 0b010, 0b000]; // drawn as '?'

/// Looks up the bitmap for a character.
pub fn glyph(c: char) -> Glyph {
    match c {
        'A' => [0b010, 0b101, 0b111, 0b101, 0b101, 0b000],
        'B' => [0b110, 0b101, 0b110, 0b101, 0b110, 0b000],
        'C' => [0b011, 0b100, 0b100, 0b100, 0b011, 0b000],
        'D' => [0b110, 0b101, 0b101, 0b101, 0b110, 0b000],
        'E' => [0b111, 0b100, 0b110, 0b100, 0b111, 0b000],
        'F' => [0b111, 0b100, 0b110, 0b100, 0b100, 0b000],
        'G' => [0b011, 0b100, 0b101, 0b101, 0b011, 0b000],
        'H' => [0b101, 0b101, 0b111, 0b101, 0b101, 0b000],
        'I' => [0b111, 0b010, 0b010, 0b010, 0b111, 0b000],
        'J' => [0b001, 0b001, 0b001, 0b101, 0b010, 0b000],
        'K' => [0b101, 0b101, 0b110, 0b101, 0b101, 0b000],
        'L' => [0b100, 0b100, 0b100, 0b100, 0b111, 0b000],
        'M' => [0b101, 0b111, 0b111, 0b101, 0b101, 0b000],
        'N' => [0b110, 0b101, 0b101, 0b101, 0b101, 0b000],
        'O' => [0b010, 0b101, 0b101, 0b101, 0b010, 0b000],
        'P' => [0b110, 0b101, 0b110, 0b100, 0b100, 0b000],
        'Q' => [0b010, 0b101, 0b101, 0b110, 0b011, 0b000],
        'R' => [0b110, 0b101, 0b110, 0b101, 0b101, 0b000],
        'S' => [0b011, 0b100, 0b010, 0b001, 0b110, 0b000],
        'T' => [0b111, 0b010, 0b010, 0b010, 0b010, 0b000],
        'U' => [0b101, 0b101, 0b101, 0b101, 0b111, 0b000],
        'V' => [0b101, 0b101, 0b101, 0b101, 0b010, 0b000],
        'W' => [0b101, 0b101, 0b111, 0b111, 0b101, 0b000],
        'X' => [0b101, 0b101, 0b010, 0b101, 0b101, 0b000],
        'Y' => [0b101, 0b101, 0b010, 0b010, 0b010, 0b000],
        'Z' => [0b111, 0b001, 0b010, 0b100, 0b111, 0b000],
        '0' => [0b111, 0b101, 0b101, 0b101, 0b111, 0b000],
        '1' => [0b010, 0b110, 0b010, 0b010, 0b111, 0b000],
        '2' => [0b111, 0b001, 0b111, 0b100, 0b111, 0b000],
        '3' => [0b111, 0b001, 0b111, 0b001, 0b111, 0b000],
        '4' => [0b101, 0b101, 0b111, 0b001, 0b001, 0b000],
        '5' => [0b111, 0b100, 0b111, 0b001, 0b111, 0b000],
        '6' => [0b111, 0b100, 0b111, 0b101, 0b111, 0b000],
        '7' => [0b111, 0b001, 0b001, 0b010, 0b010, 0b000],
        '8' => [0b111, 0b101, 0b111, 0b101, 0b111, 0b000],
        '9' => [0b111, 0b101, 0b111, 0b001, 0b111, 0b000],
        ' ' => [0b000, 0b000, 0b000, 0b000, 0b000, 0b000],
        '.' => [0b000, 0b000, 0b000, 0b000, 0b010, 0b000],
        ',' => [0b000, 0b000, 0b000, 0b010, 0b100, 0b000],
        ':' => [0b000, 0b010, 0b000, 0b010, 0b000, 0b000],
        '!' => [0b010, 0b010, 0b010, 0b000, 0b010, 0b000],
        '?' => UNKNOWN,
        '-' => [0b000, 0b000, 0b111, 0b000, 0b000, 0b000],
        '+' => [0b000, 0b010, 0b111, 0b010, 0b000, 0b000],
        '=' => [0b000, 0b111, 0b000, 0b111, 0b000, 0b000],
        '/' => [0b001, 0b001, 0b010, 0b100, 0b100, 0b000],
        '(' => [0b001, 0b010, 0b010, 0b010, 0b001, 0b000],
        ')' => [0b100, 0b010, 0b010, 0b010, 0b100, 0b000],
        '\'' => [0b010, 0b010, 0b000, 0b000, 0b000, 0b000],
        'a' => [0b000, 0b110, 0b011, 0b101, 0b011, 0b000],
        'b' => [0b100, 0b110, 0b101, 0b101, 0b110, 0b000],
        'c' => [0b000, 0b011, 0b100, 0b100, 0b011, 0b000],
        'd' => [0b001, 0b011, 0b101, 0b101, 0b011, 0b000],
        'e' => [0b000, 0b010, 0b111, 0b100, 0b011, 0b000],
        'f' => [0b011, 0b010, 0b111, 0b010, 0b010, 0b000],
        'g' => [0b000, 0b011, 0b101, 0b011, 0b001, 0b110],
        'h' => [0b100, 0b110, 0b101, 0b101, 0b101, 0b000],
        'i' => [0b010, 0b000, 0b110, 0b010, 0b111, 0b000],
        'j' => [0b001, 0b000, 0b001, 0b001, 0b101, 0b010],
        'k' => [0b100, 0b101, 0b110, 0b110, 0b101, 0b000],
        'l' => [0b110, 0b010, 0b010, 0b010, 0b011, 0b000],
        'm' => [0b000, 0b111, 0b111, 0b101, 0b101, 0b000],
        'n' => [0b000, 0b110, 0b101, 0b101, 0b101, 0b000],
        'o' => [0b000, 0b010, 0b101, 0b101, 0b010, 0b000],
        'p' => [0b000, 0b110, 0b101, 0b110, 0b100, 0b100],
        'q' => [0b000, 0b011, 0b101, 0b011, 0b001, 0b001],
        'r' => [0b000, 0b011, 0b100, 0b100, 0b100, 0b000],
        's' => [0b000, 0b011, 0b110, 0b001, 0b110, 0b000],
        't' => [0b010, 0b111, 0b010, 0b010, 0b001, 0b000],
        'u' => [0b000, 0b101, 0b101, 0b101, 0b011, 0b000],
        'v' => [0b000, 0b101, 0b101, 0b101, 0b010, 0b000],
        'w' => [0b000, 0b101, 0b111, 0b111, 0b101, 0b000],
        'x' => [0b000, 0b101, 0b010, 0b010, 0b101, 0b000],
        'y' => [0b000, 0b101, 0b101, 0b011, 0b001, 0b110],
        'z' => [0b000, 0b111, 0b011, 0b110, 0b111, 0b000],
        '"' => [0b101, 0b101, 0b000, 0b000, 0b000, 0b000],
        '#' => [0b101, 0b111, 0b101, 0b111, 0b101, 0b000],
        '$' => [0b011, 0b110, 0b010, 0b011, 0b110, 0b000],
        '%' => [0b101, 0b001, 0b010, 0b100, 0b101, 0b000],
        '&' => [0b010, 0b101, 0b010, 0b101, 0b011, 0b000],
        '*' => [0b000, 0b101, 0b010, 0b101, 0b000, 0b000],
        ';' => [0b000, 0b010, 0b000, 0b010, 0b100, 0b000],
        '<' => [0b001, 0b010, 0b100, 0b010, 0b001, 0b000],
        '>' => [0b100, 0b010, 0b001, 0b010, 0b100, 0b000],
        '@' => [0b010, 0b101, 0b111, 0b100, 0b011, 0b000],
        '[' => [0b011, 0b010, 0b010, 0b010, 0b011, 0b000],
        '\\' => [0b100, 0b100, 0b010, 0b001, 0b001, 0b000],
        ']' => [0b110, 0b010, 0b010, 0b010, 0b110, 0b000],
        '^' => [0b010, 0b101, 0b000, 0b000, 0b000, 0b000],
        '_' => [0b000, 0b000, 0b000, 0b000, 0b111, 0b000],
        '`' => [0b100, 0b010, 0b000, 0b000, 0b000, 0b000],
        '{' => [0b011, 0b010, 0b110, 0b010, 0b011, 0b000],
        '|' => [0b010, 0b010, 0b010, 0b010, 0b010, 0b000],
        '}' => [0b110, 0b010, 0b011, 0b010, 0b110, 0b000],
        '~' => [0b000, 0b011, 0b110, 0b000, 0b000, 0b000],
        _ => UNKNOWN,
    }
}

/// Is the pixel at (`col`, `row`) of this glyph lit? `col` 0 is the left column.
pub fn is_lit(glyph: &Glyph, col: usize, row: usize) -> bool {
    let bit = GLYPH_WIDTH - 1 - col;
    (glyph[row] >> bit) & 1 == 1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_printable_ascii_character_has_a_real_glyph() {
        for c in ' '..='~' {
            if c != '?' {
                assert_ne!(glyph(c), UNKNOWN, "missing glyph for {c:?}");
            }
            if c != ' ' {
                assert_ne!(glyph(c), glyph(' '), "{c:?} is drawn as nothing");
            }
        }
        assert_eq!(glyph('é'), UNKNOWN, "anything else is a question mark");
    }

    #[test]
    fn no_two_characters_look_the_same() {
        let mut seen = std::collections::HashMap::new();
        for c in ' '..='~' {
            if let Some(other) = seen.insert(glyph(c), c) {
                panic!("{other:?} and {c:?} have the same glyph");
            }
        }
    }

    #[test]
    fn only_lowercase_letters_hang_below_the_line() {
        for c in ' '..='~' {
            let hangs = glyph(c)[GLYPH_HEIGHT] != 0;
            assert_eq!(hangs, "gjpqy".contains(c), "{c:?}");
        }
    }

    #[test]
    fn lowercase_is_not_uppercase() {
        assert_ne!(glyph('a'), glyph('A'));
    }

    #[test]
    fn reads_bits_left_to_right() {
        let a = glyph('A');
        assert!(!is_lit(&a, 0, 0));
        assert!(is_lit(&a, 1, 0));
        assert!(is_lit(&a, 0, 1));
        assert!(!is_lit(&a, 1, 1));
    }
}
