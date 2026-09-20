use embassy_rp::gpio::{AnyPin, Input, Level, Pull};
use embassy_rp::Peri;
use embassy_time::{with_timeout, Duration};

use crate::config::{BUTTON_DEBOUNCE_MS, BUTTON_LONG_PRESS_MS};
use crate::debounce::Debouncer;
use crate::led_pattern;
use crate::music::{CANCEL_MELODY, MELODY_CHANNEL};
use crate::outbox;

/// One instance per button (pool_size must match `config::NUM_PEOPLE`).
/// Pull-up input, button wired to GND: a press reads as `Level::Low`.
///
/// Releasing within `BUTTON_LONG_PRESS_MS` of the press registers it (LED,
/// melody, `POST /api/press`); holding longer instead cancels that person's
/// press for today (LED off, cancel sound, `POST /api/cancel`) — and plays
/// no press melody, so the two never sound alike.
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

        // Released within the long-press time: a plain press. (`debounce` is
        // cancel-safe: it only keeps locals across awaits. The level check
        // covers the release landing right at the deadline.)
        let released = with_timeout(
            Duration::from_millis(BUTTON_LONG_PRESS_MS),
            debouncer.debounce(),
        )
        .await;
        if released.is_ok() || debouncer.level() != Level::Low {
            log::info!("button {} pressed", person_idx);
            // Just records that the server owes a delivery: never blocks and
            // doesn't care whether Wi-Fi is up (`sender_task` delivers it, and
            // keeps retrying, once it can). The LED and melody below must never
            // wait on anything network-related.
            outbox::mark_press(person_idx);
            led_pattern::set_pressed(person_idx, true);
            MELODY_CHANNEL.send(person_idx).await;
            continue;
        }

        log::info!("button {} long-pressed: cancelling", person_idx);
        outbox::mark_cancel(person_idx);
        led_pattern::set_pressed(person_idx, false);
        MELODY_CHANNEL.send(CANCEL_MELODY).await;

        // Swallow the rest of this hold so the release isn't taken for a new press.
        while debouncer.level() == Level::Low {
            debouncer.debounce().await;
        }
    }
}
