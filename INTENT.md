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
  driver. The domain-criome build script may skip generation while the
  ordinary and owner Signal contract repos do not publish Cargo schema
  metadata; it must not hard-code workspace checkout paths to compensate.
- Provider-specific vocabulary, provider credentials, and provider API calls do
  not belong in `domain-criome`; provider execution belongs to `cloud`.

## Principles

- Domain projection should remain provider-neutral until it crosses into the
  cloud provider-execution component.
- Operator integrates designer `next` work into `main` by rebasing,
  cherry-picking, re-implementing, or merging when the code is good enough.

*Source statements live in Spirit records under the `domain-criome`, `cloud`,
`schema`, `signal`, `component-triad`, and `branches` topics.*
