#![no_std]
#![no_main]

mod audio;
mod buttons;
mod config;
mod debounce;
mod events;
mod http_client;
mod irqs;
mod led;
mod music;
mod net;
mod occupancy;
mod secrets;
mod status_poll;
mod waveform;

use embassy_executor::Spawner;
use embassy_rp::peripherals::USB;
use embassy_rp::usb::Driver;
use embassy_time::{Duration, Timer};
use panic_halt as _;

use crate::irqs::Irqs;

#[embassy_executor::task]
async fn logger_task(driver: Driver<'static, USB>) {
    embassy_usb_logger::run!(1024, log::LevelFilter::Info, driver);
}

#[embassy_executor::main(
    executor = "embassy_rp::executor::Executor",
    entry = "cortex_m_rt::entry"
)]
async fn main(spawner: Spawner) {
    let p = embassy_rp::init(Default::default());

    spawner.spawn(logger_task(Driver::new(p.USB, Irqs)).unwrap());
    // Give the USB serial host a moment to enumerate before the first logs.
    Timer::after(Duration::from_secs(1)).await;
    log::info!("bath-monitor firmware starting");

    let stack = net::init(
        spawner, p.PIN_23, p.PIN_25, p.PIO0, p.PIN_24, p.PIN_29, p.DMA_CH0,
    )
    .await;

    spawner.spawn(http_client::sender_task(stack).unwrap());
    spawner.spawn(status_poll::status_poll_task(stack).unwrap());
    spawner.spawn(led::led_task(p.PIO1, p.DMA_CH2, p.PIN_15).unwrap());
    audio::start(p.CORE1, p.PIO2, p.DMA_CH3, p.PIN_16, p.PIN_17, p.PIN_18);
    spawner.spawn(occupancy::occupancy_task(p.PIN_26.into()).unwrap());

    // Hardcoded pin-to-person mapping — must stay in sync with
    // config::BUTTON_PINS/PEOPLE and docs/wiring.md (PIO/GPIO field access
    // can't be driven dynamically from the config array at runtime).
    spawner.spawn(buttons::button_task(p.PIN_2.into(), 0).unwrap());
    spawner.spawn(buttons::button_task(p.PIN_3.into(), 1).unwrap());
    spawner.spawn(buttons::button_task(p.PIN_4.into(), 2).unwrap());
    spawner.spawn(buttons::button_task(p.PIN_5.into(), 3).unwrap());
    spawner.spawn(buttons::button_task(p.PIN_6.into(), 4).unwrap());

    log::info!("all tasks spawned");
}
