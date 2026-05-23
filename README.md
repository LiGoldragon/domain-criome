# domain-criome

Criome domain registry, resolver, and projection runtime.

This repo ships the `domain-criome-daemon` runtime and its bundled thin
`domain-criome` CLI. The CLI is a text-to-Signal adapter and has exactly one
peer: `domain-criome-daemon`.

The first runtime slice has ordinary and owner Unix sockets, `signal-frame`
request/reply handling, in-memory domain registry state, delegation state,
projection policy, public-record projection, and typed rejections for unknown
domains or unavailable projection scopes. It does not call cloud providers.
