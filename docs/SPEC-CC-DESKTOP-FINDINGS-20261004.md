# Flint update spec — what the [CC] desktop app reveals

Status: research report, 2026-10-04. Source: static analysis (decompile) of the
shipped [CC] desktop app. No Flint code changed by this report.

This document is a *gap map and update spec*: it catalogs what the [CC] desktop
app implements, marks each area against Flint's current capabilities, and
proposes a prioritized set of updates. Raw per-area notes and the extraction
harness are listed in [Evidence](#8-evidence-and-reproduction).

> **Nature of the source.** The findings below are *interface-level facts*
> (protocol method names, tool names, enum values, settings keys, file layout,
> log strings) recovered from a proprietary Electron bundle. They are recorded
> for interoperability and feature planning. Do not copy proprietary code or
> verbatim prompt text into Flint (Apache-2.0). Reimplement behavior from the
> spec, not from the bundle.

---

## 1. What was decompiled

| | |
| --- | --- |
| App | [CC] desktop, `productName: "Claude"`, bundle id `com.anthropic.claudefordesktop` |
| Version | `2.16120.0` (`@ant/desktop`), Electron main entry `.vite/build/index.pre.js` |
| Bundle | `/Applications/Claude.app/Contents/Resources/app.asar` — 48.5 MB, 399 entries (389 packed + 10 unpacked), minified JS, no source maps |
| Agent SDK | `@anthropic-ai/claude-agent-sdk` `0.3.284-rc…` (+ a `…-future` alias `0.3.285-dev…`); wrapper reports `claude-code/0.3.284`; pinned CLI build `2.1.284` |
| Native | `@ant/claude-native`, `@ant/claude-swift` (Swift bridge, VM, computer-use), `node-pty` |
| Renderer windows | `main_window`, `quick_window`, `buddy_window`, `find_in_page`, `local_exec_consent`, `about_window` |

Method: a dependency-free ASAR reader (`target/cc-extract/asar.js`) parsed the
archive header and extracted the 389 in-archive files to
`target/cc-extract/dump/`; the three largest chunks (6.9 MB / 1.7 MB / 1.3 MB)
carry the runtime, the rest split across ~300 hashed chunks. String literals,
enum values and channel names survive minification and were grepped directly.

---

## 2. How the [CC] desktop app is built

The app is **not** an ACP client. It is an Electron shell that embeds the
[CC] **Agent SDK** and drives the `claude` CLI directly over the SDK's
**control protocol** (newline-delimited JSON on stdio). Three layers:

1. **Web app** — the chat/session UI is loaded remotely (`app://localhost`); the
   Electron shell only owns the title bar, native menus, helper windows, OS
   integration and a large IPC surface.
2. **Host runtime ("HostLoop")** — spawns the CLI through a "host CLI launcher",
   writes per-session settings, mounts folders, stages plugins, mints host-auth
   tokens, and boots the sandbox VM.
3. **Agent surface** — the CLI itself, plus built-in MCP servers that expose
   desktop capabilities (session control, terminal, memory, skills, plugins,
   artifacts, workspace bash/web_fetch) back to the model.

**Flint relevance:** Flint drives [CC] over **ACP**, which is a *narrow* surface
(session/new, prompt, cancel, update, request_permission, fs, terminal — see
`crates/flint-acp/src`). The desktop app's control protocol is a **superset**
that exposes plan mode, hooks, skills, MCP control, checkpoints, background
tasks, scheduling and memory. Most "missing" features below are missing from
Flint *because ACP does not carry them*, not because Flint chose not to build
them. The single highest-leverage architectural decision is therefore whether
to add a **second [CC] transport** (SDK control protocol) alongside ACP.

---

## 3. Findings by area

Each subsection ends with **Flint:** — the current state and the delta.

### 3.1 Agent runtime and wire protocol

- **Launch.** `pathToClaudeCodeExecutable` is a *launcher* binary; the real CLI
  is passed through `executableArgs`. Custom spawn via `spawnClaudeCodeProcess`
  (used for VM, SSH and the launcher). Command-line length is budgeted; when the
  settings payload is too large it is written to a per-spawn settings file
  instead, with a sweep on next start.
- **Framing.** JSON lines: `{type, session_id, request_id, request|response, payload, event_id}`.
  Message types: `user, assistant, system, result, stream_event, text, tool_use,
  tool_result, thinking, image, control_request, control_response, bash_command,
  auth_status, prompt_suggestion, rate_limit_event`.
- **Control-request subtypes (client → CLI)** — this is the effective API surface:
  `initialize, can_use_tool, request_user_dialog, elicitation, hook_callback,
  mcp_message, mcp_set_servers, mcp_status, mcp_authenticate, mcp_oauth_callback_url,
  mcp_reconnect, mcp_config_change, set_permission_mode, set_model, set_effort,
  set_color, set_setting, get_settings, get_context_usage, get_usage,
  apply_flag_settings, side_question, reload_plugins, interrupt, new_task,
  stop_task, background_tasks, read_file, write_file, rewind_files,
  create_scheduled_task, update_scheduled_task, delete_scheduled_task,
  run_scheduled_task, unattended_run_scheduled_task, get_memory_dialog,
  get_skills_dialog, host_auth_token_refresh, oauth_token_refresh,
  remote_control_work_secret, cancel_async_message`.
- **`initialize`** negotiates the whole feature set in one payload: `hooks,
  sdkMcpServers, jsonSchema, systemPrompt, appendSystemPrompt,
  planModeInstructions, toolAliases, agents, title, skills, promptSuggestions,
  forwardSubagentText, supportedDialogKinds, workspaceTrust, plugins`. Its
  response returns `account, models, agents, commands, mcpServers,
  pending_permission_requests, pending_user_dialog_requests`.
- **Stream deltas**: `text_delta, thinking_delta, user_typing, tool_progress,
  tool_use_summary`. **System subtypes**: `compact_boundary`
  (`compactMetadata.preservedSegment`), `background_tasks_changed{tasks[]}`.
- **Auth** is staged via a file descriptor when possible
  (`CLAUDE_CODE_OAUTH_TOKEN_FILE_DESCRIPTOR`, `CLAUDE_CODE_API_KEY_FILE_DESCRIPTOR`,
  host-auth refresh as a control request), env as fallback.

**Flint:** `flint-acp` speaks ACP JSON-RPC (`session/*`, `fs/*`, `terminal/*`)
and `flint-agent` speaks Chat Completions SSE. Neither has control-protocol
plumbing. **Delta:** add an SDK-control-protocol transport for [CC] to unlock
§3.2–3.7. This is the keystone item.

### 3.2 Permissions, approvals, plan mode

- **Six permission modes** with a stable set: `default, acceptEdits, plan,
  bypassPermissions, dontAsk, auto` (ordinals seen: acceptEdits=2, auto=3,
  bypassPermissions=4). `plan` is a real mode, plus a `planModeInstructions`
  payload and an `ExitPlanMode` tool.
- **Decision pipeline** (`canUseTool`), in order: disallowed-tools globs → allow
  list → standing "always allow" grants → minted approvals → default gate.
  Decisions are `behavior: allow|deny`, `permissionDecision: allow|ask|deny`,
  optional `updatedInput` (input rewriting), and a reason string.
- **Standing grants** persist per session with `decisionClassification:
  "user_permanent"`, `alwaysAllowedReasons`, `sessionPermissionUpdates`.
- **Precedence for policy**: server-managed > MDM/managed-settings.json >
  user/project settings; a managed "strict" envelope
  (`allowManagedPermissionRulesOnly`). Admin policy folds into
  `permissions.allow`/`permissions.deny` and can disable tools outright.
- **Dialog kinds** are negotiated at initialize and replayed to the UI if
  unanswered (`supportedDialogKinds`, `pending_user_dialog_requests`); a headless
  worker auto-answers `can_use_tool`, `request_user_dialog`, `elicitation`.

**Flint:** approvals are auto-run vs ask, with remembered **command-prefix**
grants scoped to allowlisted `cargo` subcommands (`crates/flint-agent/src/approvals.rs`,
`crates/flint-app/src/permission_choice.rs`). No plan mode, no per-tool standing
grants, no input rewriting. **Delta:** implement the six-mode model, a
plan-mode state machine, per-tool standing grants with a decision
classification, and an `allow/ask/deny + updatedInput` gate. Plan mode is the
single most user-visible missing feature.

### 3.3 Sandbox / isolation

- Sessions run either on the **host** or in a per-session **Linux VM**
  ("Cowork"): `claude-code-vm` storage, swift VM runtime, a **bash proxy**
  (`mcp__workspace__bash`) and `mcp__workspace__web_fetch` with an **egress
  allowlist**. Mounts: `/sessions/<vm>/mnt/<folder>`, plus reserved
  `outputs/`, `uploads/` (read-only), `.auto-memory/`, `.host-home` (read-only
  host index), `.scheduled/`.
- The system prompt discloses the sandbox honestly (Linux, files outside the
  mount are invisible, "not on the user's real computer").
- Folder access is consented (`request_cowork_directory`) and denied for
  protected host locations.

**Flint:** explicitly *not* a sandbox — path checks are lexical/canonical, and
command approval is not a shell sandbox (see `docs/GAP-CLOSURE-20261003.md`).
A real VM sandbox is a large, platform-specific project. **Delta:** track as a
known limitation; the *design* worth stealing cheaply is the **honest
disclosure in the system prompt** and a read-only host-index mount concept.

### 3.4 MCP

- **Two planes**: in-process "sdk" servers spoken to over `mcp_message`
  (raw JSON-RPC), and a dedicated utility-process **direct MCP host**
  (`mcp-runtime/directMcpHost.js`).
- **Transports**: stdio, **SSE**, **Streamable HTTP**, with full OAuth
  (`mcp-protocol-version`, `mcp-session-id`, `last-event-id`,
  `oauth-protected-resource`, PKCE `code_challenge`, 401→reauthorize) and a
  `legacy`/`modern` wire-codec negotiation.
- **Tool naming** `mcp__<server>__<tool>` with wildcard expansion; per-server
  status (`mcp_status`), reconnect (`mcp_reconnect`), set-servers with
  add/remove/errors, and auth-required marker `mcp_auth_required`.
- **Tool approval policy**: server-side gate keys `mcp_tool_approval_config`,
  `sensitive_mcp_tools`, `disable_destructive_mcp_tools_by_default`,
  `sensitive_mcps_per_call_consent`; per-tool `ask` tables (e.g. Office365).
- A **built-in Node host** runs stdio servers when no system Node exists.

**Flint:** `crates/flint-agent/src/mcp.rs` — stdio-only client, `mcp__server__tool`
naming, one-shot start per session, 16-server / 500-tool caps, no reconnect, no
OAuth, no per-tool approval. **Delta:** add SSE + Streamable HTTP transports and
OAuth; per-server status/reconnect; a per-tool approval table; wildcard allow
rules. This is the most self-contained, high-value port.

### 3.5 Skills and plugins

- **Skills**: bundled `resources/bundled-skills/*.skill` (a `.skill` is a ZIP of
  `<name>/SKILL.md` with YAML front matter `name/description/license`, plus
  scripts). Manifest entries carry `name`, `description`, `tier:"always"`. Skills
  are **invoked as slash commands** and loadable from local dirs
  (`listLocalSkills / saveLocalSkill / syncSkills`).
- **Plugins**: installable bundles that contribute only `hooks` and `mcp`
  (`PluginOnlyCustomization:["hooks","mcp"]`); `${CLAUDE_PLUGIN_ROOT}` env
  substitution; **marketplaces** from git or a `marketplace.json` (archive
  entries with `sha256`), with an allowlist and enterprise policy gates; install
  errors typed (`POLICY_BLOCKED`, `BINARY_ARCH_MISMATCH`, …); hot `reload_plugins`.

**Flint:** no skills, no plugins (slash commands are built-in only:
`/new /clear /model /agent /effort /approval /review /help`). **Delta:** a
**skills** system is cheap and high-value (a directory of `SKILL.md` files,
surfaced as slash commands, injected as context on invocation). Plugins and
marketplaces are a bigger surface; defer.

### 3.6 Subagents, background tasks, agent teams

- **Task tool** with `subagent_type`, `plugin:agent` namespacing, and separate
  host-loop vs VM subagent prompts; subagent transcripts stored under
  `<transcript>/subagents/`.
- **Background tasks**: `background_tasks` control, `background_tasks_changed`
  events, spawn/stop/dismiss, task backlog, auto-approve policy hooks
  (`shouldAutoApprovePermission`), and an MCP card flow
  (`mcp__ccd_session__spawn_task`, `dismiss_task`).
- **Dispatch orchestration**: parent → child sessions with dedicated tools
  (`dispatch_start_task`, `dispatch_send_message`, `dispatch_read_transcript`).
- **Agent teams / teammates**: a `mailbox` namespace, tools `SendMessage`,
  `ListAgents`, `SubscribePR`, cross-session message types
  (`teammate-message`, `cross-session-message`, …), and state dirs
  `mailbox/`, `agent-registry.json`.

**Flint:** native subagents up to 4 in parallel with resumable child sessions
(`crates/flint-agent/src/subagents.rs`); ACP agents have no subagent control.
**Delta:** add background-task chips and a cross-session message bus; the
mailbox/agent-registry pattern is the model to follow.

### 3.7 Memory, checkpoints/rewind, todos, plan, output styles, statusline, hooks

- **Memory**: an auto-memory dir created per session (degrading to none on
  failure), mounted at `mnt/.auto-memory/` in the VM, guarded by the permission
  gate; global instructions seeded as `CLAUDE.md` (server-side "Instructions for
  Claude" wins over local), with a seed time budget and baseline hashing; MCP
  tools `memory_read` / `memory_list`.
- **Checkpoints / rewind**: opt-in file checkpointing
  (`CLAUDE_CODE_ENABLE_SDK_FILE_CHECKPOINTING`); `rewind_files` control request
  and IPC with a **dry run**; per-user-message **rewind marks** and a branch DAG
  (`rewindEdges`, `direction: back/forward`, reasons `dead_branch`,
  `past_compact_boundary`); restore is refused while another restore runs;
  partial-restore reporting (`rewindFilesRestoredPartly`); a crash-resume
  checkpoint with a write cursor (`seq`, `skipBytes`, `midTurn`).
- **Todos / plan**: `mcp__ccd_turn__update_plan` with item states
  `pending|in_progress|completed` and scopes `first_task|continuation|new_task`;
  `wrap_up` ends a turn cycle; UI affordances `perTaskStopAffordance`,
  `rapidFollowupPreempt`. Legacy `TodoWrite` remains as an alias.
- **Slash commands**: per-session live command list plus built-in `context` and
  `design`; slash commands double as skill invocation.
- **Output styles**: read/written via `~/.claude/settings.json` (`outputStyle`),
  `get_settings.output_styles`, `set_setting`; a headless "StyleDrafter" drafts
  new styles.
- **Statusline**: a configurable `statusLine` command (with `padding`,
  `refreshInterval`), grouped under `disableAllHooks`.
- **Hooks lifecycle**: `PreToolUse, PostToolUse, SessionStart, UserPromptSubmit,
  Stop, SubagentStop, Notification, PermissionRequest, PreCompact, SessionEnd`.
  Hook I/O is JSON: `permissionDecision: allow|ask|deny`, `updatedInput`,
  `additionalContext`, `{decision:"block",reason}`. Flint-relevant detail: the
  desktop app implements **its own permission gating as synthetic PreToolUse
  hooks**.

**Flint:** rewind appears only as a UI concept (3 files), no checkpointing; no
memory, no todo/plan tool, no output styles, no statusline, hooks are minimal
(2 files). **Delta:** checkpoint/rewind is the highest-value item here
(user trust in agent edits); the todo/plan tool and hooks are strong seconds.
The "permission gate as a hook" pattern is a clean architecture to reuse.

### 3.8 UI / UX surfaces

- **Main window**: sidebar, **session panes** toggled from the View menu —
  `terminal` (⌘J), `diff` (⇧⌘D), `preview`/`browser` (⇧⌘P/⇧⌘B), `files` (⇧⌘F),
  `side_chat` (⌘;); **split view** (new right/below, focus next/prev, move,
  close); preview **tabs**; command palette (⌘K) and search (⇧⌘K).
- **Composer**: attachments, `@`-mentions, slash commands/skills, model +
  **effort** picker (`low|medium|high|xhigh`, extended thinking, fast mode),
  permission-mode picker, stop/interrupt, **queued prompts with promote/reorder
  and "steer now"**, dictation.
- **Diff/Changes**: git diff/stats/patch, commit diff, working-tree status,
  stash, branch-switch conflict checks; transcript watchers fire follow-up cards
  on `git commit|push|pull` and `gh pr create|merge`.
- **Approvals/consent**: tool-permission dialogs, folder-consent, git-origin
  pinning, trust-the-cwd, bypass-permissions confirm, device-tool re-approval.
- **Other windows**: Quick Entry (global `Alt+Space`, double-tap-Option), Buddy
  (hardware pairing), find-in-page, a dependency-free **local-exec-consent**
  dialog deliberately built without the web stack.
- **Notifications**: `turn_complete`, `idle_notification` ("needs your input"),
  focus-and-navigate on click.
- **Offline/loading**: an error card plus a canvas mini-game mascot.
- **Internal codenames** for gates/prototypes: `sableOtter`, `chicago`,
  `midnightOwl`, `epitaxy` (file preview), `clarkdown` (doc conversion),
  `omelette` (usage/limits), `yukonSilver` (VM), `grandPrix` (pairing).

**Flint:** already strong here — dockable panels, up to 8 session panes with
Chat/Terminal/Both modes, changes panel, file preview, command palette, prompt
queue + steering, image prompts, terminal dock, worktrees. **Delta:** the
notable gaps are **plan mode UI**, **checkpoint/rewind UI**, **effort picker**
(Flint has `/effort`; the desktop exposes `xhigh` and extended-thinking/fast-mode
toggles), **side chat**, and a **find-in-files / transcript search** worker.

### 3.9 Settings, config, enterprise

- **Managed config**: a Zod schema with **176 MDM keys** (`flatKey` dotted
  names) spanning inference (Bedrock/Vertex/gateway), `allowedPluginMarketplaces`,
  `allowedWorkspaceFolders`, `blockReadsOutsideWorkingDirectories`,
  `builtinToolPolicy`, `coworkEgressAllowedHosts`, retention days, OTLP, etc.
  Precedence: server-managed > MDM/managed-settings.json > user.
- **Managed-settings files**: `managed-settings.json` + `managed-settings.d/`
  under `/Library/Application Support/ClaudeCode` (macOS) or `/etc/claude-code`.
- **Per-session settings** (camelCase): `askUserQuestionTimeout`,
  `effortLevel`, `editorMode` (vim), `viewMode` (`default|verbose|focus`),
  `teammateMode`, `includeCoAuthoredBy`, `permissionMode`, `autoUpdatesChannel`.
- **Settings routes**: `/settings/{claude-code,desktop,usage,billing,connectors}`,
  plus `setToolHostDialogsHost` so the embedded CLI can open the desktop settings
  page and route logs.

**Flint:** user-level settings only; no enterprise/MDM layer (not a near-term
need for a personal desktop app). **Delta:** low priority, except the **settings
surface split** and a **`get_settings`/`set_setting` bridge** so the agent can
read/write a safe subset of user preferences — that is a nice, cheap feature.

### 3.10 Telemetry, logging, ops

- Five event families (`lam_*`, `desktop_*`, `cowork_*`, `ccd_*`, `tengu_*` —
  hundreds of event names each), Sentry (`@sentry/electron`, org 1158394), full
  OTLP/OTEL export config, and PII scrubbing (Bedrock ARNs, emails, JWTs,
  `sk-ant-` keys, plugin-id hashing).
- Winston logging with per-subsystem files; structured namespaced debug
  (`[HostLoop]`, `[CCD]`, `[BinaryDeployment]`, …).

**Flint:** no telemetry (appropriate — local-first). **Delta:** adopt the
**PII-scrubbing** and structured-logging patterns for any diagnostic report
Flint adds; skip the rest.

### 3.11 Updates / versioning

- Release channels `latest|stable|rc`; Squirrel/Forge makers; a **signed model
  catalog** fetched from `downloads.` and verified against a built-in key, with
  a bundled fallback and `<path>.raw-sig.json`; `harnessSchema:1` SDK-compat
  gate (`unsupported_harness_schema`); embedded binary pins per platform with a
  `.verified` sentinel and semver comparison; auto-update enforcement hours.
- **Unreleased model names** surfaced in the model selector: "Fable 5.1",
  "Mythos".

**Flint:** ships its own binary, no auto-update. **Delta:** not needed; the one
transferable idea is the **signed catalog + bundled fallback** pattern if Flint
ever fetches a remote model list.

---

## 4. Gap analysis at a glance

| Area | [CC] desktop | Flint today | Priority |
| --- | --- | --- | --- |
| Plan mode | 6 modes incl. `plan`, `ExitPlanMode`, plan instructions | none | **P0** |
| Checkpoint / rewind | file checkpoints, dry-run, marks, branch DAG | none | **P0** |
| MCP transports | stdio + SSE + Streamable HTTP + OAuth + status/reconnect | stdio only | **P0** |
| MCP approval policy | per-tool `ask`, sensitive/destructive gates, wildcards | none | **P0** |
| Approval model | 6 modes, standing grants, `updatedInput`, decision class | auto/ask + cargo-prefix | **P0** |
| Hooks | 10 lifecycle events, JSON in/out | minimal | **P0/P1** |
| Skills | `.skill` archives, slash-invoked, local + bundled | none | **P0** |
| Todo / plan tool | `update_plan` + UI | none | P1 |
| Background tasks | chips, backlog, spawn/dismiss, auto-approve | none | P1 |
| Agent teams / mailbox | cross-session messaging, registry | subagents only | P1 |
| Scheduled tasks (cron) | full create/run/history | none | P1 |
| Memory | auto-memory dir, global-instruction seeding | none | P1 |
| Output styles / statusline | settings-driven | none | P2 |
| Transcript / file search | dedicated workers | file index only | P1 |
| Side chat | `side_chat` pane | none | P2 |
| VM sandbox | per-session Linux VM + egress allowlist | not a sandbox (documented) | P2 (large) |
| Computer use / browser | full OS control + 19 Chrome tools | none | P2 (out of scope) |
| Artifacts | sandboxed artifact host | none | P2 |
| Enterprise / MDM | 176 managed keys | none | P3 |
| Telemetry / OTLP | 4 families + Sentry | none (local-first) | skip |
| Auto-update | Squirrel + signed catalog | none | skip |

---

## 5. Prioritized update spec

Ordered for a Rust/GPUI codebase. Each item names the target crate(s).

### P0 — parity features that change how the app feels

1. **Plan mode** (`flint-agent`, `flint-acp`, `flint-app`).
   - Model a permission-mode state machine: `default, acceptEdits, plan,
     bypassPermissions, dontAsk, auto`.
   - Add a read-only "plan" turn: tools restricted to Read/Glob/Grep, output a
     plan block, require explicit approval to exit.
   - Composer chip + shortcut; persist per session; map onto ACP
     `session/set_config_option` where the agent supports it.
2. **Checkpoint / rewind** (`flint-agent`, `flint-app`).
   - Snapshot touched files before edits (content-addressed store under
     `~/.flint/checkpoints/`), keyed per user message.
   - `rewind(session, mark, dry_run)` returning a diff; undo the rewind;
     refuse concurrent restores; report partial restores.
   - UI: rewind marks on user turns, a dry-run preview, and an undo affordance.
3. **MCP expansion** (`flint-agent/src/mcp.rs`).
   - Add SSE and Streamable HTTP transports; OAuth (protected-resource metadata,
     PKCE, `mcp-session-id`, `last-event-id`, 401→reauthorize).
   - Per-server status + reconnect; keep the current caps and cancellation.
   - Per-tool approval table and wildcard allow rules (`mcp__server__*`);
     mark destructive/sensitive tools "ask" by default.
4. **Approval model upgrade** (`flint-agent/src/approvals.rs`,
   `flint-app/src/permission_choice.rs`).
   - Standing per-tool grants with a `decision_classification`
     (`user_permanent`), scoped per session and per project.
   - Decision gate returning `allow | ask | deny` plus optional **input
     rewriting** (`updatedInput`) so a rule can sanitize arguments.
   - Keep the existing conservative cargo-prefix behavior as the narrow case.
5. **Skills** (`flint-agent`, `flint-app/src/slash.rs`).
   - Load `<skills-dir>/<name>/SKILL.md` (YAML front matter) and `.skill` ZIPs.
   - Surface each as a slash command; inject the skill body as context on
     invocation; ship 2–3 example skills (e.g. a PDF/docx helper) as reference.
6. **Hooks** (`flint-agent`).
   - Config-driven hooks for `PreToolUse, PostToolUse, SessionStart,
     UserPromptSubmit, Stop, SubagentStop, Notification, PreCompact, SessionEnd`.
   - JSON stdout contract: `permissionDecision`, `updatedInput`,
     `additionalContext`, `{decision:"block"}`.
   - Reuse the desktop app's architecture: implement Flint's own permission
     gate **as a PreToolUse hook** so external hooks compose with it.

### P1 — depth and workflow

7. **Todo/plan tool + UI** — `update_plan(items, scope)` with
   `pending|in_progress|completed`, rendered as a live checklist per session.
8. **Background tasks** — long-running tools surfaced as chips with stop/dismiss,
   a backlog, and an auto-approve policy for trusted task types.
9. **Cross-session messaging** — a mailbox + agent registry so sessions and
   subagents can message each other (`SendMessage`, `ListAgents`), building on
   the existing subagent coordinator.
10. **Scheduled tasks** — cron-style tasks with a create/run/history UI and
    transcript cards.
11. **Memory** — an auto-memory directory plus seeding of a global
    instructions file, with a time budget and a baseline hash to detect drift.
12. **Transcript + file search** — a bounded background worker (Rust thread or
    `flint-term`-style task) so `/find` searches session history and the tree.
13. **Settings bridge** — expose a safe subset of user preferences to the agent
    (`get_settings`/`set_setting`), read-only for security/trust keys.

### P2 / P3 — larger or lower-fit

14. Side chat pane; effort picker with extended-thinking/fast-mode toggles.
15. A real sandbox (VM or seatbelt/`sandbox-exec` on macOS) — a project of its
    own; at minimum adopt the **honest system-prompt disclosure** of what is and
    is not sandboxed.
16. Computer use / browser / artifacts — out of scope for a coding-agent window.
17. Enterprise managed-config / MDM — defer.
18. Telemetry — adopt only PII-scrubbing and structured logging for diagnostics.

### The architectural call

Flint's ACP transport cannot carry most of §3.1–3.7. If [CC] parity is a goal,
add a **second [CC] backend** that speaks the SDK control protocol (spawn the
`claude` CLI, negotiate `initialize`, then drive `can_use_tool`,
`set_permission_mode`, `set_model`, `set_effort`, `mcp_*`, `rewind_files`,
`background_tasks`, `create_scheduled_task`, `get_memory_dialog`,
`get_skills_dialog`, `interrupt`, `new_task`). Keep ACP for Codex/Droid. The
protocol reference in §3.1 is the spec for that backend.

---

## 6. Risks and what not to copy

- **Licensing.** The bundle is proprietary. Do not copy code, strings or prompt
  text into Flint. Reimplement from these functional facts.
- **Protocol churn.** The control protocol is unversioned and dev/rc-pinned
  (`0.3.284-rc`). A Flint backend must feature-detect at `initialize` and
  degrade gracefully; treat every field as optional.
- **Security posture.** The desktop app's defaults are broad (auto modes, VM
  egress, standing grants). Flint's conservative approvals are a deliberate
  strength — adopt the *mechanisms* without loosening the *defaults*.
- **Scope.** The desktop app is three products in one (chat, cowork, code).
  Flint is a coding-agent window; resist pulling in chat/enterprise surfaces.

---

## 7. Suggested first milestone

A two-week-shaped slice that proves the architecture and ships visible value:

1. SDK-control-protocol backend for [CC] in `flint-acp` (spawn, initialize,
   stream, interrupt, `can_use_tool`).
2. Plan mode end-to-end on that backend (composer chip → plan turn → approve).
3. MCP SSE/HTTP transports + per-tool approval in `flint-agent`.
4. Checkpoint/rewind with a dry-run preview in the changes panel.

---

## 8. Evidence and reproduction

Raw analysis lives under the git-ignored `target/cc-extract/`:

| Path | Contents |
| --- | --- |
| `target/cc-extract/asar.js` | dependency-free ASAR reader (`list`, `cat`, `extract`) |
| `target/cc-extract/list.txt` | full archive file listing with sizes |
| `target/cc-extract/dump/` | 389 extracted files (`.vite/build`, `.vite/renderer`, `resources`, `node_modules`) |
| `target/cc-extract/notes/agent-runtime.md` | HostLoop, control protocol, sessions, permissions, MCP, plugins, subagents, memory/hooks (888 lines) |
| `target/cc-extract/notes/ui-features.md` | windows, panels, composer, shortcuts, menus, i18n, codenames (123 lines) |
| `target/cc-extract/notes/feature-catalog.md` | gates, managed keys, tools, `@ant/*` packages, telemetry, integrations, updates (1,158 lines incl. 650 env-var names) |

Reproduce:

```sh
node target/cc-extract/asar.js list /Applications/Claude.app/Contents/Resources/app.asar | head
node target/cc-extract/asar.js cat  /Applications/Claude.app/Contents/Resources/app.asar package.json
```

The extraction was read-only against the installed app; nothing in
`/Applications/Claude.app` was modified, and no Flint source file was changed by
this report.
