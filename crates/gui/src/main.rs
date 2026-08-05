//! tuxstack — native Docker management for KDE Plasma.
//!
//! Built with Qt 6 / QML / Kirigami and CXX-Qt; all Docker I/O runs on
//! a shared Tokio runtime inside `tuxstack-docker-core`.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app_state;
mod bridge;
mod controllers;
mod error;
mod models;
mod runtime;

#[cfg(test)]
mod smoke_test;

use cxx_qt::casting::Upcast;
use cxx_qt_lib::{QGuiApplication, QQmlApplicationEngine, QQmlEngine, QUrl};
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

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
    if !bridge::application_icon::install() {
        tracing::warn!("bundled TuxStack application icon could not be loaded");
    }
    let mut engine = QQmlApplicationEngine::new();

    // objectCreated is emitted on both success and failure; on failure its
    // object pointer is null. Track that distinction so startup never reports
    // a failed QML load as successful.
    let created = Arc::new(AtomicUsize::new(0));
    let failed = Arc::new(AtomicBool::new(false));
    if let Some(mut engine) = engine.as_mut() {
        {
            let qml_engine: Pin<&mut QQmlEngine> = engine.as_mut().upcast_pin();
            qml_engine.set_output_warnings_to_standard_error(true);
        }

        let count = created.clone();
        let guard = engine.as_mut().on_object_created(move |_, object, _| {
            if !object.is_null() {
                count.fetch_add(1, Ordering::SeqCst);
            }
        });
        guard.release();

        let load_failed = failed.clone();
        let guard = engine.on_object_creation_failed(move |_, url| {
            load_failed.store(true, Ordering::SeqCst);
            tracing::error!(url = %url, "QML root object creation failed");
        });
        guard.release();
    }

    if let Some(engine) = engine.as_mut() {
        engine.load(&QUrl::from("qrc:/qt/qml/org/tuxstack/app/qml/Main.qml"));
    }

    let root_objects = created.load(Ordering::SeqCst);
    if failed.load(Ordering::SeqCst) || root_objects == 0 {
        tracing::error!(
            root_objects,
            creation_failed = failed.load(Ordering::SeqCst),
            "QML UI failed to load"
        );
        std::process::exit(1);
    }
    tracing::info!(root_objects, "QML UI loaded");

    if let Some(engine) = engine.as_mut() {
        let engine: Pin<&mut QQmlEngine> = engine.upcast_pin();
        engine
            .on_quit(|_| {
                tracing::info!("tuxstack quitting");
            })
            .release();
    }

    if let Some(app) = app.as_mut() {
        app.exec();
    }
}
