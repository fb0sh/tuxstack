// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use slint::Model;

slint::include_modules!();

// ── Helpers ───────────────────────────────────────────────────────

fn format_bytes(bytes: u64) -> String {
    if bytes >= 1_073_741_824 {
        format!("{:.1} GB", bytes as f64 / 1_073_741_824.0)
    } else if bytes >= 1_048_576 {
        format!("{:.1} MB", bytes as f64 / 1_048_576.0)
    } else if bytes >= 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{bytes} B")
    }
}

fn format_net(bytes: u64) -> String {
    if bytes >= 1_073_741_824 {
        format!("{:.2} GB", bytes as f64 / 1_073_741_824.0)
    } else if bytes >= 1_048_576 {
        format!("{:.1} MB", bytes as f64 / 1_048_576.0)
    } else {
        format!("{:.0} KB", bytes as f64 / 1024.0)
    }
}

fn _parse_mb(s: &str) -> f64 {
    let s = s.trim();
    if let Some(val) = s.strip_suffix(" MB").or_else(|| s.strip_suffix(" GB")) {
        if let Ok(n) = val.parse::<f64>() {
            if s.ends_with(" GB") {
                return n * 1024.0;
            }
            return n;
        }
    }
    0.0
}

fn _parse_cpu(s: &str) -> f64 {
    let s = s.trim();
    if let Some(val) = s.strip_suffix('%') {
        if let Ok(n) = val.parse::<f64>() {
            return n / 100.0;
        }
    }
    0.0
}

// ── Mock data generators ──────────────────────────────────────────

fn generate_containers() -> Vec<ContainerData> {
    let names = [
        ("web-app", "nginx:latest", "running", 3000..3003, "172.17.0.2"),
        ("redis-cache", "redis:7-alpine", "running", 6379..6380, "172.17.0.3"),
        ("postgres-db", "postgres:16", "running", 5432..5433, "172.17.0.4"),
        ("api-server", "node:22-alpine", "running", 4000..4001, "172.17.0.5"),
        ("admin-panel", "react:18", "exited", 3000..3001, "172.17.0.6"),
        ("cron-worker", "python:3.12", "exited", 0..0, "172.17.0.7"),
        ("prometheus", "prom/prometheus:latest", "running", 9090..9091, "172.17.0.8"),
        ("grafana", "grafana/grafana:latest", "paused", 3001..3002, "172.17.0.9"),
    ];

    names
        .iter()
        .enumerate()
        .map(|(i, (name, image, status, port_range, _ip))| {
            let port = port_range.start;
            let ports_str = if port > 0 {
                format!("{}→{}", port, port)
            } else {
                String::new()
            };
            let cpu = if *status == "running" {
                format!("{:.1}%", 0.5 + (i as f64) * 1.5)
            } else {
                "-".to_string()
            };
            let mem_bytes = if *status == "running" {
                8_388_608u64 + (i as u64) * 12_582_912
            } else {
                0
            };
            let mem = if mem_bytes > 0 {
                format_bytes(mem_bytes)
            } else {
                "-".to_string()
            };

            ContainerData {
                id: format!("abc{:03}def{:03}", i + 1, i + 100).into(),
                name: name.to_string().into(),
                image: image.to_string().into(),
                status: status.to_string().into(),
                created: format!("2026-07-{:02}T{:02}:00:00Z", 9 - (i % 4), 8 + i).into(),
                ports: ports_str.into(),
                cpu: cpu.into(),
                memory: mem.into(),
            }
        })
        .collect()
}

fn generate_images() -> Vec<ImageData> {
    vec![
        ImageData { id: "sha256:a1b2".into(), name: "nginx".into(), tag: "latest".into(), size: "187 MB".into(), created: "2026-07-01".into() },
        ImageData { id: "sha256:c3d4".into(), name: "redis".into(), tag: "7-alpine".into(), size: "32 MB".into(), created: "2026-06-28".into() },
        ImageData { id: "sha256:e5f6".into(), name: "postgres".into(), tag: "16".into(), size: "412 MB".into(), created: "2026-06-25".into() },
        ImageData { id: "sha256:g7h8".into(), name: "node".into(), tag: "22-alpine".into(), size: "125 MB".into(), created: "2026-06-30".into() },
        ImageData { id: "sha256:i9j0".into(), name: "python".into(), tag: "3.12".into(), size: "145 MB".into(), created: "2026-06-20".into() },
        ImageData { id: "sha256:j1k2".into(), name: "prom/prometheus".into(), tag: "latest".into(), size: "52 MB".into(), created: "2026-07-02".into() },
        ImageData { id: "sha256:l3m4".into(), name: "grafana/grafana".into(), tag: "latest".into(), size: "84 MB".into(), created: "2026-07-03".into() },
    ]
}

