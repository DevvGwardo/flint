# T3 renderer slice 00D: bounded loader-startup diagnosis

Status: approved diagnostic continuation only. Not production renderer permission.
These gates are frozen before new diagnostic execution. R0 gates are unchanged.

## Evidence and ruling

R0 compiled five times successfully, with one retained failed compile. Its 34
sandbox launches all exited on SIGABRT before helper output. This establishes
neither raster capability nor executed negative-security tests.

The architect inspected the two crash reports corresponding to native PID 50432
and Swift PID 50387 in the final R0 run. Both fault in `/usr/lib/dyld`, with
`ignition_halt`, `boot_boot`, `ignite`, and `dyld4::CacheFinder` frames before
application entry. The exact missing loader operation or resource is not known.
Do not attribute the failure to WebKit or infer a permission grant from a stack
frame alone.

A minimal native-only diagnostic may distinguish an unavailable mechanism from
a narrow loader-policy omission. No WebKit retry, Mermaid acquisition, renderer
integration, alternate sandbox, unsandboxed control, or global permission change
is authorized by this addendum.

## Ownership

The existing renderer worker owns only new diagnostic files:

- `renderer/tests/dyld_startup_probe.c`
- `renderer/macos/diagnostic-dyld-*.sb`
- `tools/renderer/probe_r0d.py`
- `tools/renderer/test_probe_r0d.py`

Preserve R0 implementation and evidence. No app, agent, ACP, Rust manifest/lock,
frozen documentation, original binary, user-home configuration, or running-user-
app edits. Architect-owned run-local reporting and the new slice hash are
separate from implementation ownership.

Before implementation, record disagreements and the exact proposed diagnostic
matrix in `specs/renderer-r0d-phase0.md`. An operation outside the following
allowance requires an architect ruling before execution.

## Allowed diagnosis

1. Inspect the already identified probe crash reports and public, installed
   system-loader headers/profiles. Read no other application's reports, user
   configuration, credentials, signing identities, or live process memory.
2. Compile a minimal C/libSystem helper into a private run-root output. Its first
   stdout record proves entry into `main`; following records contain only
   bounded synthetic file/socket results.
3. Start from deny-default, explicit own executable/scratch and system-loader
   reads. Test no more than five documented profiles, no more than two native
   launches per profile, and no more than ten native launches total.
4. Add only individually justified system-loader rules. File-content grants
   are restricted to named system-library/dyld-cache subtrees or exact loader
   files. Ancestor metadata grants must name exact literals. Executable mapping
   is restricted to the owned helper and identified system-loader/library
   locations. A Mach lookup may name only one evidenced system-loader service,
   not a general bootstrap, WindowServer, WebKit, application, or credential
   service. No broad filesystem metadata, home/Volumes read subtree, wildcard
   Mach/IPC, network, process-control, privilege, or signing grant is permitted.
5. Every process gets only the existing five-variable filtered environment,
   private scratch as cwd/home/tmp, closed extra descriptors and private
   stdin/stdout/stderr. Compile/probe execution uses the shared OS build lock.
6. For any profile that reaches `main`, verify denied reads and writes against
   separate run-owned synthetic files outside scratch, and denied connection to
   a loopback-only canary. Retain errno/results and the canary count together.
   Zero canary hits without executed helper output are inconclusive.
7. Use per-process deadlines and targeted cleanup of positively owned children.
   No tracing attachment, system log export, unknown-service kill, sandbox
   weakening, unsandboxed fallback, or environment-secret output.

At most ten minutes of compilation/probe execution after obtaining the shared
slot. At most 30 seconds per native launch, including cleanup. Retain failures
and stop when the matrix or deadline is exhausted. Do not keep trying grants
until something runs.

## Frozen diagnostic gates

- Exact commands, environment variable names, process ownership, timeouts,
  profile diffs, source/profile/binary hashes, and raw outputs are retained.
- A claimed startup success includes actual first-record output from `main`
  under deny-default, not merely compilation or absence of a crash report.
- A claimed native deny result includes an executed syscall result with an
  appropriate denial errno; a parse error, startup abort, missing file, or zero
  canary count alone is not a deny result.
- Diagnostic runner tests cover deadline/launch bounds, environment filtering,
  scratch cwd, profile-scope rejection and owned-process cleanup without
  launching an unconfined helper.
- Scope, original binary and frozen-contract guards remain unchanged.
- If startup remains blocked, report the precise attempted narrow rules and
  remaining unknowns. Even success proves no WebKit service confinement,
  permission denial, raster, interactive capability, packaging support, or
  aggregate memory guarantee.

Raw report: run-root `specs/renderer-r0d-results.md`. Evidence:
run-root `renderer-evidence/r0d-*`. Report facts only; the architect judges the
next slice. Full isolated rendering remains pending.
