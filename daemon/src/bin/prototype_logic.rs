//! PROTOTYPE — throwaway. Answers: does the JSON-RPC over Unix socket protocol work?
//!
//! Spawns a minimal daemon on a temp socket, then opens a TUI that lets
//! the user send commands and watch the protocol in action.
//!
//! Run: cargo run --bin prototype-logic

use std::io::{self, BufRead, Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::thread;
use std::time::Duration;

const SOCKET_PATH: &str = "/tmp/tuxstack-prototype.sock";

fn main() {
    // Clean up stale socket
    let _ = std::fs::remove_file(SOCKET_PATH);

    // Spawn fake daemon in background
    thread::spawn(|| {
        let listener = UnixListener::bind(SOCKET_PATH).unwrap();
        for stream in listener.incoming() {
            match stream {
                Ok(s) => {
                    thread::spawn(|| handle_client(s));
                }
                Err(_) => break,
            }
        }
    });

    // Give daemon a moment to start
    thread::sleep(Duration::from_millis(100));

    // TUI loop
    println!("\x1b[2J\x1b[H"); // clear
    println!("\x1b[1m  tuxstack protocol prototype\x1b[0m\n");
    println!("  Commands:");
    println!("    \x1b[1mps\x1b[0m        list containers (mock)");
    println!("    \x1b[1mlogs <id>\x1b[0m  stream logs (mock)");
    println!("    \x1b[1mstat\x1b[0m      system status");
    println!("    \x1b[1merr\x1b[0m       trigger a protocol error");
    println!("    \x1b[1mraw <json>\x1b[0m send raw JSON-RPC");
    println!("    \x1b[1mq\x1b[0m         quit\n");

    loop {
        print!("\x1b[1m  > \x1b[0m");
        io::stdout().flush().unwrap();

        let mut input = String::new();
        io::stdin().read_line(&mut input).unwrap();
        let input = input.trim();

        if input.is_empty() {
            continue;
        }

        match input {
            "q" | "quit" => break,
            "ps" => send_request(r#"{"jsonrpc":"2.0","id":1,"method":"docker.list_containers","params":{"all":true}}"#),
            "stat" => send_request(r#"{"jsonrpc":"2.0","id":2,"method":"system.status","params":{}}"#),
            "err" => send_request(r#"{"jsonrpc":"2.0","id":3,"method":"docker.nonexistent","params":{}}"#),
            "logs" => send_request(r#"{"jsonrpc":"2.0","id":4,"method":"docker.container_logs","params":{"id":"test","tail":10}}"#),
            raw if raw.starts_with("raw ") => send_request(raw.strip_prefix("raw ").unwrap()),
            _ => {
                println!("\x1b[31m  Unknown command: {}\x1b[0m", input);
                println!("  Press any key to continue...");
                let _ = io::stdin().read(&mut [0u8]);
            }
        }

        // Pause so user can see the response
        println!("\n  \x1b[2m(Press Enter to continue...)\x1b[0m");
        let _ = io::stdin().read_line(&mut String::new());
        print!("\x1b[2J\x1b[H");
        println!("\x1b[1m  tuxstack protocol prototype\x1b[0m\n");
    }

    let _ = std::fs::remove_file(SOCKET_PATH);
    println!("  Bye!");
}

fn send_request(body: &str) {
    match UnixStream::connect(SOCKET_PATH) {
        Ok(mut stream) => {
            // Send request
            let mut buf = body.as_bytes().to_vec();
            buf.push(b'\n'); // line delimiter
            stream.write_all(&buf).unwrap();

            // Read response(s) — for streaming, read until a brief pause
            let mut response = String::new();
            let mut buf = [0u8; 4096];
            let start = std::time::Instant::now();

            loop {
                match stream.set_read_timeout(Some(Duration::from_millis(200))) {
                    Ok(()) => {}
                    Err(_) => break,
                }
                match stream.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        response.push_str(&String::from_utf8_lossy(&buf[..n]));
                        if start.elapsed() > Duration::from_secs(2) {
                            break; // safety
                        }
                    }
                    Err(_) => break,
                }
            }

            // Pretty print response
            println!("\n  \x1b[1mRequest:\x1b[0m");
            println!("  {}", body);

            println!("\n  \x1b[1mResponse:\x1b[0m");
            for line in response.lines() {
                if let Ok(val) = serde_json::from_str::<serde_json::Value>(line) {
                    println!("  {}", serde_json::to_string_pretty(&val).unwrap());
                } else {
                    println!("  {}", line);
                }
            }
        }
        Err(e) => {
            println!("\n  \x1b[31mConnection error: {}\x1b[0m", e);
        }
    }
}

// ── Mock daemon handler ──────────────────────────────────────────

fn handle_client(stream: UnixStream) {
    // Use try_clone() so we can have separate read/write handles
    let mut reader = match stream.try_clone() {
        Ok(c) => std::io::BufReader::new(c),
        Err(_) => return,
    };
    let mut writer = stream;

    let mut line = String::new();
    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }

        let response = dispatch(&line);
        writer.write_all(response.as_bytes()).unwrap();
        writer.write_all(b"\n").unwrap();
        writer.flush().unwrap();
    }
}

fn dispatch(request: &str) -> String {
    // Parse JSON-RPC request
    let req: serde_json::Value = match serde_json::from_str(request) {
        Ok(v) => v,
        Err(_) => {
            return serde_json::json!({
                "jsonrpc": "2.0",
                "id": 0,
                "error": {"code": -32700, "message": "Parse error"}
            }).to_string();
        }
    };

    let id = req.get("id").and_then(|v| v.as_u64()).unwrap_or(0);
    let method = req
        .get("method")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    match method {
        "system.status" => ok(id, serde_json::json!({
            "docker_available": true,
            "incus_available": false,
            "docker_version": "29.4.0",
            "containers_running": 3,
            "containers_total": 5,
            "instances_running": 0,
            "instances_total": 0,
        })),
        "docker.list_containers" => ok(id, serde_json::json!([
            {
                "id": "abc123",
                "name": "web-app",
                "image": "nginx:latest",
                "status": "running",
                "created": "2026-07-09T10:00:00Z",
                "ports": [{"host_port": 8080, "container_port": 80, "protocol": "tcp"}],
                "cpu_usage": 2.3,
                "memory_usage": 45_678_912,
                "memory_limit": 268_435_456
            },
            {
                "id": "def456",
                "name": "redis-cache",
                "image": "redis:7-alpine",
                "status": "running",
                "created": "2026-07-09T09:30:00Z",
                "ports": [{"host_port": 6379, "container_port": 6379, "protocol": "tcp"}],
                "cpu_usage": 0.5,
                "memory_usage": 12_345_678,
                "memory_limit": 134_217_728
            },
            {
                "id": "ghi789",
                "name": "db",
                "image": "postgres:16",
                "status": "exited",
                "created": "2026-07-08T14:00:00Z",
                "ports": [{"host_port": 5432, "container_port": 5432, "protocol": "tcp"}],
                "cpu_usage": null,
                "memory_usage": null,
                "memory_limit": null
            }
        ])),
        "docker.container_logs" => {
            // Simulate streaming: send multiple lines
            let logs: Vec<serde_json::Value> = (1..=5)
                .map(|i| serde_json::json!({
                    "timestamp": format!("2026-07-10T10:00:{:02}Z", i),
                    "stream": if i % 2 == 0 { "stdout" } else { "stderr" },
                    "message": format!("Log line number {}", i)
                }))
                .collect();

            ok(id, serde_json::json!({
                "container_id": "abc123",
                "logs": logs
            }))
        }
        _ => err(
            id,
            -32601,
            format!("Method not found: {}", method),
        ),
    }
}

fn ok(id: u64, result: serde_json::Value) -> String {
    serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": result
    })
    .to_string()
}

fn err(id: u64, code: i64, message: String) -> String {
    serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": {"code": code, "message": message}
    })
    .to_string()
}
