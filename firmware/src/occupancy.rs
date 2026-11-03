use embassy_rp::gpio::{AnyPin, Input, Level, Pull};
use embassy_rp::Peri;
use embassy_time::Duration;

use crate::config::OCCUPANCY_DEBOUNCE_MS;
use crate::debounce::Debouncer;
use crate::led_pattern;
use crate::outbox;

/// Watches the light sensor (occupied = `Level::Low`).
/// Each debounced change goes to the LEDs and the outbox;
/// the reading at boot goes to the LEDs only.
#[embassy_executor::task]
pub async fn occupancy_task(pin: Peri<'static, AnyPin>) {
    let mut debouncer = Debouncer::new(
        Input::new(pin, Pull::Up),
        Duration::from_millis(OCCUPANCY_DEBOUNCE_MS),
    );

    let occupied = debouncer.level() == Level::Low;
    log::info!("occupancy at boot: occupied={}", occupied);
    led_pattern::set_occupied(occupied);

    loop {
        let level = debouncer.debounce().await;
        let occupied = level == Level::Low;
        log::info!("occupancy changed: occupied={}", occupied);
        led_pattern::set_occupied(occupied);
        outbox::set_occupancy(occupied);
    }
}
