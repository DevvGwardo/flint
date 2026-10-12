# T3 pass closeout

## Latest user instruction

The user requested: **just finish this up, do not continue the loop**.
Checkpoint `1618659f` has been cancelled. Do not restart this or an earlier
checkpoint, authorize another feature stage, or treat the held renderer as
renewed permission.

Close out existing evidence and perform bounded offline regression checks on
the current source only. The frontend's already-running native-lint assignment
may finish its one proposal; it has no implementation/test/launch authority.
The backend correction proposal has completed. No B2a-0 source authorization
was issued.

## Implemented versus unfinished

- F1 retained Markdown/resource policy is independently accepted.
- B1 durable acceptance/recovery is independently accepted and nonexecuting.
- N0 native copies/root/tooling are independently verified for diagnostic
  progression only; 31 mocked tooling tests pass and production check exits 0.
- F2a width/Copy/Stage code exists but is not accepted. Native Capture selection
  failure, action repairs and remaining validation gates are unresolved.
- Native library strict lint is unproven. Base Clippy was Fresh with no actual
  selected driver, Pre was unrun, and N1 was not implemented.
- B2a schema corrections and test-only codec contract are proposals, not code,
  scheduling or provider/tool execution.
- Full isolated HTML/Mermaid remains unimplemented and deliberately user-held.
  Historical 44 native diagnostics remain exhausted and unchanged.

The full requested scope is **not complete**. Stopping the loop does not waive
any gate or convert these unfinished surfaces into passing results.

## Bounded verification, not a fresh implementation stage

Run these four commands serially through the existing isolated validation
runner, with unique `closeout-` labels, `--require-stable-source`, timeout at
most 600 seconds each and an outer 2700-second deadline:

1. `cargo fmt --all -- --check`
2. `cargo clippy --offline --locked --workspace --all-targets -- -D warnings`
3. `cargo test --offline --locked --workspace -- --test-threads=1`
4. `cargo test --offline --locked -p flint-agent orchestration::tests --features serde_json/preserve_order -- --test-threads=1`

Preserve actual command exits, logs, individual suite counts/ignored tests,
whole-source/tooling/frozen-contract/proposal/binary brackets and failures.
Continue independent checks after an ordinary format/lint/test failure only
to report the remaining current-state evidence; stop on drift, timeout or
guard failure. Do not repair source, change assertions, clear caches or retry
failed checks in this closeout. The known F2a selection failure must be reported
if reproduced.

No release promotion, GUI/native probe launch, app activation, screenshot,
provider credential use, network acquisition, permission change, commit,
publishing or renderer work. Release/native inspection and full uncapped native
lint remain unrun final gates, not implicit successes.

## Final recording

Update HANDOFF, IMPLEMENTATION and VERIFICATION with the actual closeout
results, outstanding limitations and cancelled loop state. Verify preserved
starting backups, frozen histories and original binaries. Keep the running
user application untouched. Final response must distinguish bounded accepted
work from incomplete full implementation.
