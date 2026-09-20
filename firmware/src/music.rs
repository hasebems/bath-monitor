//! The "music-data playback module" from `docs/additional_spec.md`: holds
//! the 5 fixed melodies (one per `config::PEOPLE` entry) and, once
//! triggered by a message on `MELODY_CHANNEL`, schedules their note
//! events and dispatches them as `waveform::NoteOn` messages, alternating
//! between `waveform::SLOT_CHANNELS[0]` and `[1]`.
//!
//! Runs as its own task on CORE1 (spawned by `audio::start`), independent
//! of the waveform output module's per-chunk pacing — this task only cares
//! about wall-clock time, at `config::AUDIO_TIME_UNIT_MS` resolution.

use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::Channel;
use embassy_time::{with_timeout, Duration, Instant};

use crate::config::{AUDIO_TIME_UNIT_MS, NUM_PEOPLE};
use crate::waveform::{NoteOn, SLOT_CHANNELS};

/// Which melody to (re)start playing: an index into `MUSIC_DATA` — one per
/// `config::PEOPLE` entry, plus `CANCEL_MELODY` (see
/// `docs/additional_spec.md`).
/// Sent from CORE0's button tasks to this CORE1 task — the one channel that
/// crosses cores. (Neither the NeoPixel state nor data bound for the server
/// goes over a channel: see `led_pattern.rs` and `outbox.rs`.)
pub static MELODY_CHANNEL: Channel<CriticalSectionRawMutex, usize, 8> = Channel::new();

/// `MELODY_CHANNEL` value for the "press cancelled" sound, one past the
/// per-person melodies.
pub const CANCEL_MELODY: usize = NUM_PEOPLE;

/// One entry in a music data table, exactly as described in
/// `docs/additional_spec.md`: `time`/`duration` are in `AUDIO_TIME_UNIT_MS`
/// units, counted from when playback of that melody started.
pub struct NoteEvent {
    pub time: u32,
    pub pitch: u8,
    pub volume: u8,
    pub duration: u32,
}

const fn note(time: u32, pitch: u8, volume: u8, duration: u32) -> NoteEvent {
    NoteEvent {
        time,
        pitch,
        volume,
        duration,
    }
}

// Placeholder melodies, one per `config::PEOPLE` entry (index == person_idx)
// so each person's button plays a recognizably different short tune. These
// are not real compositions — just distinct enough motifs to exercise the
// playback module; replace with real tunes whenever those are authored.
static MELODY_0: &[NoteEvent] = &[
    note(0, 60, 100, 20),
    note(25, 64, 100, 20),
    note(50, 67, 100, 30),
];
static MELODY_1: &[NoteEvent] = &[
    note(0, 67, 100, 20),
    note(25, 64, 100, 20),
    note(50, 60, 100, 30),
];
static MELODY_2: &[NoteEvent] = &[
    note(0, 60, 100, 15),
    note(20, 62, 100, 15),
    note(40, 64, 100, 15),
    note(60, 65, 100, 30),
];
static MELODY_3: &[NoteEvent] = &[
    note(0, 72, 100, 15),
    note(20, 69, 100, 15),
    note(40, 65, 100, 30),
];
static MELODY_4: &[NoteEvent] = &[
    note(0, 60, 100, 10),
    note(15, 60, 100, 10),
    note(30, 60, 100, 10),
    note(50, 67, 100, 40),
];

/// "Press cancelled" sound: two low, falling notes, unlike any person's tune.
/// Placeholder like the melodies above.
static MELODY_CANCEL: &[NoteEvent] = &[note(0, 55, 100, 15), note(20, 48, 100, 40)];

/// Indexed by person_idx (0-4), matching `config::PEOPLE`, then
/// `CANCEL_MELODY` — the values sent on `MELODY_CHANNEL`.
static MUSIC_DATA: [&[NoteEvent]; NUM_PEOPLE + 1] = [
    MELODY_0,
    MELODY_1,
    MELODY_2,
    MELODY_3,
    MELODY_4,
    MELODY_CANCEL,
];

/// Drains `MELODY_CHANNEL` and plays back the selected melody. A new event
/// arriving mid-playback immediately switches to the new melody from its
/// start; per `docs/additional_spec.md`, the slots that were already
/// sounding are left alone (no explicit mute) — they just ring out under
/// their own release/damp envelope.
#[embassy_executor::task]
pub async fn music_task() {
    let mut score: &'static [NoteEvent] = &[];
    let mut next_idx: usize = 0;
    let mut next_slot: usize = 0;
    let mut start = Instant::now();

    loop {
        let wait = if next_idx < score.len() {
            let due =
                start + Duration::from_millis(score[next_idx].time as u64 * AUDIO_TIME_UNIT_MS);
            due.saturating_duration_since(Instant::now())
        } else {
            // Not `Duration::MAX`: `Timer::after` (which `with_timeout` uses
            // internally) computes `Instant::now() + duration` eagerly and
            // panics on overflow, and `now()` is already nonzero by the time
            // this runs. An hour is effectively "forever" for this use case
            // (wait for the next melody-select event) without risking that.
            Duration::from_secs(3600)
        };

        match with_timeout(wait, MELODY_CHANNEL.receive()).await {
            Ok(melody_idx) => {
                score = MUSIC_DATA[melody_idx];
                next_idx = 0;
                next_slot = 0;
                start = Instant::now();
            }
            Err(_timeout) => {
                if next_idx < score.len() {
                    let n = &score[next_idx];
                    SLOT_CHANNELS[next_slot]
                        .send(NoteOn {
                            pitch: n.pitch,
                            volume: n.volume,
                            duration: n.duration,
                        })
                        .await;
                    next_idx += 1;
                    next_slot = 1 - next_slot;
                }
            }
        }
    }
}
