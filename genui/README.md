# Hooter GenUI companion

A controlled, sandboxed macOS task-view window using Google's **actual,
experimental** Flutter GenUI SDK. This is not a mock renderer or a GPUI
embedding. Rust owns sessions, task outcomes, approvals, drafts and execution.
There are no provider credentials, model calls or command runners here.

The authoritative wire contract is [`../docs/genui-bridge.md`](../docs/genui-bridge.md).
Bootstrap comes only from `HOOTER_GENUI_SOCKET`, `HOOTER_GENUI_GENERATION` and
`HOOTER_GENUI_CAPABILITY`. Never put the capability on a command line or in logs.
The socket must use the host's private `~/.flint/genui-runtime/g-XXXXXX/s`
launch path, with exactly six ASCII alphanumeric launch characters.
The runtime uses the user's home, not configurable `FLINT_HOME` application
storage, so custom application-data directories do not widen the sandbox.
The host creates that directory with mode `0700`, owns the socket lifecycle,
and launches the bundle executable with its scrubbed environment. The companion
does not create runtime directories or reconnect to old generations.

## Reproducible build

Validated toolchain:

| Part | Version |
|---|---|
| Flutter stable | **3.47.7**, framework `abaf9c5237` |
| Dart | **3.13.5** |
| Flutter engine | `deb287481e` |
| GenUI | **0.10.5**, git `3a9721bf57d5e45877ba2093379c92b522c77392`, `packages/genui` |
| A2UI | **v0.9**, `a2ui_core` **0.1.0** |
| Schema builder | **0.1.7**, direct exact pin |

**Dependency incompatibility:** the frozen GenUI commit references
`SchemaRegistry`, but `json_schema_builder 0.1.3` does not export it. Compiling
real `SurfaceController` fails with “Type 'SchemaRegistry' not found.”
Upstream's changelog says the export arrived in **0.1.6**. The declared 0.1.3
baseline has been replaced by a direct, tested **0.1.7** pin in `pubspec.yaml`.
There is no dependency override. `pubspec.lock` records its checksum and the
exact GenUI git revision. Neither SDK source nor the wire protocol has been
patched.

From this directory:

```sh
sh tool/build_macos.sh
# Or set FLUTTER_BIN to a different already-installed Flutter executable.
```

The helper disables analytics, enforces the lockfile, checks formatting, runs
analysis and tests, builds release, checks the executable name and verifies the
local bundle's code-signature integrity and exact narrow release entitlements.

Artifact:

```text
build/macos/Build/Products/Release/hooter_genui.app
build/macos/Build/Products/Release/hooter_genui.app/Contents/MacOS/hooter_genui
```

The display name is **Hooter GenUI**, while the product/executable remain
`hooter_genui`. Copy/package the **whole `.app`**, including Frameworks,
Resources and plugins. The resulting executable does not require Flutter,
Dart, or a shell command on PATH. Never launch `flutter run` from the native
host. A local release build/signature verification is **not** Developer ID
distribution signing, App Store entitlement approval, or notarization.

## Boundary and implementation

- `lib/bridge/protocol.dart`: exact, immutable typed full snapshots; safe
  integers; strict enum/field/identifier/binding checks; whole-graph validation
  before SDK access. It rejects unknown catalogs/components/properties,
  functions, media/URL widgets, missing/unreachable references, cycles and
  shared child references.
- `lib/bridge/framing.dart`: incremental four-byte BE frames, strict UTF-8,
  256 KiB ceiling, zero/truncation/invalid JSON rejection. A pre-decoder checks
  duplicate object keys, 32-level JSON nesting and 16,384 JSON nodes.
- `lib/bridge/client.dart`: Unix-only transport, five-second connect/handshake/
  partial-frame/write deadlines, generation and strictly increasing sequence
  checks, a 32-frame outbound limit and one replaceable pending update at
  30 Hz maximum. Idle authenticated connections do not time out. EOF or any
  failure is terminal; actions stop immediately. No raw frames/tokens/errors
  are logged or reflected in diagnostic messages.
