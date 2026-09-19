use embassy_rp::gpio::{AnyPin, Input, Level, Pull};
use embassy_rp::Peri;
use embassy_time::Duration;

use crate::config::BUTTON_DEBOUNCE_MS;
use crate::debounce::Debouncer;
use crate::events::{LedEvent, LED_CHANNEL, MELODY_CHANNEL};
use crate::outbox;

/// One instance per button (pool_size must match `config::NUM_PEOPLE`).
/// Pull-up input, button wired to GND: a press reads as `Level::Low`.
#[embassy_executor::task(pool_size = 5)]
pub async fn button_task(pin: Peri<'static, AnyPin>, person_idx: usize) {
    let mut debouncer = Debouncer::new(
        Input::new(pin, Pull::Up),
        Duration::from_millis(BUTTON_DEBOUNCE_MS),
    );

    loop {
        let level = debouncer.debounce().await;
        if level == Level::Low {
            log::info!("button {} pressed", person_idx);
            // Just records that the server owes a delivery: never blocks and
            // doesn't care whether Wi-Fi is up (`sender_task` delivers it, and
            // keeps retrying, once it can). The LED and melody below must never
            // wait on anything network-related.
            outbox::mark_press(person_idx);
            LED_CHANNEL.send(LedEvent::Pressed { person_idx }).await;
            MELODY_CHANNEL.send(person_idx).await;
        }
    }
}
