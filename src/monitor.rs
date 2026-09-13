use crate::Status;
use serde::Serialize;
use std::time::Duration;

#[derive(Debug, Serialize, PartialEq)]
pub struct Rates {
    pub sent: f64,
    pub received: f64,
}

#[derive(Default)]
pub struct Monitor {
    previous: Option<(Status, Duration)>,
}

impl Monitor {
    pub fn reset(&mut self) {
        self.previous = None;
    }

    pub fn observe(&mut self, status: &Status, now: Duration, max_gap: Duration) -> Option<Rates> {
        let rates = self.previous.as_ref().and_then(|(previous, time)| {
            calculate(previous, status, now.checked_sub(*time)?, max_gap)
        });
        self.previous = (status.state == "connected").then(|| (status.clone(), now));
        rates
    }
}

pub fn counter(value: &str) -> Option<u64> {
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    value.parse().ok()
}

fn duration(value: &str) -> Option<u64> {
    let mut parts = value.split(':');
    let hours = counter(parts.next()?)?;
    let minutes = counter(parts.next()?)?;
    let seconds = counter(parts.next()?)?;
    if parts.next().is_some() || minutes >= 60 || seconds >= 60 {
        return None;
    }
    hours
        .checked_mul(3600)?
        .checked_add(minutes * 60)?
        .checked_add(seconds)
}

fn calculate(
    previous: &Status,
    current: &Status,
    elapsed: Duration,
    max_gap: Duration,
) -> Option<Rates> {
    if previous.state != "connected"
        || current.state != "connected"
        || previous.name != current.name
        || previous.interface != current.interface
        || previous.ip != current.ip
        || elapsed.is_zero()
        || elapsed > max_gap
    {
        return None;
    }
    if let (Some(before), Some(after)) = (duration(&previous.duration), duration(&current.duration))
    {
        if after < before {
            return None;
        }
    }
    // Subtract integers before converting to f64, preserving small deltas even
    // when FortiClient's lifetime counters exceed JavaScript's integer precision.
    let sent = counter(&current.sent)?.checked_sub(counter(&previous.sent)?)?;
    let received = counter(&current.received)?.checked_sub(counter(&previous.received)?)?;
    Some(Rates {
        sent: sent as f64 / elapsed.as_secs_f64(),
        received: received as f64 / elapsed.as_secs_f64(),
    })
}
