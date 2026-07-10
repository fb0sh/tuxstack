<p align="center">
  <img src="https://img.shields.io/badge/rust-1.92+-orange.svg" alt="Rust 1.92+"/>
  <img src="https://img.shields.io/badge/license-MIT-blue.svg" alt="MIT License"/>
  <img src="https://img.shields.io/badge/status-alpha-yellow.svg" alt="Alpha"/>
</p>

# tuxstack

**Docker + Incus 容器管理桌面工具** — 面向 Linux 桌面用户的轻量级原生 GUI。

受 [OrbStack](https://orbstack.dev) 启发，三栏布局：Sidebar 导航、Resource List 数据列表、Detail Panel 详情面板。

## 功能

| 类别 | 功能 | 状态 |
|:----|:----|:----:|
| **容器** | 列表查看（状态/端口/资源） | ✅ |
| | 启动/停止/重启/删除 | 🚧 |
| | 实时日志 | 🚧 |
| | CPU/内存监控 | 🚧 |
| **镜像** | 列表查看 | 🚧 |
| | 搜索/拉取/删除 | 🚧 |
| **Compose** | 项目列表 / up/down/logs | 🚧 |
| **Incus** | 实例管理 | 📅 |
| **交互** | 三栏 GUI | ✅ |
| | CLI 工具 | ✅ |
| | 系统托盘 | 📅 |

- ✅ = MVP 已完成
- 🚧 = 开发中（见 [Issue #1](https://github.com/fb0sh/tuxstack/issues/1)）
- 📅 = 未来计划

## 架构

```
┌─────────────┐     Unix Socket      ┌─────────────┐
│   GUI       │ ◄──── JSON-RPC ────► │   Daemon     │
│  (Slint)    │                      │  (Rust)      │
└─────────────┘                      └──────┬──────┘
                                            │
                                    ┌───────┴───────┐
                                    │ Docker REST API│
                                    │ Incus REST API │
                                    └───────────────┘
```

**tuxstack daemon** — 后台守护进程，通过 Unix socket 暴露 JSON-RPC 接口。负责对接 Docker 和 Incus 的 REST API。

**tuxstack GUI** — Slint 原生桌面界面，三栏布局 + macOS 原生风格。

**tuxstack CLI** — 命令行工具，通过同一 Unix socket 与 daemon 通信。

## 快速开始

### 从源码运行

需要 Rust 1.92+ 和 Docker。

```bash
# 1. 启动 daemon（终端 1）
cargo run -p tuxstack-daemon

# 2. 启动 GUI（终端 2）
cargo run -p tuxstack-gui

# 或使用 CLI
cargo run -p tuxstack-cli -- ps
```

### 项目结构

```
tuxstack/
├── Cargo.toml          # Workspace root
├── common/             # 共享类型 + JSON-RPC 协议
│   ├── container.rs    # Docker 容器类型
│   ├── instance.rs     # Incus 实例类型
│   ├── protocol.rs     # JSON-RPC method 定义
│   ├── monitor.rs      # 系统状态/资源监控类型
│   └── util.rs         # 工具函数（socket 路径等）
├── daemon/             # 后台守护进程
│   ├── docker.rs       # Docker REST API 客户端（bollard）
│   ├── incus.rs        # Incus REST API 客户端
│   ├── server.rs       # Unix socket JSON-RPC 服务
│   ├── monitor.rs      # 系统检测/监控
│   └── main.rs         # 入口
├── gui/                # Slint GUI
│   ├── ui/main.slint   # 三栏布局 (Sidebar + List + Detail)
│   ├── src/client.rs   # Daemon 通信客户端
│   └── src/main.rs     # 事件绑定
└── cli/                # CLI 工具
    └── src/main.rs     # clap CLI + daemon 启动
```

## 开发

### 运行原型

```bash
# LOGIC 原型 — 测试 JSON-RPC 协议交互
cargo run --bin prototype_logic

# UI 原型（需 feature 开关）
cargo run --bin prototype_ui --features prototype-ui
```

### 测试

```bash
cargo test --workspace
```

## 设计

- **颜色**：背景 `#F6F6F7`、面板 `#FFFFFF`、强调色 `#A13BDA`（紫色）
- **字体**：SF Pro Display / SF Pro Text
- **布局**：三栏（Sidebar 200px | List 300px | Detail 剩余）
- **风格**：macOS 原生，大量留白，极少边框

## Tickets

当前 sprint 的切片（全部在 [Issue #1](https://github.com/fb0sh/tuxstack/issues/1) 下）：

| # | 切片 | 阻塞 |
|:-|:----|:----:|
| 2 | 完善容器类型映射 | — |
| 3 | 容器操作 (start/stop/restart) | #2 |
| 4 | 容器日志 | #2 |
| 5 | 容器资源监控 | #2 |
| 6 | 镜像管理 | — |
| 7 | Compose 管理 | #3 |

## License

MIT
