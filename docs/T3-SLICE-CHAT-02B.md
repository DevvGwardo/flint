# T3 chat slice 02B: genuine bounded syntax highlighting

Status: preparation only, not authorized implementation or acquisition.
F1 and F2a acceptance, dependency coordination and a concrete frontend proposal
must precede freezing this execution slice.

## Verified dependency surface

Installed `gpui-kit 0.7.0` forwards individual Tree-sitter features to
`gpui-component 0.7.0`. The intended first language set is JSON, Rust,
JavaScript, TypeScript/TSX, Python, Bash and Diff, not all grammar features.
The installed component manifest declares:

| Dependency | Declared version |
|---|---|
| tree-sitter | 0.26.13 |
| tree-sitter-json | 0.24.8 |
| tree-sitter-rust | 0.24.0 |
| tree-sitter-javascript | 0.23.1 |
| tree-sitter-typescript | 0.23.2 |
| tree-sitter-python | 0.23.6 |
| tree-sitter-bash | 0.23.3 |
| tree-sitter-diff | 0.1.0 |

These are source declarations, not a resolved or tested lock. The inspected
local cache has Tree-sitter 0.25.10 and Bash 0.25.1, not the required versions.
No crates have been downloaded or features enabled in this preparation.

Dependency acquisition must use a separately approved exact manifest/lock in
the run root, trusted registry/checksums, a filtered environment and the shared
build slot. It must not rewrite the actively backend-owned root lock, execute
remote scripts, upgrade unrelated dependencies, or contact a live provider.
Repository feature/lock integration follows only after ownership is released
and its narrow diff is reviewed. Subsequent validation remains offline/locked.

## Resource concern requiring a ruling

The installed component's default fenced-code highlighter keeps a thread-local
HashMap keyed by the raw reported language string
(`src/text/mod.rs:84–97`). Unknown tags can therefore allocate and retain
additional entries even if the transcript cache evicts their rows.
Enabling the feature alone is not an acceptable bounded implementation.

The frontend proposal must normalize a fixed allowlist and its finite aliases
before parser/cache construction. Unknown tags remain selectable plain code.
Use actual GPUI Tree-sitter grammars, not an ad-hoc regex lexer advertised as
syntax highlighting. Bound parser state, input bytes and highlight retention;
report any post-allocation counters honestly.

Verified public override surfaces are Base
`TextViewDefaults::global/with_code_block_highlighter/install` and
`TextView::shared_code_block_highlighter`. Replacing the highlighter must
preserve the installed style, use stable shared callback identity, and survive
theme/default reinstallation. Do not call private component helpers or modify
the registry source. Cover file previews and any native TextView path that
would otherwise invoke the unbounded raw-language default, not only chat.

## Intended evidence

- Actual token spans receive nondefault theme styles for every supported
  language, including TypeScript and TSX separately. Unknown/malformed language
  tags remain readable and Copy/Stage preserve exact source.
- Many unique attacker-controlled fence tags do not grow parser/cache entries.
  Known aliases share canonical state rather than allocating additional keys.
- Explicit highlighting source limits leave canonical text intact with a clear
  plain-code fallback. Unicode spans are valid UTF-8 boundaries.
- Streaming fences, final promotion, selection, theme changes, width changes,
  parent/child row ownership and resource-denial regressions pass.
- Real retained transcript and file-preview views use the bounded highlighter.
  Repainting unchanged code does not reparse because a fresh callback is built
  every frame.
- Exact feature/lock changes, checksum provenance, focused offline checks,
  strict app Clippy and failures are retained.

This feature remains pending. A language label, monospace output, dependency
declaration or detached parser test does not establish rendered highlighting.
