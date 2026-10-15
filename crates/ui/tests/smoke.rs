use crm_ui::AppWindow;

#[test]
fn main_window_constructs_headless() {
    i_slint_backend_testing::init_no_event_loop();
    AppWindow::new().unwrap();
}
