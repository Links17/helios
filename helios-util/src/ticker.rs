use std::time::Duration;

use tokio::time::{self, Instant, Interval, MissedTickBehavior};

pub fn interval(period: Duration) -> Interval {
    configure(time::interval(period))
}

pub fn delayed_interval(period: Duration) -> Interval {
    configure(time::interval_at(Instant::now() + period, period))
}

fn configure(mut interval: Interval) -> Interval {
    interval.set_missed_tick_behavior(MissedTickBehavior::Skip);
    interval
}
