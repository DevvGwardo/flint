# R0E finite execution authorization

Status: primary static review and independent preservation verification are
complete. The primary may create the fixed external seal and execute exactly
one reviewed controller invocation in the foreground. This is the existing
user-approved exact-root-directory diagnostic, not a new permission request
or production renderer authorization.

## Closed inputs

- Runner: `c57c43b227ec6902597d754db9af81e44dba305db6d5562cf0ea2fcecfca804e`.
- Mocked tests: `60543b25331d57a95c4b1453f16f20b5aecac8bd0bef9cb8d0acdad33706df33`.
- Sole 1014-byte profile:
  `d1d448bc00a24139dfb135c70006c9727bdcd2e59162f14038e58afa7b3ffed0`.
- Unchanged original in-place helper:
  `fb428c9cf88327901b24f78efdad9de1a44c89c9c1b7019dee514a187f206364`,
  uid 501/device 16777239/inode 20701597/mode 0755/34288 bytes.

Primary review: run-root `specs/renderer-r0e-primary-source-review.md`.
Independent exact closure: 790 preserved files, 55 original/70 continuation
artifacts, 702 historical guard entries and immutable raw prior identities.
Receipt:
`evidence/renderer-r0e-primary-preservation-1791261784342457000.json`.
All 235 starting snapshot files/3,249,894 bytes were also independently verified.
No runner import, test or native launch occurred to obtain those facts.

## Unchanged execution gates

Create only primary-owned
`evidence/renderer-r0e-execution-seal.json` using the reviewed exact schema,
source/helper identities, verified preservation map and exact policy. Freeze
its SHA externally. Missing or changed evidence stops; never repair it by
editing old artifacts or resetting counts.

Run only `/usr/bin/python3 -B tools/renderer/probe_r0e.py --seal-sha256 DIGEST`
with absolute paths. The reviewed controller owns the existing shared lock,
one mocked-test child and at most E-A then E-B. Maximum 300 seconds after lock
and 30 cleanup-inclusive seconds per native case. First failed test/startup/
parser/launcher/protocol/canary/hash/ownership/cleanup/deadline gate stops.
F_SETFL nonblocking-stage denial cannot advance to E-B.

Historical consumed counts remain R0=34/R0D=8. New consumed R0E slots are 0–2;
cumulative 42–44. Do not retry or make a new experiment on failure, including
a zero-native-attempt incomplete run. No helper compile, alternate profile,
unconfined smoke/control, root listing, WebKit, Mermaid acquisition, service/
network/descendant grant, app integration or user-app activity.

## Foreground observation and judgment

The primary tool must retain the actual controller exit separately from the
planned receipt status. Any primary observer receipt belongs outside the new
immutable run inventory. It may record the exact argv, raw foreground output,
wall/monotonic observer boundaries and actual subprocess exit. It cannot alter
the controller artifacts or create another native case.

Independent follow-up reads/verifies the one-write payload/index/completion
hashes, actual child exits/markers/stages, slot counts, fixtures/canary health,
cleanup, actual lock release and timing boundaries. No native repeat is part
of that review. Mocked tests prove only tooling; even two native observations
are not a WebKit/HTML/Mermaid/isolation/capability verdict.
