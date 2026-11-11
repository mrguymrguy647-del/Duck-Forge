//! Pictures as egui textures: the game's own picture, and PNG files (a
//! game's card, a check's sheet).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use eframe::egui::{self, Color32, ColorImage, TextureHandle, TextureOptions};

/// The game's picture as an image egui can show: every pixel as it is.
pub fn game_image(width: u32, height: u32, pixels: &[u32]) -> ColorImage {
    let colors = pixels
        .iter()
        .map(|&pixel| Color32::from_rgb((pixel >> 16) as u8, (pixel >> 8) as u8, pixel as u8))
        .collect();
    ColorImage::new([width as usize, height as usize], colors)
}

/// Reads a PNG file.
pub fn read_png(path: &Path) -> Option<ColorImage> {
    let file = std::io::BufReader::new(std::fs::File::open(path).ok()?);
    let mut decoder = png::Decoder::new(file);
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().ok()?;
    let mut bytes = vec![0; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut bytes).ok()?;
    let size = [info.width as usize, info.height as usize];
    let bytes = &bytes[..info.buffer_size()];
    match info.color_type {
        png::ColorType::Rgb => Some(ColorImage::from_rgb(size, bytes)),
        png::ColorType::Rgba => Some(ColorImage::from_rgba_unmultiplied(size, bytes)),
        png::ColorType::Grayscale => Some(ColorImage::from_gray(size, bytes)),
        _ => None,
    }
}

/// PNG files already loaded as textures, each until its file changes.
#[derive(Default)]
pub struct Pictures {
    loaded: HashMap<PathBuf, (Option<SystemTime>, Option<TextureHandle>)>,
}

impl Pictures {
    /// The texture for a PNG file, loading it the first time (and again
    /// if the file has changed since). `None` if it can't be read.
    pub fn get(&mut self, ctx: &egui::Context, path: &Path) -> Option<TextureHandle> {
        let changed = std::fs::metadata(path)
            .and_then(|data| data.modified())
            .ok();
        if let Some((seen, texture)) = self.loaded.get(path)
            && *seen == changed
        {
            return texture.clone();
        }
        // Game pictures are small and sharp: never smoothed.
        let texture = read_png(path).map(|image| {
            ctx.load_texture(path.display().to_string(), image, TextureOptions::NEAREST)
        });
        self.loaded
            .insert(path.to_path_buf(), (changed, texture.clone()));
        texture
    }
}
