# Projection To Cloud

`domain-criome` projects provider-neutral state:

- domain-name-system records;
- redirect rules;
- enough domain identity to let `cloud` choose the configured provider account.

The projection does not include provider names. Provider selection is `cloud`
policy: the domain component describes what should be true, and the cloud
component decides which configured provider can make it true.

The current hand-written-stack prototype uses this path:

1. owner registers a domain through `meta-signal-domain-criome::RegisterDomain`;
2. owner enables projection with `SetPolicy`;
3. owner records provider-neutral DNS/redirect declarations with
   `SetProjection`;
4. ordinary callers read the projection with `signal-domain-criome::Project`;
5. owner hands that projection to `cloud` with
   `meta-signal-cloud::PrepareProjection`;
6. `cloud` converts it to a provider-specific plan, then applies it through the
   existing `ApprovePlan` / `ApplyPlan` ceremony.

The remaining growth point is the daemon-to-daemon handoff: `domain-criome`
should send authorized projections to `cloud` directly instead of requiring a
manual caller to bridge the two CLIs.
