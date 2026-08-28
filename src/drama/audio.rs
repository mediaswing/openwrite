//! Making a line of dialogue sound like the person who says it.
//!
//! What comes back from ElevenLabs is one voice reading one line evenly. What
//! a radio play needs is a twelve-year-old, frightened, standing stage left.
//! This module is the difference: it works on raw samples, so everything it
//! does is arithmetic on this machine with nothing sent anywhere and nothing
//! installed.
//!
//! # The three things it does
//!
//! **Age.** `age="12"` on a line lifts the pitch, because a child does not
//! sound like the adult whose voice was hired to play them. Pitch is moved
//! without moving the duration — the line still takes as long to say — by
//! stretching the waveform in time and then resampling it back, which is the
//! [WSOLA] method and is [`stretch`] and [`resample`] below.
//!
//! **State.** `state="scared"` is a tremble: a slow wobble in pitch and one in
//! loudness, which is what a frightened voice actually does. `whisper` thins
//! the voice, drops it, and lays breath over it shaped by the words. The rest
//! are in [`Treatment::for_line`], and every one of them is a few numbers
//! rather than a black box, so a state that sounds wrong can be argued with.
//!
//! **Position.** `pos="left"` is a constant-power pan, which keeps a voice the
//! same loudness wherever it stands — the thing a plain left/right fade gets
//! wrong. It is not panned all the way: a voice hard against one ear sounds
//! like a fault rather than like a room, so the far ear keeps a little of it.
//!
//! Finally [`stitch`] puts the pieces in order with a pause between them, a
//! shorter one when the same person carries on speaking, and a few
//! milliseconds of fade on each end so that no join clicks.
//!
//! [WSOLA]: https://en.wikipedia.org/wiki/Audio_time_stretching_and_pitch_scaling

use super::story::{Line, Pos, State};

/// The rate everything is asked for, worked in, and written at.
///
/// 24 kHz because it is the best PCM rate ElevenLabs will give an ordinary
/// account, and PCM because the alternative is MP3 and none of the work in
/// this file can be done to an MP3 without decoding it first.
pub const SAMPLE_RATE: u32 = 24_000;

/// Silence between one line and the next.
const GAP_MS: f32 = 380.0;
/// The shorter one, when the same person is still speaking.
const SAME_SPEAKER_GAP_MS: f32 = 240.0;
/// Fade on each end of a piece, so that a join is a join rather than a click.
const EDGE_FADE_MS: f32 = 8.0;

/// How far a voice is pushed towards one ear. Not 1.0 on purpose: see the
/// module note.
const PAN_WIDTH: f32 = 0.8;

/// What each line is normalised to before its own gain is applied, so that two
/// voices recorded at different levels sit together.
const TARGET_PEAK: f32 = 0.7;

/// The most a finished line may be.
///
/// A shout is normalised, driven and then made louder, and the three together
/// can carry it past full scale. Held here rather than left for the limiter in
/// [`stitch`], because that one is a limiter on the whole play: one line over
/// the top would quieten every other line in the recording to make room for
/// it. A ceiling on each line keeps the states in proportion — a whisper is
/// still far quieter than a shout — without letting any one of them decide how
/// loud the play is.
const CEILING: f32 = 0.95;

/// A block of mono samples.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Mono {
    pub samples: Vec<f32>,
}

impl Mono {
    pub fn len(&self) -> usize {
        self.samples.len()
    }

    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    /// How long it lasts.
    pub fn seconds(&self) -> f32 {
        self.samples.len() as f32 / SAMPLE_RATE as f32
    }

    /// Read what ElevenLabs sends for `output_format=pcm_24000`: signed
    /// 16-bit, little-endian, one channel.
    ///
    /// A trailing odd byte is a truncated reply rather than a sample; it is
    /// dropped rather than read as half of one.
    pub fn from_pcm16(bytes: &[u8]) -> Mono {
        let samples = bytes
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| i16::from_le_bytes(*pair) as f32 / 32_768.0)
            .collect();
        Mono { samples }
    }

    fn peak(&self) -> f32 {
        self.samples.iter().fold(0.0f32, |m, s| m.max(s.abs()))
    }

    /// Bring the loudest moment to a known level, so that the treatments below
    /// start from the same place for every voice.
    fn normalise(&mut self) {
        let peak = self.peak();
        if peak > 1e-6 {
            self.scale(TARGET_PEAK / peak);
        }
    }

    fn scale(&mut self, factor: f32) {
        for sample in &mut self.samples {
            *sample *= factor;
        }
    }

    /// Bring the loudest moment down to `ceiling` if it is above it. Down
    /// only: a line that is quiet was meant to be.
    fn limit(&mut self, ceiling: f32) {
        let peak = self.peak();
        if peak > ceiling {
            self.scale(ceiling / peak);
        }
    }

    /// Fade the first and last few milliseconds in and out.
    fn fade_edges(&mut self) {
        let n = ((EDGE_FADE_MS / 1000.0) * SAMPLE_RATE as f32) as usize;
        let n = n.min(self.samples.len() / 2);
        let total = self.samples.len();
        for i in 0..n {
            let ramp = i as f32 / n as f32;
            self.samples[i] *= ramp;
            self.samples[total - 1 - i] *= ramp;
        }
    }
}

