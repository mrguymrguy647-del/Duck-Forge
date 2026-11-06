//! `--video FILE.mp4`: the whole run as a video, at the game's full frame
//! rate, sharp, with its sound.
//!
//! Every frame is drawn and handed, at the game's own size, to `ffmpeg`,
//! which enlarges it by the biggest whole number that fits in 1920 x 1080
//! (each pixel a sharp square: 320 x 240 becomes 1280 x 960) and centres it
//! on black. Only whole numbers keep every pixel the same size; 4.5 times,
//! to fill the height exactly, would make some pixels wider than others.
//! After the run, the sound (`audio.wav`) is added.
//!
//! Without `ffmpeg`, the frames are saved as PNG files instead, next to
//! `audio.wav`, and the exact `ffmpeg` command to join them is printed.
//! `DUCKFORGE_FFMPEG` names another `ffmpeg` program (a full path, say).

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};

use crate::canvas::Canvas;

/// The video's size: the game is enlarged by a whole number to fit in this.
const FRAME_WIDTH: usize = 1920;
const FRAME_HEIGHT: usize = 1080;

/// The ffmpeg program to use.
fn ffmpeg() -> String {
    std::env::var("DUCKFORGE_FFMPEG").unwrap_or_else(|_| String::from("ffmpeg"))
}

/// The biggest whole number the game can be enlarged by and still fit in
/// 1920 x 1080, and the size of the video (at least the game's own size,
/// rounded up to even numbers, which video formats need).
pub(crate) fn layout(width: usize, height: usize) -> (usize, usize, usize) {
    let scale = (FRAME_WIDTH / width.max(1))
        .min(FRAME_HEIGHT / height.max(1))
        .max(1);
    let even = |n: usize| n + n % 2;
    let video_w = even(FRAME_WIDTH.max(width * scale));
    let video_h = even(FRAME_HEIGHT.max(height * scale));
    (scale, video_w, video_h)
}

/// ffmpeg's filter for that: enlarge with sharp pixels, centre on black.
fn filter(width: usize, height: usize) -> String {
    let (scale, video_w, video_h) = layout(width, height);
    format!(
        "scale={}:{}:flags=neighbor,pad={video_w}:{video_h}:(ow-iw)/2:(oh-ih)/2:black",
        width * scale,
        height * scale
    )
}

/// Collects a run's frames into a video (or into PNG files, without ffmpeg).
pub(crate) struct VideoWriter {
    path: PathBuf,
    width: usize,
    height: usize,
    fps: f64,
    frames: usize,
    /// With ffmpeg: the running encoder, and the silent video it writes.
    encoder: Option<(Child, ChildStdin, PathBuf)>,
    /// Without: where the PNG frames go.
    frames_folder: Option<PathBuf>,
    bytes: Vec<u8>,
}

