use parking_lot::Mutex;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::Instant;

const OVERLOAD_BURST: Duration = Duration::from_millis(500);
const RECOVERY_WINDOW: Duration = Duration::from_secs(2);
const RECOVERY_SPACING: Duration = Duration::from_millis(100);

#[derive(Default)]
struct State {
    last_overload: Option<Instant>,
    window: Option<Window>,
}

struct Window {
    next: Instant,
    until: Instant,
}

/// Shares overload feedback across background subtree attempts, including
/// retries, on one capability. Interactive folders and outfit reads have
/// their own queue and do not wait here.
pub(crate) struct SubtreePacer {
    cap: String,
    state: Mutex<State>,
}

impl SubtreePacer {
    pub(crate) fn for_cap(slot: &mut Option<Arc<Self>>, cap: &str) -> Arc<Self> {
        if let Some(current) = slot.as_ref().filter(|current| current.cap == cap) {
            return current.clone();
        }
        let pacer = Arc::new(Self {
            cap: cap.to_owned(),
            state: Mutex::default(),
        });
        *slot = Some(pacer.clone());
        pacer
    }

    pub(super) async fn wait(&self) {
        while let Some(next) = self.admit(Instant::now()) {
            tokio::time::sleep_until(next).await;
        }
    }

    fn admit(&self, now: Instant) -> Option<Instant> {
        let mut state = self.state.lock();
        let window = state.window.as_mut()?;
        if now >= window.until {
            state.window = None;
            return None;
        }
        if now < window.next {
            return Some(window.next);
        }
        window.next = now + RECOVERY_SPACING;
        None
    }

    pub(super) fn overloaded(&self, retry_after: Option<Duration>) {
        self.overloaded_at(Instant::now(), retry_after);
    }

    fn overloaded_at(&self, now: Instant, retry_after: Option<Duration>) {
        let mut state = self.state.lock();
        let burst = state
            .last_overload
            .replace(now)
            .is_some_and(|previous| now.saturating_duration_since(previous) <= OVERLOAD_BURST);
        let delay = retry_after.unwrap_or_default().min(Duration::from_secs(60));
        let next = now + delay;
        if let Some(window) = state.window.as_mut().filter(|window| window.until > now) {
            window.next = window.next.max(next);
            window.until = window.until.max(next + RECOVERY_WINDOW);
        } else if burst || !delay.is_zero() {
            state.window = Some(Window {
                next,
                until: next + RECOVERY_WINDOW,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subtree_pacing_spreads_a_rejection_burst_then_recovers() {
        let pace = SubtreePacer::for_cap(&mut None, "a");
        let start = Instant::now();
        assert_eq!(pace.admit(start), None);
        assert_eq!(pace.admit(start), None);
        pace.overloaded_at(start, None);
        assert_eq!(pace.admit(start), None, "an isolated rejection only backs off its own request");
        let burst = start + Duration::from_millis(40);
        pace.overloaded_at(burst, None);
        assert_eq!(pace.admit(burst), None);
        assert_eq!(pace.admit(burst), Some(burst + RECOVERY_SPACING));
        assert_eq!(pace.admit(burst + RECOVERY_SPACING), None);
        assert_eq!(pace.admit(burst + RECOVERY_WINDOW), None);
        assert_eq!(pace.admit(burst + RECOVERY_WINDOW), None);
    }

    #[test]
    fn subtree_pacing_respects_retry_after_and_is_scoped_to_the_current_capability() {
        let mut slot = None;
        let old = SubtreePacer::for_cap(&mut slot, "old");
        let now = Instant::now();
        old.overloaded_at(now, Some(Duration::from_secs(5)));
        assert_eq!(old.admit(now), Some(now + Duration::from_secs(5)));
        old.overloaded_at(now + Duration::from_millis(10), None);
        assert_eq!(old.admit(now + Duration::from_secs(1)), Some(now + Duration::from_secs(5)));
        assert!(Arc::ptr_eq(&old, &SubtreePacer::for_cap(&mut slot, "old")));
        let changed = SubtreePacer::for_cap(&mut slot, "new");
        assert_eq!(changed.admit(now), None);
        assert_eq!(old.admit(now + Duration::from_secs(5)), None);
        assert_eq!(old.admit(now + Duration::from_secs(5)), Some(now + Duration::from_millis(5100)));
    }
}
