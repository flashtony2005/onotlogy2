---
name: Idempotency & Stable IDs
slug: idempotency-stable-ids
type: concept
sources:
  - path: src/action.rs
    hash: 64d2dce7c3f6d1748ef7c756476661d8cf31ff8a60324ddeb38069754cfeac6a
  - path: src/evolution.rs
    hash: 8b22e1db63cf8cb31cf6cdc3f9664b067373f58a2c20046e9cfc21657c44cef0
  - path: src/storage.rs
    hash: e814f6bb191298d5ddc7cbfabd21208a16f55ce5c5464b6aeb8662f2c1b43439
  - path: tests/v0_3.rs
    hash: 36a45bc6af635916871b833f07de61d530477a4f6b3836010a92692759ccc5d2
sources_digest: 19279c1b2ec548149c85de159a2b62c62905a65646beb6ed975fec674c6be334
links:
  - to: action-engine
    relation: implements
    description: Upserts and deduplicated edges.
  - to: evolution-engine
    relation: implements
    description: Stable proposal/learned IDs.
generator:
  version: 1
covers: []
---
<!-- context:generated:start -->
## Summary

A cross-cutting design principle: operations must be safe to repeat. ActionEngine upserts updates and deduplicates link edges; evolution generates stable IDs via proposal_id_of/learned_id_of; memory records upsert with hit counting; repeated runs are idempotent. This enables crash recovery and reconnection without duplicate side effects.

## Related

- implements [[action-engine]] — Upserts and deduplicated edges.
- implements [[evolution-engine]] — Stable proposal/learned IDs.
<!-- context:generated:end -->

## Notes

_Anything written below the generated block is preserved when the graph is regenerated._
