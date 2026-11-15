//! Each frame, measured at a reduced size: how bright it is, how much
//! changed, its middle column and row, and a thumbnail for filmstrips.

use super::truth::{FrameTruth, luma};
use crate::canvas::Canvas;

/// The longest side frames are measured at.
pub(crate) const ANALYSIS_SIDE: usize = 480;

/// A frame's numbers.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct FrameStat {
    pub(crate) frame: u64,
    /// Average brightness, 0 to 1.
    pub(crate) mean: f32,
    /// Average brightness change from the frame before, 0 to 1.
    pub(crate) change: f32,
    /// The share of pixels that changed visibly (by 3/255 or more).
    pub(crate) changed: f32,
    /// The share of pixels almost black, and almost white.
    pub(crate) dark: f32,
    pub(crate) bright: f32,
    /// How much brightness varies over the frame (0: one flat colour).
    pub(crate) spread: f32,
}

/// A small picture: width, height, and `0xRRGGBB` pixels.
#[derive(Debug, Clone, Default)]
pub(crate) struct Small {
    pub(crate) width: usize,
    pub(crate) height: usize,
    pub(crate) pixels: Vec<u32>,
}

impl Small {
    pub(crate) fn to_canvas(&self) -> Canvas {
        Canvas::from_pixels(self.width, self.height, self.pixels.clone())
    }
}

/// `pixels` (`width` x `height`) shrunk by averaging blocks, to fit
/// `side` on its longest side (never enlarged).
pub(crate) fn shrink(pixels: &[u32], width: usize, height: usize, side: usize) -> Small {
    let factor = width.max(height).div_ceil(side.max(1)).max(1);
    let (w, h) = ((width / factor).max(1), (height / factor).max(1));
    if factor == 1 {
        return Small {
            width,
            height,
            pixels: pixels.to_vec(),
        };
    }
    let mut out = Vec::with_capacity(w * h);
    let count = (factor * factor) as u32;
    for by in 0..h {
        for bx in 0..w {
            let (mut r, mut g, mut b) = (0u32, 0u32, 0u32);
            for y in by * factor..(by * factor + factor).min(height) {
                let row =
                    &pixels[y * width + bx * factor..y * width + (bx * factor + factor).min(width)];
                for &p in row {
                    r += (p >> 16) & 255;
                    g += (p >> 8) & 255;
                    b += p & 255;
                }
            }
            out.push(((r / count) << 16) | ((g / count) << 8) | (b / count));
        }
    }
    Small {
        width: w,
        height: h,
        pixels: out,
    }
}

/// A copy of a frame kept for `viewer.png` (when a caption appears).
pub(crate) struct Kept {
    pub(crate) frame: u64,
    pub(crate) picture: Small,
    pub(crate) truth_index: usize,
}

/// Collects every frame's numbers as a run plays (or a video is read).
pub(crate) struct Recorder {
    pub(crate) width: usize,
    pub(crate) height: usize,
    pub(crate) stats: Vec<FrameStat>,
    pub(crate) thumbs: Vec<Small>,
    /// Each frame's middle column and middle row, at analysis size.
    pub(crate) columns: Vec<Vec<u32>>,
    pub(crate) rows: Vec<Vec<u32>>,
    pub(crate) truths: Vec<Option<FrameTruth>>,
    pub(crate) kept: Vec<Kept>,
    thumb_side: usize,
    previous: Vec<f32>,
    last_texts: String,
}

impl Recorder {
    /// A recorder for frames `width` x `height`, about `frames` of them
    /// (thumbnails get smaller for long runs, to fit in memory).
    pub(crate) fn new(width: usize, height: usize, frames: u64) -> Recorder {
        let thumb_side = match frames {
            0..=2400 => 160,
            2401..=6000 => 112,
            _ => 80,
        };
        Recorder {
            width,
            height,
            stats: Vec::new(),
            thumbs: Vec::new(),
            columns: Vec::new(),
            rows: Vec::new(),
            truths: Vec::new(),
            kept: Vec::new(),
            thumb_side,
            previous: Vec::new(),
            last_texts: String::new(),
        }
    }

    /// Measures one frame (full size), with what the engine knows of it.
    pub(crate) fn add(&mut self, frame: u64, pixels: &[u32], truth: Option<FrameTruth>) {
        let small = shrink(pixels, self.width, self.height, ANALYSIS_SIDE);
        self.add_small(frame, small, truth, Some(pixels));
    }