// -- the treatments -----------------------------------------------------------

/// Everything to be done to one line, worked out before any of it is done.
///
/// Kept as plain numbers rather than as a list of steps so that the window can
/// show what is about to happen — "+4.0 semitones, trembling, left" — before
/// anybody spends an API call finding out.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Treatment {
    /// How far to move the pitch, in semitones, without changing the length.
    pub semitones: f32,
    /// How much faster or slower to say it. 1.0 is as recorded.
    pub tempo: f32,
    /// Overall level, as a plain multiplier.
    pub gain: f32,
    /// Depth of the loudness wobble, 0 to 1.
    pub tremolo: f32,
    /// Depth of the pitch wobble, in cents.
    pub wobble: f32,
    /// How much breath to lay over the words, 0 to 1.
    pub breath: f32,
    /// How hard the voice is pushed into distortion, 0 to 1.
    pub drive: f32,
    pub pos: Pos,
}

impl Default for Treatment {
    fn default() -> Treatment {
        Treatment {
            semitones: 0.0,
            tempo: 1.0,
            gain: 1.0,
            tremolo: 0.0,
            wobble: 0.0,
            breath: 0.0,
            drive: 0.0,
            pos: Pos::Centre,
        }
    }
}

impl Treatment {
    /// What a line asks for: its age, its state and its position together.
    ///
    /// `age_strength` scales the age shift alone. It is there because a writer
    /// who has already cast a child's voice for a child does not want the
    /// twelve on the line applied a second time on top of it.
    pub fn for_line(line: &Line, age_strength: f32) -> Treatment {
        let (semitones, tempo) = match line.age {
            Some(age) => age_shift(age as f32),
            None => (0.0, 1.0),
        };
        let mut treatment = Treatment {
            semitones: semitones * age_strength,
            tempo,
            pos: line.pos,
            ..Treatment::default()
        };

        match line.state {
            State::Normal => {}
            State::Whisper => {
                treatment.gain *= 0.42;
                treatment.breath = 0.55;
                treatment.tempo *= 0.97;
            }
            // A frightened voice is not a quiet one or a loud one — it is an
            // unsteady one. The tremble is the whole of the effect.
            State::Scared => {
                treatment.semitones += 0.9;
                treatment.tempo *= 1.05;
                treatment.tremolo = 0.24;
                treatment.wobble = 38.0;
                treatment.breath = 0.18;
            }
            State::Shout => {
                treatment.semitones += 0.6;
                treatment.gain *= 1.25;
                treatment.drive = 0.45;
            }
            State::Angry => {
                treatment.semitones += 0.3;
                treatment.gain *= 1.1;
                treatment.drive = 0.3;
                treatment.tempo *= 1.03;
            }
            State::Sad => {
                treatment.semitones -= 0.9;
                treatment.tempo *= 0.93;
                treatment.gain *= 0.85;
            }
            State::Excited => {
                treatment.semitones += 0.7;
                treatment.tempo *= 1.07;
                treatment.gain *= 1.05;
            }
            State::Tired => {
                treatment.semitones -= 0.6;
                treatment.tempo *= 0.9;
                treatment.gain *= 0.8;
                treatment.breath = 0.2;
            }
        }
        treatment
    }

    /// Is there anything to do at all?
    pub fn is_plain(&self) -> bool {
        self.semitones.abs() < 0.01
            && (self.tempo - 1.0).abs() < 0.005
            && (self.gain - 1.0).abs() < 0.01
            && self.tremolo == 0.0
            && self.wobble == 0.0
            && self.breath == 0.0
            && self.drive == 0.0
    }

    /// Do it, and hand back the two channels.
    ///
    /// `seed` makes the wobble and the breath repeatable: the same line
    /// treated twice comes out the same both times, which is what makes it
    /// possible to regenerate one line of a finished play without the rest of
    /// it shifting underneath.
    pub fn apply(&self, input: &Mono, seed: u64) -> (Vec<f32>, Vec<f32>) {
        let mut voice = input.clone();
        voice.normalise();

        if (self.tempo - 1.0).abs() > 0.005 {
            voice.samples = stretch(&voice.samples, 1.0 / self.tempo as f64);
        }
        if self.semitones.abs() > 0.01 {
            voice.samples = pitch(&voice.samples, self.semitones);
        }
        if self.wobble > 0.0 {
            voice.samples = wobble(&voice.samples, self.wobble);
        }
        if self.tremolo > 0.0 {
            tremolo(&mut voice.samples, self.tremolo);
        }
        if self.breath > 0.0 {
            breathe(&mut voice.samples, self.breath, seed);
        }
        if self.drive > 0.0 {
            drive(&mut voice.samples, self.drive);
        }
        voice.scale(self.gain);
        voice.limit(CEILING);
        voice.fade_edges();

        pan(&voice.samples, self.pos)
    }
}

