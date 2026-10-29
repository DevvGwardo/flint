# Google GenUI bridge contract

This integration uses Google's genuine Flutter `genui` SDK, pinned at commit
`3a9721bf57d5e45877ba2093379c92b522c77392` (package source version 0.10.5).
Upstream calls it highly experimental. The production boundary is ours, not
the SDK's parser, catalog validation, or function dispatch.

## Architecture

Rust/GPUI owns the session, tools, approvals, verification, and user acceptance.
A bundled Flutter macOS companion renders an interactive task view. It has no
provider credentials, shell, arbitrary URL renderer, or authority to run tools.
This is a companion window, not an embedded GPUI widget.

The companion executable is `hooter_genui.app/Contents/MacOS/hooter_genui`.
Launch by resolved absolute/bundle-relative path, never `flutter run` in normal
operation. Use an owned process group and a scrubbed environment. Bootstrap
variables: `HOOTER_GENUI_SOCKET`, `HOOTER_GENUI_GENERATION`,
`HOOTER_GENUI_CAPABILITY`. Do not log the capability.

## Native use and model generation

Open the latest parent task's details and select **Open interactive view**.
The host shows its evidence dashboard until the model prepares a validated
surface with `render_ui`. This data-only tool is offered only to native desktop
sessions when the companion is available, never to subagents or the independent
reviewer. It is disabled by default for other native engine clients.

The tool accepts `{"surface":{...}}` using the schema below. Invalid content
returns a tool error before reaching Flutter. Generated content is scoped to
the native session, task ID, contract revision and work revision. A new task,
scope change or work change revokes it; outcome changes update the separately
trusted chrome. Surfaces are ephemeral and do not become task evidence.
Only host-selected read-only follow-up text can be staged.

Executable lookup prefers `HOOTER_GENUI_EXECUTABLE` (an absolute existing
executable), then the helper beside the executable or in `Contents/Helpers`,
then the checkout's release companion. Launching another view terminates only
the previously owned companion. It never replaces the installed Hooter app.

The renderer runtime is always under the platform user home's
`.flint/genui-runtime`, independent of `Options.home` and `FLINT_HOME`.
Those settings still select native application-data storage; they do not
broaden the signed helper's home-relative sandbox entitlement.

Native renders compare a bounded content-generation key before constructing or
cloning a surface. Task projection changes invalidate it even when task/work
revisions stay fixed. Generated replacements and target visibility changes also
invalidate it. Focus fences remain synchronous, and current snapshots publish
before pending actions are authorized. Disconnected views do not rebuild
snapshots on unrelated transcript or composer renders.

## Framing and handshake

Private Unix-domain socket, no TCP listener. Frames have a four-byte big-endian
unsigned length followed by UTF-8 JSON, maximum **256 KiB**. Reject zero lengths,
oversize frames, invalid UTF-8/JSON, unknown fields, and malformed/truncated data.
The host owns a mode-0700 `~/.flint/genui-runtime/g-XXXXXX/` directory and
mode-0600 socket `s`; the renderer permits this exact private launch shape.
Provider-free tests and smoke use the same launch shape; direct socket names
and arbitrary nested paths are rejected.

Every envelope contains `"version":1`, `"kind"`, and `"generation"` (a fresh
host-issued string per launch, 1–64 ASCII alphanumeric/dash characters).

- Child → host: `hello`, with `token` matching the privately delivered capability.
- Host → child: `ready`, only after successful authentication.
- Host → child: `snapshot`, with `sequence`, `task`, and `surface`.
- Child → host: `ack`, with the successfully applied snapshot's `sequence`.
- Child → host: `action`, with `sequence`, monotonically increasing `request`,
  `source`, and `name`. No arbitrary arguments/context.
- Either direction: `error`, with a code from `malformed_frame`,
  `invalid_snapshot`, `render_failed`, or `unauthorized`; no raw payloads.
- Host → child: `shutdown`.

Sequence/request counters are positive integers at most 9007199254740991.
Handshakes and frame reads/writes have bounded timeouts. Queues are bounded and
updates coalesce to at most 30 Hz. Close and reject further actions on EOF,
authentication failure, or protocol failure.

## Full snapshot

`task` is null or an object containing `id`, `contract_revision`,
`work_revision`, and `outcome` (host-owned strings: `answered`, `needs_review`,
`partial`, `blocked`, `unknown`, `accepted`, `verified`). IDs/revisions are
nonnegative safe integers. The native shell displays this state separately
from any model-produced surface.

