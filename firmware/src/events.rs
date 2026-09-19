use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::Channel;

use crate::config::NUM_PEOPLE;

/// Events that update the NeoPixel chain.
pub enum LedEvent {
    /// Immediate local feedback: a button was just pressed.
    Pressed { person_idx: usize },
    /// Reconciliation from the server's `/api/led-state` (also clears LEDs
    /// after the server's daily reset).
    Sync { pressed: [bool; NUM_PEOPLE] },
}

pub static LED_CHANNEL: Channel<CriticalSectionRawMutex, LedEvent, 8> = Channel::new();

/// Which melody to (re)start playing: an index into the 5 fixed music-data
/// tables, one per `config::PEOPLE` entry (see `docs/additional_spec.md`).
/// Sent from CORE0's button tasks to CORE1's music-data playback task —
/// unlike `LED_CHANNEL` above, which stays on CORE0, this is the one channel
/// here that crosses cores. (Data bound for the server isn't sent over a
/// channel at all — see `outbox.rs`.)
pub static MELODY_CHANNEL: Channel<CriticalSectionRawMutex, usize, 8> = Channel::new();