/// How much to move a voice for an age, and how much to change its pace.
///
/// A curve rather than a formula because the thing being modelled is not
/// linear: nothing much happens between twenty-five and forty-five, and a
/// great deal happens between eight and sixteen. The numbers are semitones of
/// lift over an adult voice, which is roughly where a child's speaking pitch
/// actually sits — half an octave above a man's at ten years old, converging
/// through the teens.
const AGE_CURVE: [(f32, f32, f32); 10] = [
    // age, semitones, tempo
    (4.0, 8.0, 1.06),
    (8.0, 6.0, 1.04),
    (12.0, 4.0, 1.02),
    (15.0, 2.2, 1.01),
    (18.0, 0.8, 1.0),
    (25.0, 0.0, 1.0),
    (45.0, 0.0, 1.0),
    (60.0, -0.5, 0.99),
    (75.0, -1.1, 0.96),
    (90.0, -1.8, 0.93),
];

/// Read the curve, straight-lining between its points and flat outside them.
pub fn age_shift(age: f32) -> (f32, f32) {
    let first = AGE_CURVE[0];
    if age <= first.0 {
        return (first.1, first.2);
    }
    for pair in AGE_CURVE.windows(2) {
        let (low, high) = (pair[0], pair[1]);
        if age <= high.0 {
            let t = (age - low.0) / (high.0 - low.0);
            return (
                low.1 + (high.1 - low.1) * t,
                low.2 + (high.2 - low.2) * t,
            );
        }
    }
    let last = AGE_CURVE[AGE_CURVE.len() - 1];
    (last.1, last.2)
}

// -- the arithmetic -----------------------------------------------------------

/// Move the pitch without moving the duration.
///
/// Stretch by the ratio, then resample by its inverse: the two length changes
/// cancel and the pitch change does not.
pub fn pitch(input: &[f32], semitones: f32) -> Vec<f32> {
    let ratio = 2f32.powf(semitones / 12.0) as f64;
    let stretched = stretch(input, ratio);
    resample(&stretched, ratio)
}

/// Read a signal back at a different rate, which changes its pitch and its
/// length together.
///
/// `ratio` above 1 makes it shorter and higher. Catmull-Rom between samples
/// rather than a straight line, which costs four multiplies and audibly
/// removes the grain that linear interpolation leaves on speech.
pub fn resample(input: &[f32], ratio: f64) -> Vec<f32> {
    if input.is_empty() || (ratio - 1.0).abs() < 1e-9 {
        return input.to_vec();
    }
    let length = ((input.len() as f64) / ratio).floor() as usize;
    let mut out = Vec::with_capacity(length);
    for i in 0..length {
        out.push(sample_at(input, i as f64 * ratio));
    }
    out
}

/// One sample from a fractional position.
fn sample_at(input: &[f32], at: f64) -> f32 {
    if input.is_empty() {
        return 0.0;
    }
    let whole = at.floor();
    let t = (at - whole) as f32;
    let index = whole as isize;
    let get = |i: isize| -> f32 {
        let i = i.clamp(0, input.len() as isize - 1) as usize;
        input[i]
    };
    let (p0, p1, p2, p3) = (get(index - 1), get(index), get(index + 1), get(index + 2));
    // Catmull-Rom.
    let a = -0.5 * p0 + 1.5 * p1 - 1.5 * p2 + 0.5 * p3;
    let b = p0 - 2.5 * p1 + 2.0 * p2 - 0.5 * p3;
    let c = -0.5 * p0 + 0.5 * p2;
    ((a * t + b) * t + c) * t + p1
}

/// How long a stretching frame is. Long enough to hold a pitch period of any
/// speaking voice, short enough that a consonant is not smeared across it.
const FRAME: usize = 720; // 30 ms at 24 kHz
/// How far the search may move a frame to find the alignment that does not
/// cancel the one before it. A little over one pitch period of the lowest
/// speaking voice, which is what it takes to find the alignment at all.
const SEARCH: usize = 200; // 8 ms

