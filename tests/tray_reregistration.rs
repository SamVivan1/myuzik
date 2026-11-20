//! INT-05: the tray must re-register when the `StatusNotifierWatcher` appears
//! after the app started.
//!
//! The unit tests in `src/platform/tray.rs` cover the decisions (which signals
//! count, how many retries, how long between them). This test covers the part
//! only a real bus can show: a watcher that shows up late is noticed, and the
//! item is registered with it.
//!
//! It needs its own session bus and is therefore ignored by default:
//!
//! ```text
//! dbus-run-session -- cargo test --offline --test tray_reregistration -- --ignored
//! ```
//!
//! The bus starts with no `org.kde.StatusNotifierWatcher`, the tray starts
//! first (as on autostart), and only then does a fake watcher claim the name —
//! which is exactly the scenario the audit describes.

use myuzik::platform::tray::Tray;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use zbus::blocking::Connection;

/// The minimal watcher the tray talks to: one method and one property.
struct FakeWatcher {
    registered: Arc<Mutex<Vec<String>>>,
}

#[zbus::interface(name = "org.kde.StatusNotifierWatcher")]
impl FakeWatcher {
    fn register_status_notifier_item(&self, service: &str) {
        self.registered
            .lock()
            .expect("watcher lock")
            .push(service.to_owned());
    }

    #[zbus(property)]
    fn registered_status_notifier_items(&self) -> Vec<String> {
        self.registered.lock().expect("watcher lock").clone()
    }
}

#[test]
#[ignore = "needs its own session bus: dbus-run-session -- cargo test --test tray_reregistration -- --ignored"]
fn a_watcher_that_appears_late_is_registered_with() {
    if std::env::var_os("DBUS_SESSION_BUS_ADDRESS").is_none() {
        println!("no session bus, nothing to test");
        return;
    }

    // No watcher yet: exactly the "app autostarted before the shell extension"
    // case.
    let (tray, _commands, _ready) = Tray::start_background(vec![0x40; 22 * 22 * 4], 22)
        .expect("the tray worker should start even without a watcher");

    // The watcher appears now.
    let registered = Arc::new(Mutex::new(Vec::new()));
    let connection = Connection::session().expect("session bus");
    connection
        .object_server()
        .at(
            "/StatusNotifierWatcher",
            FakeWatcher {
                registered: Arc::clone(&registered),
            },
        )
        .expect("export the fake watcher");
    connection
        .request_name("org.kde.StatusNotifierWatcher")
        .expect("own the watcher name");

    // Re-registration is retried with a backoff, so allow a few seconds.
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut items = Vec::new();
    while Instant::now() < deadline {
        items = registered.lock().expect("watcher lock").clone();
        if !items.is_empty() {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }

    tray.shutdown();
    println!("the late watcher was told about: {items:?}");
    assert!(
        !items.is_empty(),
        "the tray never registered with a watcher that appeared after it started"
    );
    assert!(
        items
            .iter()
            .any(|item| item.starts_with("org.kde.StatusNotifierItem")),
        "registered the wrong name: {items:?}"
    );
}
