//! Non-secret configuration. Must match `server/config.py`'s `PEOPLE` order/ids
//! and `docs/wiring.md`'s pin table.

pub const PEOPLE: [&str; 5] = ["alice", "bob", "carol", "dave", "eve"];
pub const NUM_PEOPLE: usize = PEOPLE.len();

// These pin numbers are documentation, cross-referenced by comments in
// main.rs — PIO/GPIO peripheral fields can't be indexed dynamically at
// runtime, so main.rs hardcodes `p.PIN_2` etc. and must be kept in sync
// with these values and with docs/wiring.md by hand.
#[allow(dead_code)]
/// GPIO for each person's button, same order as `PEOPLE`.
pub const BUTTON_PINS: [u8; NUM_PEOPLE] = [2, 3, 4, 5, 6];

#[allow(dead_code)]
/// GPIO for the light/occupancy sensor (digital two-state signal).
pub const LIGHT_SENSOR_PIN: u8 = 26;

#[allow(dead_code)]
/// GPIO for the NeoPixel (WS2812) data line, one chain of `NUM_PEOPLE` LEDs
/// in the same order as `PEOPLE`/`BUTTON_PINS`.
pub const NEOPIXEL_PIN: u8 = 15;

#[allow(dead_code)]
/// GPIO for the MAX98357A I2S amp's BCLK (bit clock) input.
pub const AUDIO_BCLK_PIN: u8 = 16;
#[allow(dead_code)]
/// GPIO for the MAX98357A I2S amp's LRC (word/left-right clock) input.
pub const AUDIO_LRCLK_PIN: u8 = 17;
#[allow(dead_code)]
/// GPIO for the MAX98357A I2S amp's DIN (audio data) input.
pub const AUDIO_DIN_PIN: u8 = 18;

/// Sample rate used for the I2S bit-clock timing derived from it in
/// `audio.rs`.
pub const AUDIO_SAMPLE_RATE_HZ: u32 = 48_000;
/// Bit depth per I2S channel; must match what `audio.rs`'s PIO program is
/// configured for (`PioI2sOutProgram`/`PioI2sOut` take this as a parameter).
pub const AUDIO_BIT_DEPTH: u32 = 16;

/// Number of samples in `audio.rs`'s shared `WAVEFORM_BUFFER`, one DMA
/// transfer's worth. CORE1 continuously streams this buffer's current
/// contents out over I2S in a loop, independent of whatever writes into it.
pub const AUDIO_BUFFER_SAMPLES: usize = 256;

pub const BUTTON_DEBOUNCE_MS: u64 = 50;
pub const OCCUPANCY_DEBOUNCE_MS: u64 = 2000;

/// How often the firmware polls `GET /api/led-state` to reconcile NeoPixels
/// with the server's daily-reset state.
pub const LED_SYNC_INTERVAL_SECS: u64 = 30;
