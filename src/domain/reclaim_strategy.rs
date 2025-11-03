use crate::macros::conditional_const;
#[cfg(feature = "std")]
use crate::sync::{AtomicU64, Ordering};
#[cfg(feature = "std")]
use core::time::Duration;

#[cfg(feature = "std")]
const DEFAULT_SYNC_THRESHOLD: Duration = Duration::from_nanos(2000000000);
const DEFAULT_RETIERED_THRESHOLD: isize = 1000;
const DEFAULT_HAZARD_POINTER_MULTIPLIER: isize = 2;

/// The strategy which should be used for reclaiming retired items in a `Domain`.
///
/// A `default` const constructor function is defined for this enum. It cannot implement `Default`
/// since we would like the `default` constructor to be a const function.
#[derive(Debug)]
#[non_exhaustive]
pub enum ReclaimStrategy {
    /// Every time an item is retired the domain will try to reclaim any items which are not
    /// currently being protected by a hazard pointer.
    Eager,

    /// Items will be reclaimed both periodically, and when the number of retired items exceeds
    /// certain thresholds.
    TimedCapped(TimedCappedSettings),

    /// Memory reclamation will only happen when the `reclaim` method on [`crate::domain::Domain`]
    /// is called.
    Manual,
}

impl ReclaimStrategy {
    pub(super) fn should_reclaim(&self, hazard_pointer_count: isize, retired_count: isize) -> bool {
        match self {
            Self::Eager => true,
            Self::TimedCapped(settings) => {
                settings.should_reclaim(hazard_pointer_count, retired_count)
            }
            Self::Manual => false,
        }
    }

    conditional_const!(
        /// Creates the default reclamation strategy for a domain
        pub fn default() -> Self {
            Self::TimedCapped(TimedCappedSettings::default())
        }
    );
}

/// The particulate settings of the `TimedCapped` reclamation strategy.
///
/// # Example
///
/// ```
/// use atom_box::domain::{ReclaimStrategy, TimedCappedSettings};
/// use core::time::Duration;
///
/// const RECLAIM_STRATEGY: ReclaimStrategy = ReclaimStrategy::TimedCapped(
///     #[cfg(feature = "std")]
///     TimedCappedSettings::default()
///         .with_timeout(Duration::from_nanos(5000000000))
///         .with_retired_threshold(1000)
///         .with_hazard_pointer_multiplier(3),
///     #[cfg(not(feature = "std"))]
///     TimedCappedSettings::default()
///         .with_retired_threshold(1000)
///         .with_hazard_pointer_multiplier(3),
/// );
#[derive(Debug)]
pub struct TimedCappedSettings {
    #[cfg(feature = "std")]
    last_sync_time: AtomicU64,
    #[cfg(feature = "std")]
    sync_timeout: Duration,
    hazard_pointer_multiplier: isize,
    retired_threshold: isize,
}

impl TimedCappedSettings {
    #[cfg(feature = "std")]
    conditional_const!(
        /// Creates a new `TimedCappedSettings`.
        ///
        /// # Arguments
        ///
        /// * `sync_timeout` - The duration between successive reclaim attempts
        /// * `retired_threshold` - The threshold after which a retired items should be reclaimed
        /// * `hazard_pointer_multiplier` - If the number of retired items exceeds the number of
        ///   hazard pointers multiplied by `hazard_pointer_multiplier` then an attempt will be made
        ///   to reclaim the retired items.
        ///
        /// # Example
        ///
        /// ```
        /// # use core::time::Duration;
        /// use atom_box::domain::{ReclaimStrategy, TimedCappedSettings};
        ///
        /// const RECLAIM_STRATEGY: ReclaimStrategy = ReclaimStrategy::TimedCapped(
        ///     TimedCappedSettings::new_with_timeout(Duration::from_nanos(5000000000), 1000, 3),
        /// );
        /// ```
        pub fn new_with_timeout(
            sync_timeout: Duration,
            retired_threshold: isize,
            hazard_pointer_multiplier: isize,
        ) -> Self {
            Self {
                #[cfg(feature = "std")]
                last_sync_time: AtomicU64::new(0),
                #[cfg(feature = "std")]
                sync_timeout,
                retired_threshold,
                hazard_pointer_multiplier,
            }
        }
    );

    conditional_const!(
        /// Creates a new `TimedCappedSettings`.
        ///
        /// # Arguments
        ///
        /// * `retired_threshold` - The threshold after which a retired items should be reclaimed
        /// * 'hazard_pointer_multiplier` - If the number of retired items exceeds the number of
        ///   hazard pointers multiplied by `hazard_pointer_multiplier` then an attempt will be
        ///   made to reclaim the retired items.
        ///
        /// # Example
        ///
        /// ```
        /// use atom_box::domain::{ReclaimStrategy, TimedCappedSettings};
        ///
        /// const RECLAIM_STRATEGY: ReclaimStrategy =
        ///     ReclaimStrategy::TimedCapped(TimedCappedSettings::new(1000, 3));
        /// ```
        pub fn new(retired_threshold: isize, hazard_pointer_multiplier: isize) -> Self {
            Self {
                #[cfg(feature = "std")]
                last_sync_time: AtomicU64::new(0),
                #[cfg(feature = "std")]
                sync_timeout: DEFAULT_SYNC_THRESHOLD,
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

    #[cfg(feature = "std")]
    fn check_sync_time(&self) -> bool {
        use core::convert::TryFrom;
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

    #[cfg(not(feature = "std"))]
    #[inline(always)]
    fn check_sync_time(&self) -> bool {
        true
    }

    conditional_const!(
        /// Creates the default `TimedCappedSettings`.
        ///
        /// This is not an implementation of `Default` since it is a const function.
        pub fn default() -> Self {
            Self::new(DEFAULT_RETIRED_THRESHOLD, DEFAULT_HAZARD_POINTER_MULTIPLIER)
        }
    );

    #[cfg(feature = "std")]
    /// Set the timeout after which a reclamation should be attempted.
    ///
    /// If the time between the previous reclaimation and now exceeds this threshold, an attempt
    /// will be made to reclaim the retired items.
    pub const fn with_timeout(self, sync_timeout: Duration) -> Self {
        Self {
            sync_timeout,
            ..self
        }
    }

    /// Set the hazard pointer multiplier.
    ///
    /// If the number of retired items exceeds the number of hazard pointers multiplied by
    /// `hazard_pointer_multiplier` then an attempt will be made to reclaim the retired items.
    pub const fn with_hazard_pointer_multiplier(self, hazard_pointer_multiplier: isize) -> Self {
        Self {
            hazard_pointer_multiplier,
            ..self
        }
    }

    /// Sets the retired threshold.
    ///
    /// If the number of retired items exceeds this threshold an attempt will be made to reclaim
    /// the retired items.
    pub const fn with_retired_threshold(self, retired_threshold: isize) -> Self {
        Self {
            retired_threshold,
            ..self
        }
    }
}
