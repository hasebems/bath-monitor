use embassy_rp::gpio::{AnyPin, Input, Level, Pull};
use embassy_rp::Peri;
use embassy_time::{with_timeout, Duration};

use crate::config::{BUTTON_DEBOUNCE_MS, BUTTON_LONG_PRESS_MS};
use crate::debounce::Debouncer;
use crate::events::{LedEvent, CANCEL_MELODY, LED_CHANNEL, MELODY_CHANNEL};
use crate::outbox;

/// One instance per button (pool_size must match `config::NUM_PEOPLE`).
/// Pull-up input, button wired to GND: a press reads as `Level::Low`.
///
/// A press registers immediately (instant LED/melody feedback); if the button
/// is then still held after `BUTTON_LONG_PRESS_MS`, that person's press for
/// today is cancelled again (LED off, cancel sound, `POST /api/cancel`).
#[embassy_executor::task(pool_size = 5)]
pub async fn button_task(pin: Peri<'static, AnyPin>, person_idx: usize) {
    let mut debouncer = Debouncer::new(
        Input::new(pin, Pull::Up),
        Duration::from_millis(BUTTON_DEBOUNCE_MS),
    );

    loop {
        let level = debouncer.debounce().await;
        if level != Level::Low {
            continue;
        }

        log::info!("button {} pressed", person_idx);
        // Just records that the server owes a delivery: never blocks and
        // doesn't care whether Wi-Fi is up (`sender_task` delivers it, and
        // keeps retrying, once it can). The LED and melody below must never
        // wait on anything network-related.
        outbox::mark_press(person_idx);
        LED_CHANNEL.send(LedEvent::Pressed { person_idx }).await;
        MELODY_CHANNEL.send(person_idx).await;

        // Released within the long-press time: a plain press, nothing more.
        // (`debounce` is cancel-safe: it only keeps locals across awaits. The
        // level check covers the release landing right at the deadline.)
        let released = with_timeout(
            Duration::from_millis(BUTTON_LONG_PRESS_MS),
            debouncer.debounce(),
        )
        .await;
        if released.is_ok() || debouncer.level() != Level::Low {
            continue;
        }

        log::info!("button {} long-pressed: cancelling", person_idx);
        outbox::mark_cancel(person_idx);
        LED_CHANNEL.send(LedEvent::Cancelled { person_idx }).await;
        MELODY_CHANNEL.send(CANCEL_MELODY).await;

        // Swallow the rest of this hold so the release isn't taken for a new press.
        while debouncer.level() == Level::Low {
            debouncer.debounce().await;
        }
    }
}
