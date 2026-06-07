# domain-criome Architecture

`domain-criome` is the Criome-domain registry and projection daemon. It is
name-server-like, but its primary contract is richer than ordinary DNS: peers
can ask for intelligent resolution and provider-neutral desired domain state.

## Triad

- Runtime repo: `domain-criome`.
- Ordinary contract: `signal-domain-criome`.
- Meta contract: `meta-signal-domain-criome`.

The CLI is bundled runtime machinery, not a separate triad leg. The CLI has
exactly one Signal peer: `domain-criome-daemon`.

## Boundary

`domain-criome` owns meaning:

- registered Criome domains;
- branch delegations;
- intelligent resolution;
- provider-neutral public record projection;
- provider-neutral redirect projection.

`cloud` owns provider execution. `domain-criome` can produce a projection that
`cloud` can plan/apply, but it does not call Cloudflare, Google, Hetzner, or
any other provider directly.

## Content-addressed per-domain authority

Per Spirit record 312 (Maximum certainty, 2026-05-23), `domain-criome` is the
authority for the `.criome` TLD as a whole, but each individual `.criome`
domain (for example `goldragon.criome`) is its own authority server. To check
the current authority for a domain, callers ask the domain's own daemon. A
top-level `domain-criome` instance acts as the `.criome` TLD registry and
delegates to per-domain daemons; the last delegation snapshot per domain
serves as cached state.

When a `Resolve(name)` arrives for a domain this daemon does not own, the
correct reply is `NotAuthoritative(Delegation { name, authority_endpoint })`
— "ask the authority at `authority_endpoint`". Returning `DomainUnknown`
hides the delegation and breaks the content-addressed model.

This gives the workspace its own content-addressed DNS: a Criome domain is a
content hash referencing the authority daemon's identity; lookups follow the
delegation chain; cached delegation snapshots make the common case fast.

## Runtime hard constraints

Per Spirit records 321 and 322 (Maximum certainty, 2026-05-23):

- The `domain-criome` runtime excludes provider APIs and direct CLI store
  access. Provider integrations live in `cloud`; CLI peers reach the daemon
  through the ordinary or meta socket, never through direct registry-file
  reads.
- Runtime feature work uses a separate worktree bookmark per
  `skills/feature-development.md` §"When the repo is already locked", since
  the canonical `domain-criome` checkout is exclusive to repo-shape changes.

## Runtime Shape

The current prototype uses an in-memory `Store` with mutex-protected registry,
delegation, projection-policy, and projection-state vectors. It binds ordinary
and meta sockets, decodes real `signal-frame` frames, and serves both contract
surfaces through the same path the CLI uses. This is intentionally the same
production-first concession as `cloud`: sema-engine persistence and actor
splitting wait until the hand-written prototype proves the domain model.

The target daemon shape remains one actor per concern:

- `RegistryStore` for registered domains and delegations;
- `ProjectionEngine` for provider-neutral desired-state projection;
- `Resolver` for intelligent resolution;
- `PolicyStore` for meta policy.

The projection engine must not block the ordinary or meta listener. Slow
resolution and projection work should be request-scoped and timeout-bounded.

## Current Implementation Slice

1. Bind ordinary and meta Unix sockets.
2. Decode `signal-domain-criome` and `meta-signal-domain-criome` frames.
3. Store domain registrations, delegations, projection policy, and projection
   declarations in memory.
4. Resolve a registered public domain from configured projection records.
5. Project public records for a registered domain.
6. Project redirect rules for a registered domain.
7. Hand the resulting provider-neutral projection to `cloud` through
   `meta-signal-cloud::PrepareProjection`.

## Remaining Runtime Growth

- Persist registry, delegation, projection-policy, and projection-state records
  in sema-engine. The deprecated `signal-core` storage-path blocker is gone;
  the remaining work is the actual storage migration from the current in-memory
  prototype.
- Split the in-memory store into the target actor topology.
- Replace the manual projection handoff with the designed daemon-to-daemon path
  that sends authorized projections to `cloud`.

## Hard Constraints

- No Cloudflare, Google, Hetzner, or provider-specific vocabulary.
- No provider credentials.
- No direct provider API calls.
- No direct state access from the CLI.
- No deprecated `signal-core` dependency in new code.
- `domain-criome-daemon` starts from one signal-encoded rkyv
  `DaemonConfiguration` file. Inline NOTA and `.nota` files are
  rejected by the daemon entrypoint; NOTA remains at the CLI/authoring
  edge.

## Schema-engine upgrade track

The schema-derived target is split by plane, not authored as one shared
component schema:

- `signal-domain-criome` owns the ordinary Signal schema for public resolution,
  observation, projection, and validation messages.
- `meta-signal-domain-criome` owns the meta policy Signal schema
  for registry, delegation, policy, and projection-declaration mutations.
- `domain-criome/schema/nexus.schema` names the daemon-owned Nexus
  decision plane schema and imports the two contract `Input`/`Output` roots
  plus SEMA roots.
- `domain-criome/schema/sema.schema` names the daemon-owned SEMA state
  plane for registry, delegation, projection policy, and projection state.

Signal contract repositories carry only the wire vocabulary that clients send
and receive. Nexus decisions, SEMA state, daemon storage, and the projection
runtime belong in this runtime crate.

`domain-criome/build.rs` is wired to the shared `schema_rust_next::build`
driver for daemon runtime schemas: `schema/nexus.schema` targets
`NexusRuntime`, and `schema/sema.schema` targets `SemaRuntime`. The build
consumes the ordinary `signal-domain-criome` schema directory and the meta
`meta-signal-domain-criome` schema directory from Cargo metadata, then
validates each authored schema as a `SchemaSource` object through text and
rkyv round-trips and freshness-checks `src/schema/{nexus,sema}.rs`. The daemon
must not hard-code local checkout paths for contract schemas.
