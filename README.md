# domain-criome

Criome domain registry, resolver, and projection runtime.

The current prototype ships a real daemon request path on the existing
hand-written Signal / NOTA stack:

- `domain-criome` is the thin CLI client.
- `domain-criome-daemon` owns in-memory domain registry, delegation,
  projection policy, and provider-neutral projection state.
- `signal-domain-criome` carries ordinary `Observe`, `Resolve`, and `Project`.
- `meta-signal-domain-criome` carries owner `RegisterDomain`, `Delegate`,
  `SetPolicy`, `SetProjection`, and retirement.

The runtime still avoids provider vocabulary and provider credentials.
Cloudflare execution stays in `cloud`; `domain-criome` produces the
provider-neutral projection that `cloud` can plan and apply.
