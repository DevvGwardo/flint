# Windows setup and validation

Windows support is experimental. The workspace can be cross-checked from macOS,
but native Windows execution must pass the Windows CI job and the checks below
before a Windows release is considered verified. Linux remains untested.

## Requirements

- Windows 10 version 1809 or newer (ConPTY), or Windows 11. Windows 11 x86-64 is
  the CI target. Windows ARM64 has not been validated.
- A Direct3D 11-capable graphics driver.
- Rust installed with rustup and the MSVC toolchain. `rust-toolchain.toml` pins
  the Rust version.
- Visual Studio 2022 Build Tools, **Desktop development with C++**, and a
  Windows 10/11 SDK. The SDK includes `fxc.exe`, which GPUI needs for release
  shaders. If discovery fails, set `GPUI_FXC_PATH` to its full path.
- Git for Windows. Node.js/npm are needed only for npm-installed ACP adapters;
  Python is needed for development tests, not for normal native-agent use.

Build from a Developer PowerShell for VS 2022:

```powershell
cargo run --release -p flint-app
cargo run --release -p flint-app -- "C:\code\my-project"
cargo run -p flint-app -- --demo
```

The executable is `target\release\hooter.exe`. A release launch does not open an
extra console window. API keys and provider endpoints are configured in
Settings, as on macOS. Do not place credentials in command-line arguments.

## Windows behavior

- Data lives in `%USERPROFILE%\.flint`, unless `FLINT_HOME` overrides it. Home
  lookup also supports `HOME` and `HOMEDRIVE`/`HOMEPATH` fallbacks.
- Native agent commands use `cmd.exe /D /S /C`. The prompt and tool description
  tell the model to use Windows syntax. Invoke `powershell.exe -NoProfile
  -Command "..."` explicitly for PowerShell scripts. Git Bash is not required.
- The terminal dock uses `cmd.exe /D` through ConPTY. Explicit custom executable
  paths with spaces are quoted.
- **Ctrl** replaces **Command** for app shortcuts. Ctrl+Enter steers or answers
  an approval. Ctrl+Shift+C/V copies/pastes in the terminal; Ctrl+C without a
  selection still interrupts the shell.
- Reveal workspace opens File Explorer. Open external terminal uses Windows
  Terminal when available and a new Command Prompt otherwise.
- Adapter/MCP lookup preserves Windows PATHs and finds `.exe`, `.com`, `.bat`,
  and npm `.cmd` shims. Literal executable/argv requests are not converted into
  shell scripts. ACP adapters and their underlying CLIs must themselves support
  Windows; this port does not change their platform requirements.
- Native commands, ACP terminals/adapters, and MCP servers run in owned
  kill-on-close Windows Job Objects. Children start suspended until assigned to
  the job. Cancellation and shutdown cannot leave ordinary descendants running.
- Saved native history uses non-inheritable directory-relative handles,
  refuses reparse points and alternate streams, takes an OS byte-range writer
  lock, flushes file contents, and atomically publishes unique temporary files.
  Newly created private files/directories have protected owner/SYSTEM ACLs.
  Windows has no supported Unix-style directory `fsync`; sudden power-loss
  durability still depends on Windows and the filesystem.
- **There is no Windows command sandbox.** Job Objects control process
  lifetime, not filesystem/network access. File tools stay workspace-confined,
  but shell commands and MCP/ACP agents run with your account's permissions.
  Keep **Ask before changes** enabled unless you intend to grant that access.

## Validation

Run from Developer PowerShell, with Python on PATH:

```powershell
$env:FLINT_LIVE = "0"
$env:FLINT_EVAL = "0"
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --exclude flint-app --locked -- --test-threads=1
cargo test -p flint-app --lib --locked -- --test-threads=1
cargo test -p flint-app --test platform_ui --locked -- --test-threads=1
cargo build --release -p flint-app --bin hooter --locked
pwsh -File tools/windows/smoke.ps1 -Executable target/release/hooter.exe
```

The smoke test uses a fresh disposable home, renders an offline scripted demo,
checks successful exit and first-frame evidence, and retains logs/state under a
unique output directory. It neither contacts a provider nor opens saved user
sessions. The Windows CI job runs these checks and retains the executable and
smoke evidence as an artifact; it does not publish a release.

The larger `blueprint_ui` suite and Python/macOS screenshot/AX harness still run
on macOS. Windows CI compiles those tests, runs the focused cross-platform UI
suite, and smoke-tests the real Windows executable instead of claiming the
macOS-specific visual checks passed.

Before declaring a release ready, test a real Windows desktop: window
minimize/maximize/close, DPI scaling, clipboard/images, IME input, terminal
resize and Ctrl+C, provider streaming, agent approvals/cancellation, saved-session
restart, file previews, Git worktrees, and whichever ACP agents you use.

### Cross-checking on macOS

`cargo-xwin` can install the Windows SDK/CRT into a local cache:

```sh
rustup target add x86_64-pc-windows-msvc
cargo install cargo-xwin --locked
cargo xwin check --target x86_64-pc-windows-msvc --workspace --all-targets --locked
cargo xwin clippy --target x86_64-pc-windows-msvc --workspace --all-targets --locked -- -D warnings
cargo xwin build --target x86_64-pc-windows-msvc -p flint-app --bin hooter --locked
```

Cross-compilation is not a Windows runtime test. Build optimized Windows
releases natively: GPUI's release shader compilation needs the Windows SDK's
`fxc.exe` running on Windows.

Keep cargo-xwin's SDK cache in a path without spaces, such as its normal
macOS `~/Library/Caches/cargo-xwin` location. Moving `HOME` into a directory
with spaces can make its generated rustflags fail before compilation. The
repository itself was successfully cross-built from a path containing spaces.
