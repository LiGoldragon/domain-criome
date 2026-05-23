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
  through the ordinary or owner socket, never through direct registry-file
  reads.
- Runtime feature work uses a separate worktree bookmark per
  `skills/feature-development.md` §"When the repo is already locked", since
  the canonical `domain-criome` checkout is exclusive to repo-shape changes.

## Actor Shape

The first daemon should use one actor per concern:

- `RegistryStore` for registered domains and delegations;
- `ProjectionEngine` for provider-neutral desired-state projection;
- `Resolver` for intelligent resolution;
- `PolicyStore` for owner policy.

The projection engine must not block the ordinary or owner listener. Slow
resolution and projection work should be request-scoped and timeout-bounded.

## First Implementation Slice

1. Bind ordinary and owner Unix sockets.
2. Decode `signal-domain-criome` and `owner-signal-domain-criome` frames.
3. Store domain registrations, delegations, and projection policy in
   sema-engine.
4. Resolve a registered public domain from local state.
5. Project public records for a registered domain.
6. Project redirect rules for a registered domain.
7. Add a daemon-to-daemon path that sends projections to `cloud`.

## Hard Constraints

- No Cloudflare, Google, Hetzner, or provider-specific vocabulary.
- No provider credentials.
- No direct provider API calls.
- No direct state access from the CLI.
- No deprecated `signal-core` dependency in new code.

## Pending schema-engine upgrade

**Status:** scheduled for migration to schema-language-based contract per `reports/designer/326-v13-spirit-complete-schema-vision.md` + `reports/designer/324-migration-mvp-spirit-handover-re-specification.md`.

**Target:** this component's hand-written `signal_channel!` invocation + Layer 2 Command/Effect + storage types convert to a single `domain-criome/domain-criome.schema` file. The brilliant macro library (`primary-ezqx.1`) reads the schema + emits all the wire types + ShortHeader projection + dispatcher + VersionProjection + storage descriptors.

**Sequence:** per `primary-kbmi.2`. Spirit is the MVP pilot landing first via `primary-ezqx.1`; schema cutover after cloud (cloud is the upstream coordination point per `primary-kbmi.1`). Domain-criome's projection-to-cloud path means cloud's schema needs to land first so domain-criome can resolve its projection record types against cloud's schema-published types.

**Per-component concerns:** Per `primary-kbmi.2`; schema cutover after cloud. The ordinary signal-domain-criome contract is paired with `owner-signal-domain-criome`; both legs of the policy-vs-working split appear in the single `domain-criome.schema` file per the schema-language's separation discipline.

**References:**
- `reports/designer/326-v13-spirit-complete-schema-vision.md` — uniform header form + schema-language design
- `reports/designer/324-migration-mvp-spirit-handover-re-specification.md` — migration MVP + handover state
- `reports/designer/322-spirit-mvp-positional-schema-worked-example.md` — Spirit MVP worked example
- `reports/operator/174-schema-import-header-design-critique-2026-05-24.md` — header/body/feature separation + lowering rules
