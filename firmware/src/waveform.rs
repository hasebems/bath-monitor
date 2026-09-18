//! The "waveform output module" from `docs/additional_spec.md`: owns 2
//! synth slots, each a sine-wavetable oscillator with an
//! attack/release/damp amplitude envelope, and mixes their output into
//! `audio::WAVEFORM_BUFFER` once per DMA chunk. Runs as its own task on
//! CORE1 (spawned by `audio::start`), paced by `audio::BUFFER_CONSUMED` so
//! it produces exactly one chunk per chunk actually played — never racing
//! ahead (which would silently drop chunks) or falling behind (which would
//! repeat one).
//!
//! `music::music_task` is the only other writer/reader of this module's
//! public state: it sends `NoteOn` messages into `SLOT_CHANNELS` to trigger
//! playback, alternating between the two slots.

use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::Channel;

use crate::audio::{BUFFER_CONSUMED, WAVEFORM_BUFFER};
use crate::config::{
    AUDIO_ATTACK_RATE, AUDIO_BUFFER_SAMPLES, AUDIO_DAMP_RATE, AUDIO_MINIMUM_LEVEL,
    AUDIO_RELEASE_RATE, AUDIO_SAMPLE_RATE_HZ, AUDIO_TIME_UNIT_MS, AUDIO_WAVETABLE_SAMPLES,
};

/// One cycle of a sine wave, peak amplitude 16383 (so two slots summed at
/// full envelope/volume top out at 32766, just inside i16 range without
/// needing to rely on the overflow clamp below in the common case).
/// Generated with `round(16383 * sin(2*pi*i/100))` for i in 0..100 — 100
/// samples exactly because `config::AUDIO_SAMPLE_RATE_HZ` (44,000Hz) was
/// deliberately chosen as 440Hz (MIDI note 69 = A4) x 100.
const WAVETABLE: [i16; AUDIO_WAVETABLE_SAMPLES] = [
    0, 1029, 2053, 3070, 4074, 5063, 6031, 6976, 7893, 8778, 9630, 10443, 11215, 11943, 12623,
    13254, 13833, 14357, 14824, 15233, 15581, 15868, 16093, 16254, 16351, 16383, 16351, 16254,
    16093, 15868, 15581, 15233, 14824, 14357, 13833, 13254, 12623, 11943, 11215, 10443, 9630,
    8778, 7893, 6976, 6031, 5063, 4074, 3070, 2053, 1029, 0, -1029, -2053, -3070, -4074, -5063,
    -6031, -6976, -7893, -8778, -9630, -10443, -11215, -11943, -12623, -13254, -13833, -14357,
    -14824, -15233, -15581, -15868, -16093, -16254, -16351, -16383, -16351, -16254, -16093,
    -15868, -15581, -15233, -14824, -14357, -13833, -13254, -12623, -11943, -11215, -10443,
    -9630, -8778, -7893, -6976, -6031, -5063, -4074, -3070, -2053, -1029,
];

/// Per-sample phase increment (Q16 fixed point) for each MIDI note number,
/// indexed directly by pitch. Generated with
/// `round(65536 * 2^((pitch - 69) / 12))` — i.e. MIDI note 69 (A4 = 440Hz)
/// reads the 100-sample `WAVETABLE` exactly one sample per output sample
/// (step == 1<<16), by construction of `AUDIO_SAMPLE_RATE_HZ`.
const PITCH_STEP_Q16: [u32; 128] = [
    1218, 1290, 1367, 1448, 1534, 1625, 1722, 1825, 1933, 2048, 2170, 2299, 2435, 2580, 2734,
    2896, 3069, 3251, 3444, 3649, 3866, 4096, 4340, 4598, 4871, 5161, 5468, 5793, 6137, 6502,
    6889, 7298, 7732, 8192, 8679, 9195, 9742, 10321, 10935, 11585, 12274, 13004, 13777, 14596,
    15464, 16384, 17358, 18390, 19484, 20643, 21870, 23170, 24548, 26008, 27554, 29193, 30929,
    32768, 34716, 36781, 38968, 41285, 43740, 46341, 49097, 52016, 55109, 58386, 61858, 65536,
    69433, 73562, 77936, 82570, 87480, 92682, 98193, 104032, 110218, 116772, 123715, 131072,
    138866, 147123, 155872, 165140, 174960, 185364, 196386, 208064, 220436, 233544, 247431,
    262144, 277732, 294247, 311744, 330281, 349920, 370728, 392772, 416128, 440872, 467088,
    494862, 524288, 555464, 588493, 623487, 660561, 699841, 741455, 785544, 832255, 881744,
    934175, 989724, 1048576, 1110928, 1176987, 1246974, 1321123, 1399681, 1482910, 1571089,
    1664511, 1763488, 1868350,
];

/// A trigger for a slot: pitch/volume/duration exactly as described in
/// `docs/additional_spec.md`'s "note on message" (duration is in
/// `config::AUDIO_TIME_UNIT_MS` units, same as `music::NoteEvent`).
#[derive(Clone, Copy)]
pub struct NoteOn {
    pub pitch: u8,
    pub volume: u8,
    pub duration: u32,
}

