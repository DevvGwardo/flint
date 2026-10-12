#!/usr/bin/env python3
"""Render an explicitly supplied evaluation evidence set, without provider calls."""

import argparse
from datetime import datetime, timezone
import html
import json
import os
from pathlib import Path
import re

def budget_censored(row):
    return (row["proxy"]["budget"]["calls"] > len(row["proxy"]["requests"])
            or "budget exhausted" in json.dumps(row.get("metrics", {}).get("turn_reason", "")).lower())


def latest_passing_rust_summary(log):
    runs = list(re.finditer(
        r"^test result: (ok|FAILED)\. (\d+) passed; (\d+) failed; (\d+) ignored",
        log, re.M))
    if not runs or runs[-1][1] != "ok" or runs[-1][3] != "0":
        return None
    starts = list(re.finditer(r"^running \d+ tests?$", log, re.M))
    if starts and starts[-1].start() > runs[-1].start():
        return None
    return runs[-1][2], runs[-1][4]


def latest_passing_python_summary(log):
    runs = list(re.finditer(r"^Ran (\d+) tests?[^\n]*$", log, re.M))
    if not runs:
        return None
    status = re.match(r"\n(?:[ \t]*\n)*OK(?: \([^\n]*\))?(?:\n|$)",
                      log[runs[-1].end():])
    if not status:
        return None
    # Unittest emits progress before its "Ran" summary. A newer visible run
    # must not borrow the old OK, including the documented combined-log header.
    tail = log[runs[-1].end() + status.end():]
    started = re.search(
        r"^(?:[.EFsxu]+|(?:test\S*|runTest) \([^\n]+\) \.\.\.[^\n]*|"
        r"Report-renderer regression sweep:)[ \t]*$", tail, re.M)
    return None if started else runs[-1][1]


