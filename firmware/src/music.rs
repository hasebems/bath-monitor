//! The "music-data playback module" from `docs/additional_spec.md`: holds
//! the 5 fixed melodies (one per `config::PEOPLE` entry) and, once
//! triggered by an event on `events::MELODY_CHANNEL`, schedules their note
//! events and dispatches them as `waveform::NoteOn` messages, alternating
//! between `waveform::SLOT_CHANNELS[0]` and `[1]`.
//!
//! Runs as its own task on CORE1 (spawned by `audio::start`), independent
//! of the waveform output module's per-chunk pacing — this task only cares
//! about wall-clock time, at `config::AUDIO_TIME_UNIT_MS` resolution.

use embassy_time::{with_timeout, Duration, Instant};

use crate::config::{AUDIO_TIME_UNIT_MS, NUM_PEOPLE};
use crate::events::MELODY_CHANNEL;
use crate::waveform::{NoteOn, SLOT_CHANNELS};

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

/// Indexed by person_idx (0-4), matching `config::PEOPLE`/`MELODY_CHANNEL`.
static MUSIC_DATA: [&[NoteEvent]; NUM_PEOPLE] =
    [MELODY_0, MELODY_1, MELODY_2, MELODY_3, MELODY_4];

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
            Ok(person_idx) => {
                score = MUSIC_DATA[person_idx];
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