    /// Measures one frame already at analysis size (a video's).
    pub(crate) fn add_small(
        &mut self,
        frame: u64,
        small: Small,
        truth: Option<FrameTruth>,
        full: Option<&[u32]>,
    ) {
        let lumas: Vec<f32> = small.pixels.iter().map(|&p| luma(p)).collect();
        let n = lumas.len().max(1) as f32;
        let mean = lumas.iter().sum::<f32>() / n;
        let variance = lumas.iter().map(|l| (l - mean) * (l - mean)).sum::<f32>() / n;
        let dark = lumas.iter().filter(|&&l| l < 0.06).count() as f32 / n;
        let bright = lumas.iter().filter(|&&l| l > 0.97).count() as f32 / n;
        let (change, changed) = if self.previous.len() == lumas.len() {
            let mut sum = 0.0;
            let mut count = 0;
            for (a, b) in self.previous.iter().zip(&lumas) {
                let d = (a - b).abs();
                sum += d;
                if d >= 3.0 / 255.0 {
                    count += 1;
                }
            }
            (sum / n, count as f32 / n)
        } else {
            (0.0, 0.0)
        };
        self.stats.push(FrameStat {
            frame,
            mean,
            change,
            changed,
            dark,
            bright,
            spread: variance.sqrt(),
        });
        let (w, h) = (small.width, small.height);
        self.columns
            .push((0..h).map(|y| small.pixels[y * w + w / 2]).collect());
        self.rows
            .push(small.pixels[(h / 2) * w..(h / 2) * w + w].to_vec());
        self.thumbs
            .push(shrink(&small.pixels, w, h, self.thumb_side));
        self.previous = lumas;
        // A new caption (a different set of words on screen): keep a
        // bigger copy of the frame for viewer.png.
        if let Some(truth) = &truth {
            let words: String = truth
                .texts
                .iter()
                .map(|t| t.text.as_str())
                .collect::<Vec<_>>()
                .join("\n");
            // At most one a second (30 frames), so a caption that types
            // itself out doesn't fill the list.
            let apart = self.kept.last().is_none_or(|k| frame >= k.frame + 30);
            if words != self.last_texts && !words.trim().is_empty() && apart && self.kept.len() < 90
            {
                let picture = match full {
                    Some(p) => shrink(p, self.width, self.height, 720),
                    None => small.clone(),
                };
                self.kept.push(Kept {
                    frame,
                    picture,
                    truth_index: self.truths.len(),
                });
            }
            self.last_texts = words;
        }
        self.truths.push(truth);
    }

    /// Where `frame` is in the lists, if it was recorded.
    pub(crate) fn index_of(&self, frame: u64) -> Option<usize> {
        self.stats.binary_search_by_key(&frame, |s| s.frame).ok()
    }

    /// The index of the recorded frame nearest `frame`.
    pub(crate) fn nearest(&self, frame: u64) -> Option<usize> {
        if self.stats.is_empty() {
            return None;
        }
        Some(match self.stats.binary_search_by_key(&frame, |s| s.frame) {
            Ok(i) => i,
            Err(i) if i >= self.stats.len() => self.stats.len() - 1,
            Err(0) => 0,
            Err(i) => {
                if frame - self.stats[i - 1].frame <= self.stats[i].frame - frame {
                    i - 1
                } else {
                    i
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shrinking_averages_blocks() {
        // 4 x 2: a white and a black 2 x 2 block, shrunk to 2 x 1.
        let pixels = vec![0xffffff, 0xffffff, 0, 0, 0xffffff, 0xffffff, 0, 0];
        let small = shrink(&pixels, 4, 2, 2);
        assert_eq!((small.width, small.height), (2, 1));
        assert_eq!(small.pixels, vec![0xffffff, 0]);
    }

    #[test]
    fn frames_are_measured() {
        let mut recorder = Recorder::new(8, 8, 2);
        recorder.add(1, &[0; 64], None);
        recorder.add(2, &[0xffffff; 64], None);
        let [a, b] = [recorder.stats[0], recorder.stats[1]];
        assert_eq!((a.mean, a.dark, a.change), (0.0, 1.0, 0.0));
        assert_eq!((b.mean, b.bright, b.changed), (1.0, 1.0, 1.0));
        assert!((b.change - 1.0).abs() < 1e-6);
        assert_eq!(recorder.columns[1].len(), 8);
        assert_eq!(recorder.nearest(5), Some(1));
        assert_eq!(recorder.index_of(2), Some(1));
    }
}
