// SPDX-FileCopyrightText: 2025-2026 Lasse Heitgres
// SPDX-License-Identifier: GPL-3.0-or-later
// Additional terms: see NOTICE

use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

/// Calls `write`, on a thread of its own, with the newest value offered, at most once every
/// `every`: the first at once, and the last of a burst when its window ends, so the last value
/// offered is always written. The thread ends once the returned offer is dropped.
pub fn newest_every<T: Send + 'static>(
    every: Duration,
    mut write: impl FnMut(T) + Send + 'static,
) -> impl Fn(T) + Send + Sync {
    let slot = Arc::new(Mutex::new(None::<T>));
    let (wake, woken) = mpsc::sync_channel::<()>(1);
    let taken = slot.clone();
    std::thread::spawn(move || {
        while woken.recv().is_ok() {
            let Some(value) = taken.lock().unwrap().take() else {
                continue;
            };
            write(value);
            std::thread::sleep(every);
        }
    });
    move |value| {
        *slot.lock().unwrap() = Some(value);
        let _ = wake.try_send(());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    const EVERY: Duration = Duration::from_millis(200);

    /// Each value written, with when, after the recorder was made.
    type Written = Arc<Mutex<Vec<(u32, Duration)>>>;

    fn recorder() -> (Written, impl FnMut(u32) + Send + 'static, Instant) {
        let written = Arc::new(Mutex::new(Vec::new()));
        let into = written.clone();
        let t0 = Instant::now();
        (
            written,
            move |v| into.lock().unwrap().push((v, t0.elapsed())),
            t0,
        )
    }

    #[test]
    fn the_first_is_written_at_once_and_the_last_of_a_burst_when_the_window_ends() {
        let (written, write, _) = recorder();
        let offer = newest_every(EVERY, write);
        offer(1);
        std::thread::sleep(EVERY / 4);
        for v in 2..=5 {
            offer(v);
        }
        std::thread::sleep(EVERY * 2);
        let written = written.lock().unwrap().clone();
        assert_eq!(written.iter().map(|(v, _)| *v).collect::<Vec<_>>(), [1, 5]);
        assert!(
            written[0].1 < EVERY / 2,
            "the first waited {:?}",
            written[0].1
        );
        assert!(
            written[1].1 >= EVERY && written[1].1 < EVERY * 3 / 2,
            "the last came at {:?}",
            written[1].1
        );
    }

    #[test]
    fn a_value_after_a_quiet_window_is_written_at_once() {
        let (written, write, t0) = recorder();
        let offer = newest_every(EVERY, write);
        offer(1);
        std::thread::sleep(EVERY * 3 / 2);
        let at = t0.elapsed();
        offer(2);
        std::thread::sleep(EVERY / 2);
        let written = written.lock().unwrap().clone();
        assert_eq!(written.iter().map(|(v, _)| *v).collect::<Vec<_>>(), [1, 2]);
        assert!(
            written[1].1 - at < EVERY / 2,
            "the second waited {:?}",
            written[1].1 - at
        );
    }

    #[test]
    fn a_value_offered_just_before_the_offer_is_dropped_is_still_written() {
        let (written, write, _) = recorder();
        let offer = newest_every(EVERY, write);
        offer(1);
        std::thread::sleep(EVERY / 4);
        offer(2);
        drop(offer);
        std::thread::sleep(EVERY * 2);
        assert_eq!(
            written
                .lock()
                .unwrap()
                .iter()
                .map(|(v, _)| *v)
                .collect::<Vec<_>>(),
            [1, 2]
        );
    }
}
