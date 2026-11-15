//! Reviewing a video made anywhere: ffmpeg reads its frames (already
//! shrunk to the size they're measured at) and its sound.

use super::frames::{ANALYSIS_SIDE, Recorder, Small};
use super::{Review, Run};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// What ffmpeg says about a video: its size, frame rate and length.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Facts {
    pub(crate) width: usize,
    pub(crate) height: usize,
    pub(crate) fps: f64,
    pub(crate) seconds: f64,
    pub(crate) sound: bool,
}

/// Reads ffmpeg's description of a file (what `ffmpeg -i` prints).
pub(crate) fn facts_from(text: &str) -> Option<Facts> {
    let video = text
        .lines()
        .find(|l| l.contains("Stream") && l.contains("Video:"))?;
    let size = video.split([' ', ',']).find_map(|word| {
        let (w, h) = word.split_once('x')?;
        Some((w.parse::<usize>().ok()?, h.parse::<usize>().ok()?))
    })?;
    let fps = video
        .split(',')
        .find_map(|part| part.trim().strip_suffix(" fps")?.trim().parse::<f64>().ok())
        .or_else(|| {
            video
                .split(',')
                .find_map(|part| part.trim().strip_suffix(" tbr")?.trim().parse::<f64>().ok())
        })
        .unwrap_or(30.0);
    let seconds = text
        .lines()
        .find_map(|l| {
            let rest = l.trim().strip_prefix("Duration: ")?;
            let clock = rest.split(',').next()?;
            let parts: Vec<f64> = clock.split(':').filter_map(|p| p.parse().ok()).collect();
            (parts.len() == 3).then(|| parts[0] * 3600.0 + parts[1] * 60.0 + parts[2])
        })
        .unwrap_or(0.0);
    let sound = text
        .lines()
        .any(|l| l.contains("Stream") && l.contains("Audio:"));
    Some(Facts {
        width: size.0,
        height: size.1,
        fps,
        seconds,
        sound,
    })
}

/// The size frames are read at: the video's, shrunk to fit the analysis
/// size, in even numbers.
pub(crate) fn analysis_size(width: usize, height: usize) -> (usize, usize) {
    let factor = (width.max(height) as f64 / ANALYSIS_SIDE as f64).max(1.0);
    let even = |v: f64| ((v / 2.0).round() as usize * 2).max(2);
    (even(width as f64 / factor), even(height as f64 / factor))
}

pub(crate) fn review(
    path: &Path,
    out: Option<&Path>,
    profiles: Option<&Path>,
) -> Result<Review, String> {
    let ffmpeg = crate::video::ffmpeg();
    if !path.is_file() {
        return Err(format!("there's no video at {}", path.display()));
    }
    let probe = Command::new(&ffmpeg)
        .arg("-hide_banner")
        .arg("-i")
        .arg(path)
        .output()
        .map_err(|e| format!("reviewing a video needs ffmpeg, and it couldn't start ({e})"))?;
    let said = String::from_utf8_lossy(&probe.stderr);
    let facts =
        facts_from(&said).ok_or_else(|| format!("ffmpeg found no video in {}", path.display()))?;
    let (aw, ah) = analysis_size(facts.width, facts.height);
    let expected = (facts.seconds * facts.fps).ceil() as u64;
    let mut rec = Recorder::new(aw, ah, expected);
    // The frames, shrunk by ffmpeg, one after another.
    let mut child = Command::new(&ffmpeg)
        .args(["-v", "error", "-i"])
        .arg(path)
        .args([
            "-vf",
            &format!("scale={aw}:{ah}:flags=area"),
            "-f",
            "rawvideo",
            "-pix_fmt",
            "rgb24",
            "-",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("could not start ffmpeg: {e}"))?;
    let mut stdout = child.stdout.take().ok_or("ffmpeg gave no frames")?;
    let mut bytes = vec![0u8; aw * ah * 3];
    let mut frame = 0;
    while stdout.read_exact(&mut bytes).is_ok() {
        frame += 1;
        let pixels = bytes
            .chunks_exact(3)
            .map(|p| (u32::from(p[0]) << 16) | (u32::from(p[1]) << 8) | u32::from(p[2]))
            .collect();
        rec.add_small(
            frame,
            Small {
                width: aw,
                height: ah,
                pixels,
            },
            None,
            None,
        );
    }
    let _ = child.wait();
    if frame == 0 {
        return Err(format!(
            "ffmpeg could not read any frames from {}",
            path.display()
        ));
    }
    // The sound, mono, as floats.
    let audio = if facts.sound {
        let sound = Command::new(&ffmpeg)
            .args(["-v", "error", "-i"])
            .arg(path)
            .args(["-vn", "-ac", "1", "-ar", "44100", "-f", "f32le", "-"])
            .output()
            .map_err(|e| format!("could not start ffmpeg: {e}"))?;
        let mono: Vec<f32> = sound
            .stdout
            .chunks_exact(4)
            .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
            .collect();
        let peak = mono.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        Some(super::audio::Audio {
            rate: 44_100,
            mono,
            peak,
            peaks: Vec::new(),
        })
    } else {
        None
    };
    let title = path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();
    let run = Run {
        title,
        ups: facts.fps,
        audio,
        source: path.display().to_string(),
        profiles_folder: profiles.map(Path::to_path_buf),
        ..Run::default()
    };
    let folder: PathBuf = match out {
        Some(out) => out.to_path_buf(),
        None => {
            let stem = path
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            path.with_file_name(format!("{stem}-review"))
        }
    };
    super::finish(&rec, &run, &folder, (facts.width, facts.height))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ffmpegs_description_is_read() {
        let said = "Input #0, mov,mp4,m4a,3gp,3g2,mj2, from 'a.mp4':\n  Duration: 00:01:02.50, start: 0.000000, bitrate: 900 kb/s\n  \
                    Stream #0:0[0x1](und): Video: h264 (High) (avc1 / 0x31637661), yuv420p(tv, progressive), 1080x1920 [SAR 1:1 DAR 9:16], \
                    800 kb/s, 30 fps, 30 tbr, 15360 tbn (default)\n  Stream #0:1[0x2](und): Audio: aac (LC), 44100 Hz, stereo, fltp\n";
        let facts = facts_from(said).unwrap();
        assert_eq!((facts.width, facts.height), (1080, 1920));
        assert_eq!(facts.fps, 30.0);
        assert!((facts.seconds - 62.5).abs() < 1e-9);
        assert!(facts.sound);
        assert_eq!(analysis_size(1080, 1920), (270, 480));
        assert_eq!(analysis_size(320, 180), (320, 180));
    }
}
