//! Wi-Fi connection manager: owns cyw43's `Control`, joins the network,
//! re-joins when the link drops, and publishes the result as
//! `is_connected()` for every task that talks to the server. Nothing else in
//! the firmware waits on Wi-Fi to start — see "Wi-Fi接続状態に依存しない動作"
//! in `docs/additional_spec.md`.
//!
//! This task also drives the Pico 2 W's onboard LED (see "オンボードLEDの
//! ハートビート点滅" there): it hangs off the CYW43439's GPIO0 (`WL_GPIO0`),
//! not an RP2350 GPIO, so it's only reachable through `Control`, which
//! `join` also needs `&mut` access to. The LED blinks whenever this task is
//! between `join` calls (backing off, or watching the link), and is held on
//! for the duration of a `join` since that call has no timeout and can't be
//! interleaved with anything else.

use core::sync::atomic::{AtomicBool, Ordering};

use cyw43::{Control, JoinOptions};
use embassy_net::Stack;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::signal::Signal;
use embassy_time::{Duration, Instant, Timer};

use crate::config::{ONBOARD_LED_BLINK_HALF_PERIOD_MS, WIFI_LINK_UP_GRACE_SECS};
use crate::outbox;
use crate::secrets::{SERVER_BASE_URL, WIFI_PASSWORD, WIFI_SSID};

/// `WL_GPIO0` on the CYW43439, which drives the Pico 2 W's onboard LED.
const ONBOARD_LED_WL_GPIO: u8 = 0;

static CONNECTED: AtomicBool = AtomicBool::new(false);

/// Signaled each time Wi-Fi goes from not-connected to connected, so
/// `status_poll_task` can re-sync the NeoPixels right away instead of
/// waiting out its polling interval. (`outbox::WORK` is signaled too, for
/// `sender_task`.)
pub static WIFI_UP: Signal<CriticalSectionRawMutex, ()> = Signal::new();

/// Whether the network is usable right now: link up *and* DHCP done (a
/// successful `join` alone doesn't count). Tasks that talk to the server
/// must not send anything while this is false.
pub fn is_connected() -> bool {
    CONNECTED.load(Ordering::Acquire)
}

fn set_connected(now: bool) {
    let was = CONNECTED.swap(now, Ordering::AcqRel);
    if now && !was {
        log::info!("wifi connected, server base url: {}", SERVER_BASE_URL);
        WIFI_UP.signal(());
        outbox::notify();
    } else if !now && was {
        log::warn!("wifi disconnected");
    }
}

/// Tracks the onboard LED's current state so it can be toggled.
struct Blinker {
    on: bool,
}

impl Blinker {
    async fn hold_on(&mut self, control: &mut Control<'static>) {
        self.on = true;
        control.gpio_set(ONBOARD_LED_WL_GPIO, true).await;
    }

    /// Toggles the LED, then waits one half-period.
    async fn tick(&mut self, control: &mut Control<'static>) {
        self.on = !self.on;
        control.gpio_set(ONBOARD_LED_WL_GPIO, self.on).await;
        Timer::after(Duration::from_millis(ONBOARD_LED_BLINK_HALF_PERIOD_MS)).await;
    }

    async fn blink_for(&mut self, control: &mut Control<'static>, total: Duration) {
        let end = Instant::now() + total;
        while Instant::now() < end {
            self.tick(control).await;
        }
    }
}

/// Watches an established association, blinking the LED and keeping
/// `is_connected()` current, until the link is lost.
async fn monitor(stack: Stack<'static>, control: &mut Control<'static>, led: &mut Blinker) {
    let joined_at = Instant::now();
    let grace = Duration::from_secs(WIFI_LINK_UP_GRACE_SECS);
    // `join` can return a moment before the link is reported up, so a
    // not-yet-up link only means "lost" once it has been up, or the grace
    // period has run out.
    let mut seen_link_up = false;

    loop {
        let link_up = stack.is_link_up();
        seen_link_up |= link_up;
        set_connected(link_up && stack.is_config_up());

        if !link_up && (seen_link_up || joined_at.elapsed() > grace) {
            return;
        }
        led.tick(control).await;
    }
}

/// Joins the configured network (retrying with exponential backoff on
/// failure, so a transient AP outage recovers without a power-cycle) and
/// re-joins whenever the link is lost. Never returns.
#[embassy_executor::task]
pub async fn wifi_task(stack: Stack<'static>, mut control: Control<'static>) -> ! {
    const MIN_BACKOFF: Duration = Duration::from_secs(1);
    const MAX_BACKOFF: Duration = Duration::from_secs(30);

    let mut led = Blinker { on: true };
    let mut backoff = MIN_BACKOFF;

    loop {
        set_connected(false);
        led.hold_on(&mut control).await;

        match control
            .join(WIFI_SSID, JoinOptions::new(WIFI_PASSWORD.as_bytes()))
            .await
        {
            Ok(()) => {
                backoff = MIN_BACKOFF;
                monitor(stack, &mut control, &mut led).await;
                set_connected(false);
            }
            Err(err) => {
                log::warn!("wifi join failed: {:?}, retrying in {:?}", err, backoff);
                led.blink_for(&mut control, backoff).await;
                backoff = core::cmp::min(backoff * 2, MAX_BACKOFF);
            }
        }
    }
}
