# T3 renderer slice 00: WebKit isolation feasibility

Status: approved bounded probe, not production preview authorization.
Acceptance gates are frozen before probe results.

## Architecture ruling

Accept the renderer PHASE 0 disagreements 1–12. Full rendering means the actual
pinned upstream core Mermaid implementation and offline HTML DOM/CSS/JavaScript,
not a native flowchart/text approximation or unrestricted web applications.

Use native GPUI raster cards plus an explicitly user-opened disposable helper
window if the security gates pass. Do not embed WebKit/native delegates in
Flint, invent cross-process view embedding, or claim process-pool/storage/home
settings alone provide isolation.

Modify the future backend preview restriction from blanket "no-script" to
"no app/tool/credential/file/native bridge": trusted Mermaid needs its actual
runtime, and interactive HTML needs isolated script execution. This does not
change B1 acceptance/storage gates or authorize new provider/tool behavior.

The available deprecated `sandbox-exec` mechanism is approved only for a
version-recorded feasibility probe. Distribution support is not established.
Do not silently switch sandbox mechanisms or expand broad directory/service
permissions to make the probe pass.

## Ownership and bounded scope

Renderer worker may create only:

- `renderer/macos/**`
- `renderer/tests/**`
- `tools/renderer/**`

No app/agent/ACP/root manifest/lock edits. No Rust renderer crate, npm acquisition
or Mermaid bundle in this slice. No user-app/browser interaction, activation,
permission granting, privileged command, signing-identity/keychain access or
external service calls. Existing dirty files are not disposable.

Use synthetic HTML, disposable sentinel files and local canaries only. Unknown
processes are never terminated, including WebKit services whose ownership cannot
be established. Initial untrusted interactive input remains disabled.

Finish with raw evidence after at most 20 minutes of probe execution. Preserve
all failed profiles and exact allowlist changes. A blocker is an informative
result, not permission to weaken the boundary.

## First probe

1. Implement a minimal Swift/AppKit/WKWebView helper, fixed private bounded pipe
   framing, scratch/environment/descriptor isolation and deny-default profile.
   Build into the run root, never the user's app target.
2. Establish a correctly sized, correctly colored bounded BGRA viewport from a
   non-activating synthetic HTML/CSS document, using ephemeral website storage.
3. Install exact memory-only resource registry, host-owned response policy,
   content rules and real navigation/auth/popup/download/file/device delegates.
   Never use page-provided paths or serve a filesystem root.
4. Probe direct denied network/file operations and web resource escape forms.
   Use synthetic files outside scratch, not actual credentials/user content.
   Record custom-scheme CSP enforcement, service confinement and permissions.
5. Exercise timeout/cancel/EOF and observe helper and demonstrably owned
   auxiliary-service lifetime. No global WebKit kills. Mark unknown ownership
   or lack of reliable service teardown as a blocker.
6. Report real script/permission APIs that cannot be safely denied. Do not enable
   interactive HTML merely because ordinary fetch/images were blocked.

Actual Mermaid dependency acquisition and complete diagram-type coverage follow
only after this feasibility ruling, in a separately frozen slice.

## Frozen acceptance gates

- A real bounded offscreen/non-activating WebKit raster is produced and checked.
- Helper receives no ambient credentials, app paths/configuration or descriptors.
- Host-selected sentinel files and network canaries cannot be accessed by the
  helper or its content through tested native and web paths.
- No app/tool/native script handler, resource-path traversal, navigation,
  download, file-picker or device-permission action escapes the restricted job.
- Source/IPC/raster/time limits are checked before trusted allocations where
  possible; post-parse counters are not advertised as preallocation guarantees.
- Cancellation retires ownership and rejects late output. Helper/service cleanup
  is demonstrated, or the exact uncovered service/resource boundary is reported.
- A sampled memory threshold is not called a hard aggregate memory ceiling.
  No full isolation/capability claim is made without all required evidence.

## Build/native slot and reporting

Acquire the same OS file lock used by Cargo validation:
`/Volumes/T7 Shield/mac-offload/flint-t3-implementation-glct94hg/evidence/build.lock`.
Run only owned Swift/probe subprocesses inside it, with a filtered environment,
private run-root output, deadlines and targeted cleanup. Do not bypass a busy
lock or interleave native jobs with another builder's Cargo validation.

Retain commands/exits, helper/profile/fixture hashes, OS/SDK/WebKit versions,
bounded raster/PNG, negative-probe evidence, process lifetime observations and
concrete blockers under run-root `renderer-evidence/`.
Report to run-root `specs/renderer-r0-results.md`. Builder reports facts, not
acceptance. Architect decides the next slice.
