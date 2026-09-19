//! What still needs to reach the server, kept as plain state (not a queue)
//! so button/occupancy tasks can record it without ever blocking, whether or
//! not Wi-Fi is up (see "Wi-Fi接続状態に依存しない動作" in
//! `docs/additional_spec.md`). `http_client.rs`'s `sender_task` drains it
//! whenever `wifi::is_connected()`.
//!
//! - **Presses**: one sequence counter per person, bumped on every press,
//!   plus the last value the server acknowledged. `seq != acked` means
//!   "pressed and not yet delivered". A press that lands while an earlier
//!   one is still in flight bumps the counter again, so it's never lost;
//!   repeated presses of the same person collapse into one pending send.
//!   The server treats same-day presses as idempotent, so re-sending is safe.
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

static PRESS_SEQ: [AtomicU32; NUM_PEOPLE] = [const { AtomicU32::new(0) }; NUM_PEOPLE];
static PRESS_ACKED: [AtomicU32; NUM_PEOPLE] = [const { AtomicU32::new(0) }; NUM_PEOPLE];

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
    Press { person_idx: usize, seq: u32 },
    Occupancy { occupied: bool },
}

pub fn notify() {
    WORK.signal(());
}

/// Records a press. Never blocks, regardless of Wi-Fi/server state.
pub fn mark_press(person_idx: usize) {
    PRESS_SEQ[person_idx].fetch_add(1, Ordering::AcqRel);
    notify();
}

/// Records the latest debounced occupancy. Never blocks.
pub fn set_occupancy(occupied: bool) {
    let v = if occupied { OCC_OCCUPIED } else { OCC_FREE };
    OCC_CURRENT.store(v, Ordering::Release);
    notify();
}

/// Whether `person_idx` pressed but the server hasn't acknowledged it yet
/// (including while that press's POST is currently in flight).
pub fn press_pending(person_idx: usize) -> bool {
    PRESS_SEQ[person_idx].load(Ordering::Acquire) != PRESS_ACKED[person_idx].load(Ordering::Acquire)
}

pub fn pending_presses() -> [bool; NUM_PEOPLE] {
    core::array::from_fn(press_pending)
}

/// The next thing to deliver, if any. Does not consume it — call `complete`
/// once the server has answered, otherwise the same item comes back.
pub fn next() -> Option<Outgoing> {
    for person_idx in 0..NUM_PEOPLE {
        let seq = PRESS_SEQ[person_idx].load(Ordering::Acquire);
        if seq != PRESS_ACKED[person_idx].load(Ordering::Acquire) {
            return Some(Outgoing::Press { person_idx, seq });
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

/// Marks `item` as no longer pending. For a press this acknowledges exactly
/// the sequence number that was sent, so a press that arrived meanwhile
/// stays pending.
pub fn complete(item: &Outgoing) {
    match *item {
        Outgoing::Press { person_idx, seq } => {
            PRESS_ACKED[person_idx].store(seq, Ordering::Release);
        }
        Outgoing::Occupancy { occupied } => {
            let v = if occupied { OCC_OCCUPIED } else { OCC_FREE };
            OCC_LAST_SENT.store(v, Ordering::Release);
        }
    }
}
