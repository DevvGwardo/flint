# R0D continuation verdict and renderer hold

Status: bounded diagnostic completed; full isolated rendering remains blocked.
No further renderer launch, grant change, fallback or integration authorized.
The existing F2a and B1 repair workers may finish their current authorized slices.

## Independent evidence review

The primary independently verified all 70 continuation artifact paths and
hashes, all 55 original diagnostic artifacts, the original report, unchanged
native helper/templates, both repaired Python sources, raw outputs and case
counts. The recorded before/after preservation bracket is unchanged. Frozen
contract and original release-binary guards also pass.

The primary reran all 52 mocked tests under the shared lock, private scratch
and five-variable environment: exit 0. This added zero native launches.
Two earlier primary preflight failures (system-Python annotation support and
an absent proposed scratch parent) were retained; neither started tests or a
native helper. No source change was needed to rerun correctly.

Independent receipt:
`evidence/renderer-r0d-independent-1791258102167676000-result.json`.
Raw worker report:
`specs/renderer-r0d-continuation-results.md`.

| Profile | Prior launches | Continuation | Total | Observation |
|---|---:|---:|---:|---|
| D1 | 2 | 0 | 2 | SIGABRT before main |
| D2 | 1 | 1 | 2 | Launcher execvp EPERM, exit 71 |
| D3 | 0 | 2 | 2 | Launcher execvp EPERM, exit 71 |
| D4 | 0 | 2 | 2 | Launcher execvp EPERM, exit 71 |
| Total | 3 | 5 | 8 | No main/native-denial output |

The repaired classifier distinguishes the exact launcher diagnostic from
profile syntax/load errors and unknown diagnostics. It does not relabel the
old artifact. The actual unlock and final accounting receipts match the
bounded execution: 1.584321041 seconds after acquisition through final
accounting; conservative cumulative allocation 31.584321041/600 seconds.
Slots are exhausted, not resettable. The controller's own exit-status receipt
is unavailable; recorded child exits and final output do not fill that gap.

## Capability judgment

Tooling and bounded accounting gates pass. Startup, executed native denial,
WebKit raster, HTML/Mermaid fidelity, WebKit service ownership/confinement,
interactive permission denial, packaging and aggregate-memory gates do not.
Zero canary hits without main are inconclusive. No renderer parity or security
success is claimed.

The installed Apple dyld-support profile describes opening the exact root
directory as an openat base. That is a loader hypothesis, not proof of the
permission responsible for any observed failure. Root-directory data access
was expressly excluded from the authorized matrix. Do not add it silently.

## Required user decision

Further native experimentation needs a separate exact authorization and new
frozen gates. The choices are a narrowly scoped exact-root-directory loader
diagnostic, a proposal-only alternate isolation design with unchanged
permissions, or a renderer hold while already authorized non-renderer slices
finish. None authorizes descendant/home reads, host networking, an unsandboxed
renderer, production enablement, Mermaid acquisition or reduced approval
controls. The recurring checkpoint pauses at this genuine user-input blocker.
