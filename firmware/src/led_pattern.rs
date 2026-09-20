//! Everything about what the NeoPixels show, kept apart from how they're
//! driven (`led.rs`'s WS2812 output and fixed-rate frame loop, which only
//! writes the colors this module hands it): the state that decides the look
//! (who has pressed, whether the bath is occupied) and the look itself
//! (`render`, a pure function of that state and the time, so it needs no
//! hardware). To change the look — colors, breathing speed, ... — or what it
//! depends on, edit only this file.
//!
//! The state is plain shared data, not a queue of events (like `outbox.rs`):
//! whoever learns something new just overwrites it with the `set_*` functions
//! below — never blocking, and the last write wins — and `frame` reads
//! whatever it is at the next frame.

use core::cell::Cell;

use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::blocking_mutex::Mutex;
use smart_leds::RGB8;

use crate::config::NUM_PEOPLE;

/// What the look depends on.
#[derive(Clone, Copy)]
struct LedState {
    pressed: [bool; NUM_PEOPLE],
    occupied: bool,
}

/// Nobody has pressed, and the bath counts as unoccupied until
/// `occupancy_task` reports its first reading (right at boot).
static STATE: Mutex<CriticalSectionRawMutex, Cell<LedState>> = Mutex::new(Cell::new(LedState {
    pressed: [false; NUM_PEOPLE],
    occupied: false,
}));

fn update(change: impl FnOnce(&mut LedState)) {
    STATE.lock(|cell| {
        let mut state = cell.get();
        change(&mut state);
        cell.set(state);
    });
}

/// One person pressed (`true`, immediate local feedback when the button is
/// released) or cancelled with a long press (`false`).
pub fn set_pressed(person_idx: usize, pressed: bool) {
    update(|state| state.pressed[person_idx] = pressed);
}

/// Everyone's pressed state at once, from the server's `/api/led-state`
/// (which is also what clears the LEDs after the server's daily reset).
pub fn set_all_pressed(pressed: [bool; NUM_PEOPLE]) {
    update(|state| state.pressed = pressed);
}

/// The occupancy light sensor's debounced state (set once at boot from the
/// initial reading, then on every change). Decides whether the "not yet
/// pressed" pattern is shown at all.
pub fn set_occupied(occupied: bool) {
    update(|state| state.occupied = occupied);
}

/// The colors to write for the frame at `now_ms` (any monotonic clock in
/// milliseconds), one per person in `config::PEOPLE` order, from the state as
/// it is right now, so a change shows up in the very next frame.
pub fn frame(now_ms: u64) -> [RGB8; NUM_PEOPLE] {
    let state = STATE.lock(|cell| cell.get());
    render(&state.pressed, state.occupied, now_ms)
}

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

/// While the bath is occupied, people who haven't pressed yet breathe in
/// white: one full dark → bright → dark cycle takes this long.
const BREATH_PERIOD_MS: u64 = 6000;
/// Brightest value of each channel at the top of the breath.
const BREATH_PEAK: u32 = 40;
/// How much later each LED's breath runs than the previous one's (in chain
/// order), so the light appears to travel along the chain from LED 0 towards
/// the last one. 0 makes all LEDs breathe in step; to flow the other way
/// round, index the LEDs from the end in `render`.
const BREATH_PHASE_STEP_MS: u64 = 600;
// The whole chain's delay must stay within one period (`render` relies on it
// to keep its subtraction from underflowing).
const _: () = assert!((NUM_PEOPLE as u64 - 1) * BREATH_PHASE_STEP_MS <= BREATH_PERIOD_MS);

/// The colors for one frame, one per person in `config::PEOPLE` order.
/// `now_ms` is any monotonic clock in milliseconds (the breathing phase is
/// taken from it, so it stays regular however often frames are drawn).
/// `occupied` is the debounced state of the occupancy light sensor.
///
/// Current look: people who pressed today stay lit steadily in their own
/// `PRESSED_COLORS` entry, whether or not the bath is occupied (it's the
/// day's record). The rest breathe in white with a `BREATH_PERIOD_MS`
/// period, each LED `BREATH_PHASE_STEP_MS` behind the one before it — but
/// only while `occupied`; otherwise they're off.
fn render(pressed: &[bool; NUM_PEOPLE], occupied: bool, now_ms: u64) -> [RGB8; NUM_PEOPLE] {
    core::array::from_fn(|person_idx| {
        if pressed[person_idx] {
            PRESSED_COLORS[person_idx]
        } else if occupied {
            // Delaying by `delay` means sampling the breath at `now - delay`;
            // adding a whole period first keeps that from underflowing.
            let delay = person_idx as u64 * BREATH_PHASE_STEP_MS;
            breath_color(now_ms + BREATH_PERIOD_MS - delay)
        } else {
            RGB8::new(0, 0, 0)
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