- `lib/rendering/safe_catalog.dart`: explicit six-item catalog with **no
  functions**. PlainText uses Flutter `Text` with genuine SDK `BoundString`
  binding, never Markdown. Custom Button dispatches only inert SDK events;
  it does not use upstream Button's `functionCall` implementation. Every event
  is consumed by `SafeActionDelegate`; only current known Button/source/action
  matches can forward a fixed-shape `open_task` or `stage_prompt` request.
- `lib/rendering/snapshot_surface.dart`: each validated full snapshot becomes a
  new real `SurfaceController`, receiving parsed A2UI `createSurface`,
  `updateComponents` and `updateDataModel` messages. The widget uses
  `Surface(surfaceContext: controller.contextFor('task'), actionDelegate: ...)`.
  Acknowledgement occurs after mounting a successful frame, not on receipt.
  A small context adapter wraps `controller.contextFor('task')` to intercept
  widget errors synchronously; the next-frame acknowledgement also allows SDK
  error-stream microtasks to drain. No SDK subclass or patched source is used.
  All parsing, models, binding and rendering remain upstream.
  Replaced models/controllers dispose after their widgets unmount.
- `lib/app.dart`: independently styled trusted native outcome/revision chrome,
  bounded-width dark task content, native text/buttons, keyboard focus, adaptive
  rows, scrolling and explicit disconnected/error/read-only states.

Both boundaries require a strict tree with **256 components**, **depth 24** and
**1 MiB resolved text bytes**, including repeated data bindings. Shared-child
graphs cannot expand inside the SDK's recursive widget builder. Literal URL or
Markdown syntax is allowed as inert text, never interpreted.

App Sandbox remains enabled in both entitlements. Access is limited to a
home-relative read/write temporary exception for `/.flint/genui-runtime/`
and this outbound-only SBPL exception for named Unix sockets in that directory:

```scheme
(allow network-outbound (home-subpath "/.flint/genui-runtime/"))
```

The real sandboxed smoke initially returned Unix-connect `EPERM` with only
`network.client` plus the file exception. The directory-scoped outbound rule
is required. The unnecessary general `network.client` entitlement was removed.
There is **no network.client or network.server** entitlement, broad home access,
file picker, arbitrary URLs, media catalog or default client functions. Debug
additionally keeps Flutter's JIT entitlement. Upstream GenUI has transitive
media/URL plugins in its dependency tree; none are exposed in the safe catalog.
The SBPL temporary exception requires separate distribution-policy review;
no distribution entitlement approval is claimed.
Release disables Xcode's injection of the debugger `get-task-allow`
entitlement; the build helper checks that it is absent.

## Tests and release gates

`flutter test` covers real SDK widgets/data binding/actions, full replacement
and disposal, literal hostile text, keyboard/resize/scrolling, strict typed
validation, graph/resource limits, framing at every split boundary, malformed
UTF-8/JSON, private Unix socket authentication, replay/generation rejection,
coalescing, acknowledgements, action allowlisting and terminal teardown.
Fixture values are local deterministic test data; the application has no demo
fallback and never contacts a provider.

For a bounded, provider-free smoke of the **actual sandboxed release bundle**:

```sh
python3 tool/smoke_companion.py
```

This creates an owned temporary socket inside the private runtime directory,
launches only the built bundle with a scrubbed environment and fresh capability,
checks authentication and two real mounted SDK frame acknowledgements, injects
an unsafe component and requires a sanitized error plus EOF. It terminates its
own child and removes only its socket (and its directory if created and still
empty). It neither launches the Rust host nor automates real native actions.
This local sandboxed bundle smoke passed with two genuine SDK acknowledgements
and unsafe-ingress rejection. It does not prove the remaining cross-lane gates.

Remaining cross-lane gates: genuine Rust-host/companion launch and actions
smoke, private-directory sandbox access on the target signed build, packaging
in the native application, dependency-exception approval, and distribution
signing/notarization. Do not infer those from unit tests or a local build.
