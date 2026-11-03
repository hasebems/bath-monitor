use embassy_rp::gpio::{AnyPin, Input, Level, Pull};
use embassy_rp::Peri;
use embassy_time::{with_timeout, Duration};

use crate::config::{BUTTON_DEBOUNCE_MS, BUTTON_LONG_PRESS_MS};
use crate::debounce::Debouncer;
use crate::led_pattern;
use crate::music::{CANCEL_MELODY, FINAL_MELODY, MELODY_CHANNEL};
use crate::outbox;

/// Watches one person's button (pressed = `Level::Low`).
/// A short press registers the press; a long press cancels it.
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

        // Wait for the release, up to the long-press time.
        let released = with_timeout(
            Duration::from_millis(BUTTON_LONG_PRESS_MS),
            debouncer.debounce(),
        )
        .await;
        if released.is_ok() || debouncer.level() != Level::Low {
            log::info!("button {} pressed", person_idx);
            // Short press: queue the POST, light the LED, play the melody.
            outbox::mark_press(person_idx);
            let last = led_pattern::set_pressed(person_idx, true);
            if last {
                log::info!("button {}: everyone has pressed", person_idx);
            }
            MELODY_CHANNEL
                .send(if last { FINAL_MELODY } else { person_idx })
                .await;
            continue;
        }

        // Long press: queue the cancel, turn the LED off, play the cancel sound.
        log::info!("button {} long-pressed: cancelling", person_idx);
        outbox::mark_cancel(person_idx);
        led_pattern::set_pressed(person_idx, false);
        MELODY_CHANNEL.send(CANCEL_MELODY).await;

        // Wait for the release.
        while debouncer.level() == Level::Low {
            debouncer.debounce().await;
        }
    }
}
