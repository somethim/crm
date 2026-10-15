# AGENTS.md

This is a Rust Cargo workspace for a portable Windows desktop CRM (Slint UI, SQLite). [CLAUDE.md](CLAUDE.md) is the guide for every coding agent: the crate boundaries (no `rusqlite` outside `crates/storage`, no `slint` outside `crates/ui`), the money/quantity/date conventions, the Albanian glossary, the testing rules and the one-line commit format `what(where): details`. Read it before changing code. The previous app at commit `544086f` is a feature reference only (`git show 544086f:<path>`); plans live in the untracked `plans/` folder.

| Task | Command |
|---|---|
| Full local gate | `just verify` |
| Windows type-check from Linux | `just verify-win` |
| Run the app | `just run` |
| Release `crm.exe` for Windows | `just dist` |
