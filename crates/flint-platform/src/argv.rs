//! Windows native command-line quoting, not cmd.exe shell escaping.

/// Encode one literal argument for the usual Windows CRT argv decoder.
/// Always quote, including empty arguments and trailing backslashes.
pub fn quote_windows_arg(argument: &str) -> String {
    let mut quoted = String::from("\"");
    let mut backslashes = 0;
    for character in argument.chars() {
        if character == '\\' {
            backslashes += 1;
            continue;
        }
        quoted.extend(std::iter::repeat_n(
            '\\',
            if character == '"' {
                backslashes * 2 + 1
            } else {
                backslashes
            },
        ));
        quoted.push(character);
        backslashes = 0;
    }
    quoted.extend(std::iter::repeat_n('\\', backslashes * 2));
    quoted.push('"');
    quoted
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_arguments_keep_empty_quotes_backslashes_and_shell_characters() {
        for (input, output) in [
            ("", "\"\""),
            ("two words", "\"two words\""),
            ("a\"b", "\"a\\\"b\""),
            (r"a\b", r#""a\b""#),
            (r"a\", r#""a\\""#),
            (r#"a\"b"#, r#""a\\\"b""#),
            ("%PATH% & $HOME ! ^", "\"%PATH% & $HOME ! ^\""),
        ] {
            assert_eq!(quote_windows_arg(input), output, "{input:?}");
        }
    }
}
