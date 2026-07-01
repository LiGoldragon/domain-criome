# domain-criome — Agent Instructions

Read `~/primary/AGENTS.md`, then this file.

This repository is the runtime leg of the `domain-criome` triad:

- `domain-criome-daemon` will own Criome-domain registry and projection state.
- `domain-criome` will be the thin CLI client that speaks only to
  `domain-criome-daemon`.
- `signal-domain-criome` is the ordinary peer contract.
- `meta-signal-domain-criome` is the meta policy authority contract.

Do not add provider API calls here. Cloud-provider execution belongs to the
`cloud` component.

Operator integrates designer `next` work into `main` by rebasing,
cherry-picking, re-implementing, or merging when the code is good enough.
