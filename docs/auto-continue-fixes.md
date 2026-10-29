# Auto-continue fixes

Enable **Auto-fix** in the native session's composer, or **Auto-continue fixes**
under **Settings → Automatic fixes**, then save. The preference is off by
default and persists across launches.

After a native **Work** task ends normally with a current **Partial** or
**Blocked** assessment, Hooter can continue the same protected task to address
its outstanding requirements and blockers, then independently review the
result. Each automatic continuation/review appears in the transcript. It does
not create a new task, clear blockers, claim personal acceptance, or expand
the task's scope.

When current checks pass and no technical blockers remain, a fresh, bounded
reviewer inspects actual workspace files against the protected requirements.
It can read/list/search, but cannot run commands, write files, use MCP tools,
delegate, or accept work. Actionable findings feed back into the repair loop.
A clean review finishes **Verified automatically**, not **Accepted by you**.
The task panel displays the independent review and still offers personal
acceptance. Historical non-verification inspections, such as an earlier
`git status`, remain observations rather than failed checks.

Searches inspect individual regular files, never recursively search a directory.
The reviewer can list a directory and then inspect relevant files. Credential
exclusions apply to both supplied paths and resolved identities; credential
aliases and outside-workspace aliases are refused; Unix also rejects hard-linked
files. Equivalent
relative, absolute and safe symlink paths cover the original persisted file
evidence without changing its spelling. File hashing runs off the async session
thread, remains bounded, and cancellation does not wait for a blocked hash.
Output is withheld from the UI and provider if source revalidation fails.

There are at most three automatic repair follow-ups per explicitly submitted prompt
or manual task continuation. Recovery also pauses if a follow-up has no
observed recovery progress. The composer shows **Auto-fix: Paused** and its
tooltip explains why. To resume after reviewing the task, send a prompt or use
**Continue task** or **Auto-fix and review**.

Automatic recovery does not bypass:

- Stop, engine errors, interrupted turns, or execution/continuation limits.
- Pending approvals, user-review blockers, or recorded scope denials.
- Failed persistence, conflicting saved history, or observation limits.
- Read-only, conversational, external ACP, already verified, or accepted tasks.

An incomplete, blocked, interrupted, or failed independent review never
certifies the result. A reviewer that cannot obtain sufficient evidence pauses
instead of inventing success. Changes to inspected files, including
dependencies, invalidate the review and require fresh verification/review.

Enabling the preference does not start idle, stopped, or restored work.
Turning it off prevents further automatic follow-ups, but does not interrupt
an already running turn. Tool approval modes and sandboxing are unchanged.
Credential/input blockers may still require user action; the toggle cannot
provide missing access or override permissions.

**Continue queued work after passing checks** is separate. Pending queued
prompts wait while the current task is being repaired and independently
reviewed. Verified automatically may release queued work, but it never records
personal acceptance. With Auto-fix disabled, the separate queue preference
retains its original checks-only behavior.
