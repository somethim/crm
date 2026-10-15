//! The gate with a real (testing) event loop: the Argon2 job runs on a worker
//! thread, its result comes back through `invoke_from_event_loop`, and a
//! successful unlock quits the loop on its own. One test per file: the
//! event-loop backend can be initialised once per process.

use crm_services::{DataPaths, KdfParams, StartState, Vault};
use crm_ui::{GateController, GateOutcome};
use slint::ComponentHandle;
use zeroize::Zeroizing;

#[test]
fn an_unlock_on_a_worker_thread_ends_the_gate_loop() {
    i_slint_backend_testing::init_integration_test_with_mock_time();
    let dir = tempfile::tempdir().unwrap();
    let vault = Vault::with_params(DataPaths::at(dir.path()), KdfParams::TEST);
    let key = vault.setup("fjalëkalim").unwrap().commit().unwrap();
    assert_eq!(vault.probe().unwrap(), StartState::Unlock);

    let gate = GateController::new(vault, &StartState::Unlock).unwrap();
    gate.window().show().unwrap();
    gate.submit_entry(Zeroizing::new("fjalëkalim".to_owned()));
    assert!(gate.window().get_busy(), "busy while Argon2 runs");
    assert!(!gate.is_done());

    slint::run_event_loop_until_quit().unwrap();

    let Some(GateOutcome::Open(got)) = gate.take_outcome() else {
        panic!("no key: {}", gate.window().get_error());
    };
    assert_eq!(got.to_hex_for_tests(), key.to_hex_for_tests());
    assert!(!gate.window().get_busy());
    assert!(!gate.window().window().is_visible());
}
