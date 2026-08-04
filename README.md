# TuxStack

[English](#english) | 中文

> Docker + Incus 桌面管理工具，类似 OrbStack 的 Linux 版本

TuxStack 是一个原生的容器和虚拟机桌面管理应用，专为 Linux（KDE Plasma）设计。提供 Docker 和 Incus 的统一管理界面，使用 Rust、Qt/Kirigami 构建。

**状态：Alpha 版本。** 项目正在积极开发中，功能持续迭代。

## 功能

### Docker 管理

| 功能 | 状态 |
| --- | --- |
| 容器列表 / 详情 / 启动停止 | ✅ 已实现 |
| 容器日志 / 监控 / Inspect | ✅ 已实现 |
| 镜像管理 / 拉取 / 导出 | ✅ 已实现 |
| 网络管理 | ✅ 已实现 |
| 卷管理 / 文件浏览 | ✅ 已实现 |
| 镜像文件浏览 | ✅ 已实现 |
| Compose 项目 | 🔜 计划中 |
| 容器终端 | 🔜 计划中 |
| 镜像构建 / 推送 | 🔜 计划中 |

### Incus 管理

| 功能 | 状态 |
| --- | --- |
| 虚拟机 / 容器列表 | 🔮 未来实现 |
| 网络 / 存储管理 | 🔮 未来实现 |
| 镜像管理 | 🔮 未来实现 |
| 终端 / 文件浏览 | 🔮 未来实现 |

## 架构

```
┌─────────────────────────────────────────┐
│               tuxstack                  │
│  QML + Kirigami UI                      │
│  CXX-Qt Rust⇄Qt 桥接                   │
│  GUI 控制器 / 应用状态                   │
└───────┬─────────────────────────────────┘
        │
        ├───────────────────────┐
        ▼                       ▼
┌───────────────────┐  ┌───────────────────┐
│   Docker Core     │  │   Incus Core      │
│  Bollard 客户端   │  │  Incus API 客户端  │
│  文件系统服务     │  │  虚拟机管理        │
│  统计/日志/事件   │  │  网络/存储管理     │
└───────┬───────────┘  └───────┬───────────┘
        │                      │
        ▼                      ▼
┌───────────────────┐  ┌───────────────────┐
│  Docker Engine    │  │  Incus Server     │
│  /var/run/docker.sock │  /var/lib/incus/unix.socket │
└───────────────────┘  └───────────────────┘
```

无守护进程、CLI 前端或 REST/JSON-RPC 层。GUI 直接链接核心库，通过原生 API 与后端通信。

## 技术栈

- **语言**：Rust (edition 2024)
- **UI**：Qt 6 / Qt Quick / QML + Kirigami (KF6)
- **桥接**：CXX-Qt
- **Docker 客户端**：Bollard + Tokio 异步运行时
- **Incus 客户端**：Incus API (计划中)
- **序列化**：Serde、thiserror、tracing

## 系统要求

- Linux（推荐 KDE Plasma；Wayland 优先，兼容 X11）
- Rust 1.85+ (MSRV) 和 Cargo
- Qt 6（Core、QML、Quick、QuickControls2、QuickLayouts）及 C++ 编译器
- Kirigami (KF6) 和 `kirigami-addons` QML 模块
- 运行中的 Docker Engine 和/或 Incus Server

### Docker 权限

- 默认 socket 路径：`/var/run/docker.sock`
- 用户需要 `docker` 组权限（添加后需重新登录）
- Docker socket 访问等同于主机高权限控制，请谨慎管理

### Incus 权限

- 默认 socket 路径：`/var/lib/incus/unix.socket`
- 用户需要 `incus` 组权限
- 或通过 `incus admin init` 初始化后使用

### 发行版依赖

参见 [docs/development.md](docs/development.md) 获取 Fedora、Arch Linux、Ubuntu 的已验证包名。

## 构建

```bash
cargo build --workspace
```

## 运行

```bash
cargo run
```

等价于 `cargo run -p tuxstack`。启动时连接本地 Docker/Incus socket，不可用时在概览页显示错误并提供重试按钮。

## 配置

配置文件位于 `$XDG_CONFIG_HOME/tuxstack/config.toml`（默认 `~/.config/tuxstack/config.toml`）：

```toml
[docker]
host = ""
connect_timeout_seconds = 5
operation_timeout_seconds = 30

[incus]
socket_path = "/var/lib/incus/unix.socket"
connect_timeout_seconds = 5

[ui]
auto_refresh_seconds = 5
stats_refresh_seconds = 2
log_line_limit = 5000
confirm_remove = true

[logging]
level = "info"
```

主题始终跟随系统（Breeze Light/Dark）。

## 测试

```bash
cargo test --workspace
```

### Docker 集成测试

```bash
cargo test -p tuxstack-docker-core --test docker -- --ignored --nocapture
cargo test -p tuxstack-docker-core --test containers -- --ignored --nocapture
cargo test -p tuxstack-docker-core --test images -- --ignored --nocapture
cargo test -p tuxstack-docker-core --test networks -- --ignored --nocapture
cargo test -p tuxstack-docker-core --test volumes -- --ignored --nocapture
```

## 安全说明

- Docker/Incus socket 访问 = 主机控制权，谨慎管理组权限
- TuxStack 仅记录容器 ID、操作类型和错误种类，不记录敏感信息
- GUI 仅显示安全的用户友好错误信息

## 路线图

### 近期计划
- [ ] Incus 虚拟机/容器管理
- [ ] Compose 项目支持
- [ ] 容器终端

### 中期计划
- [ ] 镜像构建/标签/推送
- [ ] 持久化 Registry 账户
- [ ] Docker 上下文切换

### 长期愿景
- [ ] 完整的 Incus 集成
- [ ] 远程引擎管理
- [ ] Kubernetes 集成

参见 [docs/roadmap.md](docs/roadmap.md) 了解详细规划。

## 文档

- [架构](docs/architecture.md)
- [docker-core](docs/docker-core.md)
- [GUI](docs/gui.md)
- [开发环境](docs/development.md)
- [路线图](docs/roadmap.md)

## 许可证

MIT

---

# English

> Docker + Incus desktop management tool, a Linux version of OrbStack

TuxStack is a native container and virtual machine desktop management application for Linux (KDE Plasma). It provides a unified interface for Docker and Incus management, built with Rust, Qt/Kirigami.

**Status: Alpha.** Actively developed; features iterate continuously.

## Features

### Docker Management

| Feature | Status |
| --- | --- |
| Container list/details/start/stop | ✅ Implemented |
| Container logs/stats/inspect | ✅ Implemented |
| Image management/pull/export | ✅ Implemented |
| Network management | ✅ Implemented |
| Volume management/file browsing | ✅ Implemented |
| Image file browsing | ✅ Implemented |
| Compose projects | 🔜 Planned |
| Container terminal | 🔜 Planned |
| Image build/push | 🔜 Planned |

### Incus Management

| Feature | Status |
| --- | --- |
| VM/container list | 🔮 Future |
| Network/storage management | 🔮 Future |
| Image management | 🔮 Future |
| Terminal/file browsing | 🔮 Future |

## Architecture

```
┌─────────────────────────────────────────┐
│               tuxstack                  │
│  QML + Kirigami UI                      │
│  CXX-Qt Rust⇄Qt bridge                 │
│  GUI controllers / app state            │
└───────┬─────────────────────────────────┘
        │
        ├───────────────────────┐
        ▼                       ▼
┌───────────────────┐  ┌───────────────────┐
│   Docker Core     │  │   Incus Core      │
│  Bollard client   │  │  Incus API client │
│  Filesystem svc   │  │  VM management    │
│  Stats/logs/events│  │  Network/storage  │
└───────┬───────────┘  └───────┬───────────┘
        │                      │
        ▼                      ▼
┌───────────────────┐  ┌───────────────────┐
│  Docker Engine    │  │  Incus Server     │
│  /var/run/docker.sock │  /var/lib/incus/unix.socket │
└───────────────────┘  └───────────────────┘
```

No daemon, CLI frontend, or REST/JSON-RPC layer. The GUI links directly against core libraries, communicating with backends through native APIs.

## Tech Stack

- **Language**: Rust (edition 2024)
- **UI**: Qt 6 / Qt Quick / QML + Kirigami (KF6)
- **Bridge**: CXX-Qt
- **Docker client**: Bollard + Tokio async runtime
- **Incus client**: Incus API (planned)
- **Serialization**: Serde, thiserror, tracing

## Requirements

- Linux (KDE Plasma preferred; Wayland first, X11 compatible)
- Rust 1.85+ (MSRV) and Cargo
- Qt 6 (Core, QML, Quick, QuickControls2, QuickLayouts) with C++ compiler
- Kirigami (KF6) and `kirigami-addons` QML modules
- Running Docker Engine and/or Incus Server

### Docker Permissions

- Default socket: `/var/run/docker.sock`
- User needs `docker` group membership (re-login required after adding)
- Docker socket access = high-privilege host control; manage carefully

### Incus Permissions

- Default socket: `/var/lib/incus/unix.socket`
- User needs `incus` group membership
- Or initialize with `incus admin init`

### Distribution Dependencies

See [docs/development.md](docs/development.md) for verified package names on Fedora, Arch Linux, and Ubuntu.

## Building

```bash
cargo build --workspace
```

## Running

```bash
cargo run
```

Equivalent to `cargo run -p tuxstack`. Connects to local Docker/Incus socket on startup; shows error with retry button on Overview page when unavailable.

## Configuration

Config at `$XDG_CONFIG_HOME/tuxstack/config.toml` (default `~/.config/tuxstack/config.toml`):

```toml
[docker]
host = ""
connect_timeout_seconds = 5
operation_timeout_seconds = 30

[incus]
socket_path = "/var/lib/incus/unix.socket"
connect_timeout_seconds = 5

[ui]
auto_refresh_seconds = 5
stats_refresh_seconds = 2
log_line_limit = 5000
confirm_remove = true

[logging]
level = "info"
```

Theme follows system (Breeze Light/Dark).

## Testing

```bash
cargo test --workspace
```

### Docker Integration Tests

```bash
cargo test -p tuxstack-docker-core --test docker -- --ignored --nocapture
cargo test -p tuxstack-docker-core --test containers -- --ignored --nocapture
cargo test -p tuxstack-docker-core --test images -- --ignored --nocapture
cargo test -p tuxstack-docker-core --test networks -- --ignored --nocapture
cargo test -p tuxstack-docker-core --test volumes -- --ignored --nocapture
```

## Security Notes

- Docker/Incus socket access = host control. Manage group membership carefully
- TuxStack logs only container IDs, operation types, and error kinds; never sensitive information
- GUI shows only safe, user-friendly error messages

## Roadmap

### Near-term
- [ ] Incus VM/container management
- [ ] Compose project support
- [ ] Container terminal

### Mid-term
- [ ] Image build/tag/push
- [ ] Persistent registry accounts
- [ ] Docker context switching

### Long-term
- [ ] Full Incus integration
- [ ] Remote engine management
- [ ] Kubernetes integration

See [docs/roadmap.md](docs/roadmap.md) for detailed planning.

## Documentation

- [Architecture](docs/architecture.md)
- [docker-core](docs/docker-core.md)
- [GUI](docs/gui.md)
- [Development setup](docs/development.md)
- [Roadmap](docs/roadmap.md)

## License

MIT
