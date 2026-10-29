# Harness follow-through

## Opt-in activity discovery

`tools/activity_mcp.py` is a dependency-free local stdio MCP server, tested with
Hooter's native MCP client. Reusing MCP avoids adding a second tool transport
or widening native file-tool scope. Requires POSIX descriptor-relative I/O and
no-follow flags (macOS/Linux); this is not a Windows sandbox implementation.

After explicitly choosing directories, add an entry to Hooter's `config.toml`:

```toml
[mcp_servers.activity]
command = "python3"
args = [
  "-B",
  "/absolute/path/to/flint/tools/activity_mcp.py",
  "--project-root", "/path/to/directory-containing-projects",
  "--sessions-root", "/path/to/hooter-home/sessions",
]
enabled = true
```

The session directory is optional. Repeat `--project-root` for up to eight
explicit parent directories. Restart Hooter after editing its configuration.
No configuration or directories were enabled automatically by this change.
The native model sees `mcp__activity__project_activity`, with `days` (1–30,
default 3) and `limit` (1–50, default 25). It cannot supply or change roots.
This is an advanced MCP configuration, not a new Settings UI.

What it reads:

- One level of non-hidden children, at most 200 entries per configured root.
- Git marker **metadata**, never Git contents, commands, hooks or remotes.
- At most 16 KiB of regular `meta.json` per session. Only workspace, creation
  and update timestamps are returned. No titles, prompts, transcripts, events,
  credentials or arbitrary file contents are returned.
- Session updates within the requested window. The project list is an inventory,
  not a claim that each project changed within that window.

Results contain source paths and explicit scope, unavailable-root errors, and
an `incomplete` flag for scan/result/byte limits, missing or invalid metadata,
and absent configuration. Encoded results stay within 12,000 bytes. Empty
results do not prove inactivity; session creation does not prove repository
creation or completed work.

Root traversal and child/metadata opens use descriptor-relative no-follow
operations. Symlinked children and metadata are not read; swapped ancestor
symlinks are refused. FIFO/oversized/deeply malformed metadata cannot cause an
unbounded read or silently expand scope. The service performs no writes or
network operations. Launching Python with `-B` also disables bytecode writes.

This application-level boundary is **not an OS sandbox**. MCP processes remain
trusted host programs. MCP read-only hints do not override Hooter's approvals,
explicit task scope, blocker-resolution evidence, or verification rules.
In particular, an MCP result alone is not a native inspection receipt that
automatically resolves an already-recorded blocker.

## Evaluation reliability

The legacy `crates/flint-agent/tests/eval.rs` now:

- Uses async event deadlines, including a preflight check so an always-ready
  stream cannot defeat an expired timeout.
- Waits up to 30 seconds for `SessionStopped { history_saved: true }` before
  grading. Unverified shutdown skips grading, records failure, and retains its
  temporary workspace rather than deleting potentially active artifacts.
- Reports independent artifact results, shutdown status, host assessment, and
  successful JEV verdicts separately. A failed/timed-out turn cannot pass simply
  because a file check passed.

These changes do not establish answer quality or JEV uplift. The question-only
grader still checks unchanged files, not semantic answer quality. The legacy
shell grader itself has no process deadline or isolation; **use the existing
Blueprint runner for resource-bounded live trials**, not this legacy suite as
an untrusted-code or whole-trial timeout boundary. No live trials ran here.

Existing scripted blocker recovery, real missing-input, JEV-off/on/unavailable,
and exhausted-budget regressions remain part of the full native test suite.
They are conformance evidence, not a ranking of models or competing products.

## Source-preserving context prototype

When old turns must be dropped, `context.rs` keeps up to 240 bytes of source
references per summarized turn from prior `read_file` requests, including valid
numeric offsets/limits. Paths over 160 bytes and control-character paths are
omitted. Unrelated arguments and old file bodies are not copied into references.
Existing summary eviction limits still apply.

The label is **historical; re-read**. Use the existing bounded `read_file` tool
to inspect current contents. A pointer is not proof that a read succeeded,
that a source is unchanged, or that a requirement passed. Workspace resolution,
protected task context, and verification remain unchanged.

This is not lossless transcript storage, semantic search, or a repository map.
Older references can still be evicted. A full retrieval feature would need
durable source identities and explicit stale/missing-source handling.

## Isolation design and remaining implementation

Current native macOS command confinement still restricts writes while allowing
reads and network access. No existing permissions were silently changed.
Before running untrusted tasks, implement a separately selected restricted
profile rather than treating approvals or this MCP server as complete isolation:

1. **Explicit policy:** host-owned read roots, write roots and network mode.
   Activity roots never imply write grants. Preserve existing installations'
   policy until a user chooses the new profile; show unsupported enforcement.
2. **Enforcement:** apply OS controls to commands and subprocesses; keep native
   path validation. Default network off in the restricted profile. OS/library
   read exceptions and build-cache write exceptions must be narrow and tested,
   not a broad home-directory grant.
3. **Separate surfaces:** model/auth transport, MCP processes, fetch tools,
   browser/connector tools and external ACP agents need distinct enforcement.
   A command sandbox or proxy does not constrain all of them.
4. **Fail closed:** do not advertise a restricted profile where the OS cannot
   enforce it. Do not infer permission to escalate, change task scope, or reuse
   a parent's approvals for a child.
5. **Acceptance tests:** allowed-root canaries; denied unrelated reads/writes;
   symlink and linked-worktree boundaries; denied direct/subprocess network;
   explicitly allowed local transport; cancellation; and unchanged ask-mode
   behavior. Test both allowed and denied actions on each supported platform.

That cross-platform policy/UI/enforcement work remains implementation work,
not a security capability added by this patch.

## Verification

```sh
python3 -m unittest discover -s tools -p 'test_activity_mcp.py' -v
FLINT_LIVE=0 FLINT_EVAL=0 FLINT_BENCH=0 cargo test --offline --locked -p flint-agent
cargo clippy --offline --locked -p flint-agent --all-targets -- -D warnings
cargo check --offline --locked -p flint-app --tests
```

No paid providers, real credentials, automatic root grants, installed-app
replacement, or claims of newly measured model quality are involved.

Validated: 453 native unit tests passed (one manual performance test ignored),
eight legacy evaluation tests passed, eleven discovery tests passed, and strict
agent Clippy, changed-Rust formatting, app/test compilation and diff checks
passed. Guarded live/eval/benchmark entry points did not run provider trials.
