//! The "music-data playback module" from `docs/additional_spec.md`: holds
//! the 5 fixed melodies (one per `config::PEOPLE` entry), plus the cancel
//! sound and the "last one of the day" melody, and, once
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
/// `config::PEOPLE` entry, plus `CANCEL_MELODY` and `FINAL_MELODY` (see
/// `docs/additional_spec.md`).
/// Sent from CORE0's button tasks to this CORE1 task — the one channel that
/// crosses cores. (Neither the NeoPixel state nor data bound for the server
/// goes over a channel: see `led_pattern.rs` and `outbox.rs`.)
pub static MELODY_CHANNEL: Channel<CriticalSectionRawMutex, usize, 8> = Channel::new();

/// `MELODY_CHANNEL` value for the "press cancelled" sound, one past the
/// per-person melodies.
pub const CANCEL_MELODY: usize = NUM_PEOPLE;

/// `MELODY_CHANNEL` value for the melody played, instead of the person's own,
/// by the press that makes everyone pressed today.
pub const FINAL_MELODY: usize = NUM_PEOPLE + 1;

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
    note(0, 63, 100, 20),
    note(40, 63, 100, 12),
    note(60, 63, 100, 12),
    note(80, 63, 100, 12),
    note(100, 68, 100, 12),
    note(120, 65, 100, 12),
    note(140, 63, 100, 12),
    note(200, 65, 100, 12),
    note(240, 65, 100, 12),
];
static MELODY_1: &[NoteEvent] = &[
    note(0, 76, 100, 16),
    note(20, 75, 100, 16),
    note(40, 76, 100, 16),
    note(60, 75, 100, 16),
    note(80, 76, 100, 16),
    note(100, 71, 100, 16),
    note(120, 74, 100, 16),
    note(140, 72, 100, 16),
    note(160, 69, 100, 60),
];
static MELODY_2: &[NoteEvent] = &[
    note(0, 82, 100, 10),
    note(15, 80, 100, 14),
    note(45, 82, 80, 10),
    note(60, 77, 100, 16),
    note(90, 82, 80, 16),
    note(120, 80, 100, 12),
    note(135, 77, 100, 25),
    note(165, 75, 100, 15),
    note(180, 71, 100, 50),
];
static MELODY_3: &[NoteEvent] = &[
    note(0, 71, 100, 30),
    note(40, 72, 100, 10),
    note(60, 71, 100, 16),
    note(80, 69, 100, 16),
    note(100, 67, 100, 16),
    note(120, 66, 100, 16),    
    note(140, 64, 100, 16),
    note(200, 75, 100, 10),
    note(220, 76, 100, 20),
];
static MELODY_4: &[NoteEvent] = &[
    note(0, 75, 70, 18),
    note(20, 80, 80, 18),
    note(40, 87, 80, 18),
    note(60, 90, 80, 18),
    note(80, 87, 80, 18),
    note(100, 80, 70, 18),
    note(120, 73, 80, 8),
    note(130, 80, 80, 8),
    note(140, 85, 85, 8),
    note(150, 88, 85, 8),
    note(160, 75, 70, 18),
    note(180, 80, 80, 18),
    note(200, 87, 80, 18),
    note(220, 90, 80, 18),
    note(240, 87, 80, 18),
    note(260, 80, 70, 18),
    note(280, 73, 80, 8),
    note(290, 80, 80, 8),
    note(300, 85, 85, 8),
    note(310, 88, 85, 8),
];

/// "Press cancelled" sound: two low, falling notes, unlike any person's tune.
/// Placeholder like the melodies above.
static MELODY_CANCEL: &[NoteEvent] = &[
    note(0, 78, 100, 6),
    note(12, 84, 100, 6),
    note(24, 78, 100, 6),
    note(36, 84, 100, 6),
];

/// "Everyone has bathed — you're the last": a rising arpeggio ending on a
/// held high note, unlike any person's tune. Placeholder like the melodies
/// above.
static MELODY_FINAL: &[NoteEvent] = &[
    note(0, 66, 100, 36),
    note(40, 66, 100, 36),
    note(80, 67, 100, 36),
    note(120, 69, 100, 36),
    note(160, 69, 100, 36),
    note(200, 67, 100, 36),
    note(240, 66, 100, 36),
    note(280, 64, 100, 36),
    note(320, 62, 100, 36),
    note(360, 62, 100, 36),
    note(400, 64, 100, 36),
    note(440, 66, 100, 36),
    note(480, 66, 100, 46),
    note(540, 64, 100, 16),
    note(560, 64, 100, 36),
];

/// Indexed by person_idx (0-4), matching `config::PEOPLE`, then
/// `CANCEL_MELODY` and `FINAL_MELODY` — the values sent on `MELODY_CHANNEL`.
static MUSIC_DATA: [&[NoteEvent]; NUM_PEOPLE + 2] = [
    MELODY_0,
    MELODY_1,
    MELODY_2,
    MELODY_3,
    MELODY_4,
    MELODY_CANCEL,
    MELODY_FINAL,
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
