// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod client;

use client::DaemonClient;
use slint::{Model, SharedString};

slint::include_modules!();

fn container_to_data(c: &tuxstack_common::ContainerInfo) -> ContainerData {
    ContainerData {
        id: c.id.clone().into(),
        name: c.name.clone().into(),
        image: c.image.clone().into(),
        status: c.status.as_str().into(),
        cpu: c
            .cpu_usage
            .map(|v| format!("{:.1}%", v))
            .unwrap_or_else(|| "-".to_string())
            .into(),
        memory: c
            .memory_usage
            .map(|v| format!("{:.1} MB", v as f64 / 1_048_576.0))
            .unwrap_or_else(|| "-".to_string())
            .into(),
        ports: "".into(), // TODO: fill from container port mappings
    }
}

fn main() -> anyhow::Result<()> {
    let ui = MainWindow::new()?;

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_io()
        .enable_time()
        .build()?;

    let handle = rt.handle().clone();

    // Fetch containers on startup
    let weak = ui.as_weak();
    handle.spawn(async move {
        refresh_containers(weak).await;
    });

    // Connect refresh callback
    let weak = ui.as_weak();
    ui.global::<AppState>().on_refresh_requested(move || {
        let h = handle.clone();
        let w = weak.clone();
        h.spawn(async move {
            refresh_containers(w).await;
        });
    });

    // Connect sidebar selection
    ui.global::<AppState>().on_sidebar_selected(|_idx| {
        // TODO: switch the resource list based on sidebar selection
    });

    // Connect container selection
    let weak = ui.as_weak();
    ui.global::<AppState>().on_container_selected(move |idx| {
        update_detail_panel(&weak, idx);
    });

    ui.run()?;
    Ok(())
}

fn update_detail_panel(ui: &slint::Weak<MainWindow>, idx: i32) {
    if idx < 0 {
        return;
    }
    let idx = idx as usize;

    // Fetch data outside the closure to avoid model lifetime issues
    let _ = ui.upgrade_in_event_loop(move |ui| {
        let state = ui.global::<AppState>();
        let model = state.get_containers();

        if let Some(c) = model.row_data(idx) {
            state.set_detail_name(c.name.clone());
            state.set_detail_status(c.status.clone());
            state.set_detail_image(c.image.clone());
            state.set_detail_created(SharedString::from("2026-07-10"));
            state.set_detail_ip(SharedString::from("172.17.0.2"));
            state.set_detail_ports(c.ports.clone());
            state.set_detail_cpu(c.cpu.clone());
            state.set_detail_memory(c.memory.clone());
        }
    });
}

async fn refresh_containers(ui: slint::Weak<MainWindow>) {
    let mut client = match DaemonClient::connect().await {
        Ok(c) => c,
        Err(_) => return,
    };

    match client.list_containers(true).await {
        Ok(containers) => {
            let data_vec: Vec<ContainerData> =
                containers.iter().map(container_to_data).collect();

            let is_empty = data_vec.is_empty();
            let _ = ui.upgrade_in_event_loop(move |ui| {
                let state = ui.global::<AppState>();
                state.set_containers(data_vec.as_slice().into());
                state.set_list_empty(is_empty);
            });
        }
        Err(e) => {
            tracing::error!("refresh failed: {e}");
        }
    }
}
