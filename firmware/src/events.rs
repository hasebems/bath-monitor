use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::Channel;

use crate::config::NUM_PEOPLE;

/// Events that need to be POSTed to the server.
pub enum AppEvent {
    ButtonPressed { person_idx: usize },
    OccupancyChanged { occupied: bool },
}

/// Events that update the NeoPixel chain.
pub enum LedEvent {
    /// Immediate local feedback: a button was just pressed.
    Pressed { person_idx: usize },
    /// Reconciliation from the server's `/api/led-state` (also clears LEDs
    /// after the server's daily reset).
    Sync { pressed: [bool; NUM_PEOPLE] },
}

/// Events that trigger the MAX98357A button-press beep (`audio.rs`).
pub enum AudioEvent {
    Play,
}

pub static EVENT_CHANNEL: Channel<CriticalSectionRawMutex, AppEvent, 8> = Channel::new();
pub static LED_CHANNEL: Channel<CriticalSectionRawMutex, LedEvent, 8> = Channel::new();
pub static AUDIO_CHANNEL: Channel<CriticalSectionRawMutex, AudioEvent, 8> = Channel::new();
