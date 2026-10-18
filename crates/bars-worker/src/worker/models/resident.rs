//! Модель и время завершения работы под одним замком; занятый слот не выгружается.
use std::sync::Mutex;
use std::time::{Duration, Instant};

pub(super) struct Resident<M> {
    pub model: Option<M>,
    pub last_used: Instant,
}

impl<M> Default for Resident<M> {
    fn default() -> Self {
        Self {
            model: None,
            last_used: Instant::now(),
        }
    }
}

/// Возвращает модель для drop уже после освобождения замка. Status сюда не обращается.
pub(super) fn take_idle<M>(
    slot: &Mutex<Resident<M>>,
    now: Instant,
    ttl: Duration,
    on_unload: impl FnOnce(),
) -> Option<M> {
    let mut guard = match slot.try_lock() {
        Ok(guard) => guard,
        Err(std::sync::TryLockError::Poisoned(error)) => error.into_inner(),
        Err(std::sync::TryLockError::WouldBlock) => return None,
    };
    if now.saturating_duration_since(guard.last_used) < ttl {
        return None;
    }
    let model = guard.model.take();
    if model.is_some() {
        on_unload();
    }
    model
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn busy_slot_and_recent_completion_survive_reaping() {
        let now = Instant::now();
        let slot = Mutex::new(Resident {
            model: Some(7),
            last_used: now,
        });
        let guard = slot.lock().unwrap();
        assert_eq!(
            take_idle(
                &slot,
                now + Duration::from_secs(100),
                Duration::ZERO,
                || panic!("busy")
            ),
            None
        );
        drop(guard);
        assert_eq!(
            take_idle(
                &slot,
                now + Duration::from_secs(9),
                Duration::from_secs(10),
                || panic!("recent")
            ),
            None
        );
        assert_eq!(
            take_idle(
                &slot,
                now + Duration::from_secs(10),
                Duration::from_secs(10),
                || {}
            ),
            Some(7)
        );
        assert_eq!(
            take_idle(&slot, now, Duration::ZERO, || panic!("empty")),
            None
        );
    }

    #[test]
    fn expensive_drop_can_reenter_slot_without_deadlock() {
        use std::sync::{
            Arc, Weak,
            atomic::{AtomicBool, Ordering},
        };
        struct Probe {
            slot: Weak<Mutex<Resident<Probe>>>,
            dropped: Arc<AtomicBool>,
        }
        impl Drop for Probe {
            fn drop(&mut self) {
                assert!(self.slot.upgrade().unwrap().try_lock().is_ok());
                self.dropped.store(true, Ordering::SeqCst);
            }
        }
        let slot = Arc::new(Mutex::new(Resident::default()));
        let dropped = Arc::new(AtomicBool::new(false));
        slot.lock().unwrap().model = Some(Probe {
            slot: Arc::downgrade(&slot),
            dropped: dropped.clone(),
        });
        let retired = take_idle(&slot, Instant::now(), Duration::ZERO, || {});
        assert!(!dropped.load(Ordering::SeqCst));
        drop(retired);
        assert!(dropped.load(Ordering::SeqCst));
    }
}