fn generate_logs() -> Vec<LogEntry> {
    let messages = vec![
        ("2026-07-10T10:00:00Z", "stdout", "Server started on port 3000"),
        ("2026-07-10T10:00:01Z", "stdout", "Connected to database at postgres-db:5432"),
        ("2026-07-10T10:00:05Z", "stdout", "Loaded 42 widgets from cache"),
        ("2026-07-10T10:00:10Z", "stderr", "DeprecationWarning: cookieParser is deprecated"),
        ("2026-07-10T10:00:12Z", "stdout", "GET /api/health 200 2ms"),
        ("2026-07-10T10:00:15Z", "stdout", "GET /api/users 200 15ms"),
        ("2026-07-10T10:00:20Z", "stderr", "Error: Redis connection timeout, retrying..."),
        ("2026-07-10T10:00:22Z", "stdout", "Retry 1/3: connecting to redis-cache:6379"),
        ("2026-07-10T10:00:25Z", "stdout", "Redis connection established"),
        ("2026-07-10T10:00:30Z", "stdout", "POST /api/order 201 45ms"),
        ("2026-07-10T10:00:35Z", "stderr", "WARNING: Disk usage at 82%"),
        ("2026-07-10T10:00:40Z", "stdout", "GET /api/products 200 8ms"),
        ("2026-07-10T10:00:45Z", "stdout", "Cache miss for key 'product_list', regenerating..."),
        ("2026-07-10T10:00:50Z", "stdout", "GET /api/metrics 200 3ms"),
        ("2026-07-10T10:01:00Z", "stdout", "Scheduled job 'cleanup' completed"),
    ];
    messages.iter().map(|(ts, stream, msg)| LogEntry {
        timestamp: (*ts).into(),
        stream: (*stream).into(),
        message: (*msg).into(),
    }).collect()
}

fn generate_stats() -> (f64, u64, u64, u64, u64, u64) {
    use std::time::{SystemTime, UNIX_EPOCH};
    let seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    let n = (seed.wrapping_mul(6364136223846793005) + 1442695040888963407) >> 33;

    let cpu = 2.3 + (n % 100) as f64 * 0.05;
    let mem_usage = 45_678_912 + (n % 10_000_000);
    let mem_limit = 268_435_456u64;
    let net_rx = 1_234_567_890u64 + (n % 100_000_000);
    let net_tx = 56_789_012u64 + (n % 10_000_000);
    let _pids = 12u64 + (n % 8);

    (cpu, mem_usage, mem_limit, net_rx, net_tx, _pids)
}

// ── Main ──────────────────────────────────────────────────────────