/// Make a signal longer or shorter without changing its pitch.
///
/// Waveform-similarity overlap-add: cut the input into frames, and before
/// laying each one down, slide it a few milliseconds either way to wherever it
/// best continues what is already there. Sliding is the whole trick — an
/// overlap-add that does not look first cancels its own waveform at every join
/// and turns a voice into a whisper of itself.
///
/// Where the frame is *taken from* is counted separately from where the search
/// moved it to, and that separation is not a detail. A voice is periodic, so
/// the best-matching frame is usually a whole pitch period away from the one
/// asked for; measuring the next frame from where the last one was found lets
/// that period be added again every frame, and the drift silently eats the
/// stretch. Asked to make a line a quarter longer, it hands back a line
/// exactly as long as it started. So `nominal` walks by the analysis hop and
/// nothing else, and the search moves each frame around it without moving it.
///
/// `factor` above 1 makes it longer.
pub fn stretch(input: &[f32], factor: f64) -> Vec<f32> {
    if input.is_empty() || (factor - 1.0).abs() < 1e-6 {
        return input.to_vec();
    }
    // Too short to hold even one frame: fall back to resampling, which changes
    // the pitch, but a syllable this short has none worth preserving.
    if input.len() < FRAME * 2 {
        return resample(input, 1.0 / factor);
    }

    let hop_out = FRAME / 2;
    // Kept fractional and accumulated, so that a hop of 352.9 samples is
    // 352.9 samples over a hundred frames rather than 353.
    let hop_in = (hop_out as f64) / factor;
    let window = hann(FRAME);

    let expected = ((input.len() as f64) * factor) as usize + FRAME;
    let mut out = vec![0.0f32; expected];
    let mut weight = vec![0.0f32; expected];

    let mut nominal = 0.0f64;
    let mut write = 0usize;
    // What the previous frame's tail says should come next.
    let mut template: Vec<f32> = input[hop_out..hop_out * 2].to_vec();
    let mut first = true;

    loop {
        let read = nominal.round() as usize;
        if read + FRAME >= input.len() || write + FRAME >= out.len() {
            break;
        }
        // The first frame has nothing to continue, so it is taken as it lies.
        let start = if first { read } else { best_offset(input, read, &template) };
        first = false;

        for i in 0..FRAME {
            out[write + i] += input[start + i] * window[i];
            weight[write + i] += window[i];
        }

        let tail = start + hop_out;
        if tail + hop_out >= input.len() {
            write += hop_out;
            break;
        }
        template.copy_from_slice(&input[tail..tail + hop_out]);
        write += hop_out;
        nominal += hop_in;
    }

    let end = (write + FRAME).min(out.len());
    out.truncate(end);
    weight.truncate(end);
    for (sample, w) in out.iter_mut().zip(weight) {
        if w > 1e-4 {
            *sample /= w;
        }
    }
    out
}

/// Where near `read` the next frame best continues `template`.
fn best_offset(input: &[f32], read: usize, template: &[f32]) -> usize {
    let low = read.saturating_sub(SEARCH);
    let high = (read + SEARCH).min(input.len().saturating_sub(FRAME + 1));
    if high <= low {
        return low.min(input.len().saturating_sub(FRAME + 1));
    }
    let mut best = low;
    let mut best_score = f32::NEG_INFINITY;
    for candidate in low..=high {
        let mut dot = 0.0f32;
        let mut energy = 1e-6f32;
        for (i, wanted) in template.iter().enumerate() {
            let sample = input[candidate + i];
            dot += sample * wanted;
            energy += sample * sample;
        }
        // Normalised, so a loud frame is not preferred to a well-aligned one.
        let score = dot / energy.sqrt();
        if score > best_score {
            best_score = score;
            best = candidate;
        }
    }
    best
}

fn hann(n: usize) -> Vec<f32> {
    (0..n)
        .map(|i| {
            let t = i as f32 / n as f32;
            0.5 - 0.5 * (std::f32::consts::TAU * t).cos()
        })
        .collect()
}

/// The two rates the tremble is built from. Two rather than one, and not
/// harmonically related, because a single sine wave sounds like an effect
/// pedal and two sound like a person.
const WOBBLE_HZ: [f32; 2] = [5.4, 7.9];

/// An unsteady pitch: read the signal back through a read-head that drifts
/// backwards and forwards a fraction of a millisecond.
///
/// `cents` is the peak departure from the true pitch. The average rate stays
/// at one, so the line comes out the length it went in.
pub fn wobble(input: &[f32], cents: f32) -> Vec<f32> {
    if input.is_empty() || cents <= 0.0 {
        return input.to_vec();
    }
    let rate = SAMPLE_RATE as f32;
    // A delay of D·sin(ωt) bends the pitch by D·ω at its steepest, so the
    // depth needed for a given number of cents falls out of the frequency.
    let deviation = 2f32.powf(cents / 1200.0) - 1.0;
    let depths: Vec<f32> = WOBBLE_HZ
        .iter()
        .map(|hz| deviation / (std::f32::consts::TAU * hz) * rate)
        .collect();
    // Split between the two so that together they reach the depth asked for.
    let share = [0.62f32, 0.38];

    let mut out = Vec::with_capacity(input.len());
    for i in 0..input.len() {
        let t = i as f32 / rate;
        let mut offset = 0.0f32;
        for (which, hz) in WOBBLE_HZ.iter().enumerate() {
            offset += depths[which] * share[which] * (std::f32::consts::TAU * hz * t).sin();
        }
        out.push(sample_at(input, (i as f32 + offset).max(0.0) as f64));
    }
    out
}

