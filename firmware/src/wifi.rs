//! Wi-Fi connection manager: owns cyw43's `Control`, joins the network,
//! re-joins when the link drops, and publishes the result as
//! `is_connected()` for every task that talks to the server. Nothing else in
//! the firmware waits on Wi-Fi to start — see "Wi-Fi接続状態に依存しない動作"
//! in `docs/additional_spec.md`.
//!
//! This task also drives the Pico 2 W's onboard LED (see "オンボードLEDの
//! ハートビート点滅" there): it hangs off the CYW43439's GPIO0 (`WL_GPIO0`),
//! not an RP2350 GPIO, so it's only reachable through `Control`, which
//! `join` also needs `&mut` access to. The LED shows the join outcome:
//!
//! - held on while a `join` call is in progress (it can't be interleaved
//!   with anything else; cyw43 gives it no timeout, so this task adds one
//!   — `WIFI_JOIN_TIMEOUT_SECS` — and counts running over as a failure);
//! - 1Hz blink once a `join` has succeeded (while watching the link);
//! - "ピピ" — two short flashes, then a gap, repeated — after a `join`
//!   failed, until the next attempt starts.

use core::sync::atomic::{AtomicBool, Ordering};

use cyw43::{Control, JoinOptions};
use embassy_net::Stack;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::signal::Signal;
use embassy_time::{with_timeout, Duration, Instant, Timer};

use crate::config::{
    ONBOARD_LED_BLINK_HALF_PERIOD_MS, ONBOARD_LED_FAIL_FLASH_MS, ONBOARD_LED_FAIL_PATTERN_GAP_MS,
    WIFI_JOIN_TIMEOUT_SECS, WIFI_LINK_UP_GRACE_SECS,
};
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
    async fn set(&mut self, control: &mut Control<'static>, on: bool) {
        self.on = on;
        control.gpio_set(ONBOARD_LED_WL_GPIO, on).await;
    }

    async fn hold_on(&mut self, control: &mut Control<'static>) {
        self.set(control, true).await;
    }

    /// One step of the 1Hz "joined" blink: toggles the LED, then waits one
    /// half-period.
    async fn tick(&mut self, control: &mut Control<'static>) {
        let on = !self.on;
        self.set(control, on).await;
        Timer::after(Duration::from_millis(ONBOARD_LED_BLINK_HALF_PERIOD_MS)).await;
    }

    /// The "join failed" pattern — two short flashes, then a gap, repeated —
    /// for `total`, starting immediately and leaving the LED off at the end.
    async fn failure_pattern_for(&mut self, control: &mut Control<'static>, total: Duration) {
        let end = Instant::now() + total;
        let flash = Duration::from_millis(ONBOARD_LED_FAIL_FLASH_MS);
        let gap = Duration::from_millis(ONBOARD_LED_FAIL_PATTERN_GAP_MS);

        while Instant::now() < end {
            for _ in 0..2 {
                self.set(control, true).await;
                Timer::after(flash).await;
                self.set(control, false).await;
                Timer::after(flash).await;
            }
            Timer::after(core::cmp::min(
                gap,
                end.saturating_duration_since(Instant::now()),
            ))
            .await;
        }
        self.set(control, false).await;
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

        log::info!("wifi joining \"{}\"", WIFI_SSID);
        let outcome = with_timeout(
            Duration::from_secs(WIFI_JOIN_TIMEOUT_SECS),
            control.join(WIFI_SSID, JoinOptions::new(WIFI_PASSWORD.as_bytes())),
        )
        .await;

        match outcome {
            Ok(Ok(())) => {
                log::info!("wifi joined");
                backoff = MIN_BACKOFF;
                monitor(stack, &mut control, &mut led).await;
                set_connected(false);
                continue;
            }
            Ok(Err(err)) => {
                log::warn!("wifi join failed: {:?}, retrying in {:?}", err, backoff);
            }
            Err(_) => {
                log::warn!(
                    "wifi join timed out after {}s, retrying in {:?}",
                    WIFI_JOIN_TIMEOUT_SECS,
                    backoff
                );
                // The abandoned attempt may have left the chip half-associated.
                control.leave().await;
            }
        }

        led.failure_pattern_for(&mut control, backoff).await;
        backoff = core::cmp::min(backoff * 2, MAX_BACKOFF);
    }
}
