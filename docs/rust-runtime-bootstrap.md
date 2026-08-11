# Rust Runtime Bootstrap

R24 adds an isolated `luminus-runtime-bootstrap` crate that coordinates the native Blackbox hydrator and the conclusively classified legacy BYOK Blackbox hydrator before constructing one Router.

`BlackboxRuntimeBootstrap` consumes already-constructed hydrators and an existing `ProviderRegistry`. It never reads environment variables, SQLite paths, credentials, or legacy encryption state. Hydrators can register directly into a caller-owned `AccountPool`; standalone `hydrate()` APIs remain available as compatibility wrappers.

`BlackboxSourceOrder` makes precedence explicit: `NativeThenLegacy` registers each source in its repository order, while `LegacyThenNative` reverses the source groups. Accounts are never globally sorted. AccountPool rejects duplicate runtime IDs, and source-level hydration errors prevent a successful snapshot. Individual account failures remain in the source reports and do not stop later accounts.

`RuntimeSnapshot` owns the same `Arc<AccountPool>` passed to Router. Router construction uses the existing constructor and fresh process-local health and selection state. No persisted health, cooldown, or RoundRobin cursor is used. After bootstrap, Router and AccountPool contain only normal runtime provider accounts; request execution does not resolve storage, configuration, credentials, or legacy state.

R24 remains synthetic and isolated. It does not open a production database, read production secrets, wire server startup, add a transport, or change the TypeScript production backend.
