# Engine and ACP improvements

## Current safeguard follow-up

The [2026-10-03 gap-closure ledger](GAP-CLOSURE-20261003.md) supersedes the
older coverage counts below. It records terminal-completion checks, preserved
JSON argument values, conservative approvals, confined undo, bounded file and
protocol input, MCP cancellation cleanup, successful-edit accounting, ACP
deadlines/group cleanup, incremental terminal UTF-8 and paste integrity.
Each has offline failing-before/passing-after evidence. File commits are
awaited rather than abandoned after cancellation; they are not transactional.
Path checks are not an OS sandbox. ACP event-channel backpressure remains open.

2026-10-03. This pass prioritized reproducible correctness, confinement and
resource-use problems. It is not an exhaustive audit of every feature.
Pre-existing changes were preserved; no dependencies were added.

## Changes

- **Streaming text:** SSE parsing buffers bytes until a complete line arrives,
  so network chunks cannot corrupt split UTF-8 characters. LF, CR and CRLF
  delimiters work across chunk boundaries. Final data retains significant
  spaces.
- **Parsing cost:** delimiter scanning visits only new bytes, and consumed
  bytes are removed once per chunk rather than once per line.
- **Tool calls:** an ID arriving after the name and arguments updates the
  existing call. A genuinely different ID at a reused index still creates a
  separate call.
- **Provider responses:** processing stops at `[DONE]`, including when more
  events or a partial event follow in the same chunk. Waiting for response
  headers has a 180-second timeout. Error-body reads are cancellable, retain at
  most 2,000 bytes and display at most 500 characters.
- **Retry handling:** numeric `Retry-After` values are capped before conversion
  to `Duration`, avoiding overflow panics. Negative and non-finite values are
  rejected.
- **Credential handling:** provider debug output no longer includes its API
  key.
- **Commands:** closing stdout and stderr no longer bypasses cancellation or
  the timeout while the process remains alive. Output readers use a bounded
  32-chunk queue (8 KiB per chunk) and are aborted after the drain window.
  Flooded output still preserves the final result's head and tail and emits
  only one live-output pause notice.
- **Search:** ripgrep output is consumed incrementally, retaining at most 201
  matches and 4 KiB of diagnostics. The extra match proves the displayed
  200-match result is truncated, then ripgrep is stopped and reaped. Paths
  starting with `-` are not parsed as options. The fallback caps matches even
  within one file, checks cancellation and does not follow descendant
  symlinks.
- **ACP paths:** file reads, writes and terminal working directories check
  canonical ancestors, not just lexical containment. Outside and dangling
  symlinks are refused; internal symlinks and new descendants remain usable.
  Native and ACP approval behavior remains separate.

## Measurement

The new `bench_sse_coalesced_events` fixture compares the previous parser's
algorithm with the new parser in the same process, using identical input and
asserting equal output. The debug-build median of five runs for 20,000 events
in a single 260,000-byte chunk was **92.65 ms -> 7.06 ms**, approximately
**13.1x faster**.

The existing 4 KiB-chunk parse-and-assemble benchmark measured 240.96 ms before
and 190.94 ms after for its 3.2 MiB fixture. These are local synthetic engine
measurements, not an end-to-end app or provider latency guarantee.

Reproduce from the repository root:

```sh
FLINT_BENCH=1 cargo test -p flint-agent --lib bench_sse -- \
  --nocapture --test-threads=1
```

Raw benchmark output:
`/var/folders/m_/lmx0wysj2p122001hkcqnw3r0000gn/T/droid-bg-1791048264796.out`.

## Regression coverage

Added 20 regression tests and one opt-in benchmark. Before implementation,
13 new regression tests failed against the existing checkout. The focused
agent and ACP suites now pass (119 and 31 tests respectively), as does strict
Clippy for both crates.

The local HTTP fixtures use loopback and fake keys. Live inference and eval
suites remain disabled. Automated UI checks use isolated settings and
workspaces.

## Full workspace validation

The isolated final run passes:

- `cargo fmt --all -- --check`.
- `cargo clippy --workspace --all-targets --locked -- -D warnings`.
- `cargo test --workspace --locked`: ACP 31, agent 119, app 90, UI 150 and
  terminal 14 tests. Integration entry points also return successfully;
  live-provider and model-eval entry points are disabled, not live passes.
- `cargo test -p flint-app --locked --test blueprint_ui -- --test-threads=1`:
  150/150 in 111.64 seconds.
- `cargo build --release --locked -p flint-app`.
- `git diff --check`.

Logs: `/tmp/flint-improvements-final-20261003.swiToH/`.
The initial temporary-home sandbox test failure is retained separately in
`/tmp/flint-improvements-20261003.NjnKD5/workspace-tests.log`.

## Limits

- Canonical path checks do not make arbitrary ACP commands a sandbox and do
  not eliminate filesystem check/use races.
- A disposable test home under `/tmp` invalidates the existing sandbox
  home-write denial test, because temporary directories are intentionally
  writable. Full checks use a disposable home under the repository's
  git-ignored `target/` instead. Sandbox permissions were not relaxed.
- No new live-provider, visual, VoiceOver, Linux or Windows validation was
  performed in this pass.
