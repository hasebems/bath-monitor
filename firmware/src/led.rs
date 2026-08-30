use embassy_rp::peripherals::{DMA_CH2, PIN_15, PIO1};
use embassy_rp::pio::Pio;
use embassy_rp::pio_programs::ws2812::{PioWs2812, PioWs2812Program};
use embassy_rp::Peri;
use smart_leds::RGB8;

use crate::config::NUM_PEOPLE;
use crate::events::{LedEvent, LED_CHANNEL};
use crate::irqs::Irqs;

/// Dim green — bright enough to read, low enough to keep current draw
/// modest across 5 LEDs on a single 5V rail.
const PRESSED_COLOR: RGB8 = RGB8::new(0, 40, 0);
const OFF: RGB8 = RGB8::new(0, 0, 0);

/// Owns the WS2812 NeoPixel chain (one LED per person, same order as
/// `config::PEOPLE`). Lights a pixel immediately on `LedEvent::Pressed` for
/// instant feedback, and reconciles the whole chain on `LedEvent::Sync`
/// (from `status_poll.rs`), which is what turns LEDs off again after the
/// server's daily reset.
/// `pin` must be `config::NEOPIXEL_PIN` (PIO requires a concrete pin type,
/// so this can't be type-erased/data-driven like the button pins).
#[embassy_executor::task]
pub async fn led_task(pio1: Peri<'static, PIO1>, dma_ch2: Peri<'static, DMA_CH2>, pin: Peri<'static, PIN_15>) {
    let Pio { mut common, sm0, .. } = Pio::new(pio1, Irqs);
    let program = PioWs2812Program::new(&mut common);
    let mut ws2812 = PioWs2812::new(&mut common, sm0, dma_ch2, Irqs, pin, &program);

    let mut data = [OFF; NUM_PEOPLE];
    ws2812.write(&data).await;

    loop {
        match LED_CHANNEL.receive().await {
            LedEvent::Pressed { person_idx } => {
                data[person_idx] = PRESSED_COLOR;
            }
            LedEvent::Sync { pressed } => {
                for (slot, &is_pressed) in data.iter_mut().zip(pressed.iter()) {
                    *slot = if is_pressed { PRESSED_COLOR } else { OFF };
                }
            }
        }
        ws2812.write(&data).await;
    }
}
