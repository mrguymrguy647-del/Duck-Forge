//! The game in the middle of the Studio: its picture, shown exactly as
//! the game drew it.
//!
//! Every game pixel is a sharp square: the picture is enlarged by a whole
//! number of *screen* pixels (not egui's points, which Windows' display
//! scaling stretches), with nearest-pixel sampling, and it starts on a
//! whole screen pixel. Black fills the rest.

use eframe::egui::{self, Color32, Rect, TextureHandle, TextureOptions, pos2, vec2};
use studio_core::play::Picture;

use crate::keys::Placement;
use crate::pictures::game_image;

/// Where a `width` x `height` game goes in `space` (in points), on a
/// screen with `pixels_per_point`: as big as fits at a whole number of
/// screen pixels per game pixel, centred, on whole screen pixels.
pub fn place(space: Rect, width: u32, height: u32, pixels_per_point: f32) -> Placement {
    let room = space.size() * pixels_per_point; // in screen pixels
    let scale = (room.x / width.max(1) as f32)
        .min(room.y / height.max(1) as f32)
        .floor()
        .max(1.0);
    let size = vec2(width as f32, height as f32) * scale; // in screen pixels
    let corner = ((space.min.to_vec2() * pixels_per_point) + (room - size) / 2.0).round();
    Placement {
        rect: Rect::from_min_size(
            pos2(corner.x, corner.y) / pixels_per_point,
            size / pixels_per_point,
        ),
        scale,
        pixels_per_point,
    }
}

/// The game's picture as a texture, kept up to date.
#[derive(Default)]
pub struct GameTexture {
    texture: Option<TextureHandle>,
    /// Which picture the texture holds.
    serial: u64,
}

impl GameTexture {
    /// Makes sure the texture holds `picture`.
    pub fn show(&mut self, ctx: &egui::Context, picture: &Picture) -> &TextureHandle {
        let image = || game_image(picture.width, picture.height, &picture.pixels);
        match &mut self.texture {
            Some(texture) => {
                if self.serial != picture.serial {
                    texture.set(image(), TextureOptions::NEAREST);
                }
            }
            None => {
                self.texture = Some(ctx.load_texture("the game", image(), TextureOptions::NEAREST));
            }
        }
        self.serial = picture.serial;
        self.texture.as_ref().expect("just set")
    }

    pub fn forget(&mut self) {
        self.texture = None;
    }
}

/// Paints the game's picture into `space`, and says where it went.
pub fn paint(
    ui: &mut egui::Ui,
    space: Rect,
    texture: &mut GameTexture,
    picture: &Picture,
    dimmed: bool,
) -> Placement {
    let placement = place(
        space,
        picture.width,
        picture.height,
        ui.ctx().pixels_per_point(),
    );
    let painter = ui.painter_at(space);
    painter.rect_filled(space, 0.0, Color32::BLACK);
    let whole = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
    let tint = if dimmed {
        Color32::from_gray(90)
    } else {
        Color32::WHITE
    };
    let texture = texture.show(ui.ctx(), picture);
    painter.image(texture.id(), placement.rect, whole, tint);
    placement
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_game_is_enlarged_by_a_whole_number_of_screen_pixels() {
        // 1000 x 700 points at 100%: 320 x 240 fits 2 times (640 x 480);
        // 3 times would be 720 tall.
        let space = Rect::from_min_size(pos2(0.0, 40.0), vec2(1000.0, 700.0));
        let placed = place(space, 320, 240, 1.0);
        assert_eq!(placed.scale, 2.0);
        assert_eq!(placed.rect.size(), vec2(640.0, 480.0));
        assert_eq!(placed.rect.min, pos2(180.0, 150.0), "centred");
        // The same window at 150% has 1500 x 1050 screen pixels: 4 times.
        let placed = place(space, 320, 240, 1.5);
        assert_eq!(placed.scale, 4.0);
        // In screen pixels the picture is exactly 1280 x 960, on whole pixels.
        let pixels = placed.rect.size() * 1.5;
        assert!((pixels.x - 1280.0).abs() < 0.01 && (pixels.y - 960.0).abs() < 0.01);
        let corner = placed.rect.min.to_vec2() * 1.5;
        assert!((corner.x - corner.x.round()).abs() < 0.001, "{corner:?}");
        assert!((corner.y - corner.y.round()).abs() < 0.001, "{corner:?}");
        // At 125% too.
        let placed = place(space, 320, 240, 1.25);
        assert_eq!(placed.scale, 3.0);
        let corner = placed.rect.min.to_vec2() * 1.25;
        assert!((corner.x - corner.x.round()).abs() < 0.001, "{corner:?}");
    }

    #[test]
    fn a_space_too_small_still_shows_the_game_at_its_own_size() {
        let space = Rect::from_min_size(pos2(0.0, 0.0), vec2(200.0, 100.0));
        assert_eq!(place(space, 320, 240, 1.0).scale, 1.0);
    }
}
