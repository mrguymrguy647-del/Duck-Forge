//! The sound, measured: how loud it is over time, where it's silent, its
//! spectrum, and when each sound becomes audible.

/// A run's sound, as the review hears it.
#[derive(Debug, Clone, Default)]
pub(crate) struct Audio {
    pub(crate) rate: u32,
    /// Mono, after the limiter: what a viewer hears.
    pub(crate) mono: Vec<f32>,
    /// The loudest the mix was before the limiter (1.0 is full volume).
    pub(crate) peak: f32,
    /// Each moment the mix went over full volume, and what was sounding.
    pub(crate) peaks: Vec<Peak>,
}

/// A moment the mix went over full volume.
#[derive(Debug, Clone)]
pub(crate) struct Peak {
    pub(crate) seconds: f32,
    pub(crate) level: f32,
    /// What was sounding, loudest first: name, loop?, volume, how loud there.
    pub(crate) sounds: Vec<(String, bool, f32, f32)>,
    /// Volumes that would keep it under: name, loop?, volume now, suggested.
    pub(crate) suggestion: Vec<(String, bool, f32, f32)>,
}

/// One sound started by the game.
#[derive(Debug, Clone)]
pub(crate) struct SoundStart {
    pub(crate) frame: u64,
    pub(crate) name: String,
    pub(crate) event: Option<String>,
    pub(crate) volume: f32,
    /// Seconds of near-silence at its start, before it's heard.
    pub(crate) lead: f32,
}

/// Seconds of near-silence (below -40 dB) at the start of `samples`
/// (`rate` per second, as (left, right) frames via `frame`).
pub(crate) fn lead_silence(frames: usize, frame: impl Fn(usize) -> (f32, f32), rate: u32) -> f32 {
    let quiet = 0.01;
    let first = (0..frames).find(|&i| {
        let (l, r) = frame(i);
        l.abs() > quiet || r.abs() > quiet
    });
    first.unwrap_or(frames) as f32 / rate.max(1) as f32
}

/// Loudness every tenth of a second, in dB below full volume (RMS).
pub(crate) fn loudness(mono: &[f32], rate: u32) -> Vec<f32> {
    let window = (rate as usize / 10).max(1);
    mono.chunks(window)
        .map(|chunk| {
            let power = chunk.iter().map(|s| s * s).sum::<f32>() / chunk.len() as f32;
            (10.0 * power.max(1e-12).log10()).max(-90.0)
        })
        .collect()
}

/// Stretches quieter than `below` dB for at least `seconds`: (start, end)
/// in seconds, from a loudness curve of tenths of a second.
pub(crate) fn silences(curve: &[f32], below: f32, seconds: f32) -> Vec<(f32, f32)> {
    let mut out = Vec::new();
    let mut start = None;
    for (i, &db) in curve.iter().chain(std::iter::once(&0.0)).enumerate() {
        match (db < below, start) {
            (true, None) => start = Some(i),
            (false, Some(s)) => {
                if (i - s) as f32 / 10.0 >= seconds {
                    out.push((s as f32 / 10.0, i as f32 / 10.0));
                }
                start = None;
            }
            _ => {}
        }
    }
    out
}

