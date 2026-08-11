mod accounts;
mod error;
mod health;
mod registry;
mod router;
mod selection;

pub use accounts::{AccountPool, AccountPoolError, ProviderAccount};
pub use error::{RouterError, RouterErrorCategory};
pub use health::{AccountHealthStore, AccountRuntimeState, Clock, CooldownPolicy, SystemClock};
pub use registry::ProviderRegistry;
pub use router::{
    RouteAttempt, RouteAttemptOutcome, RouteCandidate, RouteExecution, RoutePlan, RouteTarget,
    Router, RoutingPolicy,
};
pub use selection::{AccountSelectionStrategy, AccountSelector};