impl VideoWriter {
    /// Starts a video of `width` x `height` frames at `fps`, to be saved as
    /// `path`. Working files go in `folder` (the run's output folder).
    pub(crate) fn start(
        path: &Path,
        width: usize,
        height: usize,
        fps: f64,
        folder: &Path,
    ) -> Result<VideoWriter, String> {
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            fs::create_dir_all(parent)
                .map_err(|e| format!("could not create the folder {}: {e}", parent.display()))?;
        }
        let silent = folder.join("video-silent.mp4");
        let spawned = Command::new(ffmpeg())
            .args([
                "-y",
                "-loglevel",
                "error",
                "-f",
                "rawvideo",
                "-pix_fmt",
                "rgb24",
            ])
            .args([
                "-s",
                &format!("{width}x{height}"),
                "-r",
                &fps.to_string(),
                "-i",
                "-",
            ])
            .args(["-vf", &filter(width, height)])
            .args(["-c:v", "libx264", "-preset", "veryfast", "-crf", "16"])
            .args(["-pix_fmt", "yuv420p"])
            .arg(&silent)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn();
        let (encoder, frames_folder) = match spawned {
            Ok(mut child) => {
                let stdin = child.stdin.take().ok_or("could not talk to ffmpeg")?;
                (Some((child, stdin, silent)), None)
            }
            Err(_) => {
                // No ffmpeg: keep the frames as pictures instead.
                let frames = folder.join("video-frames");
                let _ = fs::remove_dir_all(&frames);
                fs::create_dir_all(&frames)
                    .map_err(|e| format!("could not create {}: {e}", frames.display()))?;
                (None, Some(frames))
            }
        };
        Ok(VideoWriter {
            path: path.to_path_buf(),
            width,
            height,
            fps,
            frames: 0,
            encoder,
            frames_folder,
            bytes: Vec::with_capacity(width * height * 3),
        })
    }

    /// Adds one frame.
    pub(crate) fn add(&mut self, picture: &Canvas) -> Result<(), String> {
        self.frames += 1;
        if let Some((_, stdin, _)) = &mut self.encoder {
            self.bytes.clear();
            for &pixel in picture.pixels() {
                self.bytes.extend_from_slice(&[
                    (pixel >> 16) as u8,
                    (pixel >> 8) as u8,
                    pixel as u8,
                ]);
            }
            stdin
                .write_all(&self.bytes)
                .map_err(|e| format!("ffmpeg stopped taking frames ({e}); is it working?"))?;
        } else if let Some(folder) = &self.frames_folder {
            let path = folder.join(format!("{:06}.png", self.frames));
            picture
                .save_png(&path)
                .map_err(|e| format!("could not save {}: {e}", path.display()))?;
        }
        Ok(())
    }

    /// Finishes the video, with `audio` (a WAV) if there's any sound, and
    /// says what it made, in one line for the summary.
    pub(crate) fn finish(self, audio: Option<&Path>) -> Result<String, String> {
        let (scale, video_w, video_h) = layout(self.width, self.height);
        let seconds = self.frames as f64 / self.fps;
        let size = format!(
            "{} x {} enlarged {scale} times in {video_w} x {video_h}",
            self.width, self.height
        );
        match (self.encoder, self.frames_folder) {
            (Some((child, stdin, silent)), _) => {
                drop(stdin); // the end of the frames: ffmpeg finishes the file
                let output = child
                    .wait_with_output()
                    .map_err(|e| format!("ffmpeg didn't finish: {e}"))?;
                if !output.status.success() {
                    return Err(format!(
                        "ffmpeg couldn't make the video: {}",
                        String::from_utf8_lossy(&output.stderr).trim()
                    ));
                }
                match audio {
                    Some(wav) => {
                        let joined = Command::new(ffmpeg())
                            .args(["-y", "-loglevel", "error", "-i"])
                            .arg(&silent)
                            .arg("-i")
                            .arg(wav)
                            .args(["-c:v", "copy", "-c:a", "aac", "-b:a", "192k", "-shortest"])
                            .arg(&self.path)
                            .output()
                            .map_err(|e| format!("ffmpeg didn't start: {e}"))?;
                        if !joined.status.success() {
                            return Err(format!(
                                "ffmpeg couldn't add the sound: {}",
                                String::from_utf8_lossy(&joined.stderr).trim()
                            ));
                        }
                        let _ = fs::remove_file(&silent);
                    }
                    None => {
                        fs::copy(&silent, &self.path)
                            .map_err(|e| format!("could not save {}: {e}", self.path.display()))?;
                        let _ = fs::remove_file(&silent);
                    }
                }
                let sound = if audio.is_some() {
                    "with sound"
                } else {
                    "no sound"
                };
                Ok(format!(
                    "video: {} ({} frames, {seconds:.1} s at {} fps, {size}, {sound})",
                    self.path.display(),
                    self.frames,
                    self.fps
                ))
            }
            (None, Some(folder)) => {
                let audio_part = audio
                    .map(|wav| format!(" -i {}", wav.display()))
                    .unwrap_or_default();
                let audio_codec = if audio.is_some() {
                    " -c:a aac -shortest"
                } else {
                    ""
                };
                Ok(format!(
                    "video: ffmpeg wasn't found, so the {} frames are in {} ({size} when joined); \
                     make the video with: ffmpeg -y -framerate {} -i {}{audio_part} -vf \"{}\" \
                     -c:v libx264 -pix_fmt yuv420p{audio_codec} {}",
                    self.frames,
                    folder.display(),
                    self.fps,
                    folder.join("%06d.png").display(),
                    filter(self.width, self.height),
                    self.path.display()
                ))
            }
            (None, None) => Err(String::from("the video had nowhere to go")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn games_are_enlarged_by_whole_numbers_into_1080p() {
        assert_eq!(layout(320, 240), (4, 1920, 1080), "1280 x 960, centred");
        assert_eq!(layout(240, 320), (3, 1920, 1080), "720 x 960");
        assert_eq!(layout(480, 270), (4, 1920, 1080), "fills it exactly");
        assert_eq!(layout(1920, 1080), (1, 1920, 1080));
        // Bigger than 1080p: as it is, rounded up to even sizes.
        assert_eq!(layout(2001, 1201), (1, 2002, 1202));
        assert_eq!(
            filter(320, 240),
            "scale=1280:960:flags=neighbor,pad=1920:1080:(ow-iw)/2:(oh-ih)/2:black"
        );
    }
}