def main():
    cli = argparse.ArgumentParser(description=__doc__)
    for name in ("baseline", "post", "offline", "validation", "out"):
        cli.add_argument("--" + name, type=Path, required=True)
    cli.add_argument("--workflow", type=Path, help="optional fresh Bun/analysis live evidence")
    cli.add_argument("--ui-validation", type=Path, help="optional passing scoped headless UI log")
    args = cli.parse_args()
    baseline, post, offline = [json.loads(path.read_text())
                               for path in (args.baseline, args.post, args.offline)]
    workflow = json.loads(args.workflow.read_text()) if args.workflow else None
    dated_report = args.workflow if workflow else args.post
    date = datetime.fromtimestamp(int(dated_report.parent.name.split("-")[1]),
                                  timezone.utc).date().isoformat()
    log = args.validation.read_text()
    rust = latest_passing_rust_summary(log)
    python = latest_passing_python_summary(log)
    if (not rust or not python or
            "Finished `release` profile" not in log or "error:" in log):
        raise ValueError("complete passing validation evidence required")
    ui = None
    if args.ui_validation:
        ui_log = args.ui_validation.read_text()
        ui = latest_passing_rust_summary(ui_log)
        if not ui or "error:" in ui_log:
            raise ValueError("passing scoped UI evidence required")
    args.out.parent.mkdir(parents=True, exist_ok=True)

    def link(path, label):
        return f'<a href="{html.escape(os.path.relpath(path, args.out.parent))}">{label}</a>'

    rows = []
    censored_ids = []
    for variant, values in baseline["summary"].items():
        runs = [row for row in baseline["results"] if row["variant"] == variant]
        # Preserve the historical pilot's global-reservation metric. The new
        # workflow separately labels both local and global token cutoffs.
        censored = [row for row in runs if row["proxy"]["budget"]["calls"] >
                    len(row["proxy"]["requests"])]
        censored_ids.extend(row["id"] for row in censored)
        data = (variant, f'{values["scenario_passed"]}/{values["scenarios"]}',
                f'{values["artifacts_passed"]}/{values["deliverable_tasks"]}',
                values["artifacts_held_by_host"], len(censored), values["provider_calls"])
        rows.append("<tr>" + "".join(f"<td>{html.escape(str(x))}</td>" for x in data) + "</tr>")
    probes = sum(row["score"]["passed"] for row in post["results"])
    offline_pass = sum(row["score"]["passed"] for row in offline["results"])
    budget = post["budget"]
    intro = ("Direct Bun checks now count as verification; output-masking wrappers do not. "
             "Analysis-only requests leave files unchanged. The new live pilot confirmed Bun "
             "recovery paths but stopped at its conservative budget, so incomplete trials stay visible."
             if workflow else "A contained Gemini subscription pilot reproduced completion friction "
             "even with the judge off. The recovered-invocation fix now passes a replay of the exact "
             "failing task snapshot.")
    workflow_section = ""
    if workflow:
        workflow_rows = []
        for row in workflow["results"]:
            metrics = row["metrics"]
            values = (row["task"], row["variant"],
                      "PASS" if row["score"]["passed"] else "CAPPED" if budget_censored(row) else "FAIL",
                      metrics["outcome"], metrics.get("observed_edits", "n/a"),
                      metrics["counts"].get("HarnessNudge", 0),
                      "PASS" if row["grader"]["passed"] else "FAIL",
                      row["grader"]["tests_run"])
            workflow_rows.append("<tr>" + "".join(
                f"<td>{html.escape(str(value))}</td>" for value in values) + "</tr>")
        passed = sum(row["score"]["passed"] for row in workflow["results"])
        debit = workflow["budget"]
        limits = workflow["budget_limits"]
        executed = [row for row in workflow["results"] if row["proxy"]["requests"]]
        censored = [row for row in executed if budget_censored(row)]
        artifacts = [row for row in executed if row["task"] != "analysis-gaps"]
        artifact_passes = sum(row["grader"]["passed"] for row in artifacts)
        workflow_section = f"""
<h2>New: Bun checks and analysis-only requests</h2>
<p><strong>{passed}/{len(workflow['results'])} raw live records passed;
{len(executed)} received model execution, of {workflow.get('planned_scenarios', len(workflow['results']))} planned.</strong>
{len(censored)} executed records were budget-censored. Independent artifacts:
{artifact_passes}/{len(artifacts)}. Interrupted records stay in raw totals and are not fair quality failures.
No live analysis result is claimed if it received no model execution.</p><p>
Real Bun tests, TypeScript compilation, and read-only file integrity were evaluated.
Each case was scheduled once per variant; this small, capped sample cannot rank policies.</p>
<div class="scroll"><table><thead><tr><th>Case</th><th>Variant</th><th>Pipeline</th>
<th>Outcome</th><th>Edits</th><th>Finish nudges</th><th>Independent grade</th>
<th>Hidden + visible tests</th></tr></thead>
<tbody>{''.join(workflow_rows)}</tbody></table></div>
<p>The masked-check case begins with failing tests behind <code>2&gt;&amp;1 | tail -5</code>.
Its zero wrapper exit is not credited. A direct failed check, an actual fix, and both current
<code>bun run typecheck</code> and <code>bun test</code> passes are required.
Protected test/config files stay intact; each Bun grader runs at least four tests.</p>
<p>The exact “analyze this for improvements and gaps to fix” request runs read-only, with
zero edits and unchanged files. Recommendations are checked for the known fixture findings
(trim, uppercase, tests), not comprehensively judged for natural-language quality.
Analysis ends Needs review without an unnecessary verification requirement.
Explicit “and please fix” still authorizes implementation.</p>
<p>Hooter now asks for direct checks, explains rejected wrappers in command output, and stops
an identical finish follow-up early when no actual edit, current check, completed plan step
or usable child result changed. It does not raise the continuation cap or auto-accept work.</p>
<p>Separate, explicitly authorized quota: {debit['calls']:,}/{limits['max_calls']:,} calls and
{debit['charged_tokens']:,}/{limits['max_tokens']:,} tokens. Subscription-only routing remains
pinned and attested; no paid fallback. Installed Bun/compiler hashes are checked before and
after. Earlier frozen pilot evidence below is historical, not a pre-fix Bun A/B.</p>
<p>{link(args.workflow, 'Fresh workflow evidence, commands, grades and route receipts')}</p>"""
        if ui:
            workflow_section += f"""<p>{ui[0]} scoped headless task-outcome UI tests passed.
{link(args.ui_validation, 'Native task/queue/review UI evidence')}.
The full UI suite was stopped at an unrelated, CPU-heavy 140-conversation markdown-cache test;
no full-suite pass or native interactive-session validation is claimed.</p>"""
    page = f"""<!doctype html><html lang="en"><meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>Hooter · Orchestrator evidence</title><style>
:root{{color-scheme:light dark;--bg:#f5f3ee;--ink:#24231f;--line:#c8c5bb;--accent:#7d4b19}}
@media(prefers-color-scheme:dark){{:root{{--bg:#171713;--ink:#efede3;--line:#444439;--accent:#e4b76d}}}}
*{{box-sizing:border-box}}body{{background:var(--bg);color:var(--ink);font:16px/1.6 system-ui;
max-width:1180px;margin:40px auto;padding:0 24px}}h1{{font:700 clamp(32px,5vw,52px)/1.08 Georgia;
max-width:760px}}h2{{margin-top:36px}}a{{color:var(--accent)}}.eyebrow{{color:var(--accent);
text-transform:uppercase;letter-spacing:.12em;font-size:12px}}.note{{border-left:4px solid
var(--accent);padding:12px 20px}}.metrics{{display:grid;grid-template-columns:repeat(auto-fit,
minmax(190px,1fr));gap:16px}}.metric{{border:1px solid var(--line);padding:16px}}
.metric strong{{display:block;font-size:30px}}table{{border-collapse:collapse;width:100%;
font-size:14px}}th,td{{padding:10px;text-align:left;border-bottom:1px solid var(--line)}}
.scroll{{overflow:auto}}code,small{{overflow-wrap:anywhere}}p{{max-width:85ch}}
</style><p class="eyebrow">Hooter / Native orchestrator / Evidence, {date}</p>
<h1>Completion checks, without the loop.</h1>
<p>{intro}</p>
<div class="metrics"><div class="metric"><strong>{len(baseline['results'])}</strong>baseline attempts</div>
<div class="metric"><strong>{probes}/{len(post['results'])}</strong>earlier Python live probes</div>
<div class="metric"><strong>{offline_pass}/{len(offline['results'])}</strong>final offline scenarios</div>
<div class="metric"><strong>{rust[0]} + {python}</strong>agent + harness tests passed</div></div>
{workflow_section}
<h2>Historical Python baseline</h2><div class="scroll"><table><thead><tr><th>Variant</th>
<th>Raw pipeline</th><th>Artifact grade</th><th>Host-held passes</th><th>Censored</th>
<th>Calls</th></tr></thead><tbody>{''.join(rows)}</tbody></table></div>
<p class="note">Censored: {html.escape(', '.join(censored_ids) or 'none')}. Global reservations
were denied before forwarding. Keep this failed attempt in raw pipeline totals, but do not
interpret it as a fair model-quality failure or rank the three policies from this small sample.
Correct blocked scenarios are not shipped artifacts. This historical column counts only
global-reservation denials; the new feature table labels local token cutoffs too.</p>
<h2>Earlier repair: Python compiler-only recovery</h2><p>An optional Python inline check failed to compile. The engine retained
that failed command even after a corrected check and current unittest verification passed.
The fix requires both a statically invalid simple invocation and matching compiler diagnostics,
followed by a newer current passing check. Failed evidence stays visible. Semantic failures and
user-specified commands remain blocking. Acceptance, child presets and approvals are unchanged.</p>
<p>The live probes had no finish nudges, but did not repeat the malformed command. They are not
a causal A/B result. The saved failing task snapshot supplies the deterministic regression.</p>
<h2>Earlier pilot limits and quota</h2><p>Historical cumulative budget debit: {budget['calls']:,} calls,
{budget['charged_tokens']:,} tokens, including the conservative routing-probe debit.
Only Antigravity Gemini subscription routing was requested. No paid fallback was authorized.
Exact billing is unavailable. The Gemini judge is experimental, not calibrated Typesafe JEV.</p>
<p>B1 durable acceptance does not execute provider jobs. This tests native sessions and subagents.
One Rust test remains ignored. Native UI was not rerun in that historical pass.
Current scoped UI evidence, when supplied, is shown above. Other-platform runtime checks remain untested.</p>
<h2>Raw evidence and reproduction</h2><ul>
<li>{link(args.baseline, 'Frozen baseline, with batch provenance')}</li>
<li>{link(args.post, 'Post-fix live probes')}</li><li>{link(args.offline, 'Final-source offline pilot')}</li>
<li>{link(args.validation, 'Unit tests, Clippy and release-build output')}</li>
<li>{link(args.out.parent.parent / 'ORCHESTRATOR-EVAL.md', 'Commands, scoring, safeguards and findings')}</li>
</ul><p>Each raw report records its own source and binary hashes. Baseline and post-fix engine
versions differ; later user-command protection was tested on the final source offline.</p></html>"""
    args.out.write_text(page)
    print(args.out)


if __name__ == "__main__":
    main()
