use crate::conditional_const;
use crate::sync::{AtomicU64, Ordering};
use std::time::Duration;

const DEFAULT_SYNC_THRESHOLD: Duration = Duration::from_nanos(2000000000);
const DEFAULT_RETIERED_THRESHOLD: isize = 1000;
const DEFAULT_HAZARD_POINTER_MULTIPLIER: isize = 2;

#[derive(Debug)]
pub enum ReclaimStrategy {
    Eager,
    TimedCapped(TimeCappedSettings),
}

impl ReclaimStrategy {
    pub(super) fn should_reclaim(&self, hazard_pointer_count: isize, retired_count: isize) -> bool {
        match self {
            Self::Eager => true,
            Self::TimedCapped(settings) => {
                settings.should_reclaim(hazard_pointer_count, retired_count)
            }
        }
    }

    conditional_const!(
        pub,
        fn default() -> Self {
            Self::TimedCapped(TimeCappedSettings::default())
        }
    );
}

#[derive(Debug)]
pub struct TimeCappedSettings {
    last_sync_time: AtomicU64,
    sync_timeout: Duration,
    hazard_pointer_multiplier: isize,
    retired_threshold: isize,
}

impl TimeCappedSettings {
    conditional_const!(
        pub,
        fn new(
            sync_timeout: Duration,
            retired_threshold: isize,
            hazard_pointer_multiplier: isize,
        ) -> Self {
            Self {
                last_sync_time: AtomicU64::new(0),
                sync_timeout,
                retired_threshold,
                hazard_pointer_multiplier,
            }
        }
    );

    fn should_reclaim(&self, hazard_pointer_count: isize, retired_count: isize) -> bool {
        if retired_count >= self.retired_threshold
            && retired_count >= hazard_pointer_count * self.hazard_pointer_multiplier
        {
            return true;
        }
        self.check_sync_time()
    }

    fn check_sync_time(&self) -> bool {
        use std::convert::TryFrom;
        let time = u64::try_from(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system time is set to before the epoch")
                .as_nanos(),
        )
        .expect("system time is too far into the future");
        let last_sync_time = self.last_sync_time.load(Ordering::Relaxed);

        // If it's not time to clean yet, or someone else just started cleaning, don't clean.
        time > last_sync_time
            && self
                .last_sync_time
                .compare_exchange(
                    last_sync_time,
                    time + self.sync_timeout.as_nanos() as u64,
                    Ordering::Relaxed,
                    Ordering::Relaxed,
                )
                .is_ok()
    }

    conditional_const!(
        pub(self),
        fn default() -> Self {
            Self::new(
                DEFAULT_SYNC_THRESHOLD,
                DEFAULT_RETIERED_THRESHOLD,
                DEFAULT_HAZARD_POINTER_MULTIPLIER,
            )
        }
    );
}