/// A spectrogram: `columns` moments spread over the sound, each `rows`
/// bands from 40 Hz to 16 kHz on a log scale, in dB (about -100 to 0).
pub(crate) fn spectrogram(mono: &[f32], rate: u32, columns: usize, rows: usize) -> Vec<Vec<f32>> {
    const SIZE: usize = 2048;
    if mono.is_empty() || columns == 0 || rows == 0 {
        return Vec::new();
    }
    let hann: Vec<f32> = (0..SIZE)
        .map(|i| 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / SIZE as f32).cos())
        .collect();
    let bin_hz = rate as f32 / SIZE as f32;
    let (low, high) = (40f32, 16_000f32.min(rate as f32 / 2.0));
    // Which FFT bins each row covers.
    let bands: Vec<(usize, usize)> = (0..rows)
        .map(|r| {
            let f0 = low * (high / low).powf(r as f32 / rows as f32);
            let f1 = low * (high / low).powf((r + 1) as f32 / rows as f32);
            let b0 = ((f0 / bin_hz) as usize).clamp(1, SIZE / 2 - 1);
            let b1 = ((f1 / bin_hz).ceil() as usize).clamp(b0 + 1, SIZE / 2);
            (b0, b1)
        })
        .collect();
    let mut re = vec![0.0f32; SIZE];
    let mut im = vec![0.0f32; SIZE];
    (0..columns)
        .map(|c| {
            let center = (c as f64 + 0.5) / columns as f64 * mono.len() as f64;
            let start = center as i64 - SIZE as i64 / 2;
            for i in 0..SIZE {
                let at = start + i as i64;
                re[i] = if at >= 0 && (at as usize) < mono.len() {
                    mono[at as usize] * hann[i]
                } else {
                    0.0
                };
                im[i] = 0.0;
            }
            fft(&mut re, &mut im);
            bands
                .iter()
                .map(|&(b0, b1)| {
                    let power = (b0..b1)
                        .map(|b| re[b] * re[b] + im[b] * im[b])
                        .fold(0.0f32, f32::max);
                    let amplitude = power.sqrt() / (SIZE as f32 / 4.0);
                    (20.0 * amplitude.max(1e-6).log10()).clamp(-100.0, 0.0)
                })
                .collect()
        })
        .collect()
}

/// A radix-2 FFT in place (`re.len()` a power of two).
fn fft(re: &mut [f32], im: &mut [f32]) {
    let n = re.len();
    let mut j = 0;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j |= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let mut len = 2;
    while len <= n {
        let angle = -std::f32::consts::TAU / len as f32;
        let (wr, wi) = (angle.cos(), angle.sin());
        for start in (0..n).step_by(len) {
            let (mut cr, mut ci) = (1.0f32, 0.0f32);
            for k in 0..len / 2 {
                let (a, b) = (start + k, start + k + len / 2);
                let tr = re[b] * cr - im[b] * ci;
                let ti = re[b] * ci + im[b] * cr;
                re[b] = re[a] - tr;
                im[b] = im[a] - ti;
                re[a] += tr;
                im[a] += ti;
                let next = cr * wr - ci * wi;
                ci = cr * wi + ci * wr;
                cr = next;
            }
        }
        len <<= 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tone_shows_in_its_band() {
        let rate = 44_100;
        let tone: Vec<f32> = (0..rate)
            .map(|i| (std::f32::consts::TAU * 1000.0 * i as f32 / rate as f32).sin() * 0.5)
            .collect();
        let picture = spectrogram(&tone, rate, 4, 64);
        let column = &picture[1];
        let loudest = column
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.total_cmp(b.1))
            .unwrap()
            .0;
        // 1 kHz on a log scale from 40 Hz to 16 kHz: about 54% of the way up.
        let expected = ((1000f32 / 40.0).ln() / (16000f32 / 40.0).ln() * 64.0) as usize;
        assert!(loudest.abs_diff(expected) <= 1, "{loudest} vs {expected}");
        assert!(column[loudest] > -12.0, "{}", column[loudest]);
    }

    #[test]
    fn loudness_and_silences() {
        let rate = 1000;
        let mut mono = vec![0.5f32; 1000];
        mono.extend(vec![0.0; 4000]);
        mono.extend(vec![0.5; 1000]);
        let curve = loudness(&mono, rate);
        assert_eq!(curve.len(), 60);
        assert!((curve[0] + 6.02).abs() < 0.1);
        assert_eq!(silences(&curve, -50.0, 3.0), vec![(1.0, 5.0)]);
        assert!(silences(&curve, -50.0, 4.5).is_empty());
        let lead = lead_silence(5, |i| if i < 3 { (0.0, 0.0) } else { (0.5, 0.5) }, 10);
        assert!((lead - 0.3).abs() < 1e-6);
    }
}
