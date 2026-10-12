//! Shared command evidence classification. This is not an approval allowlist
//! or a shell sandbox. Unknown effects remain conservative; check programs
//! may write build artifacts, but are not presumed to edit source files.

use crate::harness::guard::is_inspection;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CommandEvidence {
    pub verification: bool,
    pub mutates: bool,
}

pub(crate) fn classify(command: &str) -> CommandEvidence {
    let Some(parts) = shell_parts(command) else {
        return CommandEvidence {
            // A shell test still owns its final exit status when its operands
            // expand. Those expansions remain potentially mutating.
            verification: command.trim_start().starts_with("test ")
                && !command.contains(['|', '&', ';', '\n', '\r', '>', '<', '`']),
            mutates: true,
        };
    };
    let mut verification = false;
    let mut mutates = false;
    for (part, sequence) in parts {
        let Some(words) = shlex::split(part) else {
            return CommandEvidence {
                verification: false,
                mutates: true,
            };
        };
        let words: Vec<_> = words.iter().map(String::as_str).collect();
        let words = executable_words(&words);
        let check = is_check(words);
        if sequence {
            // A semicolon/newline doesn't propagate an earlier command's exit.
            verification = check;
        } else {
            verification |= check;
        }
        mutates |= !check_without_source_writes(words) && !inspection(words);
    }
    CommandEvidence {
        verification,
        mutates,
    }
}

/// Advisory only. Never turns an unattributable shell status into evidence.
pub(crate) fn verification_rejection(command: &str) -> Option<&'static str> {
    if classify(command).verification {
        return None;
    }
    let attempted = command.split(['&', '|', ';', '\n']).any(|part| {
        shlex::split(part).is_some_and(|words| {
            let words: Vec<_> = words.iter().map(String::as_str).collect();
            is_check(executable_words(&words))
        })
    });
    attempted.then_some(
        "The shell wrapper does not preserve a recognizable check's exit status. \
         Run the test/typecheck/build directly, without pipes, `; echo`, `|| true`, \
         backgrounding, or file redirection. Output is already bounded to its head and tail.",
    )
}

fn executable_words<'a>(mut words: &'a [&'a str]) -> &'a [&'a str] {
    if words.first() == Some(&"env") {
        words = &words[1..];
    }
    while words.first().is_some_and(|word| {
        word.split_once('=').is_some_and(|(name, _)| {
            !name.is_empty()
                && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
                && !name.starts_with(|c: char| c.is_ascii_digit())
        })
    }) {
        words = &words[1..];
    }
    words
}

/// A narrow compile-only failure, not a failed assertion. Do not infer this
/// from diagnostic prose alone: the inline source must be plainly invalid too.
pub(crate) fn is_unexecuted_python_check(command: &str, detail: &str) -> bool {
    if !detail.starts_with("  File \"<string>\", line ")
        || !detail.lines().any(|line| line.starts_with("SyntaxError:"))
    {
        return false;
    }
    let Some(parts) = shell_parts(command) else {
        return false;
    };
    if parts.len() != 1 {
        return false;
    }
    let Some(words) = shlex::split(parts[0].0) else {
        return false;
    };
    if words.len() != 3
        || !matches!(words[0].rsplit('/').next(), Some("python" | "python3"))
        || words[1] != "-c"
    {
        return false;
    }
    let code = words[2].as_bytes();
    let (mut quote, mut comment, mut invalid, mut i) = (None, false, false, 0);
    while i < code.len() {
        let c = code[i];
        if comment {
            comment = c != b'\n';
        } else if let Some(q) = quote {
            if c == b'\\' {
                i += 2;
                continue;
            }
            if c == q {
                quote = None;
            }
        } else if matches!(c, b'\'' | b'"') {
            // Prefixed/f/triple strings need a full Python parser. Stay
            // conservative instead of guessing at their embedded expressions.
            if (i > 0 && (code[i - 1].is_ascii_alphanumeric() || code[i - 1] == b'_'))
                || code.get(i..i + 3).is_some_and(|s| s == [c, c, c])
            {
                return false;
            }
            quote = Some(c);
        } else if c == b'#' {
            comment = true;
        } else if c == b'\\'
            && code
                .get(i + 1)
                .is_some_and(|c| matches!(c, b'n' | b'r' | b't'))
        {
            invalid = true;
        } else if c == b';' {
            let mut next = i + 1;
            while code.get(next).is_some_and(|c| matches!(c, b' ' | b'\t')) {
                next += 1;
            }
            // A compound try statement cannot follow a semicolon on the
            // same logical line. A real newline remains valid.
            invalid |= code.get(next..next + 4) == Some(b"try:");
        }
        i += 1;
    }
    invalid && quote.is_none()
}

fn is_check(words: &[&str]) -> bool {
    let Some((program, args)) = words.split_first() else {
        return false;
    };
    let program = program.rsplit('/').next().unwrap_or(program);
    match (program, args) {
        ("test" | "pytest" | "pytest-3" | "jest" | "vitest" | "mypy" | "eslint", _) => true,
        ("cargo", ["test" | "check" | "clippy" | "build", ..]) => true,
        ("cargo", ["fmt", rest @ ..]) => rest.contains(&"--check"),
        ("rustfmt" | "prettier", rest) => rest.contains(&"--check"),
        ("tsc", rest) => rest.contains(&"--noEmit"),
        ("npm" | "pnpm" | "yarn", ["exec", rest @ ..]) | ("npx" | "bunx", rest) => {
            let start = rest
                .iter()
                .position(|arg| !matches!(*arg, "--" | "-y" | "--yes" | "--no-install"))
                .unwrap_or(rest.len());
            is_check(&rest[start..])
        }
        (
            "npm" | "pnpm" | "yarn" | "bun",
            ["test" | "check" | "lint" | "typecheck" | "build", ..],
        )
        | (
            "npm" | "pnpm" | "yarn" | "bun",
            [
                "run",
                "test" | "check" | "lint" | "typecheck" | "build" | "validate",
                ..,
            ],
        ) => true,
        ("go", ["test" | "vet" | "build", ..]) => true,
        ("git", rest) => {
            crate::harness::guard::git_subcommand(rest) == Some("diff") && rest.contains(&"--check")
        }
        ("ruff", ["check", ..]) | ("biome", ["check" | "lint", ..]) => true,
        ("ruff", ["format", rest @ ..]) => rest.contains(&"--check"),
        ("python" | "python3", ["-m", "pytest" | "unittest", ..]) => true,
        ("python" | "python3", ["-c", code, ..]) => code.contains("assert "),
        ("python" | "python3", [script, ..]) => std::path::Path::new(script)
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with("test_") || name.starts_with("validate")),
        _ => false,
    }
}

