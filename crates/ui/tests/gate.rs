//! Headless tests of the start-up gate (plan 016): first-run setup and the
//! codes screen, the migration of plaintext data, unlock, recovery and the wipe.
//! Worker jobs run inline (no event loop here).

use crm_services::{
    Crm, DataPaths, DbKey, KdfParams, StartState, Unlock, Vault, messages, secure_storage,
};
use crm_ui::shell::{texts, worker};
use crm_ui::{GateController, GateOutcome, UnlockWindow};
use i_slint_backend_testing::ElementHandle;
use slint::ComponentHandle;
use std::path::Path;
use std::rc::Rc;

const PASSWORD: &str = "fjalëkalim";

fn element(w: &UnlockWindow, id: &str) -> ElementHandle {
    ElementHandle::find_by_element_id(w, id)
        .next()
        .unwrap_or_else(|| panic!("element {id} not found"))
}

fn type_into(w: &UnlockWindow, id: &str, text: &str) {
    element(w, id).set_accessible_value(text);
}

fn click(w: &UnlockWindow, id: &str) {
    element(w, id).invoke_accessible_default_action();
}

thread_local! {
    static INIT: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

fn gate(dir: &Path) -> (Vault, Rc<GateController>) {
    if !INIT.with(|i| i.replace(true)) {
        i_slint_backend_testing::init_no_event_loop();
    }
    worker::set_inline_for_tests(true);
    let vault = Vault::with_params(DataPaths::at(dir), KdfParams::TEST);
    let state = vault.probe().unwrap();
    let gate = GateController::new(vault.clone(), &state).unwrap();
    gate.window().show().unwrap();
    (vault, gate)
}

fn new_password(w: &UnlockWindow, password: &str, repeat: &str) {
    type_into(w, "PasswordScreen::first", password);
    type_into(w, "PasswordScreen::second", repeat);
    click(w, "PasswordScreen::ok");
}

fn entry(w: &UnlockWindow, text: &str) {
    type_into(w, "PasswordScreen::first", text);
    click(w, "PasswordScreen::ok");
}

fn is_grouped(code: &str, groups: usize) -> bool {
    let parts: Vec<&str> = code.split('-').collect();
    parts.len() == groups
        && parts.iter().all(|p| {
            p.len() == 4
                && p.bytes()
                    .all(|b| b.is_ascii_digit() || b.is_ascii_uppercase())
        })
}

/// Runs a first-run setup through the gate; returns the codes and the key.
fn first_run(dir: &Path) -> (String, String, DbKey) {
    let (_vault, gate) = gate(dir);
    let w = gate.window();
    new_password(w, PASSWORD, PASSWORD);
    let recovery = w.get_recovery_code().to_string();
    let wipe = w.get_wipe_code().to_string();
    click(w, "PasswordScreen::written");
    click(w, "PasswordScreen::next");
    let Some(GateOutcome::Open(key)) = gate.take_outcome() else {
        panic!("no key");
    };
    (recovery, wipe, key)
}

#[test]
fn first_run_validates_then_shows_the_codes_once() {
    let dir = tempfile::tempdir().unwrap();
    let paths = DataPaths::at(dir.path());
    let (_vault, gate) = gate(dir.path());
    let w = gate.window();
    assert_eq!(w.get_mode(), "setup");

    new_password(w, PASSWORD, "fjalëkalin");
    assert_eq!(w.get_error(), texts::PASSWORD_MISMATCH);
    new_password(w, "12345", "12345");
    assert_eq!(w.get_error(), messages::PASSWORD_TOO_SHORT);
    // A code-shaped password is refused.
    let other = tempfile::tempdir().unwrap();
    let wipe_shaped = Vault::with_params(DataPaths::at(other.path()), KdfParams::TEST)
        .setup("tjetër-fjalëkalim")
        .unwrap()
        .wipe_display()
        .to_string();
    new_password(w, &wipe_shaped, &wipe_shaped);
    assert_eq!(w.get_error(), messages::PASSWORD_NOT_ALLOWED);
    assert!(w.get_error().contains("nuk lejohet"));
    assert!(!paths.key.exists());

    new_password(w, PASSWORD, PASSWORD);
    assert_eq!(w.get_mode(), "codes", "{}", w.get_error());
    assert!(
        is_grouped(&w.get_recovery_code(), 8),
        "{}",
        w.get_recovery_code()
    );
    assert!(is_grouped(&w.get_wipe_code(), 3), "{}", w.get_wipe_code());
    assert!(
        !paths.key.exists(),
        "nothing is written before the confirmation"
    );
    assert_eq!(
        element(w, "PasswordScreen::next").accessible_enabled(),
        Some(false)
    );
    click(w, "PasswordScreen::written");
    assert_eq!(
        element(w, "PasswordScreen::next").accessible_enabled(),
        Some(true)
    );
    click(w, "PasswordScreen::next");
    assert!(matches!(gate.take_outcome(), Some(GateOutcome::Open(_))));
    assert!(paths.key.exists());
    assert!(!w.window().is_visible());
}

#[test]
fn closing_the_gate_before_the_confirmation_writes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let paths = DataPaths::at(dir.path());
    let (_vault, gate) = gate(dir.path());
    new_password(gate.window(), PASSWORD, PASSWORD);
    assert_eq!(gate.window().get_mode(), "codes");
    drop(gate);
    assert!(!paths.key.exists());
    let vault = Vault::with_params(paths, KdfParams::TEST);
    assert_eq!(vault.probe().unwrap(), StartState::FirstRun);
}

