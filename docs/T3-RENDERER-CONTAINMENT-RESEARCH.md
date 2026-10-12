# Disposable renderer containment: research-only authority

## Current decision and preserved boundary

The user selected **Research disposable containment** after the W1 review.
This authorizes a bounded architecture proposal, not creation or execution of
a containment environment. Full isolated HTML/Mermaid remains the target.
Source-only helpers, partial native diagrams and unexecuted mocks do not meet
that target.

`T3-RENDERER-W1-REVIEW.md` remains frozen and in force. R0, R0D and R0E retain
their historical results and exhausted counts: 34 + 8 + 2 = **44**. No new
compile or native slot is allocated. The R0E root-directory exception is not
available to a new renderer or environment.

Owner: the existing renderer worker
`02895b31-eb3f-4035-b152-214cb1b9853c`. Resume it; do not create a second
renderer owner.

## Permitted work

Read the newest HANDOFF/IMPLEMENTATION and the frozen W1 review/proposal.
Inspect known local source and public API documentation without executing
project code. Read-only published documentation searches are allowed; this is
not permission for network probes, package/image acquisition or a renderer
network interface.

Write exactly one new report:

`/Volumes/T7 Shield/mac-offload/flint-t3-implementation-glct94hg/specs/renderer-containment-phase0.md`

The report must select one concrete enforcing design, or state that no
researched design satisfies the unchanged gates. Briefly record rejected
alternatives where they explain the selection. Do not return an unranked menu
or silently relax a gate. Limit the research to 20 public-document/search
queries and 90 minutes of active work; report the actual scope and stop when
the proposal or a concrete blocker is established.

## Explicitly prohibited

- VM/container/service creation, launch, restart, installation or activation.
- Image/package/runtime/Mermaid acquisition, downloads or dependency changes.
- Mounts, shared host directories, credential/home/browser-profile sharing.
- Network tests, exposed ports, device access or host permission prompts.
- Compilation, Cargo, renderer/native execution, test/tooling imports or mocks.
- Privilege changes, entitlement/signing changes, private service/PID/task APIs.
- Process-name cleanup, global kills or inspection of unrelated user apps.
- Production source, root manifest/lock, runner, prior evidence or frozen-doc edits.
- Commits, publishing, real providers or credential inspection/export.

No existing execution seal may be reused. Merely finding an installed
hypervisor, browser, image or tool does not authorize using it.

## Required evidence and architecture

1. **Trust boundary.** Identify the untrusted renderer, trusted controller,
   guest/kernel/hypervisor or equivalent enforcement boundary, and every
   channel crossing it. Explain why hostile page JavaScript cannot reach host
   credentials, files, external network, device APIs or permission UI.
   Page CSP/browser flags are defense in depth, not the enforcement boundary.
2. **Positive ownership and teardown.** Cite supported lifecycle APIs and the
   exact owner of all render processes/services. Specify acknowledgement,
   forced-stop behavior, controller death, crash, timeout and retained evidence.
   Killing the helper or releasing a web view is not all-service teardown.
3. **Enforceable resources.** Distinguish guest RAM, host/hypervisor overhead,
   renderer/GPU/service allocations, CPU time, disk and protocol/output buffers.
   Give the enforcing API for each claimed hard aggregate limit. Samples,
   helper-only rlimits and a VM's configured guest RAM are not automatically a
   hard ceiling on all host-side memory. Mark an unavailable hard bound as a
   blocker rather than replacing it with observation.
4. **No-prompt/no-device behavior.** Account for clipboard, downloads, files,
   camera/microphone, location, notifications, external URLs and authentication.
   Do not invoke an API to learn whether it prompts. A guest with no passed
   device or route must still have a bounded, noninteractive failure path.
5. **Bounded bridge.** Specify request/response framing, lengths, concurrency,
   deadlines, cancellation, versioning, stale identities and exact raster
   dimensions/format. No general guest shell, host file resolver, arbitrary
   host socket/URL proxy or ambient authority may ride on this bridge.
6. **Actual rendering.** Explain supported headless HTML/CSS/page-JS and pinned
   offline Mermaid acquisition/build/distribution provenance. Distinguish
   documented feasibility from measured raster/parity results. None exist
   for this design yet.
7. **Shipping practicality.** Give exact platform/architecture/OS prerequisites,
   public API and entitlement/signing requirements, package/image sources and
   licenses, reproducible pinning/update strategy, storage/startup implications
   and unsupported platforms. No automatic update/download authority.
8. **Future finite experiment.** Propose the smallest independently sealed
   experiment with exact setup/acquisition effects, compile/launch counts,
   deadlines, budgets, negative canaries, cleanup/preservation gates and
   stopping conditions. It is a proposal requiring separate approval, not an
   execution plan already authorized by this document.
9. **Mandatory disagreements.** Number every substantive limitation or conflict,
   cite actual local source/public APIs, and recommend accept/reject/modify.
   Clearly separate facts, documented promises and unverified assumptions.

## Research acceptance and next decision

Acceptance of the report means only that the architect has reviewed its
evidence and ruled on its disagreements. It enables no renderer capability.
Any environmental acquisition/creation, source implementation, permissions,
build or launch requires a separately frozen scope and explicit user approval
for the concrete effects. If an enforcing design is unavailable, retain the
full-renderer hold and report the blocker plainly.