fn check_without_source_writes(words: &[&str]) -> bool {
    is_check(words)
        && !words.iter().any(|word| {
            matches!(*word, "--fix" | "--write" | "-w")
                || word.starts_with("--fix=")
                || word.starts_with("--write=")
                || word.starts_with("--output")
        })
        // Inline assertions can also contain arbitrary mutations.
        && !matches!(words, ["python" | "python3", "-c", ..])
}

fn inspection(words: &[&str]) -> bool {
    !words.iter().any(|word| {
        matches!(
            *word,
            "-i" | "--in-place" | "-delete" | "-exec" | "-execdir" | "-fprint"
        ) || word.starts_with("--output")
            || word.starts_with("--pre")
            || word.starts_with("--open-files-in-pager")
            || word.starts_with("--ext-diff")
            || word.starts_with("--textconv")
    }) && words.first().is_some_and(|program| {
        let program = program.rsplit('/').next().unwrap_or(program);
        match program {
            "cargo" | "npm" | "pnpm" | "yarn" | "bun" | "go" | "rustc" | "rustfmt"
                if matches!(&words[1..], ["--version" | "-V" | "--help" | "-h"]) =>
            {
                true
            }
            "git" => crate::harness::guard::git_subcommand(&words[1..]).is_some_and(|sub| {
                matches!(
                    sub,
                    "status" | "diff" | "log" | "show" | "branch" | "blame" | "ls-files"
                )
            }),
            // An awk program can invoke a command or write a file.
            "awk" => false,
            _ => is_inspection(program),
        }
    })
}

