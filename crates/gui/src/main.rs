//! tuxstack-gui — native Docker management for KDE Plasma.
//!
//! Built with Qt 6 / QML / Kirigami and CXX-Qt; all Docker I/O runs on
//! a shared Tokio runtime inside `tuxstack-docker-core`.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app_state;
mod bridge;
mod error;
mod runtime;
mod settings;

#[cfg(test)]
mod smoke_test;

use cxx_qt::casting::Upcast;
use cxx_qt_lib::{QGuiApplication, QQmlApplicationEngine, QQmlEngine, QUrl};
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

fn main() {
    runtime::init();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .or_else(|_| {
                    tracing_subscriber::EnvFilter::try_new(
                        std::env::var("TUXSTACK_LOG").unwrap_or_else(|_| "info".into()),
                    )
                })
                .unwrap_or_else(|_| "info".into()),
        )
        .with_target(false)
        .init();

    let mut app = QGuiApplication::new();
    let mut engine = QQmlApplicationEngine::new();

    // Count root objects created while loading Main.qml (debug aid).
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
    tracing::info!(
        root_objects = created.load(Ordering::SeqCst),
        "QML UI loaded"
    );

    if let Some(engine) = engine.as_mut() {
        let engine: Pin<&mut QQmlEngine> = engine.upcast_pin();
        engine
            .on_quit(|_| {
                tracing::info!("tuxstack-gui quitting");
            })
            .release();
    }

    if let Some(app) = app.as_mut() {
        app.exec();
    }
}
