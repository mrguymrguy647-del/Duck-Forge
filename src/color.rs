//! Colors.

/// A color: red, green and blue, one byte (0-255) each, plus `a`, how
/// solid it is.
///
/// `a` is the *alpha*: 255 (what [`Color::rgb`] gives) is fully solid, 0
/// is invisible, and anything between is see-through, so whatever is
/// already on the canvas shows through. Use [`Color::rgba`] or
/// [`Color::with_alpha`] to make a see-through color.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    /// How solid the color is: 255 fully solid, 0 invisible.
    pub a: u8,
}

impl Color {
    pub const BLACK: Color = Color::rgb(0, 0, 0);
    pub const WHITE: Color = Color::rgb(255, 255, 255);
    pub const GRAY: Color = Color::rgb(128, 128, 128);
    pub const RED: Color = Color::rgb(230, 60, 60);
    pub const ORANGE: Color = Color::rgb(245, 150, 50);
    pub const YELLOW: Color = Color::rgb(245, 220, 70);
    pub const GREEN: Color = Color::rgb(80, 200, 100);
    pub const BLUE: Color = Color::rgb(70, 130, 230);
    pub const PURPLE: Color = Color::rgb(160, 90, 220);

    /// `const fn` means this can run at compile time, which is what lets
    /// us use it to define the constants above.
    ///
    /// The color is fully solid (its alpha is 255).
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 255 }
    }

    /// A color with an alpha: `a` is 255 for fully solid, 0 for invisible,
    /// 128 for about half see-through.
    ///
    /// ```
    /// # use duckforge::prelude::*;
    /// let shadow = Color::rgba(0, 0, 0, 100); // a light, see-through black
    /// ```
    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    /// The same color with a different alpha (0 invisible, 255 solid).
    ///
    /// ```
    /// # use duckforge::prelude::*;
    /// let glass = Color::BLUE.with_alpha(80);
    /// assert_eq!(glass.a, 80);
    /// ```
    pub const fn with_alpha(self, a: u8) -> Self {
        Self { a, ..self }
    }

    /// True if the color is fully solid (alpha 255).
    pub const fn is_opaque(self) -> bool {
        self.a == 255
    }

    /// Builds a solid color from a hex literal like `0xFF8800`.
    pub const fn from_hex(hex: u32) -> Self {
        Self::rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
    }

    /// Mixes two colors (alpha too). `t = 0.0` gives `self`, `t = 1.0`
    /// gives `other`.
    pub fn lerp(self, other: Color, t: f32) -> Color {
        let t = t.clamp(0.0, 1.0);
        let mix = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * t).round() as u8;
        Color::rgba(
            mix(self.r, other.r),
            mix(self.g, other.g),
            mix(self.b, other.b),
            mix(self.a, other.a),
        )
    }

    /// Packs the color into the `0x00RRGGBB` format used by the pixel
    /// buffer. The alpha is left out: the canvas itself is always solid.
    pub const fn to_u32(self) -> u32 {
        ((self.r as u32) << 16) | ((self.g as u32) << 8) | self.b as u32
    }
}

/// Paints `color` (`0x00RRGGBB`) over the canvas pixel `under` with
/// strength `alpha` (0 to 255), and gives the mixed pixel.
///
/// Each channel becomes `(new * alpha + old * (255 - alpha)) / 255`,
/// rounded to the nearest whole number, using only whole-number math so
/// the result is exactly the same on every computer.
pub(crate) fn blend(under: u32, color: u32, alpha: u8) -> u32 {
    let a = alpha as u32;
    let mix = |shift: u32| {
        let new = (color >> shift) & 0xff;
        let old = (under >> shift) & 0xff;
        ((new * a + old * (255 - a) + 127) / 255) << shift
    };
    mix(16) | mix(8) | mix(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rgb_is_solid_and_alpha_can_be_set() {
        assert_eq!(Color::rgb(1, 2, 3).a, 255);
        assert!(Color::RED.is_opaque());
        let glass = Color::RED.with_alpha(10);
        assert_eq!(glass, Color::rgba(230, 60, 60, 10));
        assert!(!glass.is_opaque());
        assert_eq!(glass.to_u32(), Color::RED.to_u32(), "alpha isn't packed");
        assert_eq!(Color::from_hex(0x112233).a, 255);
    }

    #[test]
    fn blending_half_red_over_blue() {
        let red = Color::rgb(255, 0, 0).to_u32();
        let blue = Color::rgb(0, 0, 255).to_u32();
        assert_eq!(blend(blue, red, 128), Color::rgb(128, 0, 127).to_u32());
        assert_eq!(blend(blue, red, 255), red);
        assert_eq!(blend(blue, red, 0), blue);
    }

    #[test]
    fn hex_round_trip() {
        let c = Color::from_hex(0x12_34_56);
        assert_eq!(c, Color::rgb(0x12, 0x34, 0x56));
        assert_eq!(c.to_u32(), 0x12_34_56);
    }

    #[test]
    fn lerp_endpoints_and_middle() {
        assert_eq!(Color::BLACK.lerp(Color::WHITE, 0.0), Color::BLACK);
        assert_eq!(Color::BLACK.lerp(Color::WHITE, 1.0), Color::WHITE);
        assert_eq!(
            Color::BLACK.lerp(Color::WHITE, 0.5),
            Color::rgb(128, 128, 128)
        );
    }
}