fn main() -> Result<(), slint::PlatformError> {
    let ui = MainWindow::new()?;

    let containers = generate_containers();
    let images = generate_images();

    // Set initial state
    let state = ui.global::<AppState>();
    state.set_containers(containers.as_slice().into());
    state.set_containers_empty(false);
    state.set_images(images.as_slice().into());
    state.set_images_empty(false);

    // ── Sidebar selection ──
    let ui_weak = ui.as_weak();
    state.on_sidebar_selected(move |idx| {
        let ui = ui_weak.unwrap();
        let state = ui.global::<AppState>();
        state.set_selected_sidebar(idx);

        let (module, title) = match idx {
            0 => (0, "Containers"),
            1 => (1, "Images"),
            2 => (2, "Volumes"),
            3 => (3, "Networks"),
            4 => (4, "Instances"),
            _ => (0, "Containers"),
        };
        state.set_current_module(module);
        state.set_module_title(title.into());
        state.set_selected_container(-1);
    });

    // ── Container selection ──
    let ui_weak = ui.as_weak();
    state.on_container_selected(move |idx| {
        let ui = ui_weak.unwrap();
        let state = ui.global::<AppState>();
        let model = state.get_containers();
        if let Some(c) = model.row_data(idx as usize) {
            state.set_detail_name(c.name.clone());
            state.set_detail_status(c.status.clone());
            state.set_detail_image(c.image.clone());
            state.set_detail_created(c.created.clone());
            state.set_detail_ip(format!("172.17.0.{}", idx + 2).into());
            state.set_detail_ports(c.ports.clone());
            state.set_detail_cpu(c.cpu.clone());
            state.set_detail_memory(c.memory.clone());

            // Mock stats
            let (cpu, mem_usage, mem_limit, net_rx, net_tx, _pids) = generate_stats();
            state.set_stats_cpu_percent(format!("{:.1}%", cpu).into());
            state.set_stats_cpu_value((cpu / 100.0) as f32);
            state.set_stats_memory_usage(format_bytes(mem_usage).into());
            state.set_stats_memory_limit(format_bytes(mem_limit).into());
            state.set_stats_memory_value((mem_usage as f64 / mem_limit as f64) as f32);
            state.set_stats_network_rx(format_net(net_rx).into());
            state.set_stats_network_tx(format_net(net_tx).into());
            state.set_stats_pids(format!("{}", 12 + (idx as u64) * 2).into());

            // Mock logs
            let logs = generate_logs();
            state.set_logs(logs.as_slice().into());
        }
    });

    // ── Tab selection ──
    let ui_weak = ui.as_weak();
    state.on_tab_selected(move |idx| {
        let ui = ui_weak.unwrap();
        let state = ui.global::<AppState>();
        state.set_detail_tab(idx);
    });

    // ── Refresh ──
    let ui_weak = ui.as_weak();
    state.on_refresh_requested(move || {
        let ui = ui_weak.unwrap();
        let state = ui.global::<AppState>();
        let containers = generate_containers();
        state.set_containers(containers.as_slice().into());
        state.set_containers_empty(false);
    });

    // ── Container actions ──
    let ui_weak = ui.as_weak();
    state.on_container_action(move |id, action| {
        let ui = ui_weak.unwrap();
        let state = ui.global::<AppState>();
        let mut containers: Vec<ContainerData> = state.get_containers().iter().collect();

        if let Some(c) = containers.iter_mut().find(|c| c.id == id) {
            match action.as_str() {
                "start" => {
                    c.status = "running".into();
                    c.cpu = format!("{:.1}%", 1.0 + (rand_noise() as f64) * 0.5).into();
                    c.memory = format_bytes(16_777_216 + (rand_noise() * 8_388_608.0) as u64).into();
                }
                "stop" => {
                    c.status = "exited".into();
                    c.cpu = "-".into();
                    c.memory = "-".into();
                }
                "restart" => {
                    c.cpu = format!("{:.1}%", 2.0 + (rand_noise() as f64) * 1.0).into();
                }
                "remove" => {
                    containers.retain(|x| x.id != id);
                }
                _ => {}
            }
            state.set_containers(containers.as_slice().into());
            state.set_containers_empty(containers.is_empty());
        }
    });

    // ── Image actions ──
    state.on_image_pull(|_name| {});
    let ui_weak = ui.as_weak();
    state.on_image_remove(move |id| {
        let ui = ui_weak.unwrap();
        let state = ui.global::<AppState>();
        let mut images: Vec<ImageData> = state.get_images().iter().collect();
        images.retain(|x| x.id != id);
        state.set_images(images.as_slice().into());
        state.set_images_empty(images.is_empty());
    });

    // ── Search ──
    let ui_weak = ui.as_weak();
    state.on_search_changed(move |text| {
        let ui = ui_weak.unwrap();
        let state = ui.global::<AppState>();
        let all = generate_containers();
        if text.is_empty() {
            state.set_containers(all.as_slice().into());
        } else {
            let filtered: Vec<ContainerData> = all
                .into_iter()
                .filter(|c| {
                    let t = text.to_lowercase();
                    c.name.to_lowercase().contains(&t)
                        || c.image.to_lowercase().contains(&t)
                        || c.status.to_lowercase().contains(&t)
                })
                .collect();
            state.set_containers(filtered.as_slice().into());
            state.set_containers_empty(filtered.is_empty());
        }
    });

    ui.run()
}

/// A simple pseudo-random float 0.0..1.0
fn rand_noise() -> f64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    let seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos() as u64;
    let n = seed.wrapping_mul(6364136223846793005) + 1442695040888963407;
    ((n >> 33) % 1000) as f64 / 1000.0
}
