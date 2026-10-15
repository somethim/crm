# Patched libsqlite3-sys 0.38.2: SQLite3 Multiple Ciphers 2.6.1

This directory is `libsqlite3-sys` 0.38.2 from crates.io (the version rusqlite 0.40.2
uses), with its bundled SQLite replaced by **SQLite3 Multiple Ciphers 2.6.1** (SQLite
3.54.0), built with ChaCha20-Poly1305 only. The workspace uses it through
`[patch.crates-io]` in the root `Cargo.toml`; `exclude = ["vendor"]` keeps it out of the
workspace (fmt, clippy and tests do not run on it). Plans 013 and 014.

## Upstream sources

- Crate: <https://crates.io/crates/libsqlite3-sys/0.38.2> (MIT, `LICENSE`).
- sqlite3mc: <https://github.com/utelle/SQLite3MultipleCiphers/releases/tag/v2.6.1>, asset
  `sqlite3mc-2.6.1-sqlite-3.54.0-amalgamation.zip`
  (<https://github.com/utelle/SQLite3MultipleCiphers/releases/download/v2.6.1/sqlite3mc-2.6.1-sqlite-3.54.0-amalgamation.zip>),
  released 2026-10-12. MIT, `LICENSE-sqlite3mc` (the `LICENSE` file of the v2.6.1 tag).

| File | sha256 |
|---|---|
| the release zip | `3f25af603566a23c821d8e581778b5ece2a541d2435776de57b8d845c176815a` (matches the release's own `sqlite3mc-2.6.1-SHA256SUMS`) |
| `sqlite3/sqlite3mc_amalgamation.c` (byte-identical to the zip's file) | `9b129d1ceb2c7d5dbf374b833eb29a640ecfa8e4a5af6d48508b429019c707b0` |

`sqlite3/SHA256SUMS` holds the second line for `just vendor-check`.

## What differs from the crates.io crate

- `sqlite3/sqlite3.c` (stock SQLite 3.53.2 upstream) is replaced by a 1 KB wrapper that
  sets the build options and includes `sqlite3mc_amalgamation.c`:
  - `CODEC_TYPE_CHACHA20` as the default cipher; AES-128/256-CBC, SQLCipher, RC4, Ascon
    and AEGIS are compiled out;
  - `SQLITE_SECURE_DELETE`, `SQLITE3MC_SECURE_MEMORY` (freed memory is wiped) and
    `SQLITE_TEMP_STORE 2` (temporary data stays in memory) unless build.rs sets it;
  - the clang-cl `SODIUM_*` block (below).
- `sqlite3/sqlite3mc_amalgamation.c` is added (13.3 MB, committed: builds stay offline
  and reproducible; `.gitattributes` marks it `linguist-vendored -diff`).
- `sqlcipher/` is removed (only used by the `sqlcipher` feature, which is not enabled).
- `build.rs`, `Cargo.toml`, `src/`, `bindgen-bindings/` and everything else are unchanged.

### Headers and bindings

`sqlite3/sqlite3.h`, `sqlite3ext.h` and the prebuilt `bindgen_bundled_version*.rs`
stay the upstream **3.53.2** ones, and the amalgamation's `sqlite3mc_amalgamation.h`
is not used (013, A-02). rusqlite only uses an older API subset, which is identical in
3.54.0; the runtime reports 3.54.0. `crates/storage/tests/cipher_build.rs` asserts
`rusqlite::version()` starts with `3.54.` and `sqlite3mc_version()` is
`SQLite3 Multiple Ciphers 2.6.1`.

### The clang-cl workaround

cargo-xwin compiles C with clang-cl, which defines both `_MSC_VER` and
`__SIZEOF_INT128__`. sqlite3mc 2.6.1's poly1305 then takes its 128-bit code path but
defines neither of its `SODIUM_*` macro sets (one is keyed on MSVC, the other on
non-MSVC compilers), so the build fails. The wrapper defines the 128-bit set for that
exact combination only; GCC, Clang and MSVC `cl.exe` are unaffected.

## Updating

1. Download the new amalgamation zip, check it against the release's `SHA256SUMS`, and
   copy its `sqlite3mc_amalgamation.c` over `sqlite3/sqlite3mc_amalgamation.c`.
2. Update both hashes and the URL above, `sqlite3/SHA256SUMS`, the version comment in
   `sqlite3/sqlite3.c`, and the expected versions in `crates/storage/tests/cipher_build.rs`.
3. Touch `sqlite3/sqlite3.c` (build.rs only watches that file) or `cargo clean -p libsqlite3-sys`.
4. Run `just vendor-check verify dist`, then the CI `xwin-fixture` and `windows-fixture` jobs.
