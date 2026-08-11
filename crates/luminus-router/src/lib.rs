mod error;
mod registry;
mod router;

pub use error::{RouterError, RouterErrorCategory};
pub use registry::ProviderRegistry;
pub use router::{
    RouteAttempt, RouteAttemptOutcome, RouteCandidate, RouteExecution, RoutePlan, RouteTarget,
    Router, RoutingPolicy,
};