#[test]
fn plaintext_data_is_migrated_after_the_setup() {
    let dir = tempfile::tempdir().unwrap();
    let paths = DataPaths::at(dir.path());
    let ts = "2026-10-14T00:00:00Z";
    crm_storage_seed(&paths, ts);
    let (_vault, gate) = gate(dir.path());
    let w = gate.window();
    assert_eq!(w.get_mode(), "migrate");
    new_password(w, PASSWORD, PASSWORD);
    click(w, "PasswordScreen::written");
    click(w, "PasswordScreen::next");
    let Some(GateOutcome::Open(key)) = gate.take_outcome() else {
        panic!("no key");
    };
    secure_storage(&paths, &key).unwrap();
    assert!(
        !std::fs::read(&paths.db)
            .unwrap()
            .starts_with(b"SQLite format 3")
    );
    let crm = Crm::open(paths.clone(), &key).unwrap();
    assert_eq!(
        crm.search_owners(crm_services::domain::OwnerKind::Client, "klienti", 10)
            .unwrap()
            .len(),
        2
    );
    crm.close().unwrap();
}

/// Two clients in a plaintext database, as an older version left it.
fn crm_storage_seed(paths: &DataPaths, ts: &str) {
    let mut seed = String::new();
    for name in ["Klienti Alfa", "Klienti Beta"] {
        seed.push_str(&format!(
            "INSERT INTO owners (kind, name, name_key, created_at, updated_at)
             VALUES (0, '{name}', lower('{name}'), '{ts}', '{ts}');"
        ));
    }
    crm_services::create_legacy_plaintext(paths, &seed).unwrap();
}

#[test]
fn unlock_wrong_then_lockout() {
    let dir = tempfile::tempdir().unwrap();
    first_run(dir.path());
    let (_vault, gate) = gate(dir.path());
    let w = gate.window();
    assert_eq!(w.get_mode(), "unlock");
    for _ in 0..4 {
        entry(w, "gabim-gabim");
        assert_eq!(w.get_error(), "Fjalëkalim i gabuar");
    }
    entry(w, "gabim-gabim");
    assert_eq!(w.get_error(), "Provoni përsëri pas 30 sekondash");
    assert!(!gate.is_done());
}

#[test]
fn unlock_with_the_password() {
    let dir = tempfile::tempdir().unwrap();
    let (_, _, key) = first_run(dir.path());
    let (_vault, gate) = gate(dir.path());
    entry(gate.window(), PASSWORD);
    let Some(GateOutcome::Open(got)) = gate.take_outcome() else {
        panic!("no key");
    };
    assert_eq!(got.to_hex_for_tests(), key.to_hex_for_tests());
}

#[test]
fn the_recovery_key_then_a_mandatory_new_password() {
    let dir = tempfile::tempdir().unwrap();
    let (recovery, _, key) = first_run(dir.path());
    let (vault, gate) = gate(dir.path());
    let w = gate.window();
    entry(w, &recovery);
    assert_eq!(w.get_mode(), "new-password");
    new_password(w, "i-ri-fjalëkalim", "i-ri");
    assert_eq!(w.get_error(), texts::PASSWORD_MISMATCH);
    new_password(w, "i-ri-fjalëkalim", "i-ri-fjalëkalim");
    let Some(GateOutcome::Open(got)) = gate.take_outcome() else {
        panic!("no key: {}", w.get_error());
    };
    assert_eq!(got.to_hex_for_tests(), key.to_hex_for_tests());
    assert!(matches!(vault.submit(PASSWORD).unwrap(), Unlock::Wrong));
}

#[test]
fn the_wipe_code_gives_a_fresh_first_run() {
    let dir = tempfile::tempdir().unwrap();
    let (_, wipe, key) = first_run(dir.path());
    let paths = DataPaths::at(dir.path());
    let crm = Crm::open(paths.clone(), &key).unwrap();
    crm.backup_now().unwrap();
    crm.close().unwrap();
    std::fs::write(paths.logs.join("crm.log"), b"line").unwrap();

    let (_vault, gate) = gate(dir.path());
    let w = gate.window();
    // A mistyped wipe code wipes nothing.
    let mut typo: Vec<char> = wipe.chars().collect();
    typo[1] = if typo[1] == '7' { '8' } else { '7' };
    let typo: String = typo.into_iter().collect();
    let before = std::fs::read_dir(&paths.backups).unwrap().count();
    entry(w, &typo);
    assert_eq!(w.get_error(), "Fjalëkalim i gabuar");
    assert_eq!(std::fs::read_dir(&paths.backups).unwrap().count(), before);
    assert!(paths.key.exists());

    entry(w, &wipe);
    assert_eq!(w.get_mode(), "setup");
    assert_eq!(w.get_error(), "");
    assert!(!gate.is_done());
    for dir in [&paths.backups, &paths.logs, &paths.data_dir()] {
        assert_eq!(
            std::fs::read_dir(dir).unwrap().count(),
            0,
            "{}",
            dir.display()
        );
    }
}
