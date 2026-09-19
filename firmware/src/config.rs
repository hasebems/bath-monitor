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
/// `audio.rs`. Not a standard consumer audio rate on purpose — this is a
/// closed point-to-point I2S link (PIO to the MAX98357A) with no external
/// interop requirement, so it's chosen as 440 Hz x 100 instead: at MIDI
/// note 69 (A4 = 440 Hz exactly), the single-cycle waveform table used by
/// the additional-spec synth (`docs/additional_spec.md`) comes out to
/// exactly 100 samples with zero pitch error.
pub const AUDIO_SAMPLE_RATE_HZ: u32 = 44_000;
/// Bit depth per I2S channel; must match what `audio.rs`'s PIO program is
/// configured for (`PioI2sOutProgram`/`PioI2sOut` take this as a parameter).
pub const AUDIO_BIT_DEPTH: u32 = 16;

/// Number of samples in `audio.rs`'s shared `WAVEFORM_BUFFER`, one DMA
/// transfer's worth. CORE1 continuously streams this buffer's current
/// contents out over I2S in a loop, independent of whatever writes into it.
pub const AUDIO_BUFFER_SAMPLES: usize = 256;

/// Length of `waveform.rs`'s single-cycle sine wavetable. Exactly 100
/// because `AUDIO_SAMPLE_RATE_HZ` was chosen as 440Hz x 100, so MIDI note 69
/// (A4 = 440Hz exactly) reads this table one sample per output sample with
/// zero pitch error (see `docs/additional_spec.md`).
pub const AUDIO_WAVETABLE_SAMPLES: usize = 100;

/// The unit `music.rs`'s `NoteEvent::time`/`duration` are counted in.
pub const AUDIO_TIME_UNIT_MS: u64 = 10;

/// Per-sample envelope rate constants and silence threshold for
/// `waveform.rs`'s slots (`amplitude += (target - amplitude) * rate`,
/// applied once per sample at `AUDIO_SAMPLE_RATE_HZ`). These are
/// placeholder values, not yet tuned by ear on real hardware — see the
/// "未決定事項" list in `docs/additional_spec.md`.
pub const AUDIO_ATTACK_RATE: f32 = 0.01;
pub const AUDIO_RELEASE_RATE: f32 = 0.002;
pub const AUDIO_DAMP_RATE: f32 = 0.05;
pub const AUDIO_MINIMUM_LEVEL: f32 = 0.001;

pub const BUTTON_DEBOUNCE_MS: u64 = 50;
pub const OCCUPANCY_DEBOUNCE_MS: u64 = 2000;

/// Half of the onboard LED's blink period (on for this long, then off for
/// this long) — see `wifi.rs`. 500ms gives a 1Hz blink.
pub const ONBOARD_LED_BLINK_HALF_PERIOD_MS: u64 = 500;

/// Upper bound for a whole HTTP request to the server (connect + send +
/// response), after which it's abandoned and treated as failed so the
/// sender/poll tasks can never hang on an unreachable server.
pub const SERVER_REQUEST_TIMEOUT_SECS: u64 = 10;

/// How long `http_client.rs` waits before re-trying a POST that failed
/// while Wi-Fi is still up (e.g. the server is down).
pub const SEND_RETRY_INTERVAL_SECS: u64 = 5;

/// After a successful `join`, how long `wifi.rs` tolerates the link not
/// being reported up yet before it gives up on that association and re-joins.
pub const WIFI_LINK_UP_GRACE_SECS: u64 = 10;

/// How often the firmware polls `GET /api/led-state` to reconcile NeoPixels
/// with the server's daily-reset state.
pub const LED_SYNC_INTERVAL_SECS: u64 = 30;
