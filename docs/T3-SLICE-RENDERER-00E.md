# R0E: user-approved exact-root-directory native diagnostic

Status: PHASE 0 proposal authorized; implementation awaits separate rulings.
The user explicitly chose “Approve narrow root-directory diagnostic” after
R0D exhausted eight launches without main entry. This is a new bounded
experiment, not replay of R0D or production-renderer permission.

## Exact permission and hypothesis

Installed `/System/Library/Sandbox/Profiles/dyld-support.sb:53–59` describes
opening `/` as an openat base. This is a hypothesis, not a proven cause.

One candidate profile only: preserve the exact version-1 D1 baseline
`renderer/macos/diagnostic-dyld-01-base.sb` (SHA-256
`1cec28403b16936fcb57052d66bf2f3001c1480f6017c930408952ce67261ae4`)
and append only:

```scheme
(allow file-read-data file-read-metadata file-test-existence
  (require-all (literal "/") (vnode-type DIRECTORY)))
```

This grants reading/enumerating the exact root directory itself. It does not
grant data reads to its children, a root subtree, home/workspace/Volumes reads,
writes, IPC/Mach/network, process control or signing. No profile imports,
other root/location/operation grants, version change or prior D3/D4 bundle.
The exact-directory selector is present in installed profiles; verify syntax
against them, not by extra native smoke launches.

The bundle also permits root metadata/existence, so even success does not
attribute an earlier failure specifically to root data access.

## Immutable inputs and ownership

Reuse the verified original native helper, no recompilation or source edit:
`renderer-evidence/r0d-1791256686399009000/DyldStartupProbe`, SHA-256
`fb428c9cf88327901b24f78efdad9de1a44c89c9c1b7019dee514a187f206364`.

Its source accepts fixture paths only under the run-owned `renderer-evidence/
r0d-` prefix and a numeric protocol argument 1–8. New evidence may use
`r0d-root-e-<stamp>` and protocol arguments 1/2, labelled distinctly as E-A/E-B.
Those arguments are not global counters. Retain separate experiment and
cumulative counts: R0=34, R0D=8, R0E=0–2, total=42–44. No prior slot is
reset, replayed or repurposed.

The same renderer worker owns only new files:

- `renderer/macos/diagnostic-root-01.sb`
- `tools/renderer/probe_r0e.py`
- `tools/renderer/test_probe_r0e.py`
- run-local proposal/evidence/report.

Do not modify old C/Swift/profiles/runners/tests, original artifacts/reports,
app, agent, ACP, manifests/lock, frozen docs, user config or release binaries.
Reuse verified tooling primitives only if their scope remains unchanged.

## Frozen execution ceilings and stop rules

- Maximum two native launches, E-A then E-B, same single frozen profile.
  Increment and retain the count before spawn; all failure forms consume slots.
- Execute only after PHASE 0 disagreements are ruled on and the new runner/
  tests/profile are frozen by hash. No compiled helper, WebKit/Swift launch,
  unconfined control, hidden smoke test, retry or automatic next experiment.
- Shared run-root exclusive build/native lock. At most 300 seconds after
  acquisition including mocked tests, cases, positively owned cleanup,
  preservation guards, final inventory and actual lock release/accounting.
  At most 30 seconds cleanup-inclusive per native case.
- A first startup/parser/launcher failure, malformed or incomplete output,
  unexpected permission/canary, scope/hash drift, timeout, uncertain process/
  fixture ownership or cleanup failure stops immediately. E-B is eligible only
  after E-A has complete main/probe/done records, valid fixtures, appropriate
  denial errnos and zero canary hits.
- Five-variable filtered environment only, private scratch cwd/home/tmp,
  private bounded pipes, closed extra FDs, positively owned session/PGID and
  run-owned synthetic read/write sentinels outside granted scratch.
- Fresh fixture provenance and independent 127.0.0.1 canary for each case.
  Preserve the actual failed network stage (socket, nonblocking or connect).
  Do not call a socket denial an executed connect denial, or other errno a
  permission denial. No real user content or root-directory listing is emitted.

## Frozen evidence gates

Retain exact profile delta, commands, exits, counts, raw records, environment
names, source/profile/helper hashes, sentinel/port provenance, original artifact
hashes, frozen-contract/binary guards, process cleanup, actual unlock and final
accounting. Mock tests cover only tooling, never startup or confinement.

Main entry needs the actual first constant record. Native denial needs the
actual syscall stage and EPERM/EACCES with valid fixtures. Zero canary hits
without execution remain inconclusive. Even two clean native results do not
prove WebKit raster, HTML/Mermaid fidelity, service isolation, interactive
permission denial, packaging or aggregate-memory bounds. No automatic renderer
integration, Mermaid acquisition, service grant or reduced approval follows.

PHASE 0: `specs/renderer-r0e-phase0.md`, exact plan and substantive disagreements
citing installed APIs, existing helper/runner and the finite profile.
Raw results: `specs/renderer-r0e-results.md`, no verdict.
