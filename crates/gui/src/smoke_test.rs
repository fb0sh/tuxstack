//! GUI smoke tests (in-process, headless).
//!
//! These load the full QML UI with an offscreen QPA platform and assert
//! that the QML module, types and pages instantiate without errors.

#![cfg(test)]

use cxx_qt_lib::{QGuiApplication, QQmlApplicationEngine, QUrl};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

#[test]
fn main_qml_loads_without_errors() {
    // Ensure we can run headless even when no display is available.
    unsafe {
        std::env::set_var("QT_QPA_PLATFORM", "offscreen");
    }

    let app = QGuiApplication::new();
    assert!(!app.is_null(), "QGuiApplication must be creatable");

    let mut engine = QQmlApplicationEngine::new();

    let created = Arc::new(AtomicUsize::new(0));
    if let Some(engine) = engine.as_mut() {
        let count = created.clone();
        let guard = engine.on_object_created(move |_, _, _| {
            count.fetch_add(1, Ordering::SeqCst);
        });
        guard.release();
    }

    if let Some(engine) = engine.as_mut() {
        engine.load(&QUrl::from("qrc:/qt/qml/org/tuxstack/app/qml/Main.qml"));
    }

    assert!(
        created.load(Ordering::SeqCst) > 0,
        "Main.qml must produce at least one root object"
    );

    drop(engine);
    drop(app);
}
