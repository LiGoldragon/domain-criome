# domain-criome Architecture

`domain-criome` is the Criome-domain registry and projection daemon. It is
name-server-like, but its primary contract is richer than ordinary DNS: peers
can ask for intelligent resolution and provider-neutral desired domain state.

## Triad

- Runtime repo: `domain-criome`.
- Ordinary contract: `signal-domain-criome`.
- Owner contract: `owner-signal-domain-criome`.

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

## Actor Shape

The first daemon should use one actor per concern:

- `RegistryStore` for registered domains and delegations;
- `ProjectionEngine` for provider-neutral desired-state projection;
- `Resolver` for intelligent resolution;
- `PolicyStore` for owner policy.

The projection engine must not block the ordinary or owner listener. Slow
resolution and projection work should be request-scoped and timeout-bounded.

## Current Implementation Slice

1. Bind ordinary and owner Unix sockets.
2. Decode `signal-domain-criome` and `owner-signal-domain-criome` frames.
3. Store domain registrations, delegations, and projection policy through a
   runtime store abstraction.
4. Resolve a registered public domain from local state.
5. Project public records for a registered domain.
6. Return typed `ProjectionUnavailable` replies for redirect projections until
   the redirect projection model exists.

`sema-engine` persistence is intentionally deferred because the current engine
still pulls the deprecated `signal-core` dependency. The store boundary is kept
small so persistence can replace the in-memory store after that dependency is
removed.

The daemon-to-daemon path that sends projections to `cloud` remains a later
slice. This runtime only produces provider-neutral projection records.

## Hard Constraints

- No Cloudflare, Google, Hetzner, or provider-specific vocabulary.
- No provider credentials.
- No direct provider API calls.
- No direct state access from the CLI.
- No deprecated `signal-core` dependency in new code.
