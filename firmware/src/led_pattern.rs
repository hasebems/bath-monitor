//! How the NeoPixels *look*, kept apart from what they show (the per-person
//! pressed state `led.rs` keeps) and from how they're driven (`led.rs`'s
//! WS2812 output and frame loop). To change the look — colors, breathing
//! speed, ... — edit only this file: `render` is a pure function of the state
//! and the time, so it needs no hardware.

use smart_leds::RGB8;

use crate::config::NUM_PEOPLE;

/// The color each person lights up in once they've pressed today, in
/// `config::PEOPLE` order (grandpa, grandma, father, mother, son). Kept dim
/// to limit current draw across 5 LEDs on a single 5V rail, and none is
/// whitish so it can't be mistaken for the "not yet" breathing below.
const PRESSED_COLORS: [RGB8; NUM_PEOPLE] = [
    RGB8::new(0, 0, 48),  // grandpa: blue
    RGB8::new(40, 0, 20), // grandma: pink
    RGB8::new(0, 40, 0),  // father: green
    RGB8::new(48, 16, 0), // mother: orange
    RGB8::new(40, 28, 0), // son: yellow
];

/// People who haven't pressed yet breathe in white: one full dark → bright →
/// dark cycle takes this long.
const BREATH_PERIOD_MS: u64 = 6000;
/// Brightest value of each channel at the top of the breath.
const BREATH_PEAK: u32 = 40;

/// The colors for one frame, one per person in `config::PEOPLE` order.
/// `now_ms` is any monotonic clock in milliseconds (the breathing phase is
/// taken from it, so it stays regular however often frames are drawn, and
/// everyone who hasn't pressed breathes in step).
///
/// Current look: people who pressed today stay lit steadily in their own
/// `PRESSED_COLORS` entry; the rest breathe in white with a
/// `BREATH_PERIOD_MS` period.
pub fn render(pressed: &[bool; NUM_PEOPLE], now_ms: u64) -> [RGB8; NUM_PEOPLE] {
    let waiting = breath_color(now_ms);
    core::array::from_fn(|person_idx| {
        if pressed[person_idx] {
            PRESSED_COLORS[person_idx]
        } else {
            waiting
        }
    })
}

/// White at the breath's brightness for `now_ms`: a triangle wave (up for half
/// the period, down for the other half), squared so the perceived brightness
/// changes evenly — WS2812 output is linear, but the eye isn't.
fn breath_color(now_ms: u64) -> RGB8 {
    let half = BREATH_PERIOD_MS / 2;
    let phase = now_ms % BREATH_PERIOD_MS;
    let rising = if phase < half {
        phase
    } else {
        BREATH_PERIOD_MS - phase
    };
    let rising = rising as u32;
    let half = half as u32;
    let level = (rising * rising * BREATH_PEAK / (half * half)) as u8;
    RGB8::new(level, level, level)
}
