#![no_std]
#![no_main]

mod audio;
mod buttons;
mod config;
mod debounce;
mod http_client;
mod irqs;
mod led;
mod led_pattern;
mod music;
mod net;
mod occupancy;
mod outbox;
mod secrets;
mod status_poll;
mod waveform;
mod wifi;

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
    let p: embassy_rp::Peripherals = embassy_rp::init(Default::default());

    spawner.spawn(logger_task(Driver::new(p.USB, Irqs)).unwrap());
    // Wait for the USB serial host to connect.
    Timer::after(Duration::from_secs(1)).await;
    log::info!("bath-monitor firmware starting");

    // Start the local hardware first so it works without Wi-Fi.
    spawner.spawn(led::led_task(p.PIO1, p.DMA_CH2, p.PIN_15).unwrap());
    audio::start(p.CORE1, p.PIO2, p.DMA_CH3, p.PIN_16, p.PIN_17, p.PIN_18);
    spawner.spawn(occupancy::occupancy_task(p.PIN_26.into()).unwrap());

    // One button task per person.
    spawner.spawn(buttons::button_task(p.PIN_2.into(), 0).unwrap());
    spawner.spawn(buttons::button_task(p.PIN_3.into(), 1).unwrap());
    spawner.spawn(buttons::button_task(p.PIN_4.into(), 2).unwrap());
    spawner.spawn(buttons::button_task(p.PIN_5.into(), 3).unwrap());
    spawner.spawn(buttons::button_task(p.PIN_6.into(), 4).unwrap());

    // Bring up Wi-Fi and start the network tasks.
    let (stack, control) = net::init(
        spawner, p.PIN_23, p.PIN_25, p.PIO0, p.PIN_24, p.PIN_29, p.DMA_CH0,
    )
    .await;

    spawner.spawn(wifi::wifi_task(stack, control).unwrap());
    spawner.spawn(http_client::sender_task(stack).unwrap());
    spawner.spawn(status_poll::status_poll_task(stack).unwrap());

    log::info!("all tasks spawned");
}
