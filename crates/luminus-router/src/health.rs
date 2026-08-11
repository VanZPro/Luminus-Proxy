use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use luminus_core::{
    model::AccountId,
    provider::{ProviderError, ProviderErrorCategory},
};

pub trait Clock: Send + Sync {
    fn now(&self) -> Instant;
}

#[derive(Debug, Default)]
pub struct SystemClock;
impl Clock for SystemClock {
    fn now(&self) -> Instant {
        Instant::now()
    }
}

#[derive(Clone)]
pub struct AccountHealthStore {
    states: Arc<Mutex<HashMap<AccountId, AccountRuntimeState>>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountRuntimeState {
    pub cooldown_until: Option<Instant>,
    pub last_error_category: Option<ProviderErrorCategory>,
}

impl AccountHealthStore {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn state(&self, id: &AccountId) -> Option<AccountRuntimeState> {
        self.states.lock().unwrap().get(id).cloned()
    }
    pub fn is_eligible(&self, id: &AccountId, now: Instant) -> bool {
        let mut states = self.states.lock().unwrap();
        if let Some(state) = states.get_mut(id) {
            if state.cooldown_until.is_some_and(|until| now >= until) {
                state.cooldown_until = None;
            }
            return state.cooldown_until.is_none();
        }
        true
    }
    pub fn mark_cooldown(
        &self,
        id: AccountId,
        category: ProviderErrorCategory,
        now: Instant,
        duration: Duration,
    ) {
        let mut states = self.states.lock().unwrap();
        states.insert(
            id,
            AccountRuntimeState {
                cooldown_until: Some(now + duration),
                last_error_category: Some(category),
            },
        );
    }
    pub fn clear(&self, id: &AccountId) {
        self.states.lock().unwrap().remove(id);
    }
    pub fn record(
        &self,
        id: &AccountId,
        error: &ProviderError,
        now: Instant,
        policy: &CooldownPolicy,
    ) {
        if let Some(duration) = policy.cooldown_for(error) {
            self.mark_cooldown(id.clone(), error.category, now, duration);
        }
    }
}
impl Default for AccountHealthStore {
    fn default() -> Self {
        Self {
            states: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct CooldownPolicy;
impl Default for CooldownPolicy {
    fn default() -> Self {
        Self
    }
}

impl CooldownPolicy {
    pub const fn new() -> Self {
        Self
    }
    pub fn cooldown_for(&self, error: &ProviderError) -> Option<Duration> {
        let default = match error.category {
            ProviderErrorCategory::RateLimit => Duration::from_secs(30),
            ProviderErrorCategory::QuotaExceeded | ProviderErrorCategory::Authentication => {
                Duration::from_secs(60)
            }
            ProviderErrorCategory::Timeout
            | ProviderErrorCategory::UpstreamUnavailable
            | ProviderErrorCategory::ProviderFailure => Duration::from_secs(5),
            ProviderErrorCategory::InvalidRequest
            | ProviderErrorCategory::UnsupportedCapability => return None,
        };
        Some(error.cooldown_seconds.map_or(default, Duration::from_secs))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct ManualClock(Mutex<Instant>);
    impl ManualClock {
        fn new() -> Self {
            Self(Mutex::new(Instant::now()))
        }
        fn advance(&self, d: Duration) {
            *self.0.lock().unwrap() += d;
        }
    }
    impl Clock for ManualClock {
        fn now(&self) -> Instant {
            *self.0.lock().unwrap()
        }
    }
    #[test]
    fn cooldown_expires_and_accounts_are_isolated() {
        let c = ManualClock::new();
        let s = AccountHealthStore::new();
        let a = AccountId::from("a");
        let b = AccountId::from("b");
        assert!(s.is_eligible(&a, c.now()));
        s.mark_cooldown(
            a.clone(),
            ProviderErrorCategory::RateLimit,
            c.now(),
            Duration::from_secs(30),
        );
        assert!(!s.is_eligible(&a, c.now()));
        assert!(s.is_eligible(&b, c.now()));
        c.advance(Duration::from_secs(30));
        assert!(s.is_eligible(&a, c.now()));
    }
    #[test]
    fn policy_categories_and_retry_after() {
        let p = CooldownPolicy::new();
        for category in [
            ProviderErrorCategory::RateLimit,
            ProviderErrorCategory::QuotaExceeded,
            ProviderErrorCategory::Authentication,
            ProviderErrorCategory::Timeout,
            ProviderErrorCategory::UpstreamUnavailable,
            ProviderErrorCategory::ProviderFailure,
        ] {
            assert!(
                p.cooldown_for(&ProviderError::new(category, "x", true))
                    .is_some()
            );
        }
        for category in [
            ProviderErrorCategory::InvalidRequest,
            ProviderErrorCategory::UnsupportedCapability,
        ] {
            assert!(
                p.cooldown_for(&ProviderError::new(category, "x", false))
                    .is_none()
            );
        }
        let mut e = ProviderError::new(ProviderErrorCategory::RateLimit, "x", true);
        e.cooldown_seconds = Some(120);
        assert_eq!(p.cooldown_for(&e), Some(Duration::from_secs(120)));
    }
}
