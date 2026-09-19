use embassy_rp::gpio::{AnyPin, Input, Level, Pull};
use embassy_rp::Peri;
use embassy_time::Duration;

use crate::config::OCCUPANCY_DEBOUNCE_MS;
use crate::debounce::Debouncer;
use crate::outbox;

/// Light sensor wired to read `Level::Low` when the bath is occupied
/// (adjust the `Level::Low` comparison below if the sensor board's polarity
/// differs). Debounced with a longer window than buttons to reject chatter
/// near the light threshold.
#[embassy_executor::task]
pub async fn occupancy_task(pin: Peri<'static, AnyPin>) {
    let mut debouncer = Debouncer::new(
        Input::new(pin, Pull::Up),
        Duration::from_millis(OCCUPANCY_DEBOUNCE_MS),
    );

    loop {
        let level = debouncer.debounce().await;
        let occupied = level == Level::Low;
        log::info!("occupancy changed: occupied={}", occupied);
        outbox::set_occupancy(occupied);
    }
}
