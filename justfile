# Lists the recipes.
default:
    @just --list

# Formats every crate.
fmt:
    cargo fmt --all

# Fails when any file is not formatted.
fmt-check:
    cargo fmt --all -- --check

# Lints every crate and target; warnings are errors.
clippy:
    cargo clippy --workspace --all-targets -- -D warnings

# Runs every test, including the headless Slint smoke test.
test:
    cargo test --workspace

# Type-checks the Windows target through cargo-xwin.
verify-win:
    cargo xwin check --target x86_64-pc-windows-msvc --workspace --all-targets

# The release profile unwinds (F-014): asserts the panic strategy rustc gets for a
# release build, then runs the writer's panic-containment tests in release mode.
verify-panic:
    #!/usr/bin/env sh
    set -e
    strategy=$(cargo rustc -q -p crm-storage --release --lib -- --print cfg | grep '^panic=')
    if [ "$strategy" != 'panic="unwind"' ]; then
        echo "release profile must unwind, got: $strategy"
        exit 1
    fi
    cargo test --release -p crm-storage --lib db::tests

# The local gate: fmt-check, clippy, tests and the release panic check, then the Windows check when cargo-xwin is installed.
verify: vendor-check fmt-check clippy test verify-panic
    #!/usr/bin/env sh
    if command -v cargo-xwin >/dev/null 2>&1; then
        just verify-win
    else
        echo "skip: cargo-xwin missing"
    fi

# Fails when the vendored sqlite3mc amalgamation differs from the hash in PATCHED.md.
vendor-check:
    cd vendor/libsqlite3-sys/sqlite3 && sha256sum -c SHA256SUMS

# Builds the encrypted-fixture tests (storage database, services key file) for Windows
# (clang-cl) and prints the .exe paths.
win-fixture:
    cargo xwin test --release --no-run --target x86_64-pc-windows-msvc -p crm-storage -p crm-services --test encrypted_fixture --test keyfile_fixture --message-format=json | jq -r 'select(.executable != null and (.target.name == "encrypted_fixture" or .target.name == "keyfile_fixture")) | .executable'

# Starts the app.
run:
    cargo run -p crm

# Builds the release crm.exe for Windows (target/x86_64-pc-windows-msvc/release/crm.exe).
dist:
    cargo xwin build --release -p crm --target x86_64-pc-windows-msvc
