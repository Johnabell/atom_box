use crate::macros::conditional_const;
use crate::sync::{AtomicU64, Ordering};
use std::time::Duration;

const DEFAULT_SYNC_THRESHOLD: Duration = Duration::from_nanos(2000000000);
const DEFAULT_RETIERED_THRESHOLD: isize = 1000;
const DEFAULT_HAZARD_POINTER_MULTIPLIER: isize = 2;

/// The strategy which should be used for reclaiming retired items in a `Domain`.
///
/// A `default` const constructor function is defined for this enum. It cannot implement `Default`
/// since we would like the `default` constructor to be a const function.
#[derive(Debug)]
pub enum ReclaimStrategy {
    /// Every time an item is retired the domain will try to reclaim any items which are not
    /// currently being protected by a hazard pointer.
    Eager,

    /// Items will be reclaimed both periodically, and when the number of retired items exceeds
    /// certain threasholds.
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
        "Creates the default reclaimation strategy for a domain",
        pub,
        fn default() -> Self {
            Self::TimedCapped(TimeCappedSettings::default())
        }
    );
}

/// The particulate settings of the `TimeCapped` reclaimation strategy.
#[derive(Debug)]
pub struct TimeCappedSettings {
    last_sync_time: AtomicU64,
    sync_timeout: Duration,
    hazard_pointer_multiplier: isize,
    retired_threshold: isize,
}

impl TimeCappedSettings {
    conditional_const!(
        "Creates a new `TimeCappedSettings`.

# Arguments

* `sync_timeout` - The duration between successive reclaim attempts
* `retired_threshold` - The threshold after which a retired items should be reclaimed
* 'hazard_pointer_multiplier` - If the number of retired items exceeds the number of hazard
pointers multiplied by `hazard_pointer_multiplier` then an atempt will be made to reclaim
the retired items.

# Example

```
use atom_box::domain::{ReclaimStrategy, TimeCappedSettings};

const RECLAIM_STRATEGY: ReclaimStrategy = ReclaimStrategy::TimedCapped(TimeCappedSettings::new(
    std::time::Duration::from_nanos(5000000000),
    1000,
    3,
));
```
",
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
"Creates the default `TimeCappedSettings`.

This is not an implementation of `Default` since it is a const function.",
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
