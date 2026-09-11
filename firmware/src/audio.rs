use embassy_rp::peripherals::{DMA_CH3, PIN_16, PIN_17, PIN_18, PIO2};
use embassy_rp::pio::Pio;
use embassy_rp::pio_programs::i2s::{PioI2sOut, PioI2sOutProgram};
use embassy_rp::Peri;
use static_cell::StaticCell;

use crate::config::{AUDIO_BIT_DEPTH, AUDIO_SAMPLE_RATE_HZ};
use crate::events::AUDIO_CHANNEL;
use crate::irqs::Irqs;

/// One cycle of a sine wave (peak amplitude 24000, leaving headroom below
/// full i16 scale), used as the wavetable for the button-press beep.
/// Generated with `round(24000 * sin(2*pi*i/256))` for i in 0..256.
const WAVETABLE_LEN: usize = 256;
const WAVETABLE: [i16; WAVETABLE_LEN] = [
    0, 589, 1178, 1766, 2352, 2938, 3522, 4103, 4682, 5258, 5832, 6401, 6967, 7528, 8085, 8637,
    9184, 9726, 10261, 10791, 11314, 11830, 12338, 12840, 13334, 13819, 14297, 14766, 15225, 15676,
    16117, 16549, 16971, 17382, 17783, 18173, 18552, 18920, 19277, 19622, 19955, 20276, 20585,
    20882, 21166, 21437, 21696, 21941, 22173, 22392, 22597, 22789, 22967, 23131, 23281, 23417,
    23539, 23647, 23740, 23820, 23884, 23935, 23971, 23993, 24000, 23993, 23971, 23935, 23884,
    23820, 23740, 23647, 23539, 23417, 23281, 23131, 22967, 22789, 22597, 22392, 22173, 21941,
    21696, 21437, 21166, 20882, 20585, 20276, 19955, 19622, 19277, 18920, 18552, 18173, 17783,
    17382, 16971, 16549, 16117, 15676, 15225, 14766, 14297, 13819, 13334, 12840, 12338, 11830,
    11314, 10791, 10261, 9726, 9184, 8637, 8085, 7528, 6967, 6401, 5832, 5258, 4682, 4103, 3522,
    2938, 2352, 1766, 1178, 589, 0, -589, -1178, -1766, -2352, -2938, -3522, -4103, -4682, -5258,
    -5832, -6401, -6967, -7528, -8085, -8637, -9184, -9726, -10261, -10791, -11314, -11830, -12338,
    -12840, -13334, -13819, -14297, -14766, -15225, -15676, -16117, -16549, -16971, -17382, -17783,
    -18173, -18552, -18920, -19277, -19622, -19955, -20276, -20585, -20882, -21166, -21437, -21696,
    -21941, -22173, -22392, -22597, -22789, -22967, -23131, -23281, -23417, -23539, -23647, -23740,
    -23820, -23884, -23935, -23971, -23993, -24000, -23993, -23971, -23935, -23884, -23820, -23740,
    -23647, -23539, -23417, -23281, -23131, -22967, -22789, -22597, -22392, -22173, -21941, -21696,
    -21437, -21166, -20882, -20585, -20276, -19955, -19622, -19277, -18920, -18552, -18173, -17783,
    -17382, -16971, -16549, -16117, -15676, -15225, -14766, -14297, -13819, -13334, -12840, -12338,
    -11830, -11314, -10791, -10261, -9726, -9184, -8637, -8085, -7528, -6967, -6401, -5832, -5258,
    -4682, -4103, -3522, -2938, -2352, -1766, -1178, -589,
];

/// Pitch and length of the button-press beep.
const TONE_FREQ_HZ: u32 = 880;
const TONE_DURATION_MS: u32 = 180;
/// Linear fade in/out at the start/end of the tone, long enough to avoid an
/// audible click from the amp seeing a stepped edge.
const FADE_MS: u32 = 8;

const TOTAL_SAMPLES: usize = (AUDIO_SAMPLE_RATE_HZ * TONE_DURATION_MS / 1000) as usize;
const FADE_SAMPLES: usize = (AUDIO_SAMPLE_RATE_HZ * FADE_MS / 1000) as usize;

/// Synthesizes the fixed button-press tone once at startup: walks the sine
/// wavetable with a fixed-point (Q16) phase accumulator at `TONE_FREQ_HZ`,
/// applying a linear fade-in/out envelope, and duplicates each mono sample
/// into both halves of the 32-bit I2S DMA word (left/right channels get the
/// same value, matching how the MAX98357A's mono output is fed in embassy's
/// own `pio_i2s` example).
fn build_tone() -> [u32; TOTAL_SAMPLES] {
    let step = ((WAVETABLE_LEN as u64 * TONE_FREQ_HZ as u64 * (1 << 16))
        / AUDIO_SAMPLE_RATE_HZ as u64) as u32;
    let mut phase: u32 = 0;
    let mut buf = [0u32; TOTAL_SAMPLES];
    for (n, slot) in buf.iter_mut().enumerate() {
        let idx = (phase >> 16) as usize % WAVETABLE_LEN;
        phase = phase.wrapping_add(step);

        let envelope: i32 = if n < FADE_SAMPLES {
            (n as i32 * 256) / FADE_SAMPLES as i32
        } else if n >= TOTAL_SAMPLES - FADE_SAMPLES {
            ((TOTAL_SAMPLES - n) as i32 * 256) / FADE_SAMPLES as i32
        } else {
            256
        };
        let sample = ((WAVETABLE[idx] as i32 * envelope) >> 8) as i16;
        *slot = (sample as u16 as u32) * 0x1_0001;
    }
    buf
}

/// Owns the I2S link to the MAX98357A amp (PIO2 + DMA_CH3, `config::AUDIO_*`
/// pins). Plays the fixed beep tone once per `AudioEvent::Play` (sent by
/// `buttons.rs` on every debounced press), draining any presses that land
/// while a tone is already playing so they play back-to-back rather than
/// being dropped or overlapping.
#[embassy_executor::task]
pub async fn audio_task(
    pio2: Peri<'static, PIO2>,
    dma_ch3: Peri<'static, DMA_CH3>,
    bclk: Peri<'static, PIN_16>,
    lrclk: Peri<'static, PIN_17>,
    din: Peri<'static, PIN_18>,
) {
    let Pio {
        mut common, sm0, ..
    } = Pio::new(pio2, Irqs);
    let program = PioI2sOutProgram::new(&mut common);
    let mut i2s = PioI2sOut::new(
        &mut common,
        sm0,
        dma_ch3,
        Irqs,
        din,
        bclk,
        lrclk,
        AUDIO_SAMPLE_RATE_HZ,
        AUDIO_BIT_DEPTH,
        &program,
    );
    i2s.start();

    static TONE: StaticCell<[u32; TOTAL_SAMPLES]> = StaticCell::new();
    let tone = TONE.init_with(build_tone);

    loop {
        AUDIO_CHANNEL.receive().await;
        i2s.write(&tone[..]).await;
    }
}
