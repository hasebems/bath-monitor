use cyw43::aligned_bytes;
use cyw43_pio::{PioSpi, RM2_CLOCK_DIVIDER};
use embassy_executor::Spawner;
use embassy_net::{Config, Runner as NetRunner, Stack, StackResources};
use embassy_rp::clocks::RoscRng;
use embassy_rp::dma;
use embassy_rp::gpio::{Level, Output};
use embassy_rp::peripherals::{DMA_CH0, PIN_23, PIN_24, PIN_25, PIN_29, PIO0};
use embassy_rp::pio::Pio;
use embassy_rp::Peri;
use static_cell::StaticCell;

use crate::irqs::Irqs;

#[embassy_executor::task]
async fn cyw43_task(
    runner: cyw43::Runner<'static, cyw43::SpiBus<Output<'static>, PioSpi<'static, PIO0, 0>>>,
) -> ! {
    runner.run().await
}

#[embassy_executor::task]
async fn net_task(mut runner: NetRunner<'static, cyw43::NetDriver<'static>>) -> ! {
    runner.run().await
}

/// Brings up the cyw43 Wi-Fi chip and the embassy-net stack, but does *not*
/// join a network — that's `wifi.rs`'s job, so nothing else in the firmware
/// has to wait on Wi-Fi. Returns the `Stack` handle (usable for
/// `http_client.rs`/`status_poll.rs` from the start; DHCP completes on its own
/// once `wifi_task` gets the link up) and the cyw43 `Control` handle, which
/// `main.rs` hands to `wifi_task`.
pub async fn init(
    spawner: Spawner,
    pwr: Peri<'static, PIN_23>,
    cs: Peri<'static, PIN_25>,
    pio0: Peri<'static, PIO0>,
    dio: Peri<'static, PIN_24>,
    clk: Peri<'static, PIN_29>,
    dma_ch0: Peri<'static, DMA_CH0>,
) -> (Stack<'static>, cyw43::Control<'static>) {
    let mut rng = RoscRng;

    let fw = aligned_bytes!("../cyw43-firmware/43439A0.bin");
    let clm = aligned_bytes!("../cyw43-firmware/43439A0_clm.bin");
    // Same CYW43439 chip/nvram blob as the Pico W despite the filename;
    // embassy's own rp235x Pico 2 W example loads this identical file.
    let nvram = aligned_bytes!("../cyw43-firmware/nvram_rp2040.bin");

    let pwr = Output::new(pwr, Level::Low);
    let cs = Output::new(cs, Level::High);
    let Pio {
        mut common,
        sm0,
        irq0,
        sm1,
        sm2,
        sm3,
        ..
    } = Pio::new(pio0, Irqs);
    let spi = PioSpi::new(
        &mut common,
        sm0,
        // Pico 2 W needs a divider larger than DEFAULT_CLOCK_DIVIDER or cyw43
        // SPI communication is unreliable (embassy-rs/embassy#3960).
        RM2_CLOCK_DIVIDER,
        irq0,
        cs,
        dio,
        clk,
        dma::Channel::new(dma_ch0, Irqs),
    );
    // Do NOT let the rest of this `Pio` drop when `init` returns.
    // embassy-rp 0.10.0 (still so upstream) keeps the PIO "users" count and
    // "used pins" set in a `static` inside a default trait method, which Rust
    // shares between PIO0, PIO1 and PIO2. Dropping a `Common`/`StateMachine`
    // therefore decrements one counter for all three blocks, and when it
    // reaches zero `on_pio_drop` resets *every* recorded pin to NULL — which
    // silently disconnected the NeoPixel pin (GPIO14) from PIO1 right after
    // this function returned. Leaking the unused parts keeps that from ever
    // firing (`led.rs` and `audio.rs` do the same for their unused SMs).
    core::mem::forget((common, sm1, sm2, sm3));

    static STATE: StaticCell<cyw43::State> = StaticCell::new();
    let state = STATE.init(cyw43::State::new());
    let (net_device, mut control, runner) = cyw43::new(state, pwr, spi, fw, nvram).await;
    spawner.spawn(cyw43_task(runner).unwrap());

    control.init(clm).await;
    control
        .set_power_management(cyw43::PowerManagementMode::PowerSave)
        .await;

    let seed = rng.next_u64();

    static RESOURCES: StaticCell<StackResources<4>> = StaticCell::new();
    let (stack, runner) = embassy_net::new(
        net_device,
        Config::dhcpv4(Default::default()),
        RESOURCES.init(StackResources::new()),
        seed,
    );
    spawner.spawn(net_task(runner).unwrap());

    log::info!("network stack ready (not yet joined to Wi-Fi)");

    (stack, control)
}
