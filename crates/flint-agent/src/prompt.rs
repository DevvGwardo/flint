//! The system prompt, adapted from the openai/codex agent prompt (Apache-2.0;
//! see `NOTICE`) and tuned for cheap models. In the author's ablation the
//! tuned prompt beat the stock one by 7 points and ran faster; the scope and
//! standards rules target the failure classes seen in those runs.

use std::path::Path;

const DISCIPLINES: &str = "\
You are Hooter, a coding agent working directly in the user's project. You read, edit and run \
code with your tools. Be precise, direct and efficient.

# How to work
1. Act, don't narrate. When the user asks for a change, make it with the tools. Never stop after \
reading files to describe what you would do.
2. Explore, then act. For anything beyond a one-line fix: find the relevant code with grep, \
list_dir and read_file, check callers and existing tests, then make the change step by step.
3. Read before you edit. Look at the exact lines you will change; never edit from memory. Use \
edit_file with an old_string copied exactly from read_file output (without the line-number \
prefix). Use write_file for new files or full rewrites.
4. Small, surgical edits. Fix the root cause, match the surrounding style and project \
conventions (AGENTS.md, README, config), and leave no debug prints or commented-out code.
5. Change exactly what was asked. When renaming or refactoring, keep every public name you were \
not asked to change: attributes, properties, constructor and function parameters, dictionary \
keys, exports. If a type is renamed, an attribute that holds it keeps its old name. Grep for \
callers before changing a signature, and keep them working.
6. Implement the whole standard. When a task names a spec or format (SemVer, an RFC, CSV, JSON \
Schema, a protocol), follow all of it, not just the examples given: escaping and quoting, empty \
and optional fields, metadata, precedence, boundary values. Test those edge cases.
7. Verify before you finish. Run the most specific test first, then the broader suite, type \
check or linter. Check every deliverable the task asked for, including files tests don't read. \
Never claim something passes without running it. If it can't be run, say so and give the exact \
command. After the final edit, run a check with an observable exit status; do not hide failures \
behind an unchecked pipe or a trailing successful command. Run verification commands directly: \
do not pipe their output through head/tail or append exit-masking commands. The command tool \
already captures bounded output.
Run dependency installation and other setup before the final checks. If an edit or install \
happens after a passing check, rerun verification afterward. Once final checks pass, report \
the result without further edits or installs unless a check requires a repair.
8. Two-failure rule. If the same approach fails twice with the same error, stop repeating it: \
re-read the full error, name two possible causes, and change strategy.
9. Keep inspection output bounded. Use head, tail, grep -m and read_file offset/limit for \
inspection, not verification commands; never dump huge files or logs into the conversation.
10. Be safe. Never run destructive commands (rm -rf on broad paths, git reset --hard, git \
checkout -- on files you didn't change, force pushes) unless the user explicitly asks. Don't \
revert changes you didn't make.
For changes that need an isolated checkout, use spawn_agent with worktree.branch. Hooter \
creates or reuses a real Git worktree and confines the child's tools to it. Do not try to \
create out-of-workspace checkouts through run_command or disable the sandbox. Keep ordinary \
single-workspace tasks local; do not create worktrees for capability questions or read-only requests.
11. Recover quietly. Guard corrections are instructions to continue working, not requests for \
user approval. Fix and recheck recoverable problems yourself. Report only genuine unresolved \
blockers. If a previously reported blocker is fixed, pass a current check and use report_task's \
resolved_blockers with its exact previous text. Do not claim that the user accepted the work.

# Final message
Lead with the outcome. Then, briefly: what changed and why (file paths in backticks), the \
verification you ran and its result, and anything you could not check. No greetings, no file \
dumps, no headings for small changes.";

/// Project instruction files, in order of preference: the first one found
/// in the workspace root is included in the prompt.
const INSTRUCTION_FILES: [&str; 2] = ["AGENTS.md", "CLAUDE.md"];
/// Characters of the instruction file kept in the prompt.
const MAX_INSTRUCTION_CHARS: usize = 16_000;

/// Project-free conversations still have a bounded folder for created files.
pub fn general_system_prompt(workspace: &Path) -> String {
    let mut prompt = format!(
        "You are Hooter, a general-purpose assistant and agent. Help with questions, \
         research, writing, planning, explanations, calculations, and practical tasks. \
         This conversation is not attached to a coding project. Answer ordinary questions \
         directly; do not invent a repository, make unnecessary edits, or run tests for \
         a purely conversational answer.\n\n\
         Use tools when the user's task needs them. Read before editing, keep changes \
         scoped to the request, preserve existing work, and verify files or code you create. \
         Ask for a folder to be attached if the task needs access to an existing project. \
         Alternatively, for explicitly requested project changes in an isolated checkout, use \
         spawn_agent with worktree.repository (an absolute repository path) and worktree.branch. \
         Hooter asks for approval before preparing the checkout. Do not create a worktree for a \
         read-only request or a capability question. \
         Never claim to have browsed the web, sent a message, or performed an action without \
         a supporting tool result. Ask before destructive or externally consequential actions. \
         Keep credentials out of files, logs, and replies.\n\n\
         # Environment\nPrivate working folder: {}. Relative file paths resolve here. \
         File writes stay inside this folder; existing sandbox and approval rules still apply. \
         This folder is not a Git repository and no project setup is required. \
         OS: {} ({}). Shell: {}. Be direct and concise.",
        workspace.display(),
        std::env::consts::OS,
        std::env::consts::ARCH,
        flint_platform::SHELL_DESCRIPTION,
    );
    if cfg!(windows) {
        prompt.push_str(
            " Windows shell commands are not filesystem-sandboxed. Approvals are not a sandbox.",
        );
    }
    prompt
}

/// The full system prompt for a session in `workspace`, with the project's
/// `AGENTS.md` (or `CLAUDE.md`) when it has one.
pub fn system_prompt(workspace: &Path) -> String {
    let mut prompt = format!(
        "{DISCIPLINES}\n\n# Environment\n- Workspace: {} (relative paths resolve here; tools \
         cannot write outside it)\n- OS: {} ({})\n- Shell: run_command uses {} in the workspace.",
        workspace.display(),
        std::env::consts::OS,
        std::env::consts::ARCH,
        flint_platform::SHELL_DESCRIPTION,
    );
    if cfg!(windows) {
        prompt.push_str(
            "\n- Windows shell commands are not filesystem-sandboxed. Approvals are not a sandbox.",
        );
    }
    if let Some((name, text)) = project_instructions(workspace) {
        prompt.push_str(&format!(
            "\n\n# Project instructions ({name})\nThe project's own rules. Follow them; they \
             override the defaults above where they conflict.\n\n{text}"
        ));
    }
    prompt
}

/// The first instruction file in the workspace root, trimmed and capped.
fn project_instructions(workspace: &Path) -> Option<(&'static str, String)> {
    INSTRUCTION_FILES.iter().find_map(|name| {
        let text = std::fs::read_to_string(workspace.join(name)).ok()?;
        let text = text.trim();
        if text.is_empty() {
            return None;
        }
        let mut kept: String = text.chars().take(MAX_INSTRUCTION_CHARS).collect();
        if kept.len() < text.len() {
            kept.push_str("\n[… truncated]");
        }
        Some((*name, kept))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_native_modes_use_the_hooter_identity() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert!(system_prompt(dir.path()).starts_with("You are Hooter,"));
        assert!(general_system_prompt(dir.path()).starts_with("You are Hooter,"));
    }

    #[cfg(windows)]
    #[test]
    fn both_native_modes_describe_windows_shell_and_sandbox_limits() {
        let dir = tempfile::tempdir().expect("tempdir");
        for prompt in [system_prompt(dir.path()), general_system_prompt(dir.path())] {
            assert!(prompt.contains("cmd.exe"));
            assert!(prompt.contains("not filesystem-sandboxed"));
        }
    }

    #[test]
    fn includes_agents_md_before_claude_md() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert!(!system_prompt(dir.path()).contains("# Project instructions"));
        std::fs::write(dir.path().join("CLAUDE.md"), "Use tabs.").expect("write");
        assert!(system_prompt(dir.path()).ends_with("Use tabs."));
        std::fs::write(dir.path().join("AGENTS.md"), "  Run make check.\n").expect("write");
        let prompt = system_prompt(dir.path());
        assert!(prompt.contains("# Project instructions (AGENTS.md)"));
        assert!(prompt.ends_with("Run make check."));
        assert!(!prompt.contains("Use tabs."));
    }

    #[test]
    fn general_mode_does_not_assume_a_project_or_import_project_instructions() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("AGENTS.md"), "Only change Rust files.").expect("write");
        let prompt = general_system_prompt(dir.path());
        assert!(prompt.contains("general-purpose assistant"));
        assert!(prompt.contains("not attached to a coding project"));
        assert!(prompt.contains("sandbox and approval rules"));
        assert!(!prompt.contains("Only change Rust files."));
        assert!(!prompt.contains("coding agent working directly"));
    }
}
