//! Move eligible archives out of the queue before payload cleanup or diagnostics.
use std::sync::Mutex;

pub(super) fn retire_expired<T>(
    archives: &Mutex<Vec<T>>,
    now_ms: u64,
    grace_ms: u64,
    handed_out_ms: impl Fn(&T) -> u64,
    mut release: impl FnMut(T),
) {
    let limit = match archives.try_lock() {
        Ok(archives) => archives.len(),
        Err(_) => return,
    };
    // Only an initial queue's worth of work; a concurrent handout need not wait for cleanup.
    for _ in 0..limit {
        let archive = {
            let Ok(mut archives) = archives.try_lock() else {
                return;
            };
            let Some(index) = archives
                .iter()
                .position(|archive| now_ms.saturating_sub(handed_out_ms(archive)) >= grace_ms)
            else {
                return;
            };
            archives.remove(index)
        };
        release(archive);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Weak,
    };

    struct Payload {
        handed_out_ms: u64,
        bytes: Vec<u8>,
        queue: Weak<Mutex<Vec<Payload>>>,
        dropped: Arc<AtomicUsize>,
    }

    impl Drop for Payload {
        fn drop(&mut self) {
            // A payload destructor must be able to acquire the native handout's queue lock.
            let queue = self.queue.upgrade().unwrap();
            assert!(
                queue.try_lock().is_ok(),
                "payload dropped with queue locked"
            );
            self.dropped.fetch_add(1, Ordering::SeqCst);
        }
    }

    #[test]
    fn grace_preserves_native_buffer_and_cleanup_allows_handout() {
        let queue = Arc::new(Mutex::new(Vec::new()));
        let dropped = Arc::new(AtomicUsize::new(0));
        let payload = |handed_out_ms| Payload {
            handed_out_ms,
            bytes: vec![7; 32],
            queue: Arc::downgrade(&queue),
            dropped: dropped.clone(),
        };
        let original = payload(1_000);
        let original_pointer = original.bytes.as_ptr();
        // Deliberately not age-sorted, with a future timestamp retained by saturating age.
        queue
            .lock()
            .unwrap()
            .extend([payload(2_000), original, payload(10_000)]);
        retire_expired(
            &queue,
            5_999,
            5_000,
            |p| p.handed_out_ms,
            |_| panic!("early"),
        );
        assert_eq!(queue.lock().unwrap()[1].bytes.as_ptr(), original_pointer);
        assert_eq!(dropped.load(Ordering::SeqCst), 0);
        {
            let _handout_guard = queue.lock().unwrap();
            retire_expired(
                &queue,
                6_000,
                5_000,
                |p| p.handed_out_ms,
                |_| panic!("busy"),
            );
        }
        retire_expired(
            &queue,
            6_000,
            5_000,
            |p| p.handed_out_ms,
            |mut archive| {
                assert_eq!(archive.bytes.as_ptr(), original_pointer);
                assert_eq!(archive.bytes, [7; 32]);
                // The actual cleanup callback can accept a new handout before poison/drop.
                queue
                    .try_lock()
                    .expect("cleanup held queue lock")
                    .push(payload(6_000));
                archive.bytes.fill(0xA5);
            },
        );
        assert_eq!(dropped.load(Ordering::SeqCst), 1);
        assert_eq!(queue.lock().unwrap().len(), 3);
        retire_expired(&queue, 15_000, 5_000, |p| p.handed_out_ms, drop);
        assert!(queue.lock().unwrap().is_empty());
        assert_eq!(dropped.load(Ordering::SeqCst), 4);
    }
}