/// A deliberately small foreground grammar. Expansions, redirection,
/// conditionals and pipelines cannot reliably attribute a check's exit code.
/// Quoted punctuation is data, not another command.
fn shell_parts(command: &str) -> Option<Vec<(&str, bool)>> {
    if cfg!(windows) && command.contains(['%', '!', '^']) {
        return None;
    }
    let mut quote = None;
    let mut escaped = false;
    let mut start = 0;
    let mut sequence = false;
    let mut parts = Vec::new();
    let mut chars = command.char_indices().peekable();
    while let Some((index, c)) = chars.next() {
        if escaped {
            escaped = false;
            continue;
        }
        if c == '\\' && quote != Some('\'') {
            escaped = true;
            continue;
        }
        if quote == Some('\'') {
            if c == '\'' {
                quote = None;
            }
            continue;
        }
        if matches!(c, '$' | '`') {
            return None;
        }
        if let Some(q) = quote {
            if c == q {
                quote = None;
            }
            continue;
        }
        // Descriptor merging does not write a file or mask the exit status.
        // Accept this exact standalone spelling, not arbitrary redirections.
        if c == '2'
            && command[index..].starts_with("2>&1")
            && (index == 0 || command[..index].ends_with(char::is_whitespace))
            && command[index + 4..]
                .chars()
                .next()
                .is_none_or(|c| c.is_whitespace() || matches!(c, '&' | ';'))
        {
            for _ in 0..3 {
                chars.next();
            }
            continue;
        }
        match c {
            '\'' | '"' => quote = Some(c),
            '|' | '<' | '>' | '(' | ')' | '#' | '\r' => return None,
            '&' | ';' | '\n' => {
                let and = c == '&';
                if and && chars.next().map(|(_, c)| c) != Some('&') {
                    return None;
                }
                let part = command[start..index].trim();
                if !part.is_empty() {
                    parts.push((part, sequence));
                    sequence = !and;
                } else if and || c == ';' {
                    return None;
                }
                start = chars.peek().map_or(command.len(), |(i, _)| *i);
            }
            _ => {}
        }
    }
    if quote.is_some() || escaped {
        return None;
    }
    let last = command[start..].trim();
    if !last.is_empty() {
        parts.push((last, sequence));
    } else if !parts.is_empty() && !sequence {
        return None;
    }
    (!parts.is_empty()).then_some(parts)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unexecuted_python_checks_require_static_compile_errors_not_diagnostic_claims() {
        let detail = "  File \"<string>\", line 1\nSyntaxError: invalid syntax";
        for command in [
            r#"python3 -c "assert True\nassert True""#,
            r#"python3 -c "assert True; try: pass""#,
        ] {
            assert!(is_unexecuted_python_check(command, detail), "{command}");
            assert!(!is_unexecuted_python_check(
                command,
                "SyntaxError: invalid syntax"
            ));
            assert!(!is_unexecuted_python_check(
                command,
                "Traceback (most recent call last):\nSyntaxError: invalid syntax"
            ));
        }
        for command in [
            r#"python3 -c "assert '\\n'""#,
            r#"python3 -c "assert True # \n""#,
            r#"python3 -c "assert True; print('try:')""#,
            r#"python3 -c "assert f'\\n'""#,
            "python3 -c 'assert True;\ntry:\n pass\nexcept ValueError:\n pass'",
            r#"python3 -c "assert True\nassert True" && touch output"#,
            r#"python3 -c "assert False""#,
            "cargo test",
        ] {
            assert!(!is_unexecuted_python_check(command, detail), "{command}");
        }
    }

    #[test]
    fn common_checks_share_verification_and_effect_classification() {
        for command in [
            "cargo fmt --check",
            "cargo fmt --all -- --check",
            "npx tsc --noEmit",
            "npx eslint src",
            "ruff check src",
            "ruff format --check src",
            "prettier --check src",
            "env CI=1 cargo test",
            "cd 'two words' && cargo test && git status --short",
            "git diff --stat && npm run build",
            "cargo test &&\n git status --short",
            "npx --yes tsc --noEmit",
            "git diff --check",
            "bun test",
            "bun run typecheck",
            "bun run check",
            "bunx tsc --noEmit",
            "cd 'two words' && bun run typecheck 2>&1 && echo checked",
            "bun test 2>&1",
        ] {
            assert_eq!(
                classify(command),
                CommandEvidence {
                    verification: true,
                    mutates: false,
                },
                "{command}"
            );
        }
    }

    #[test]
    fn effects_and_exit_status_are_never_guessed_from_check_named_operands() {
        for command in [
            "touch test_output",
            "printf test | sh",
            "cargo test $(touch result)",
            "cargo test > result",
            "cargo test &",
            "true || cargo test",
            "git diff --output=result",
            "find . -delete",
            "cargo fmt",
            "npx eslint --fix src",
            "bun test | tail -20",
            "bun test || true",
            "bun test 2> errors.txt",
            "bun run lint --fix",
        ] {
            assert!(classify(command).mutates, "{command}");
        }
        assert!(!classify("touch test_output").verification);
        assert!(!classify("cargo test; echo done").verification);
        assert!(!classify("bun run typecheck; echo done").verification);
        assert!(!classify("cargo test | tail").verification);
        assert!(classify("touch output; test -s output").verification);
        assert!(!classify("echo 'cargo test; touch x'").verification);
        assert!(!classify("echo 'cargo test; touch x'").mutates);
        assert!(verification_rejection("bun test | tail -20").is_some());
        assert!(verification_rejection("bun run typecheck; echo done").is_some());
        assert!(verification_rejection("bun test").is_none());
        assert!(!classify("bun run arbitrary").verification);
    }
}
