//! How the NeoPixels *look*, kept apart from what they show (the per-person
//! pressed state `led.rs` keeps) and from how they're driven (`led.rs`'s
//! WS2812 output and frame loop). To change the look — steady, breathing,
//! per-person colors, ... — edit only this file: `render` is a pure function
//! of the state and the time, so it needs no hardware.

use smart_leds::RGB8;

use crate::config::{NEOPIXEL_BLINK_HALF_PERIOD_MS, NUM_PEOPLE};

/// Dim green — bright enough to read, low enough to keep current draw
/// modest across 5 LEDs on a single 5V rail.
const PRESSED_COLOR: RGB8 = RGB8::new(0, 40, 0);
const OFF: RGB8 = RGB8::new(0, 0, 0);

/// The colors for one frame, one per person in `config::PEOPLE` order.
/// `now_ms` is any monotonic clock in milliseconds (the blink phase is taken
/// from it, so it stays regular however often frames are drawn).
///
/// Current look: people who pressed today blink in `PRESSED_COLOR`, on for
/// `NEOPIXEL_BLINK_HALF_PERIOD_MS` then off for as long; the rest stay off.
pub fn render(pressed: &[bool; NUM_PEOPLE], now_ms: u64) -> [RGB8; NUM_PEOPLE] {
    let blink_on = (now_ms / NEOPIXEL_BLINK_HALF_PERIOD_MS).is_multiple_of(2);
    core::array::from_fn(|person_idx| {
        if pressed[person_idx] && blink_on {
            PRESSED_COLOR
        } else {
            OFF
        }
    })
}