/// An unsteady loudness, on the same two rates as the pitch tremble so that
/// the two move together the way a real one does.
pub fn tremolo(samples: &mut [f32], depth: f32) {
    let rate = SAMPLE_RATE as f32;
    for (i, sample) in samples.iter_mut().enumerate() {
        let t = i as f32 / rate;
        let mut m = 0.0;
        for (which, hz) in WOBBLE_HZ.iter().enumerate() {
            let share = if which == 0 { 0.7 } else { 0.3 };
            m += share * (std::f32::consts::TAU * hz * t).sin();
        }
        *sample *= 1.0 - depth * 0.5 * (1.0 - m);
    }
}

/// Thin the voice and lay breath over it.
///
/// The breath is noise shaped by the loudness of the words themselves rather
/// than a constant hiss, which is what makes it sound like breathing out while
/// speaking instead of like a bad microphone.
pub fn breathe(samples: &mut [f32], amount: f32, seed: u64) {
    let mut noise = Noise::new(seed);
    let mut envelope = 0.0f32;
    let mut previous = 0.0f32;
    let mut high = 0.0f32;
    let mut smoothed = 0.0f32;
    // A one-pole high pass at about 300 Hz, which is what takes the chest out
    // of a voice.
    let tilt = 0.93f32;

    for sample in samples.iter_mut() {
        let input = *sample;
        high = tilt * (high + input - previous);
        previous = input;

        let level = input.abs();
        // Quick to follow a syllable, slow to let go of it.
        let coefficient = if level > envelope { 0.02 } else { 0.0008 };
        envelope += (level - envelope) * coefficient;

        // Take the edge off the noise so it sits behind the voice.
        smoothed += (noise.next() - smoothed) * 0.55;

        let voice = input * (1.0 - amount * 0.55) + high * (amount * 0.75);
        *sample = voice + smoothed * envelope * amount * 1.6;
    }
}

/// Push the voice until it starts to break up, which is what shouting is.
pub fn drive(samples: &mut [f32], amount: f32) {
    let k = 1.0 + amount * 6.0;
    let normalise = k.tanh();
    for sample in samples.iter_mut() {
        *sample = (*sample * k).tanh() / normalise;
    }
}

/// Place a mono voice in the stereo picture at constant power.
pub fn pan(samples: &[f32], pos: Pos) -> (Vec<f32>, Vec<f32>) {
    let position = match pos {
        Pos::Left => -PAN_WIDTH,
        Pos::Centre => 0.0,
        Pos::Right => PAN_WIDTH,
    };
    // Half of a quarter turn either way: the sines square-sum to one, so the
    // voice keeps its loudness all the way across.
    let angle = (position + 1.0) * std::f32::consts::FRAC_PI_4;
    let (left_gain, right_gain) = (angle.cos(), angle.sin());
    (
        samples.iter().map(|s| s * left_gain).collect(),
        samples.iter().map(|s| s * right_gain).collect(),
    )
}

/// A repeatable white noise source. Xorshift, because the breath does not need
/// a good random number and does need the same one every time.
struct Noise(u64);

impl Noise {
    fn new(seed: u64) -> Noise {
        Noise(seed | 1)
    }

    fn next(&mut self) -> f32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        // The top 24 bits, mapped to −1..1.
        ((x >> 40) as f32 / 8_388_608.0) - 1.0
    }
}

// -- putting it together ------------------------------------------------------

/// One treated line, waiting to be laid down.
pub struct Piece {
    pub left: Vec<f32>,
    pub right: Vec<f32>,
    /// Who says it, so that two lines from the same person can sit closer
    /// together than two lines from different people.
    pub speaker: String,
}

impl Piece {
    pub fn seconds(&self) -> f32 {
        self.left.len() as f32 / SAMPLE_RATE as f32
    }
}

/// Lay the pieces end to end with a pause between them.
///
/// Returns interleaved stereo, ready for [`wav`]. The whole thing is brought
/// down if anything in it would clip — down, never up, so that a play mixed
/// quietly stays quiet rather than being dragged to the top by one loud line.
pub fn stitch(pieces: &[Piece]) -> Vec<f32> {
    let gap = |ms: f32| ((ms / 1000.0) * SAMPLE_RATE as f32) as usize;
    let mut out: Vec<f32> = Vec::new();
    let mut previous: Option<&str> = None;

    for piece in pieces {
        if let Some(previous) = previous {
            let ms = if previous == piece.speaker {
                SAME_SPEAKER_GAP_MS
            } else {
                GAP_MS
            };
            out.extend(std::iter::repeat_n(0.0, gap(ms) * 2));
        }
        for i in 0..piece.left.len().min(piece.right.len()) {
            out.push(piece.left[i]);
            out.push(piece.right[i]);
        }
        previous = Some(&piece.speaker);
    }

    let peak = out.iter().fold(0.0f32, |m, s| m.max(s.abs()));
    if peak > 0.99 {
        let correction = 0.99 / peak;
        for sample in &mut out {
            *sample *= correction;
        }
    }
    out
}

