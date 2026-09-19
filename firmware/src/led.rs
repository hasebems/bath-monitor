use embassy_rp::peripherals::{DMA_CH2, PIN_15, PIO1};
use embassy_rp::pio::Pio;
use embassy_rp::pio_programs::ws2812::{PioWs2812, PioWs2812Program};
use embassy_rp::Peri;
use embassy_time::{with_deadline, Duration, Instant};

use crate::config::{NEOPIXEL_FRAME_INTERVAL_MS, NUM_PEOPLE};
use crate::events::{LedEvent, LED_CHANNEL};
use crate::irqs::Irqs;
use crate::led_pattern;

/// Owns the WS2812 NeoPixel chain (one LED per person, same order as
/// `config::PEOPLE`) and drives it at a fixed frame rate
/// (`NEOPIXEL_FRAME_INTERVAL_MS`, 10 fps), writing every frame whether or not
/// anything changed. Between frames it only folds `LED_CHANNEL` events into
/// the per-person pressed state (`Pressed`/`Cancelled` from the buttons,
/// `Sync` from `status_poll.rs` — which is what turns LEDs off again after
/// the server's daily reset), so a change shows up within one frame.
///
/// What each frame looks like is decided entirely by `led_pattern::render`;
/// this task knows nothing about colors or blinking.
///
/// `pin` must be `config::NEOPIXEL_PIN` (PIO requires a concrete pin type,
/// so this can't be type-erased/data-driven like the button pins).
#[embassy_executor::task]
pub async fn led_task(
    pio1: Peri<'static, PIO1>,
    dma_ch2: Peri<'static, DMA_CH2>,
    pin: Peri<'static, PIN_15>,
) {
    let Pio {
        mut common,
        sm0,
        sm1,
        sm2,
        sm3,
        ..
    } = Pio::new(pio1, Irqs);
    // Never drop the unused state machines: see `net.rs` (embassy-rp's PIO
    // drop bookkeeping is shared by all PIO blocks and would blank this pin).
    core::mem::forget((sm1, sm2, sm3));
    let program = PioWs2812Program::new(&mut common);
    let mut ws2812 = PioWs2812::new(&mut common, sm0, dma_ch2, Irqs, pin, &program);

    let frame_interval = Duration::from_millis(NEOPIXEL_FRAME_INTERVAL_MS);
    let mut pressed = [false; NUM_PEOPLE];
    let mut next_frame = Instant::now();

    loop {
        ws2812
            .write(&led_pattern::render(&pressed, Instant::now().as_millis()))
            .await;

        // Fixed cadence (no drift from the write time); if we ever fall
        // behind, restart from now rather than catching up with a burst.
        next_frame = (next_frame + frame_interval).max(Instant::now());

        while let Ok(event) = with_deadline(next_frame, LED_CHANNEL.receive()).await {
            match event {
                LedEvent::Pressed { person_idx } => pressed[person_idx] = true,
                LedEvent::Cancelled { person_idx } => pressed[person_idx] = false,
                LedEvent::Sync { pressed: synced } => pressed = synced,
            }
        }
    }
}
