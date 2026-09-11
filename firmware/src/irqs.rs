use embassy_rp::bind_interrupts;
use embassy_rp::dma;
use embassy_rp::peripherals::{DMA_CH0, DMA_CH2, DMA_CH3, PIO0, PIO1, PIO2, USB};
use embassy_rp::pio;
use embassy_rp::usb;

// Every DMA channel type in embassy-rp is bound to DMA_IRQ_0 (channel
// routing is handled internally); there is no per-channel choice of IRQ.
bind_interrupts!(pub struct Irqs {
    PIO0_IRQ_0 => pio::InterruptHandler<PIO0>;
    PIO1_IRQ_0 => pio::InterruptHandler<PIO1>;
    PIO2_IRQ_0 => pio::InterruptHandler<PIO2>;
    DMA_IRQ_0 => dma::InterruptHandler<DMA_CH0>, dma::InterruptHandler<DMA_CH2>, dma::InterruptHandler<DMA_CH3>;
    USBCTRL_IRQ => usb::InterruptHandler<USB>;
});