`surface` contains only:

- `surface_id`: exactly `task`.
- `catalog_id`: exactly `dev.hooter.safe.v1`.
- `components`: at most 256 complete components, unique IDs, including `root`.
- `data`: at most 64 ASCII keys, each mapping to a plain string.

Component IDs/data keys are 1–64 ASCII alphanumeric/underscore/dash characters.
Each text/data string is at most 16 KiB. Allowed components and exact fields:

| Component | Fields besides `id`, `component` |
|---|---|
| `PlainText` | `text`, optional `style` (`heading`, `body`, `muted`, `code`) |
| `Column` | `children`, array of component IDs |
| `Row` | `children`, array of component IDs |
| `Card` | `child`, one component ID |
| `Divider` | none |
| `Button` | `child`, `action: {"name":"open_task"}` or `{"name":"stage_prompt"}` |

`text` is either a literal string or exactly `{"path":"/dataKey"}`, referring to
an existing `data` entry. There are no expressions, functions, assets, Markdown,
URLs-as-widgets, arbitrary themes, or unrecognized properties.

The component graph must be a tree: root has no parent, and each other component
has exactly one parent. Reused child IDs are prohibited, including repeated
references inside a single container. This prevents a tiny repeated DAG from
expanding into exponentially many rendered widgets.
Resolved text across the entire tree is limited to **1 MiB**, including repeated
bindings. Model-produced tool output reserves 1 KiB of framing/metadata overhead.

Validate the entire snapshot before applying any SDK mutation. Reject missing
references, cycles, unreachable components, graph depth over 24, containers
over 64 children, excessive JSON nesting, and invalid bindings. A full snapshot
replaces all previous component/data state, so cumulative growth is bounded.
Use an explicit custom catalog, not the default or `asNoAssetCatalog()`.

Translate validated snapshots to actual SDK A2UI `v0.9` messages:
`createSurface`, `updateComponents`, `updateDataModel`. Render using
`SurfaceController` and `Surface`. PlainText must use native Flutter `Text`,
not the SDK's Markdown widget. Dispose replaced controllers after their
widgets unmount; cancel listeners and close sockets on teardown.

## Native action boundary

Consume all SDK events in an explicit `ActionDelegate`. Forward only known
Button event names and source component IDs. Reject `functionCall` at ingress.

Rust checks generation, current snapshot sequence, replay counter, source
component/action match, active native session, task ID, and current scope/work
revision before handling an action. The renderer's payload is never authority.

- `open_task`: open the existing native task details/controls.
- `stage_prompt`: insert a host-selected follow-up into the native composer.
  Preserve existing drafts and never submit or run it automatically.

No GenUI action accepts work, approves tools, changes permissions, submits a
prompt, runs commands, or activates a self-improvement candidate.

## Release gates

Rust protocol/validation/action tests, Dart validation/widget/IPC tests,
strict linting, macOS companion release build, real native/companion smoke,
and packaging independent of Flutter/Dart on PATH. Distribution signing and
notarization must be reported separately, not claimed from a local build.

### Local bundle integrity gate

`tools/package_hooter_genui.py` creates a new versioned directory, never overwrites
an existing bundle, and prints the manifest SHA-256. Retain that digest outside
the candidate directory after validating the build. The manifest records regular
file checksums, exact framework link targets and source receipts.

Verify a newly packaged candidate without executing or installing it:

```sh
python3 -B /absolute/checkout/tools/verify_hooter_genui.py \
  --manifest /absolute/versioned-directory/manifest.json \
  --expected-manifest-sha256 <independently-retained-digest>
```

Do not compute the expected digest from a candidate supplied for verification.
That would allow a modified candidate to provide its own new checksum anchor.
The verifier rejects old manifests lacking exact link receipts, unknown or
duplicate fields, path aliases, extra/missing/changed files, retargeted or escaping
links, hard links, special files, oversized inputs, source drift, failed strict
signatures and helper entitlements outside the narrow policy. It repeats artifact
checks after signature inspection to detect ordinary concurrent drift.

This is a bounded local consistency check, not a signed build attestation or
proof that the binary was compiled from the recorded source. Ad-hoc signing is
not publisher identity or notarization. The check does not seal writable parent
directories or protect a subsequent launch from concurrent replacement.
Executable activation, external immutable gates, isolation and rollback
supervision remain held; success does not authorize activation.
