# TuxStack

[English](#english) | 中文

> Docker + Incus 桌面管理工具，类似 OrbStack 的 Linux 版本

TuxStack 是一个原生的容器和虚拟机桌面管理应用，专为 Linux（KDE Plasma）设计。提供 Docker 和 Incus 的统一管理界面，使用 Rust、Qt/Kirigami 构建。

**状态：Alpha 版本。** 项目正在积极开发中，功能持续迭代。

## 功能

### Docker 管理

| 功能 | 状态 |
| --- | --- |
| 容器列表 / 结构化详情 / 生命周期操作 | ✅ 已实现 |
| Compose 标签分组 / 批量操作 | ✅ 已实现 |
| 容器日志 / 实时监控 / Docker Events | ✅ 已实现 |
| 外部容器终端（系统终端 → tuxstack-cli → tuxstackd） | ✅ 已实现 |
| 统一只读文件浏览（FUSE，容器/镜像/卷） | ✅ 已实现 |
| 创建容器（端口、挂载、网络、资源） | ✅ 已实现 |
| 镜像管理 / 拉取 / 导出 | ✅ 已实现 |
| 网络管理 | ✅ 已实现 |
| 卷管理 | ✅ 已实现 |
| 独立 Compose 项目页面 / up/down | 🔜 计划中 |
| 镜像构建 / 推送 | 🔜 计划中 |

### Incus 管理

| 功能 | 状态 |
| --- | --- |
| 虚拟机 / 容器列表 | 🔮 未来实现 |
| 网络 / 存储管理 | 🔮 未来实现 |
| 镜像管理 | 🔮 未来实现 |

## 架构

```
┌─────────────────────────────────────────┐
│               tuxstack                  │
│  QML + Kirigami UI                      │
│  CXX-Qt Rust⇄Qt 桥接                   │
│  只依赖 tuxstack-client（typed IPC）    │
└───────┬─────────────────────────────────┘
        │  Unix socket（$XDG_RUNTIME_DIR/tuxstack/control.sock）
        ▼
┌─────────────────────────────────────────┐
│              tuxstackd                  │
│  唯一 Docker Engine 客户端（Bollard）   │
│  容器/镜像/网络/卷/Compose/终端/流      │
│  只读 FUSE namespace 生命周期           │
└───────┬─────────────────────┬──────────┘
        ▼                     ▼
┌───────────────────┐  ┌───────────────────┐
│  Docker Engine    │  │  FUSE 只读挂载     │
│  /var/run/docker.sock │  ~/TuxStack/docker │
└───────────────────┘  └───────────────────┘
```

GUI 和 CLI 不直接持有 Docker 客户端。无特权用户服务 `tuxstackd` 是唯一与
Docker Engine 通信的进程，通过带鉴权的本地 Unix socket 提供 typed IPC；同时
维护一个只读 FUSE namespace（`~/TuxStack/docker`），把容器、镜像、卷暴露为
普通目录。旧的文件浏览后端（helper 会话/export tar 解析）已删除。

## 技术栈

- **语言**：Rust (edition 2024)
- **UI**：Qt 6 / Qt Quick / QML + Kirigami (KF6)
- **桥接**：CXX-Qt
- **后台服务**：`tuxstackd`（systemd user service，唯一 Docker 客户端）
- **IPC**：tuxstack-protocol / tuxstack-client（typed CBOR，Unix socket）
- **文件系统**：tuxstack-vfs（只读 FUSE，fuser）
- **Docker 客户端**：Bollard + Tokio 异步运行时（仅 daemon）
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
# 开发环境直接运行 GUI；若 daemon 未运行，GUI 会自动启动同目录的 tuxstackd
cargo run

# 也可以在另一个终端手动运行 daemon
cargo run -p tuxstack-daemon --bin tuxstackd
```

### 安装后使用 systemd 启动 tuxstackd

RPM 安装会把 daemon 安装为 systemd **用户服务**，不会创建 root daemon，也不需要
sudo 启动 Docker 管理服务。安装 RPM 后执行：

```bash
systemctl --user daemon-reload
systemctl --user enable --now tuxstackd.service
systemctl --user status tuxstackd.service
```

查看日志：

```bash
journalctl --user -u tuxstackd.service -f
```

卸载或升级 RPM 不会自动停止你的用户服务；需要停止时执行：

```bash
systemctl --user disable --now tuxstackd.service
```

`systemctl --user` 使用当前登录用户的 systemd manager。若希望 daemon 在退出图形
会话后继续运行，可按发行版策略启用 user lingering：

```bash
loginctl enable-linger "$USER"
```

GUI 通过 `$XDG_RUNTIME_DIR/tuxstack/control.sock` 连接 tuxstackd；服务或 Docker
Engine 不可用时显示明确状态并提供重试/启动服务操作。开发环境的 `cargo run` 仍会
在找不到 daemon 时自动启动同目录的 `tuxstackd`。

### RPM 打包

在 Fedora/RHEL 类系统上，先安装 Rust、Qt 6、Kirigami、FUSE 和 RPM 构建工具，
然后在仓库根目录执行：

```bash
./packaging/rpm/build-rpm.sh
```

生成的 RPM 位于 `packaging/rpm/RPMS/`。安装后可使用上面的
`systemctl --user enable --now tuxstackd.service` 启动 daemon。RPM 构建会同时安装
`tuxstack` GUI、`tuxstackd` daemon、`tuxstackctl`/`tuxstack-cli` CLI、桌面文件、AppStream 元数据、
hicolor 图标和 systemd 用户服务。

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
- [ ] 独立 Compose 项目页面与 up/down 工作流

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
| Container list/structured details/lifecycle actions | ✅ Implemented |
| Compose label grouping/group actions | ✅ Implemented |
| Container logs/live stats/Docker Events | ✅ Implemented |
| External container terminal (system terminal → tuxstack-cli → tuxstackd) | ✅ Implemented |
| Unified read-only Files browsing (FUSE, container/image/volume) | ✅ Implemented |
| Create container (ports, mounts, networks, resources) | ✅ Implemented |
| Image management/pull/export | ✅ Implemented |
| Network management | ✅ Implemented |
| Volume management | ✅ Implemented |
| Dedicated Compose projects page/up/down | 🔜 Planned |
| Image build/push | 🔜 Planned |

### Incus Management

| Feature | Status |
| --- | --- |
| VM/container list | 🔮 Future |
| Network/storage management | 🔮 Future |
| Image management | 🔮 Future |

## Architecture

```
┌─────────────────────────────────────────┐
│               tuxstack                  │
│  QML + Kirigami UI                      │
│  CXX-Qt Rust⇄Qt bridge                 │
│  depends only on tuxstack-client (IPC)  │
└───────┬─────────────────────────────────┘
        │  Unix socket ($XDG_RUNTIME_DIR/tuxstack/control.sock)
        ▼
┌─────────────────────────────────────────┐
│              tuxstackd                  │
│  sole Docker Engine client (Bollard)    │
│  containers/images/network/volumes/…    │
│  read-only FUSE namespace lifecycle     │
└───────┬─────────────────────┬──────────┘
        ▼                     ▼
┌───────────────────┐  ┌───────────────────┐
│  Docker Engine    │  │  read-only FUSE    │
│  /var/run/docker.sock │  ~/TuxStack/docker │
└───────────────────┘  └───────────────────┘
```

Neither the GUI nor the CLI owns a Docker client. The unprivileged user
service `tuxstackd` is the only process that talks to Docker Engine, serving
typed IPC over an authenticated local Unix socket and a persistent read-only
FUSE namespace at `~/TuxStack/docker`. The legacy file-browsing backends
(helper sessions / export tar parsing) were removed.

## Tech Stack

- **Language**: Rust (edition 2024)
- **UI**: Qt 6 / Qt Quick / QML + Kirigami (KF6)
- **Bridge**: CXX-Qt
- **Background service**: `tuxstackd` (systemd user service, sole Docker client)
- **IPC**: tuxstack-protocol / tuxstack-client (typed CBOR over Unix socket)
- **Filesystem**: tuxstack-vfs (read-only FUSE via fuser)
- **Docker client**: Bollard + Tokio async runtime (daemon only)
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
# Development: run the GUI directly; it auto-starts the sibling tuxstackd when needed
cargo run

# Alternatively run the daemon in another terminal
cargo run -p tuxstack-daemon --bin tuxstackd
```

### Start tuxstackd with systemd after installation

The RPM installs `tuxstackd` as a systemd **user service**. It does not create a
root daemon and does not require `sudo` to start the TuxStack service. After
installing the RPM:

```bash
systemctl --user daemon-reload
systemctl --user enable --now tuxstackd.service
systemctl --user status tuxstackd.service
```

Follow the daemon log with:

```bash
journalctl --user -u tuxstackd.service -f
```

To stop and disable it:

```bash
systemctl --user disable --now tuxstackd.service
```

`systemctl --user` talks to the current user's systemd manager. To keep the
service running after the graphical session ends, enable user lingering if it
matches your distribution's policy:

```bash
loginctl enable-linger "$USER"
```

The GUI connects to tuxstackd over `$XDG_RUNTIME_DIR/tuxstack/control.sock`.
When the service or Docker Engine is unavailable it shows an explicit state and
provides retry/start-service actions. In development, `cargo run` still
auto-starts a sibling `tuxstackd` when no daemon is available.

### RPM packaging

On Fedora/RHEL-like systems, install Rust, Qt 6, Kirigami, FUSE, and the RPM
build tools, then run from the repository root:

```bash
./packaging/rpm/build-rpm.sh
```

The resulting RPM is written to `packaging/rpm/RPMS/`. It contains the
`tuxstack` GUI, `tuxstackd` daemon, `tuxstackctl`/`tuxstack-cli` CLI, desktop file, AppStream
metadata, hicolor icons, and the systemd user service.

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
- [ ] Dedicated Compose project page and up/down workflow

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
