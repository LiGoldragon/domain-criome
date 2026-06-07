# INTENT — domain-criome

*What the psyche has explicitly intended for this project. Synthesised from
Spirit records; not embellished.*

## Goals

- `domain-criome` owns domain meaning and provider-neutral projection; `cloud`
  owns provider execution.
- The domain runtime moves toward the same schema-interface and triad-engine
  approach as the cloud component, with separate runtime plane schemas for
  Nexus and SEMA.

## Constraints

- Signal contract repositories carry only Signal wire vocabulary. Runtime
  planes, storage, projection runtime, and broader daemon behavior belong in the
  runtime component or the relevant schema/runtime repo.
- Runtime plane schemas should be implementation artifacts, not sketches. Missing generator support is a blocker to name explicitly, not a
  reason to leave sketch files as the destination.
- Daemon runtime schema generation uses the shared `schema_rust_next::build`
  driver. The domain-criome build consumes ordinary and meta Signal contract
  schemas through Cargo schema metadata and must not hard-code workspace
  checkout paths to compensate for missing metadata.
- Provider-specific vocabulary, provider credentials, and provider API calls do
  not belong in `domain-criome`; provider execution belongs to `cloud`.
- `domain-criome-daemon` starts from one signal-encoded rkyv
  `DaemonConfiguration` file. Inline NOTA and `.nota` configuration
  files are CLI/authoring surfaces and are rejected by the daemon
  entrypoint.
- The daemon runtime is actor-native: generated listener machinery owns the
  ordinary and meta sockets, and both sockets carry length-prefixed schema
  frames. The older handshake/exchange-frame compatibility layer is retired.

## Principles

- Domain projection should remain provider-neutral until it crosses into the
  cloud provider-execution component.
- Operator integrates designer `next` work into `main` by rebasing,
  cherry-picking, re-implementing, or merging when the code is good enough.

*Source statements live in Spirit records under the `domain-criome`, `cloud`,
`schema`, `signal`, `component-triad`, and `branches` topics.*