/// One channel per slot; `music::music_task` sends into these to trigger
/// playback, alternating which slot gets each note.
pub static SLOT_CHANNELS: [Channel<CriticalSectionRawMutex, NoteOn, 4>; 2] =
    [Channel::new(), Channel::new()];

#[derive(Clone, Copy, PartialEq)]
enum Phase {
    Idle,
    Attack,
    Release,
    /// Rapidly silencing the current note (at the old pitch) because a new
    /// note-on interrupted it; `pending` starts once amplitude crosses
    /// `AUDIO_MINIMUM_LEVEL`.
    Damp,
}

struct Slot {
    phase: Phase,
    phase_acc: u32,
    step: u32,
    amplitude: f32,
    target: f32,
    rate: f32,
    elapsed_samples: u32,
    release_at_samples: u32,
    pending: Option<NoteOn>,
}

impl Slot {
    const fn new() -> Self {
        Slot {
            phase: Phase::Idle,
            phase_acc: 0,
            step: 0,
            amplitude: 0.0,
            target: 0.0,
            rate: 0.0,
            elapsed_samples: 0,
            release_at_samples: 0,
            pending: None,
        }
    }

    /// Applies an incoming note-on per `docs/additional_spec.md`: starts
    /// immediately if idle, otherwise damps the currently-sounding note
    /// (keeping its pitch until the damp finishes) and remembers this note
    /// to attack next. A note-on arriving while already damping just
    /// replaces whichever note was queued to play next.
    fn note_on(&mut self, note: NoteOn) {
        match self.phase {
            Phase::Idle => self.start_attack(note),
            Phase::Damp => self.pending = Some(note),
            Phase::Attack | Phase::Release => {
                self.phase = Phase::Damp;
                self.target = 0.0;
                self.rate = AUDIO_DAMP_RATE;
                self.pending = Some(note);
            }
        }
    }

    fn start_attack(&mut self, note: NoteOn) {
        self.phase_acc = 0;
        self.step = PITCH_STEP_Q16[(note.pitch & 0x7f) as usize];
        let v = note.volume as f32 / 127.0;
        self.target = v * v;
        self.rate = AUDIO_ATTACK_RATE;
        self.phase = Phase::Attack;
        self.elapsed_samples = 0;
        let samples_per_unit = (AUDIO_SAMPLE_RATE_HZ as u64 * AUDIO_TIME_UNIT_MS) / 1000;
        self.release_at_samples = (note.duration as u64 * samples_per_unit) as u32;
    }

    /// Advances the envelope and oscillator by one sample and returns the
    /// resulting output sample. The "constant proportion of the remaining
    /// distance to target" step here is what `docs/additional_spec.md`
    /// calls the asymptotic attack/release/damp curve, applied once per
    /// sample (not once per chunk) to avoid audible zipper noise.
    fn tick(&mut self) -> i16 {
        self.amplitude += (self.target - self.amplitude) * self.rate;

        match self.phase {
            Phase::Attack => {
                self.elapsed_samples += 1;
                if self.elapsed_samples >= self.release_at_samples {
                    self.phase = Phase::Release;
                    self.target = 0.0;
                    self.rate = AUDIO_RELEASE_RATE;
                }
            }
            Phase::Release => {
                if self.amplitude <= AUDIO_MINIMUM_LEVEL {
                    self.amplitude = 0.0;
                    self.phase = Phase::Idle;
                }
            }
            Phase::Damp => {
                if self.amplitude <= AUDIO_MINIMUM_LEVEL {
                    self.amplitude = 0.0;
                    match self.pending.take() {
                        Some(note) => self.start_attack(note),
                        None => self.phase = Phase::Idle,
                    }
                }
            }
            Phase::Idle => {}
        }

        let idx = (self.phase_acc >> 16) as usize % AUDIO_WAVETABLE_SAMPLES;
        self.phase_acc = self.phase_acc.wrapping_add(self.step);
        (WAVETABLE[idx] as f32 * self.amplitude) as i16
    }
}

/// Waits for `BUFFER_CONSUMED`, applies any pending note-on messages, then
/// synthesizes and sums exactly `AUDIO_BUFFER_SAMPLES` samples from the 2
/// slots into `WAVEFORM_BUFFER` — one chunk per signal, staying paced 1:1
/// with actual playback.
#[embassy_executor::task]
pub async fn waveform_task() {
    let mut slot0 = Slot::new();
    let mut slot1 = Slot::new();
    let mut chunk = [0u32; AUDIO_BUFFER_SAMPLES];

    loop {
        BUFFER_CONSUMED.wait().await;

        while let Ok(note) = SLOT_CHANNELS[0].try_receive() {
            slot0.note_on(note);
        }
        while let Ok(note) = SLOT_CHANNELS[1].try_receive() {
            slot1.note_on(note);
        }

        for word in chunk.iter_mut() {
            let s0 = slot0.tick() as i32;
            let s1 = slot1.tick() as i32;
            // Overflow saturates at the max (or min) value per docs/additional_spec.md.
            let mixed = (s0 + s1).clamp(i16::MIN as i32, i16::MAX as i32) as i16;
            *word = (mixed as u16 as u32) * 0x1_0001;
        }

        WAVEFORM_BUFFER.lock(|buf| *buf.borrow_mut() = chunk);
    }
}
