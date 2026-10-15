//! Plan 015 step 8: a full flow under a TRACE-level subscriber logs no secret,
//! and no secret type prints its secret through `Debug`.

mod common;

use common::product;
use crm_services::{
    Crm, DataPaths, KdfParams, StorageError, Unlock, Vault, VaultError, secure_storage,
};
use std::io::Write;
use std::sync::{Arc, Mutex};

#[derive(Clone)]
struct Capture(Arc<Mutex<Vec<u8>>>);

impl Write for Capture {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

const PASSWORD: &str = "sekret-fjalëkalim";
const NEW_PASSWORD: &str = "tjetër-sekret-2026";

#[test]
fn a_full_flow_logs_no_secret() {
    let captured = Arc::new(Mutex::new(Vec::new()));
    let writer = Capture(captured.clone());
    let subscriber = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::TRACE)
        .with_ansi(false)
        .with_writer(move || writer.clone())
        .finish();
    tracing::subscriber::set_global_default(subscriber).unwrap();

    let dir = tempfile::tempdir().unwrap();
    let pc = tempfile::tempdir().unwrap();
    let paths = DataPaths::at(dir.path());

    // Plaintext data from an older version.
    common::legacy_root(&paths, 1);

    let vault = Vault::with_params(paths.clone(), KdfParams::TEST);
    tracing::info!("start state {:?}", vault.probe().unwrap());
    let pending = vault.setup(PASSWORD).unwrap();
    let recovery = pending.recovery_display().to_string();
    let wipe = pending.wipe_display().to_string();
    let key_hex = pending.db_key_for_tests().to_hex_for_tests();
    tracing::debug!("pending {pending:?}");
    let key = pending.commit().unwrap();
    tracing::debug!("key {key:?}");
    secure_storage(&paths, &key).unwrap();

    let crm = Crm::open(paths.clone(), &key).unwrap();
    product(&crm, "B1", "Artikull B", 1, 1, 2);
    crm.backup_now().unwrap();
    crm.backup_to_pc(pc.path()).unwrap();
    assert!(matches!(
        vault.submit("gabim-gabim").unwrap(),
        Unlock::Wrong
    ));
    let outcome = vault.submit(&recovery).unwrap();
    tracing::debug!("outcome {outcome:?}");
    let Unlock::Recovered(token) = outcome else {
        panic!("not recovered");
    };
    tracing::debug!("token {token:?}");
    let again = vault
        .set_password_after_recovery(token, NEW_PASSWORD)
        .unwrap();
    tracing::debug!("again {again:?}");
    assert_eq!(
        vault.change_password(PASSWORD, "x-x-x-x-x-x"),
        Err(VaultError::Wrong)
    );
    vault.change_password(NEW_PASSWORD, PASSWORD).unwrap();
    let mut typo: Vec<char> = wipe.chars().collect();
    typo[0] = if typo[0] == '7' { '8' } else { '7' };
    let typo: String = typo.into_iter().collect();
    assert!(matches!(vault.submit(&typo).unwrap(), Unlock::Wrong));
    let wrong_key = crm_services::DbKey::for_tests(3);
    let err = Crm::open(DataPaths::at(dir.path()), &wrong_key).unwrap_err();
    tracing::warn!("expected failure: {err} / {err:?}");
    assert!(matches!(
        err,
        crm_services::ServiceError::Storage(StorageError::WrongKey)
    ));
    let outcome = vault.submit_with(&wipe, || crm.abandon()).unwrap();
    assert!(matches!(outcome, Unlock::Wiped));
    assert!(!crm.refresh_if_changed());

    let text = String::from_utf8(captured.lock().unwrap().clone()).unwrap();
    assert!(text.contains("start state"), "the capture works");
    assert!(text.contains("expected failure"), "the capture works");
    let secrets = [
        key_hex.clone(),
        key_hex.to_uppercase(),
        PASSWORD.to_owned(),
        NEW_PASSWORD.to_owned(),
        recovery.clone(),
        recovery.replace('-', ""),
        wipe.clone(),
        wipe.replace('-', ""),
        typo.replace('-', ""),
    ];
    for secret in &secrets {
        assert!(!text.contains(secret.as_str()), "a secret reached the log");
    }
    let logs: String = std::fs::read_dir(&paths.logs)
        .map(|entries| {
            entries
                .flatten()
                .map(|e| std::fs::read_to_string(e.path()).unwrap_or_default())
                .collect()
        })
        .unwrap_or_default();
    for secret in &secrets {
        assert!(!logs.contains(secret.as_str()));
    }
}

#[test]
fn debug_output_of_secret_types_is_redacted() {
    let dir = tempfile::tempdir().unwrap();
    let vault = Vault::with_params(DataPaths::at(dir.path()), KdfParams::TEST);
    let pending = vault.setup(PASSWORD).unwrap();
    let recovery = pending.recovery_display().replace('-', "");
    let wipe = pending.wipe_display().replace('-', "");
    let hex = pending.db_key_for_tests().to_hex_for_tests();
    let mut texts = vec![format!("{pending:?}"), format!("{vault:?}")];
    let key = pending.commit().unwrap();
    texts.push(format!("{key:?}"));
    texts.push(format!("{:?}", vault.submit(PASSWORD).unwrap()));
    let Unlock::Recovered(token) = vault.submit(&recovery).unwrap() else {
        panic!("not recovered");
    };
    texts.push(format!("{token:?}"));
    texts.push(format!("{:?}", Unlock::Recovered(token)));
    let rk = crm_services::classify(&recovery);
    texts.push(format!("{rk:?}"));
    for text in texts {
        for secret in [&hex, &recovery, &wipe, &PASSWORD.to_owned()] {
            assert!(!text.contains(secret.as_str()), "{text}");
        }
    }
}
