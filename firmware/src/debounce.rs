use embassy_rp::gpio::{Input, Level};
use embassy_time::{Duration, Timer};

pub struct Debouncer<'a> {
    input: Input<'a>,
    debounce: Duration,
}

impl<'a> Debouncer<'a> {
    pub fn new(input: Input<'a>, debounce: Duration) -> Self {
        Self { input, debounce }
    }

    /// Waits for a level change that is still stable after `debounce`, and
    /// returns the new stable level. Chatter that reverts before the
    /// debounce window elapses is silently discarded.
    pub async fn debounce(&mut self) -> Level {
        loop {
            let before = self.input.get_level();
            self.input.wait_for_any_edge().await;
            Timer::after(self.debounce).await;
            let after = self.input.get_level();
            if before != after {
                break after;
            }
        }
    }
}
