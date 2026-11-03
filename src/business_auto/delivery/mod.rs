mod executor;
mod message;
pub mod ports;
mod runtime;

pub use executor::{
    execute_delivery_run, execute_delivery_run_as, execute_delivery_run_with_admin,
    WAITING_STOCK_BUYER_NOTICE,
};
pub use message::compose_delivery_message;
pub use runtime::{
    clear_wake_sender_if_generation, install_test_wake_sender, new_worker_instance_id,
    start_runtime, wake_account, wake_pool, worker_name, TestWakeInstall,
};

#[cfg(test)]
fn record_test_account_wake(uid: &str, reason: &str) {
    #[cfg(test)]
    TEST_ACCOUNT_WAKES
        .lock()
        .expect("delivery wake recorder poisoned")
        .push((uid.to_string(), reason.to_string()));
}

#[cfg(test)]
static TEST_ACCOUNT_WAKES: once_cell::sync::Lazy<std::sync::Mutex<Vec<(String, String)>>> =
    once_cell::sync::Lazy::new(|| std::sync::Mutex::new(Vec::new()));

#[cfg(test)]
pub(crate) fn take_test_account_wakes() -> Vec<(String, String)> {
    std::mem::take(
        &mut *TEST_ACCOUNT_WAKES
            .lock()
            .expect("delivery wake recorder poisoned"),
    )
}
