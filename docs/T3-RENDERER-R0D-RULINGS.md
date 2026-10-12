# Renderer R0D PHASE 0 rulings

Status: approved native-only diagnostic implementation and execution.
This additive ruling does not change R0/R0D acceptance gates or permit root
directory data reads. Renderer capability remains disabled.

Proposal: run-root `specs/renderer-r0d-phase0.md`, frozen by SHA-256 alongside
this document before implementation.

## Disagreements 1–10

1. Accept the scope conflict. **D5 is not authorized.** Root-directory data
   visibility exceeds the current frozen allowance. Omit D5 entirely; its
   slots cannot be repurposed or hidden in another profile.
2. Accept hypothesis versus observed cause. Installed loader documentation
   makes root-open relevant, not proof of the operation denied in R0.
3. Accept no wholesale profile imports or broader compatibility grants.
4. Accept no Mach lookup. There is no evidenced loader service in this proposal.
5. Accept the separate profile-version experiment and no portability claim.
6. Accept an unbuffered, constant first record from `main` before tests.
7. Accept operation-bundle attribution only. No adaptive follow-up launches.
8. Accept counting every launcher invocation, including startup/parser failures.
9. Accept executed EPERM/EACCES with controller-verified fixtures as denial
   observations. Zero canary hits or unrelated errno do not establish denial.
10. Accept native-only evidence. No automatic WebKit/raster/interactive follow-on.

## Exact matrix and operation ruling

Authorize only proposal D1, D2, D3 and D4: common B library/cache roots, exact
M metadata/existence literals and the exact documented O operation bundle.
The proposal enumerates these finite sets; no new root, import, operation,
service, subtree, guessed physical UUID path or dynamic grant is authorized.

D4's documented filesystem-status operations can expose loader mount metadata.
This limited diagnostic permission is explicit, not called file-content
confinement or a production renderer policy. The fixed native helper must not
enumerate/report mount information, root contents or any user data.

At most two launches per profile and eight native launches total. Each attempt
consumes a slot before spawn. Stop after two actual startup/denial observations
under one profile, any unexpected permission/canary hit, profile parse error,
scope/hash drift, malformed output, ownership uncertainty or cleanup failure.

No D5 file or root-data rule may be created. If D4 cannot reach `main`, report
the root-open hypothesis and other remaining unknowns; do not retry with root
data access. Any next mechanism or permission decision is a separate ruling.

## Execution and reporting

Implement only the R0D-owned new C/profile/runner/test files. Preserve all
existing R0 source/evidence. Enforce the exact contract and ruling hashes plus
the frozen proposal hash. No manifest, lock, app, agent, ACP or documentation
edits by the worker.

Use the shared run-root `evidence/build.lock`. The 600-second after-lock total
deadline includes one bounded warning-clean C compile, mocked runner tests and
native cases. Each case's 30-second cap includes positively owned cleanup.
No extra smoke/positive-control launches, WebKit retry, unsandboxed helper,
trace attachment, unknown-service kill, signing action or user-app interaction.

Five-variable environment, private scratch cwd/home/tmp, closed extra
descriptors, bounded private pipes and synthetic outside-scratch sentinels/
loopback canaries are mandatory. Retain failures and first-main records; no
failure may disappear because a pipe or cleanup step failed.

Raw report: run-root `specs/renderer-r0d-results.md`, including actual commands,
exit/counts, deadline/ownership/denial observations, exact profiles and source
hashes, scope guards and remaining blockers. Builder does not grade the slice.
