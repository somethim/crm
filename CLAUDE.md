# CLAUDE.md

Guidance for Claude Code and other coding agents working in this repository.

## Project

A portable Windows 11 desktop CRM for an Albanian beverage and food wholesaler: order keeping (Porosi Shitje / Porosi Blerje), stock, clients, suppliers, CSV/XLSX import, reports and receipt printing. It ships as one `crm.exe` that runs from a USB stick and keeps a local, encrypted SQLite database next to itself. Rust Cargo workspace, Slint UI (Fluent style), rusqlite over SQLite3 Multiple Ciphers (compiled in, WAL, ChaCha20-Poly1305).

### The previous app (feature reference only)

The previous server-hosted version (a PHP API plus a TypeScript web front end, formerly `apps/provider` and `apps/web`) was removed from the tree. It is a **feature reference only, not a style guide**: use it to check what a feature did, never to copy its structure or code style. Read it from history at commit `544086f`:

```sh
git show 544086f:apps/provider/app/Actions/Import.php
git ls-tree -r --name-only 544086f -- apps/web/src
```

`apps/crm` is the new Rust binary; nothing else lives under `apps/`.

### Plans

Task plans live in `plans/` (untracked, excluded via `.git/info/exclude`). `plans/001-crm-rust-desktop-rewrite.md` is the epic; each `plans/NNN-*.md` is one task with its own steps and verification.

## Workspace layout and crate boundaries

```
crates/domain    crm-domain    types, rules, formatting; no IO
crates/storage   crm-storage   SQLite: migrations, writer thread, reader pool, backups, instance lock
crates/search    crm-search    in-memory fuzzy search index
crates/io        crm-io        CSV/XLSX import, CSV export, XLSX reports
crates/services  crm-services  use cases orchestrating storage, search and io
crates/reports   crm-reports   receipt rendering and printing
crates/ui        crm-ui        Slint windows and dialogs (ui/*.slint)
apps/crm         crm           the binary (crm.exe), Windows resources in apps/crm/resources
vendor/libsqlite3-sys  patched libsqlite3-sys 0.38.2 with SQLite3 Multiple Ciphers 2.6.1 (see PATCHED.md)
```

Dependency graph (an arrow means "is used by"):

```
domain  ← storage, search, io, reports, services, ui
storage ← services, apps/crm
search  ← services
io      ← services
reports ← ui
services← ui, apps/crm
ui      ← apps/crm
```

Rules:

- `crm-domain` has no IO (deps: thiserror, rust_decimal, jiff, serde).
- **No `rusqlite` outside `crates/storage`.** Its types never appear in another crate's API.
- **No `slint` outside `crates/ui`** (only `apps/crm` may drive the event loop).
- `anyhow` only in `apps/crm`; libraries use `thiserror` error types.
- Every dependency is pinned with `=` in the root `[workspace.dependencies]`; crates use `dep.workspace = true`. Do not add dependencies without the plan saying so.
- `unsafe_code` is denied workspace-wide. Only `cfg(windows)` FFI modules may opt out, with `#[allow(unsafe_code)]` and a `// SAFETY:` comment on every block.
- Clippy runs with `-D warnings`.

## Conventions

- **Money:** integer minor units (qindarka, `i64`). `rust_decimal` is used only at the parse and format edges.
- **Quantities:** integers everywhere. Stock can never go negative.
- **Dates:** `jiff`, Europe/Tirane. Orders carry an issue date `issued_on` (`YYYY-MM-DD`); filters, reports and receipts use it.
- **Formatting** lives in `crm_domain::format` only: money `1.234,56 Lekë`, date `dd.MM.yyyy`, timestamp `dd.MM.yyyy HH:mm`, quantity `1.234`. UI and reports never format ad hoc.
- **Albanian-only UI.** No i18n crate, no English user-visible strings. Use the glossary below.

### Glossary (fixed Albanian terms)

