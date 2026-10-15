# CRM

A portable desktop CRM for Windows 11: invoices (Faturë Shitje / Faturë Blerje), stock, clients, suppliers, CSV/XLSX import, reports and receipt printing. It is one `crm.exe` with no installer, and it keeps its data in folders next to itself. The interface is in Albanian.

## Build and run

You need Rust (the version in `rust-toolchain.toml` is installed automatically by `rustup`) and [`just`](https://github.com/casey/just).

### Linux

Building needs the fontconfig headers (Debian package name):

```sh
sudo apt-get install libfontconfig1-dev
```

Running the window also needs the usual desktop libraries (xkbcommon, Wayland or X11), which any Linux desktop already has.

Then:

```sh
just run      # start the app
just verify   # format check, clippy, tests (and the Windows check if cargo-xwin is installed)
```

To build the Windows `crm.exe` from Linux, install `cargo-xwin` (`cargo install cargo-xwin --locked`) and run `just dist`. The exe is at `target/x86_64-pc-windows-msvc/release/crm.exe`.

### Windows

Install Rust with the MSVC toolchain (Visual Studio Build Tools), then:

```sh
cargo run -p crm
cargo build --release -p crm --target x86_64-pc-windows-msvc
```

The release exe is at `target\x86_64-pc-windows-msvc\release\crm.exe`. The `release` GitHub Actions workflow also builds it and uploads `crm-windows-x64.zip` (`crm.exe`, `README.md` and `LICENSES/NotoSans-OFL.txt`).

### Third-party licences

`crm.exe` embeds the Noto Sans Regular and Bold fonts (used on receipts), licensed under the SIL Open Font License 1.1. The licence text is `crates/reports/assets/fonts/OFL.txt` in the repository and `LICENSES/NotoSans-OFL.txt` in the release zip.

## Installing on a USB stick

Copy `crm.exe` (with the `LICENSES` folder from the release zip) into an empty folder on the stick and start it. **NTFS is recommended** for the stick; the database uses SQLite WAL, which needs a local disk (network shares are refused). Always eject the stick safely.

## Data folders

The app creates these folders next to `crm.exe` on first start:

| Folder | Contents |
|---|---|
| `data/` | `crm.db`, the encrypted SQLite database (with `crm.db-wal`, `crm.db-shm` and the instance lock while running), and `crm.key`, its wrapped key |
| `backups/` | Automatic backup copies |
| `logs/` | Daily log files |

Nothing is written to the registry or to the user profile, so the drive letter can change freely. Keep the folders together with `crm.exe`.

## Fjalëkalimi dhe enkriptimi (password and encryption)

Everything the app stores is encrypted: `data/crm.db` (with its WAL), every copy in `backups/` and every "Back up to this PC" copy (SQLite3 Multiple Ciphers, ChaCha20-Poly1305). The key is random; `data/crm.key` keeps it wrapped under your password and under a recovery key. Each backup copy `X.db` has a sibling `X.key` (a copy of `crm.key`): keep them together.

**Setup.** On first start (or the first start after upgrading, when the existing data is encrypted in place) the app asks for a password of at least 6 characters, then shows two codes **once**:

- the **recovery key** (8 groups of 4 characters): opens the data if you forget the password; the app then asks for a new password;
- the **emergency wipe code** (3 groups of 4 characters): typed instead of the password, at start-up or at the lock screen, it deletes the key, the data, every backup on the stick and the logs, and the app starts as a fresh install.

Write both on paper and keep them away from the USB stick. The key file is written only after you confirm "I kam shkruar kodet".

**Unlock.** Five wrong entries in a row lock the entry for 30 seconds. "Ndrysho fjalëkalimin…" in the File menu changes the password; it also rewrites the `.key` siblings in `backups/`, so the old password opens none of them.

**Limitations.**

- Losing both the password and the recovery key loses the data. There is no other way in.
- An older genuine copy of the files (an earlier backup put back in place) is not detected as such: it opens with the password that was valid for it.
- "Back up to this PC" copies are outside the wipe's reach, and their `.key` keeps the password that was valid when the copy was made (the recovery key still opens them).
- Overwriting before deleting is best effort on flash memory: a plaintext remnant of the old, unencrypted data may survive the upgrade on the stick.
- After a wipe from the lock screen, a second launch of `crm.exe` activates the running app again only after a restart.
- The previous version cannot open the encrypted data: there is no going back after the upgrade.

## Backups

- The app keeps rotating copies in `backups/`: 7 daily, 4 weekly and 12 monthly.
- It also takes a copy before every import and before every database upgrade.
- "Back up to this PC" in the File menu writes a copy to a folder you choose. Use it regularly: a USB stick can fail or get lost.
- "Restore from backup" in the File menu replaces the current database with a chosen copy.