/// Write interleaved stereo as a `.wav` file.
///
/// Sixteen-bit PCM in a canonical 44-byte RIFF header — the format every
/// editor, every phone and every browser opens without being asked twice.
pub fn wav(interleaved: &[f32], rate: u32, channels: u16) -> Vec<u8> {
    let bits = 16u16;
    let block_align = channels * bits / 8;
    let byte_rate = rate * block_align as u32;
    let data_bytes = (interleaved.len() * 2) as u32;

    let mut out = Vec::with_capacity(44 + data_bytes as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_bytes).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM, uncompressed
    out.extend_from_slice(&channels.to_le_bytes());
    out.extend_from_slice(&rate.to_le_bytes());
    out.extend_from_slice(&byte_rate.to_le_bytes());
    out.extend_from_slice(&block_align.to_le_bytes());
    out.extend_from_slice(&bits.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_bytes.to_le_bytes());
    for sample in interleaved {
        // Clamped before conversion: a float over one would otherwise wrap
        // round to full-scale the other way, which is the loudest sound a
        // computer can make.
        let clamped = sample.clamp(-1.0, 1.0);
        let value = (clamped * 32_767.0).round() as i16;
        out.extend_from_slice(&value.to_le_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A crude voiced sound: a buzz with harmonics, loud and soft like
    /// syllables. Enough of a voice for the arithmetic to be measured on.
    fn buzz(f0: f32, seconds: f32) -> Vec<f32> {
        let n = (seconds * SAMPLE_RATE as f32) as usize;
        (0..n)
            .map(|i| {
                let t = i as f32 / SAMPLE_RATE as f32;
                let mut sample = 0.0;
                for harmonic in 1..=12 {
                    sample += (std::f32::consts::TAU * f0 * harmonic as f32 * t).sin()
                        / harmonic as f32;
                }
                let envelope = 0.5 + 0.5 * (std::f32::consts::TAU * 3.5 * t).sin();
                sample * envelope * 0.25
            })
            .collect()
    }

    /// The fundamental, by autocorrelation over the middle of the signal.
    fn fundamental(samples: &[f32]) -> f32 {
        let middle = samples.len() / 2;
        let window = &samples[middle - 4096..middle + 4096];
        let (low, high) = (SAMPLE_RATE as usize / 500, SAMPLE_RATE as usize / 60);
        let (mut best, mut best_score) = (low, f32::NEG_INFINITY);
        for lag in low..high {
            let mut dot = 0.0;
            let mut energy = 1e-9;
            for i in 0..4096 {
                dot += window[i] * window[i + lag];
                energy += window[i + lag] * window[i + lag];
            }
            let score = dot / f32::sqrt(energy);
            if score > best_score {
                best_score = score;
                best = lag;
            }
        }
        SAMPLE_RATE as f32 / best as f32
    }

    /// The whole point of the pitch shift: a twelve-year-old should sound
    /// higher than the adult who was hired, and take exactly as long saying it.
    #[test]
    fn a_pitch_shift_moves_the_pitch_and_leaves_the_length_alone() {
        let source = buzz(120.0, 3.0);
        for semitones in [-2.0f32, 2.2, 4.0, 7.0] {
            let shifted = pitch(&source, semitones);
            let wanted = 120.0 * 2f32.powf(semitones / 12.0);
            let got = fundamental(&shifted);
            assert!(
                (got / wanted - 1.0).abs() < 0.02,
                "{semitones:+} semitones gave {got:.1} Hz, wanted {wanted:.1}"
            );
            let drift = shifted.len() as f32 / source.len() as f32 - 1.0;
            assert!(
                drift.abs() < 0.03,
                "{semitones:+} semitones changed the length by {:.1}%",
                drift * 100.0
            );
        }
    }

    /// The failure this catches is silent and total: an overlap-add that
    /// measures the next frame from where the search moved the last one adds a
    /// pitch period every frame, and hands back a line exactly as long as it
    /// started however much stretch was asked for.
    #[test]
    fn a_stretch_actually_stretches_and_leaves_the_pitch_alone() {
        let source = buzz(120.0, 3.0);
        for factor in [0.8f64, 0.93, 1.07, 1.25] {
            let stretched = stretch(&source, factor);
            let got = stretched.len() as f64 / source.len() as f64;
            assert!(
                (got / factor - 1.0).abs() < 0.05,
                "asked for x{factor}, got x{got:.3}"
            );
            let pitch = fundamental(&stretched);
            assert!(
                (pitch / 120.0 - 1.0).abs() < 0.02,
                "x{factor} moved the pitch to {pitch:.1} Hz"
            );
        }
    }

    #[test]
    fn resampling_changes_the_length_and_the_pitch_together() {
        let source = buzz(120.0, 3.0);
        let half = resample(&source, 2.0);
        assert!((half.len() as f32 / source.len() as f32 - 0.5).abs() < 0.01);
        assert!((fundamental(&half) / 240.0 - 1.0).abs() < 0.02);
        // A ratio of one is not work worth doing.
        assert_eq!(resample(&source, 1.0), source);
    }

    /// A syllable too short to hold a frame still has to come back as
    /// something rather than as nothing.
    #[test]
    fn a_very_short_sound_survives_being_stretched() {
        let short = buzz(200.0, 0.02);
        let stretched = stretch(&short, 1.3);
        assert!(!stretched.is_empty());
        assert!(stretched.iter().all(|s| s.is_finite()));
        assert!(stretch(&[], 1.5).is_empty());
        assert!(pitch(&[], 4.0).is_empty());
    }

    /// Ages read off the curve, and nothing outside it runs away.
    #[test]
    fn the_age_curve_lifts_children_and_lowers_the_old() {
        let (child, _) = age_shift(12.0);
        let (teenager, _) = age_shift(15.0);
        let (adult, adult_tempo) = age_shift(30.0);
        let (old, old_tempo) = age_shift(80.0);

        assert!(child > teenager, "a twelve-year-old is higher than a fifteen-year-old");
        assert!(teenager > adult);
        assert_eq!(adult, 0.0, "an adult voice is left as it was recorded");
        assert_eq!(adult_tempo, 1.0);
        assert!(old < 0.0 && old_tempo < 1.0, "an old voice is lower and slower");

        // Flat outside the curve rather than running off it.
        assert_eq!(age_shift(0.0), age_shift(4.0));
        assert_eq!(age_shift(200.0), age_shift(90.0));
    }

    /// What the story file asks for, read off a line: the twelve is a lift and
    /// the fright is a tremble.
    #[test]
    fn a_line_gets_the_treatment_its_attributes_ask_for() {
        let line = |age, state, pos| Line {
            number: None,
            id: None,
            name: Some("ben".to_string()),
            age,
            state,
            pos,
            text: "Anything".to_string(),
        };

        let plain = Treatment::for_line(&line(None, State::Normal, Pos::Centre), 1.0);
        assert!(plain.is_plain(), "a normal adult line is left as recorded");

        let child = Treatment::for_line(&line(Some(12), State::Normal, Pos::Left), 1.0);
        assert!(child.semitones > 3.0, "twelve should lift the pitch");
        assert_eq!(child.pos, Pos::Left);

        let scared = Treatment::for_line(&line(None, State::Scared, Pos::Centre), 1.0);
        assert!(scared.wobble > 0.0 && scared.tremolo > 0.0, "fright is a tremble");

        let whisper = Treatment::for_line(&line(None, State::Whisper, Pos::Centre), 1.0);
        assert!(whisper.gain < 0.5 && whisper.breath > 0.0);
        assert_eq!(whisper.wobble, 0.0, "a whisper is quiet, not unsteady");
    }

    /// The slider exists because a child's voice already cast for a child
    /// should not have the child applied to it twice.
    #[test]
    fn the_age_strength_scales_the_age_and_nothing_else() {
        let line = Line {
            number: None,
            id: None,
            name: Some("ben".to_string()),
            age: Some(12),
            state: State::Scared,
            pos: Pos::Centre,
            text: "Anything".to_string(),
        };
        let full = Treatment::for_line(&line, 1.0);
        let none = Treatment::for_line(&line, 0.0);
        // The fright's own lift stays; only the twelve goes.
        assert!(none.semitones < full.semitones);
        assert_eq!(none.semitones, 0.9);
        assert_eq!(none.wobble, full.wobble);
        assert_eq!(none.tremolo, full.tremolo);
    }

    /// A voice must not get louder or quieter for having moved across the room.
    #[test]
    fn panning_keeps_a_voice_the_same_loudness_wherever_it_stands() {
        let source = buzz(120.0, 0.5);
        let power = |(left, right): (Vec<f32>, Vec<f32>)| -> f32 {
            left.iter().map(|s| s * s).sum::<f32>() + right.iter().map(|s| s * s).sum::<f32>()
        };
        let centre = power(pan(&source, Pos::Centre));
        for pos in [Pos::Left, Pos::Right] {
            let moved = power(pan(&source, pos));
            assert!((moved / centre - 1.0).abs() < 0.001, "{pos:?} changed the loudness");
        }

        // And it really does move: left is mostly in the left ear.
        let (left, right) = pan(&source, Pos::Left);
        let energy = |channel: &[f32]| channel.iter().map(|s| s * s).sum::<f32>();
        assert!(energy(&left) > energy(&right) * 5.0);
        // But not entirely, or it sounds like a fault rather than a room.
        assert!(energy(&right) > 0.0);
        // The centre is even.
        let (left, right) = pan(&source, Pos::Centre);
        assert!((energy(&left) / energy(&right) - 1.0).abs() < 0.001);
    }

    /// The same line rendered twice must come out the same, or regenerating
    /// one line of a finished play would shift everything around it.
    #[test]
    fn the_tremble_and_the_breath_are_the_same_every_time() {
        let input = Mono { samples: buzz(120.0, 0.5) };
        let treatment = Treatment {
            wobble: 38.0,
            tremolo: 0.24,
            breath: 0.4,
            ..Treatment::default()
        };
        let (first, _) = treatment.apply(&input, 99);
        let (again, _) = treatment.apply(&input, 99);
        assert_eq!(first, again);
        // A different line trembles differently.
        let (other, _) = treatment.apply(&input, 100);
        assert_ne!(first, other);
    }

    #[test]
    fn every_treatment_stays_inside_the_range_a_sample_has() {
        let input = Mono { samples: buzz(120.0, 1.0) };
        for state in State::ALL {
            let line = Line {
                number: None,
                id: None,
                name: Some("ben".to_string()),
                age: Some(9),
                state,
                pos: Pos::Left,
                text: "Anything".to_string(),
            };
            let (left, right) = Treatment::for_line(&line, 1.0).apply(&input, 7);
            assert!(!left.is_empty(), "{state:?} produced nothing");
            // Every state must fit inside a sample on its own, so that no one
            // loud line decides how loud the whole play is.
            assert!(
                left.iter().chain(right.iter()).all(|s| s.is_finite() && s.abs() <= CEILING),
                "{state:?} went out of range"
            );
        }
    }

    #[test]
    fn samples_are_read_the_way_elevenlabs_sends_them() {
        // Little-endian, signed, one channel.
        let mono = Mono::from_pcm16(&[0x00, 0x00, 0xff, 0x7f, 0x00, 0x80]);
        assert_eq!(mono.len(), 3);
        assert_eq!(mono.samples[0], 0.0);
        assert!((mono.samples[1] - 0.99997).abs() < 1e-4);
        assert_eq!(mono.samples[2], -1.0);
        // A truncated reply loses the half-sample rather than reading it.
        assert_eq!(Mono::from_pcm16(&[0x00, 0x00, 0x11]).len(), 1);
        assert!(Mono::from_pcm16(&[]).is_empty());
    }

    #[test]
    fn the_pieces_are_laid_end_to_end_with_a_pause_between_them() {
        let piece = |speaker: &str, seconds: f32| {
            let samples = vec![0.5f32; (seconds * SAMPLE_RATE as f32) as usize];
            Piece { left: samples.clone(), right: samples, speaker: speaker.to_string() }
        };
        // One person carrying on gets a shorter pause than a new speaker.
        let same = stitch(&[piece("ben", 1.0), piece("ben", 1.0)]).len() / 2;
        let different = stitch(&[piece("ben", 1.0), piece("faith", 1.0)]).len() / 2;
        assert!(different > same);
        let gap = (different - same) as f32 / SAMPLE_RATE as f32;
        assert!((gap - (GAP_MS - SAME_SPEAKER_GAP_MS) / 1000.0).abs() < 0.01);

        // A single piece gets no pause at all.
        assert_eq!(stitch(&[piece("ben", 1.0)]).len() / 2, SAMPLE_RATE as usize);
        assert!(stitch(&[]).is_empty());
    }

    /// Brought down to fit, never up: a play mixed quietly should stay quiet.
    #[test]
    fn a_mix_that_would_clip_is_brought_down_and_a_quiet_one_is_left_alone() {
        let loud = Piece {
            left: vec![1.4; 100],
            right: vec![-1.4; 100],
            speaker: "ben".to_string(),
        };
        let mixed = stitch(&[loud]);
        assert!(mixed.iter().all(|s| s.abs() <= 0.99 + 1e-6));

        let quiet = Piece { left: vec![0.1; 100], right: vec![0.1; 100], speaker: "b".into() };
        let mixed = stitch(&[quiet]);
        assert!(mixed.iter().all(|s| (s - 0.1).abs() < 1e-6), "a quiet mix was dragged up");
    }

    #[test]
    fn the_wav_header_is_the_one_every_player_expects() {
        let bytes = wav(&[0.0, 0.0, 0.5, -0.5], SAMPLE_RATE, 2);
        assert_eq!(&bytes[0..4], b"RIFF");
        assert_eq!(&bytes[8..12], b"WAVE");
        assert_eq!(&bytes[36..40], b"data");
        assert_eq!(bytes.len(), 44 + 4 * 2);
        // Sample rate, channels and the sizes agree with each other.
        assert_eq!(u32::from_le_bytes(bytes[24..28].try_into().unwrap()), SAMPLE_RATE);
        assert_eq!(u16::from_le_bytes(bytes[22..24].try_into().unwrap()), 2);
        assert_eq!(u32::from_le_bytes(bytes[40..44].try_into().unwrap()), 8);
        assert_eq!(u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize, bytes.len() - 8);
    }

    /// A float over one would otherwise wrap round to full scale the other
    /// way, which is the loudest sound a computer can make.
    #[test]
    fn a_sample_out_of_range_is_clamped_rather_than_wrapped() {
        let bytes = wav(&[9.0, -9.0], SAMPLE_RATE, 1);
        assert_eq!(i16::from_le_bytes(bytes[44..46].try_into().unwrap()), 32_767);
        assert_eq!(i16::from_le_bytes(bytes[46..48].try_into().unwrap()), -32_767);
    }
}
