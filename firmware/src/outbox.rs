//! What still needs to reach the server, kept as plain state (not a queue)
//! so button/occupancy tasks can record it without ever blocking, whether or
//! not Wi-Fi is up (see "Wi-Fi接続状態に依存しない動作" in
//! `docs/additional_spec.md`). `http_client.rs`'s `sender_task` drains it
//! whenever `wifi::is_connected()`.
//!
//! - **Presses and cancels**: one state word per person — a sequence counter
//!   bumped on every press or cancel (long-press), with a "was a cancel" flag
//!   in the low bit — plus the last word the server acknowledged. `state !=
//!   acked` means "something owed and not yet delivered", and the flag says
//!   which: only the *latest* action per person is ever sent (a press
//!   followed by a cancel collapses into a single cancel). An action that
//!   lands while an earlier one is still in flight changes the word again, so
//!   it's never lost. The server treats both as idempotent, so re-sending is
//!   safe.
//! - **Occupancy**: the latest debounced value, plus the last value the
//!   server acknowledged. Only the *current* value is ever sent, and only
//!   when it differs from the last delivered one — changes that happened
//!   (and reverted) while offline are intentionally not replayed.

use core::sync::atomic::{AtomicU32, AtomicU8, Ordering};

use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::signal::Signal;

use crate::config::NUM_PEOPLE;

/// Signaled whenever there may be something new to send (a press/occupancy
/// change, or Wi-Fi just came up) so `sender_task` re-checks the state below.
pub static WORK: Signal<CriticalSectionRawMutex, ()> = Signal::new();

/// `(counter << 1) | is_cancel`; 0 means nothing has happened yet.
static PRESS_STATE: [AtomicU32; NUM_PEOPLE] = [const { AtomicU32::new(0) }; NUM_PEOPLE];
static PRESS_ACKED: [AtomicU32; NUM_PEOPLE] = [const { AtomicU32::new(0) }; NUM_PEOPLE];

const CANCEL_FLAG: u32 = 1;

const OCC_UNKNOWN: u8 = 0;
const OCC_FREE: u8 = 1;
const OCC_OCCUPIED: u8 = 2;

/// `OCC_UNKNOWN` until the first debounced change after boot — nothing is
/// sent before that, same as before this module existed.
static OCC_CURRENT: AtomicU8 = AtomicU8::new(OCC_UNKNOWN);
/// `OCC_UNKNOWN` until the first successful delivery after boot.
static OCC_LAST_SENT: AtomicU8 = AtomicU8::new(OCC_UNKNOWN);

/// One thing `sender_task` should try to deliver next.
pub enum Outgoing {
    /// `state` is the person's state word being delivered (see `PRESS_STATE`).
    Press { person_idx: usize, state: u32 },
    Cancel { person_idx: usize, state: u32 },
    Occupancy { occupied: bool },
}

pub fn notify() {
    WORK.signal(());
}

/// Records a press. Never blocks, regardless of Wi-Fi/server state.
pub fn mark_press(person_idx: usize) {
    record(person_idx, false);
}

/// Records a cancel (long-press), superseding any press not yet delivered.
/// Never blocks, regardless of Wi-Fi/server state.
pub fn mark_cancel(person_idx: usize) {
    record(person_idx, true);
}

fn record(person_idx: usize, cancel: bool) {
    let flag = if cancel { CANCEL_FLAG } else { 0 };
    let _ = PRESS_STATE[person_idx].fetch_update(Ordering::AcqRel, Ordering::Acquire, |v| {
        Some(((v >> 1).wrapping_add(1) << 1) | flag)
    });
    notify();
}

/// Records the latest debounced occupancy. Never blocks.
pub fn set_occupancy(occupied: bool) {
    let v = if occupied { OCC_OCCUPIED } else { OCC_FREE };
    OCC_CURRENT.store(v, Ordering::Release);
    notify();
}

/// What `person_idx` did last that the server hasn't acknowledged yet
/// (including while that POST is currently in flight): `Some(true)` for a
/// press, `Some(false)` for a cancel, `None` if nothing is owed.
pub fn pending_intent(person_idx: usize) -> Option<bool> {
    let state = PRESS_STATE[person_idx].load(Ordering::Acquire);
    (state != PRESS_ACKED[person_idx].load(Ordering::Acquire)).then_some(state & CANCEL_FLAG == 0)
}

pub fn pending_intents() -> [Option<bool>; NUM_PEOPLE] {
    core::array::from_fn(pending_intent)
}

/// The next thing to deliver, if any. Does not consume it — call `complete`
/// once the server has answered, otherwise the same item comes back.
pub fn next() -> Option<Outgoing> {
    for person_idx in 0..NUM_PEOPLE {
        let state = PRESS_STATE[person_idx].load(Ordering::Acquire);
        if state != PRESS_ACKED[person_idx].load(Ordering::Acquire) {
            return Some(if state & CANCEL_FLAG == 0 {
                Outgoing::Press { person_idx, state }
            } else {
                Outgoing::Cancel { person_idx, state }
            });
        }
    }

    let current = OCC_CURRENT.load(Ordering::Acquire);
    if current != OCC_UNKNOWN && current != OCC_LAST_SENT.load(Ordering::Acquire) {
        return Some(Outgoing::Occupancy {
            occupied: current == OCC_OCCUPIED,
        });
    }

    None
}

/// Marks `item` as no longer pending. For a press or cancel this acknowledges
/// exactly the state word that was sent, so an action that arrived meanwhile
/// stays pending.
pub fn complete(item: &Outgoing) {
    match *item {
        Outgoing::Press { person_idx, state } | Outgoing::Cancel { person_idx, state } => {
            PRESS_ACKED[person_idx].store(state, Ordering::Release);
        }
        Outgoing::Occupancy { occupied } => {
            let v = if occupied { OCC_OCCUPIED } else { OCC_FREE };
            OCC_LAST_SENT.store(v, Ordering::Release);
        }
    }
}
