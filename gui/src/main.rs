#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use slint::Model;

slint::include_modules!();

fn fmt_bytes(b: u64) -> String {
    if b >= 1_073_741_824 { format!("{:.1} GB", b as f64 / 1_073_741_824.0) }
    else if b >= 1_048_576 { format!("{:.1} MB", b as f64 / 1_048_576.0) }
    else if b >= 1024 { format!("{:.1} KB", b as f64 / 1024.0) }
    else { format!("{b} B") }
}

fn fmt_net(b: u64) -> String {
    if b >= 1_073_741_824 { format!("{:.2} GB", b as f64 / 1_073_741_824.0) }
    else { format!("{:.1} MB", b as f64 / 1_048_576.0) }
}

fn gen_containers() -> Vec<ContainerData> {
    let data = [
        ("web-app", "nginx:latest", "running", 3000, "172.17.0.2"),
        ("redis-cache", "redis:7-alpine", "running", 6379, "172.17.0.3"),
        ("postgres-db", "postgres:16", "running", 5432, "172.17.0.4"),
        ("api-server", "node:22-alpine", "running", 4000, "172.17.0.5"),
        ("admin-panel", "react:18", "exited", 3000, "172.17.0.6"),
        ("cron-worker", "python:3.12", "exited", 0, "172.17.0.7"),
        ("prometheus", "prom/prometheus:latest", "running", 9090, "172.17.0.8"),
        ("grafana", "grafana/grafana:latest", "paused", 3001, "172.17.0.9"),
    ];
    data.iter().enumerate().map(|(i, (n, img, st, port, _ip))| {
        let p = if *port > 0 { format!("{port}→{port}") } else { String::new() };
        let cpu = if *st == "running" { format!("{:.1}%", 0.5 + i as f64 * 1.5) } else { "-".into() };
        let mem = if *st == "running" { fmt_bytes(8_388_608 + i as u64 * 12_582_912) } else { "-".into() };
        ContainerData {
            id: format!("c{:03}", i + 1).into(), name: (*n).into(),
            image: (*img).into(), status: (*st).into(),
            created: format!("2026-07-{:02}T{:02}:00:00Z", 9 - (i % 4), 8 + i).into(),
            ports: p.into(), cpu: cpu.into(), memory: mem.into(),
        }
    }).collect()
}

fn gen_images() -> Vec<ImageData> {
    vec![
        ImageData { id: "i1".into(), name: "nginx".into(), tag: "latest".into(), size: "187 MB".into(), created: "2026-07-01".into() },
        ImageData { id: "i2".into(), name: "redis".into(), tag: "7-alpine".into(), size: "32 MB".into(), created: "2026-06-28".into() },
        ImageData { id: "i3".into(), name: "postgres".into(), tag: "16".into(), size: "412 MB".into(), created: "2026-06-25".into() },
        ImageData { id: "i4".into(), name: "node".into(), tag: "22-alpine".into(), size: "125 MB".into(), created: "2026-06-30".into() },
        ImageData { id: "i5".into(), name: "python".into(), tag: "3.12".into(), size: "145 MB".into(), created: "2026-06-20".into() },
        ImageData { id: "i6".into(), name: "prom/prometheus".into(), tag: "latest".into(), size: "52 MB".into(), created: "2026-07-02".into() },
        ImageData { id: "i7".into(), name: "grafana/grafana".into(), tag: "latest".into(), size: "84 MB".into(), created: "2026-07-03".into() },
    ]
}

fn gen_logs() -> Vec<LogEntry> {
    let msgs = [
        ("10:00:00", "stdout", "Server started on port 3000"),
        ("10:00:01", "stdout", "Connected to database"),
        ("10:00:05", "stdout", "Loaded 42 widgets from cache"),
        ("10:00:10", "stderr", "DeprecationWarning: cookieParser deprecated"),
        ("10:00:12", "stdout", "GET /api/health 200 2ms"),
        ("10:00:15", "stdout", "GET /api/users 200 15ms"),
        ("10:00:20", "stderr", "Redis connection timeout, retrying..."),
        ("10:00:22", "stdout", "Retry 1/3 connecting to redis-cache:6379"),
        ("10:00:25", "stdout", "Redis connection established"),
        ("10:00:30", "stdout", "POST /api/order 201 45ms"),
        ("10:00:35", "stderr", "WARNING: Disk usage at 82%"),
        ("10:00:40", "stdout", "GET /api/products 200 8ms"),
        ("10:00:45", "stdout", "Cache miss for key 'product_list'"),
        ("10:00:50", "stdout", "GET /api/metrics 200 3ms"),
        ("10:01:00", "stdout", "Scheduled job 'cleanup' completed"),
    ];
    msgs.iter().map(|(t, s, m)| LogEntry {
        timestamp: (*t).into(), stream: (*s).into(), message: (*m).into(),
    }).collect()
}

