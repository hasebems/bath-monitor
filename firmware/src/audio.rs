//! Owns the I2S link to the MAX98357A amp (`config::AUDIO_*` pins), running
//! entirely on RP2350's second core (CORE1) so the real-time DMA-feeding
//! loop can never be delayed by CORE0's networking/LED/button tasks.
//!
//! `WAVEFORM_BUFFER` is a fixed-size sample buffer shared across cores:
//! `core1_task` continuously copies its current contents out and DMAs them
//! over I2S in a loop, forever, regardless of what's in it. `waveform.rs`'s
//! `waveform_task` (also spawned on CORE1, see `start`) is what writes new
//! sample data into `WAVEFORM_BUFFER`, paced by `BUFFER_CONSUMED` below.

use core::cell::RefCell;

use embassy_rp::executor::Executor;
use embassy_rp::multicore::{spawn_core1, Stack};
use embassy_rp::peripherals::{CORE1, DMA_CH3, PIN_16, PIN_17, PIN_18, PIO2};
use embassy_rp::pio::Pio;
use embassy_rp::pio_programs::i2s::{PioI2sOut, PioI2sOutProgram};
use embassy_rp::Peri;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::blocking_mutex::Mutex;
use embassy_sync::signal::Signal;
use static_cell::StaticCell;

use crate::config::{AUDIO_BIT_DEPTH, AUDIO_BUFFER_SAMPLES, AUDIO_SAMPLE_RATE_HZ};
use crate::irqs::Irqs;
use crate::music::music_task;
use crate::waveform::waveform_task;

/// Shared I2S output buffer. Each `u32` DMA word packs one sample into both
/// the left and right channel slots (see `PioI2sOut`/`PioI2sOutProgram`).
/// A `CriticalSectionRawMutex` is required (not a plain `RefCell`) since
/// this is read on CORE1 and, eventually, written from CORE0 — the
/// `critical-section-impl` backend uses a hardware SIO spinlock, which is
/// the only thing that makes that safe across cores.
pub static WAVEFORM_BUFFER: Mutex<CriticalSectionRawMutex, RefCell<[u32; AUDIO_BUFFER_SAMPLES]>> =
    Mutex::new(RefCell::new([0; AUDIO_BUFFER_SAMPLES]));

/// Signaled by `core1_task` each time it finishes DMAing a chunk out over
/// I2S. `waveform.rs`'s `waveform_task` waits on this before synthesizing
/// the next chunk, so it stays paced 1:1 with actual playback instead of
/// racing ahead (which would silently drop a chunk that's never played) or
/// falling behind (which would repeat one) — see `docs/additional_spec.md`.
pub static BUFFER_CONSUMED: Signal<CriticalSectionRawMutex, ()> = Signal::new();

/// CORE1's stack, sized generously since the whole embassy executor for
/// that core (task + PIO/DMA driver state) runs on it.
static CORE1_STACK: StaticCell<Stack<8192>> = StaticCell::new();
static CORE1_EXECUTOR: StaticCell<Executor> = StaticCell::new();

/// Boots CORE1 and hands it the I2S peripherals. Called once from `main.rs`
/// on CORE0; never returns to the caller beyond spawning CORE1.
pub fn start(
    core1: Peri<'static, CORE1>,
    pio2: Peri<'static, PIO2>,
    dma_ch3: Peri<'static, DMA_CH3>,
    bclk: Peri<'static, PIN_16>,
    lrclk: Peri<'static, PIN_17>,
    din: Peri<'static, PIN_18>,
) {
    let stack = CORE1_STACK.init(Stack::new());
    spawn_core1(core1, stack, move || {
        let executor = CORE1_EXECUTOR.init(Executor::new());
        executor.run(|spawner| {
            spawner
                .spawn(core1_task(pio2, dma_ch3, bclk, lrclk, din).unwrap());
            spawner.spawn(waveform_task().unwrap());
            spawner.spawn(music_task().unwrap());
        });
    });
}

/// Runs on CORE1: owns the I2S link (PIO2 + DMA_CH3) and continuously DMAs
/// `WAVEFORM_BUFFER`'s current contents out over I2S, looping forever. Each
/// iteration copies the shared buffer into a local, unshared array first so
/// `waveform_task` can update `WAVEFORM_BUFFER` at any time without racing
/// the DMA transfer already in flight, then signals `BUFFER_CONSUMED` right
/// after the transfer completes so `waveform_task` knows it's time to
/// synthesize the next chunk.
#[embassy_executor::task]
async fn core1_task(
    pio2: Peri<'static, PIO2>,
    dma_ch3: Peri<'static, DMA_CH3>,
    bclk: Peri<'static, PIN_16>,
    lrclk: Peri<'static, PIN_17>,
    din: Peri<'static, PIN_18>,
) {
    let Pio {
        mut common,
        sm0,
        sm1,
        sm2,
        sm3,
        ..
    } = Pio::new(pio2, Irqs);
    // Never drop the unused state machines: see `net.rs` (embassy-rp's PIO
    // drop bookkeeping is shared by all PIO blocks and would blank pins).
    core::mem::forget((sm1, sm2, sm3));
    let program = PioI2sOutProgram::new(&mut common);
    let mut i2s = PioI2sOut::new(
        &mut common,
        sm0,
        dma_ch3,
        Irqs,
        din,
        bclk,
        lrclk,
        AUDIO_SAMPLE_RATE_HZ,
        AUDIO_BIT_DEPTH,
        &program,
    );
    i2s.start();

    let mut out_buf = [0u32; AUDIO_BUFFER_SAMPLES];
    loop {
        WAVEFORM_BUFFER.lock(|buf| out_buf.copy_from_slice(&buf.borrow()[..]));
        i2s.write(&out_buf).await;
        BUFFER_CONSUMED.signal(());
    }
}
