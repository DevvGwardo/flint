# R0E independent verdict: native diagnostic complete

Status: the finite R0E diagnostic is complete and accepted as native startup/
negative-permission evidence on the current recorded machine. All two new
slots are consumed. No further native/WebKit launch or renderer capability is
authorized by this verdict.

## Observed results, independently checked

- Foreground observer actual controller exit: 0, no observer timeout.
- Mocked tooling tests: 57 passed, 0 failed.
- E-A/E-B: actual child exit 0/0; both first records are protocol-1 main and
  both streams contain exactly main, read, write, network and done records.
- Both outside-scratch synthetic read-open and write-open calls were denied
  with EPERM. Neither is evidence of an actual sentinel-content read/write.
- Both network probes reached **connect**, which returned EPERM. These are not
  socket-only or nonblocking-stage classifications.
- Both independent 127.0.0.1 listeners were healthy, closed, had no accept
  errors and recorded zero hits. Fixture identities/modes/bytes/hashes stayed
  unchanged. Private streams, sessions, FD/environment isolation and process
  reaping/cleanup evidence match the reviewed controller.
- E-A/E-B cases completed within 30 seconds. Actual lock release:
  1.434068458 seconds after acquisition. Closed final inventory:
  1.439128042 seconds. Foreground observed completion-receipt boundary:
  1.439767500 seconds, excluding final output serialization/process return.
  Separate observer elapsed time, including lock wait: 1.903689791 seconds.
  Do not substitute clock origins between controller and observer.
- Exactly 33 indexed immutable payloads plus index/completion, 35 total
  artifacts, match bytes/hashes/regular single-link ownership. All 790 sealed
  preservation hashes and three source hashes remain unchanged. Shared lock
  was independently observed released.

Counts remain distinct: R0=34, R0D=8, R0E=2, cumulative=44. No historical slot
was reset or replayed. Independent evidence review added zero native launches.

## Retained proof

Run:
`renderer-evidence/r0d-root-e-1791261933054066000/`.

Primary foreground observer:
`evidence/renderer-r0e-observer-1791261932377852000/observer-result.json`,
with exact argv, actual exit, raw foreground streams and their hashes.

Independent inventory/raw/count/guard/timing verification:
`evidence/renderer-r0e-primary-verification-1791262008570462000.json`.

Artifact index SHA-256:
`64376b6059a6c4f9faa226faeea01ea68e217f343860a4337c42543d92f2d9e0`.
Completion SHA-256:
`9d93d8fda28247d4d47995ce7a3d2d2f96efe5ccfe1a99f52428581727b59fcb`.

## What this does not prove

The exact root bundle changes data, metadata and existence together. Successful
startup is consistent with the loader hypothesis, not proof that root data
alone caused the old failures. No root entries or user data were enumerated.

This bare-C probe does not prove Swift/WebKit startup, raster fidelity, actual
HTML/Mermaid, service/storage confinement, interaction/device denials, reliable
auxiliary-service ownership, aggregate memory or distribution packaging.
Deprecated sandbox-exec support remains machine-specific.

## Next authorized work

The same renderer owner may record the raw R0E observations and prepare
**PHASE 0 only** for a smallest full WebKit feasibility follow-up. Read the
original R0 failure artifacts/helper/profile/delegate implementation and installed
real APIs; identify exact permission differences, process/service ownership,
negative web/native probes, memory/raster/IPC bounds, deadlines and stop rules.

The R0E exact-root exception is diagnostic-only. Carrying it into a WebKit
experiment, adding any permission or executing a new probe requires a separate
explicit user/architect ruling and frozen finite gates. No source/toolkit/app/
manifest/lock edits, compilation, downloads, native/control/WebKit launch or
automatic production integration in this proposal.