fn main() -> Result<(), slint::PlatformError> {
    let ui = MainWindow::new()?;

    let containers = gen_containers();
    ui.global::<AppState>().set_containers(containers.as_slice().into());
    ui.global::<AppState>().set_containers_empty(false);
    ui.global::<AppState>().set_images(gen_images().as_slice().into());
    ui.global::<AppState>().set_images_empty(false);

    // ── Sidebar ──
    let w = ui.as_weak();
    ui.global::<AppState>().on_sidebar_selected(move |idx| {
        let ui = w.unwrap();
        let s = ui.global::<AppState>();
        s.set_selected_sidebar(idx);
        s.set_current_module(match idx { 0 => 0, 1 => 1, 2 => 2, 3 => 3, _ => 4 });
        s.set_selected_container(-1);
    });

    // ── Container selection ──
    let w = ui.as_weak();
    ui.global::<AppState>().on_container_selected(move |idx| {
        let ui = w.unwrap();
        let s = ui.global::<AppState>();
        if let Some(c) = s.get_containers().row_data(idx as usize) {
            s.set_detail_name(c.name.clone());
            s.set_detail_status(c.status.clone());
            s.set_detail_image(c.image.clone());
            s.set_detail_created(c.created.clone());
            s.set_detail_ip(format!("172.17.0.{}", idx + 2).into());
            s.set_detail_ports(c.ports.clone());
            s.set_detail_cpu(c.cpu.clone());
            s.set_detail_memory(c.memory.clone());

            let base = idx as f64;
            let cpu = 2.3 + base * 0.8;
            let mu = 45_678_912 + (idx as u64) * 5_000_000;
            let ml = 268_435_456;
            let nr = 1_234_567_890 + (idx as u64) * 100_000_000;
            let nt = 56_789_012 + (idx as u64) * 10_000_000;
            let pids = 12 + (idx as u64) * 3;

            s.set_stats_cpu_percent(format!("{:.1}%", cpu).into());
            s.set_stats_cpu_value((cpu / 100.0) as f32);
            s.set_stats_memory_usage(fmt_bytes(mu).into());
            s.set_stats_memory_limit(fmt_bytes(ml).into());
            s.set_stats_memory_value((mu as f64 / ml as f64) as f32);
            s.set_stats_network_rx(fmt_net(nr).into());
            s.set_stats_network_tx(fmt_net(nt).into());
            s.set_stats_pids(format!("{pids}").into());
            s.set_logs(gen_logs().as_slice().into());
        }
    });

    // ── Tab ──
    let w = ui.as_weak();
    ui.global::<AppState>().on_tab_selected(move |idx| {
        w.unwrap().global::<AppState>().set_detail_tab(idx);
    });

    // ── Refresh ──
    let w = ui.as_weak();
    ui.global::<AppState>().on_refresh_requested(move || {
        w.unwrap().global::<AppState>().set_containers(gen_containers().as_slice().into());
    });

    // ── Container actions ──
    let w = ui.as_weak();
    ui.global::<AppState>().on_container_action(move |id, action| {
        let ui = w.unwrap();
        let s = ui.global::<AppState>();
        let mut v: Vec<ContainerData> = s.get_containers().iter().collect();
        if let Some(c) = v.iter_mut().find(|x| x.id == id) {
            match action.as_str() {
                "start" => { c.status = "running".into(); c.cpu = "1.2%".into(); c.memory = fmt_bytes(22_020_096).into(); }
                "stop" => { c.status = "exited".into(); c.cpu = "-".into(); c.memory = "-".into(); }
                "restart" => { c.cpu = "3.5%".into(); }
                "remove" => { v.retain(|x| x.id != id); }
                _ => {}
            }
            s.set_containers(v.as_slice().into());
            s.set_containers_empty(v.is_empty());
        }
    });

    // ── Image actions ──
    let w = ui.as_weak();
    ui.global::<AppState>().on_image_remove(move |id| {
        let ui = w.unwrap();
        let s = ui.global::<AppState>();
        let mut v: Vec<ImageData> = s.get_images().iter().collect();
        v.retain(|x| x.id != id);
        s.set_images(v.as_slice().into());
        s.set_images_empty(v.is_empty());
    });

    ui.global::<AppState>().on_image_pull(|_| {});

    // ── Search ──
    let w = ui.as_weak();
    ui.global::<AppState>().on_search_changed(move |text| {
        let ui = w.unwrap();
        let s = ui.global::<AppState>();
        let all = gen_containers();
        if text.is_empty() {
            s.set_containers(all.as_slice().into());
        } else {
            let t = text.to_lowercase();
            let f: Vec<ContainerData> = all.into_iter().filter(|c|
                c.name.to_lowercase().contains(&t) ||
                c.image.to_lowercase().contains(&t) ||
                c.status.to_lowercase().contains(&t)
            ).collect();
            s.set_containers(f.as_slice().into());
            s.set_containers_empty(f.is_empty());
        }
    });

    ui.run()
}
