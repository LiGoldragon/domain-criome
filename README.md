# domain-criome

Criome domain registry, resolver, and projection runtime.

The current prototype ships a real daemon request path on the generated
schema-frame stack:

- `domain-criome` is the thin CLI client.
- `domain-criome-daemon` owns in-memory domain registry, delegation,
  projection policy, and provider-neutral projection state.
- `signal-domain-criome` carries ordinary `Observe`, `Resolve`, and `Project`.
- `meta-signal-domain-criome` carries meta `RegisterDomain`, `Delegate`,
  `SetPolicy`, `SetProjection`, and retirement.

The CLI remains the NOTA edge adapter; the daemon takes a single
signal-encoded rkyv configuration file and both daemon sockets carry
length-prefixed schema frames.

The runtime still avoids provider vocabulary and provider credentials.
Cloudflare execution stays in `cloud`; `domain-criome` produces the
provider-neutral projection that `cloud` can plan and apply.
