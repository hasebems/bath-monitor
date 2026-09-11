use embassy_rp::gpio::{AnyPin, Input, Level, Pull};
use embassy_rp::Peri;
use embassy_time::Duration;

use crate::config::BUTTON_DEBOUNCE_MS;
use crate::debounce::Debouncer;
use crate::events::{AppEvent, AudioEvent, LedEvent, AUDIO_CHANNEL, EVENT_CHANNEL, LED_CHANNEL};

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
            EVENT_CHANNEL
                .send(AppEvent::ButtonPressed { person_idx })
                .await;
            LED_CHANNEL.send(LedEvent::Pressed { person_idx }).await;
            AUDIO_CHANNEL.send(AudioEvent::Play).await;
        }
    }
}
