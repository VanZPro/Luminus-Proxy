use luminus_core::model::{AccountId, ProviderId};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccountSelectionStrategy {
    FirstEligible,
    RoundRobin,
}

#[derive(Clone)]
pub struct AccountSelector {
    strategy: AccountSelectionStrategy,
    last_primary: Arc<Mutex<HashMap<String, AccountId>>>,
}

impl AccountSelector {
    pub fn new(strategy: AccountSelectionStrategy) -> Self {
        Self {
            strategy,
            last_primary: Arc::new(Mutex::new(HashMap::new())),
        }
    }
    pub fn strategy(&self) -> AccountSelectionStrategy {
        self.strategy
    }
    pub fn select(
        &self,
        provider: &ProviderId,
        eligible: Vec<AccountId>,
        order: Vec<AccountId>,
    ) -> Vec<AccountId> {
        if eligible.is_empty() {
            return eligible;
        }
        if self.strategy == AccountSelectionStrategy::FirstEligible {
            return eligible;
        }
        let mut state = self.last_primary.lock().unwrap();
        let primary = state
            .get(&provider.0)
            .and_then(|last| {
                let pos = order.iter().position(|id| id == last)?;
                (1..=order.len())
                    .map(|step| &order[(pos + step) % order.len()])
                    .find(|id| eligible.contains(id))
                    .cloned()
            })
            .unwrap_or_else(|| eligible[0].clone());
        state.insert(provider.0.clone(), primary.clone());
        let start = eligible.iter().position(|id| id == &primary).unwrap_or(0);
        eligible[start..]
            .iter()
            .chain(eligible[..start].iter())
            .cloned()
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn ids(values: &[&str]) -> Vec<AccountId> {
        values.iter().map(|v| AccountId::from(*v)).collect()
    }
    #[test]
    fn first_eligible_is_stateless() {
        let s = AccountSelector::new(AccountSelectionStrategy::FirstEligible);
        let p = ProviderId::from("p");
        assert_eq!(
            s.select(&p, ids(&["a", "b", "c"]), ids(&["a", "b", "c"])),
            ids(&["a", "b", "c"])
        );
        assert_eq!(
            s.select(&p, ids(&["b", "c"]), ids(&["a", "b", "c"])),
            ids(&["b", "c"])
        );
    }
    #[test]
    fn round_robin_rotates_by_identity() {
        let s = AccountSelector::new(AccountSelectionStrategy::RoundRobin);
        let p = ProviderId::from("p");
        let all = ids(&["a", "b", "c"]);
        assert_eq!(
            s.select(&p, all.clone(), all.clone()),
            ids(&["a", "b", "c"])
        );
        assert_eq!(
            s.select(&p, all.clone(), all.clone()),
            ids(&["b", "c", "a"])
        );
        assert_eq!(
            s.select(&p, ids(&["b", "c"]), all.clone()),
            ids(&["c", "b"])
        );
    }
    #[test]
    fn providers_are_isolated() {
        let s = AccountSelector::new(AccountSelectionStrategy::RoundRobin);
        let a = ProviderId::from("a");
        let b = ProviderId::from("b");
        let x = ids(&["x", "y"]);
        assert_eq!(s.select(&a, x.clone(), x.clone())[0], AccountId::from("x"));
        assert_eq!(s.select(&b, x.clone(), x.clone())[0], AccountId::from("x"));
    }
}
