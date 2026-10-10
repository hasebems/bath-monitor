//! Everything about what the NeoPixels show, kept apart from how they're
//! driven (`led.rs`'s SK6812RGBW output and fixed-rate frame loop, which only
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
use smart_leds::{White, RGBW};

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
///
/// Returns whether this press made the person the last one of the day: they
/// hadn't pressed yet and now everyone has (checked under the same lock as the
/// write, so two buttons released together can't both, or neither, count as
/// last). Re-pressing once everyone has pressed doesn't count. This board's
/// state is all it goes by — before the first `/api/led-state` sync after boot,
/// the others count as not pressed.
pub fn set_pressed(person_idx: usize, pressed: bool) -> bool {
    let mut completed = false;
    update(|state| {
        let was_pressed = state.pressed[person_idx];
        state.pressed[person_idx] = pressed;
        completed = pressed && !was_pressed && state.pressed.iter().all(|&p| p);
    });
    completed
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
pub fn frame(now_ms: u64) -> [RGBW<u8>; NUM_PEOPLE] {
    let state = STATE.lock(|cell| cell.get());
    render(&state.pressed, state.occupied, now_ms)
}

/// One SK6812RGBW pixel: the red, green and blue LEDs plus the separate
/// white LED (`w`).
const fn rgbw(r: u8, g: u8, b: u8, w: u8) -> RGBW<u8> {
    RGBW { r, g, b, a: White(w) }
}

const OFF: RGBW<u8> = rgbw(0, 0, 0, 0);

/// The color each person lights up in once they've pressed today, in
/// `config::PEOPLE` order (grandpa, grandma, father, mother, son). Kept dim
/// to limit current draw across 5 LEDs on a single 5V rail, and none is
/// whitish (the white LED stays off) so it can't be mistaken for the "not
/// yet" breathing below.
const PRESSED_COLORS: [RGBW<u8>; NUM_PEOPLE] = [
    rgbw(0, 40, 0, 0),  // grandpa: green
    rgbw(40, 0, 20, 0), // grandma: pink
    rgbw(40, 16, 0, 0), // father: orange
    rgbw(50, 0, 0, 0),  // mother: red
    rgbw(0, 0, 48, 0),  // son: blue
];

/// While the bath is occupied, people who haven't pressed yet breathe in
/// white (the SK6812RGBW's own white LED, not R+G+B mixed): one full dark →
/// bright → dark cycle takes this long.
const BREATH_PERIOD_MS: u64 = 1000;
/// Brightest value of the white LED at the top of the breath.
const BREATH_PEAK: u32 = 16;
/// Dimmest value at the bottom of the breath. Well above 0: the LED only has
/// whole levels, and at the very bottom one level is a big jump to the eye
/// (off → 1 looked like the LED suddenly going out, 1 → 2 doubles the light),
/// so the breath stays where the steps are small enough to look smooth.
/// (Dithering between levels over time was tried instead and flickered
/// visibly at this dimness, so it isn't used.)
const BREATH_FLOOR: u32 = 4;
const _: () = assert!(BREATH_FLOOR < BREATH_PEAK && BREATH_PEAK <= 255);

/// The colors for one frame, one per person in `config::PEOPLE` order.
/// `now_ms` is any monotonic clock in milliseconds (the breathing phase is
/// taken from it, so it stays regular however often frames are drawn).
/// `occupied` is the debounced state of the occupancy light sensor.
///
/// Current look: people who pressed today stay lit steadily in their own
/// `PRESSED_COLORS` entry, whether or not the bath is occupied (it's the
/// day's record). The rest breathe in white between `BREATH_FLOOR` and
/// `BREATH_PEAK` with a `BREATH_PERIOD_MS` period, all in step — but only
/// while `occupied`; otherwise they're off.
fn render(pressed: &[bool; NUM_PEOPLE], occupied: bool, now_ms: u64) -> [RGBW<u8>; NUM_PEOPLE] {
    let breath = breath_color(now_ms);
    core::array::from_fn(|person_idx| {
        if pressed[person_idx] {
            PRESSED_COLORS[person_idx]
        } else if occupied {
            breath
        } else {
            OFF
        }
    })
}

/// White at the breath's brightness for `now_ms`: a triangle wave (up for half
/// the period, down for the other half), squared so the perceived brightness
/// changes evenly — the LED output is linear, but the eye isn't — and running
/// from `BREATH_FLOOR` to `BREATH_PEAK`.
fn breath_color(now_ms: u64) -> RGBW<u8> {
    let half = BREATH_PERIOD_MS / 2;
    let phase = now_ms % BREATH_PERIOD_MS;
    let rising = if phase < half {
        phase
    } else {
        BREATH_PERIOD_MS - phase
    };
    let span = u64::from(BREATH_PEAK - BREATH_FLOOR);
    let level = BREATH_FLOOR + (rising * rising * span / (half * half)) as u32;
    rgbw(0, 0, 0, level as u8)
}
