# R0D retained launcher-error classification and remaining slots

Status: scoped diagnostic repair authorized. No new permission or renderer
capability. Original R0/R0D gates, exact profiles and root-data refusal remain.

## Actual prior results

The completed R0D report records one warning-clean C compile, 39 passing mocked
tests, D1-A/B SIGABRT before output and D2-A launcher exit 71:
`sandbox-exec: execvp() ... failed: Operation not permitted`.
No main-entry or native-denial record exists. D3/D4 were not executed.

The runner labels every `sandbox-exec:` stderr as `profile_error`; the retained
D2-A diagnostic is an execution permission error, not a proven profile parse
error. Keeping those classifications distinct is required. Do not turn a
launcher errno into a synthetic helper-denial result.

## Exact remaining execution allowance

Prior counts are immutable: D1=2, D2=1, D3=0, D4=0, total=3. At most these five
unused slots remain:

1. D2-B
2. D3-A
3. D3-B
4. D4-A
5. D4-B

Do not rerun D1 or D2-A, reset counters, reuse their canaries/fixtures, add a
profile, compile another helper, change a template/rule, or acquire any new
resource/service/syscall permission. Verify and reuse the original owned helper
binary and profile templates by their retained SHA-256 values. No D5 or root
directory data read. No WebKit, unsandboxed control or unknown-service cleanup.

Treat a recognized sandbox launcher `execvp` permission error as an observed
startup denial, eligible only to proceed to the next already approved unused
slot. It is not `main`, native sentinel/network denial, parser success, or
renderer confinement. Actual profile syntax/load errors and unknown diagnostic
forms still stop immediately, as do existing malformed output, unexpected
access/canary, guard drift and ownership/cleanup failures.

## Implementation and evidence

The same renderer worker may modify only its R0D Python runner/tests to:

- Distinguish exact known launcher permission diagnostics from profile parsing
  errors and unknown diagnostics. Add mocked classification regressions.
- Validate the prior retained case counts, original input/binary/template hashes
  and immutable case identities before a continuation. Reject absent/conflicting
  prior evidence. No guessed or reset accounting.
- Start only the listed unused slots with fresh private scratch, verified
  synthetic sentinels, separate loopback canaries, filtered five-variable
  environment, private pipes and positively owned cleanup.
- Record shared-lock release and final accounting through cleanup/guards, not
  just a pre-inventory timestamp.

Use the same exclusive run-root build/native lock. No native work outside it.
Reserve 30 seconds conservatively for the first invocation's compilation,
tests, cases and bookkeeping; the continuation may consume at most 570 seconds
after locking, including mocked tests/cleanup/guard bookkeeping. Each native
case remains at most 30 seconds including cleanup. The cumulative native
launch ceiling is eight and the cumulative execution allocation is 600 seconds.

Retain the original report and artifacts byte-for-byte. New evidence references
their hashes and records continuation counts separately and cumulatively. Do
not redefine old source/input hashes as if no repair occurred.
Report to run-root `specs/renderer-r0d-continuation-results.md`, with commands,
exits, classifications, actual first-main/denial observations if any, counts,
scope/cleanup/deadline guards and unresolved boundaries. No verdict.