| Concept | Term | Notes |
|---|---|---|
| Order type 0 / 1 | Shitje / Blerje; document "Porosi Shitje" / "Porosi Blerje"; report Lloji column "Shitje" / "Blerje"; menu "Porositë" | New tab titles "Porosi Shitje (e re)" / "Porosi Blerje (e re)"; toolbar "+ Porosi Shitje" / "+ Porosi Blerje"; receipt header "Porosia Nr: …". Never "Faturë"/"Fatura" |
| Owner kind 0 / 1 | Klient / Furnitor; plural Klientët / Furnitorët | Never "Furnizues" |
| Product | Artikull; plural Artikujt | Menu "Artikujt" |
| Password lock | Fjalëkalimi; Çelësi i rikuperimit (recovery key); Kodi i fshirjes emergjente (wipe code); menu "Ndrysho fjalëkalimin…" | Never "PIN" |
| Order number | Nr. Porosie | |
| Fields | Kodi, Emri, Njësia, Çmimi i blerjes, Çmimi i shitjes, Gjendja (stock), Sasia (line quantity), Çmimi, Vlera, Totali, Përshkrimi, NIPT, Adresa, Llogari, Data, Modifikuar | |
| Buttons | OK, Anulo, Zbato, Ruaj, Fshi, Printo, Mbyll, Importo, Eksporto, Rifresko, Shto, Ndrysho, Kërko | |
| Messages | "Duke kërkuar…", "Nuk u gjet asnjë rezultat", "Mbyll të gjitha", "Mbyll të tjerat", "Mbyll {t}", "Porosia #id", "Kërko", "Zgjidhni datën", "Rreshti n:", "…dhe n gabime të tjera.", "çmimin e blerjes dhe të shitjes" | Replace the old English strings |
| Stock error | "Gjendje e pamjaftueshme për «{emri}»: ka {N}, kërkohen {M}." | |
| Purchase delete/edit blocked | "Kjo porosi blerjeje nuk mund të {fshihet\|ndryshohet}: malli i saj është shitur tashmë («{emri}»: gjendja {N}, duhet të hiqen {M})." | |

## Commands

| Task | Command |
|---|---|
| Full local gate (fmt check, clippy `-D warnings`, tests, then the Windows check if cargo-xwin is installed) | `just verify` |
| Windows type-check from Linux | `just verify-win` |
| Run the app | `just run` |
| Release `crm.exe` for Windows from Linux | `just dist` |
| Check the vendored sqlite3mc amalgamation hash (part of `verify`) | `just vendor-check` |
| Build the encrypted-fixture tests for Windows (clang-cl) and print the `.exe` paths | `just win-fixture` |
| Format | `just fmt` |
| One crate's tests | `cargo test -p crm-<name>` |

The toolchain is pinned in `rust-toolchain.toml` (1.97.1, with the `x86_64-pc-windows-msvc` target). Windows builds link the CRT statically (`.cargo/config.toml`). CI runs `just fmt-check clippy test` on Linux (`.github/workflows/ci.yml`); `release.yml` builds `crm.exe` on windows-latest and uploads `crm-windows-x64.zip`.

## Data layout

Everything lives next to `crm.exe`, resolved from `std::env::current_exe()`. No registry, no APPDATA, no stored absolute paths (the USB drive letter changes):

```
crm.exe
data/crm.db        SQLite, WAL, synchronous=FULL (plus crm.db-wal, crm.db-shm, crm.lock), encrypted
data/crm.key       the wrapped data key (format v1, 260 bytes)
backups/           rotating VACUUM INTO copies, pre-import and pre-migration copies, each X.db with
                   its X.key sibling (a byte copy of data/crm.key)
logs/              daily rolling logs
```

The database, its WAL and every backup are encrypted with SQLite3 Multiple Ciphers ChaCha20-Poly1305 under a random 32-byte data key (`crm_storage::DbKey`; every connection runs `PRAGMA key` first). `data/crm.key` wraps that key under the password and under the recovery key (Argon2id + XChaCha20-Poly1305, `crm_services::Vault`); the emergency wipe code is checked against an Argon2id verifier in the same file. Nothing opens the database without a key: the start-up gate (`crm_ui::run_gate`) unlocks, sets up or migrates first. Never log key material, passwords or codes.

## Testing

- Tests are mandatory for `crm-domain`, `crm-storage`, `crm-search`, `crm-io` and `crm-services`. Every rule or bug fix comes with a test.
- Storage tests use temporary databases (`tempfile`), never files in the repo. Exception: `crates/storage/tests/fixtures/encrypted-v1.db` is a committed encrypted fixture (key `DbKey::for_tests(7)`); regenerate it only with `cargo test -p crm-storage --test encrypted_fixture -- --ignored regenerate_fixture`. Likewise `crates/services/tests/fixtures/crm-v1.key` wraps that key (`KdfParams::TEST`, password and codes in `keyfile_fixture.rs`); regenerate it only with `cargo test -p crm-services --test keyfile_fixture -- --ignored regenerate_fixture`.
- UI tests run headless through `i-slint-backend-testing` (see `crates/ui/tests/smoke.rs`).
- Sample import files are in `crates/io/tests/fixtures/` (ARTIKUJ, KLIENTET, FURNITORET). They are synthetic: every name, address, NIPT, account code and price is invented, written by `crates/io/tests/fixtures/generate.py` (deterministic, standard library only) in the layout of the Bilanc exports. Never commit real client, supplier or product data, in fixtures or in test strings. To change them, edit the generator and rerun `python3 crates/io/tests/fixtures/generate.py`; never hand-edit the CSVs. The rows the tests look up by value are planted in the generator.

## Commits

Every commit message is exactly one line: `what(where): details`, for example `feat(storage): add the backup rotation` or `fix(orders): block deleting a sold purchase`. `what` is a lowercase Conventional Commit type; `where` is the crate, feature or task. No body, no trailers, no co-author lines.
