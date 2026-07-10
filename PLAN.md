# tuxstack — 实现计划

Docker + Incus 的 Linux 桌面 GUI 工具。面向普通 Linux 桌面用户。

## 架构

```
tuxstack/
├── Cargo.toml          # workspace root
├── daemon/             # 后台守护进程
│   ├── Cargo.toml
│   └── src/
│       ├── main.rs
│       ├── docker/     # Docker REST API
│       ├── incus/      # Incus REST API
│       ├── server/     # Unix socket JSON-RPC 服务
│       └── monitor/    # 状态监控
├── gui/                # Slint GUI
│   ├── Cargo.toml
│   └── src/
│       ├── main.rs
│       ├── ui/         # .slint 文件
│       └── client/     # daemon 客户端
├── cli/                # CLI 工具
│   ├── Cargo.toml
│   └── src/main.rs
├── common/             # 共享类型 + 协议
│   ├── Cargo.toml
│   └── src/lib.rs
└── install/            # systemd service 等
```

通信: daemon ↔ client 走 Unix socket + JSON-RPC。

## 执行顺序

### Phase 0 — 环境初始化
- [x] 确认技术选型（Rust + Slint + REST API）
- [x] 确认 MVP 范围
- [ ] 清理当前 git repo（移除 config dotfiles 关联）
- [ ] 创建 GitHub 仓库 `tuxstack`
- [ ] 初始化 Cargo workspace

### Phase 1 — common 和 daemon
- [ ] **common crate**: 定义共享类型（ContainerInfo, InstanceInfo, Protocol）
- [ ] **daemon crate**:
  - [ ] Docker socket 连接 + 基础 REST 调用封装
  - [ ] 容器列表、详情、日志流
  - [ ] 容器操作（start/stop/restart/delete）
  - [ ] 镜像搜索、拉取、删除
  - [ ] Compose up/down/logs
  - [ ] Incus API 封装骨架（待 Linux 环境测试）
  - [ ] Unix socket JSON-RPC 服务
  - [ ] 基础监控（容器 CPU / 内存数据）

### Phase 2 — GUI
- [ ] Slint 项目初始化
- [ ] daemon 客户端库
- [ ] **引导页**: 检测 Docker/Incus → 引导安装 → 进主面板
- [ ] **主面板**: Docker 容器列表（状态、资源、操作按钮）
- [ ] 容器详情页（日志、终端、操作）
- [ ] 镜像浏览管理
- [ ] Compose 项目管理
- [ ] Incus 实例列表（骨架，实际功能待 Linux 环境）
- [ ] 系统托盘

### Phase 3 — CLI
- [ ] `tuxstack ps` — 列出容器
- [ ] `tuxstack logs <id>` — 查看日志
- [ ] `tuxstack start/stop <id>` — 操作容器
- [ ] `tuxstack daemon` — 启动/管理 daemon

### Phase 4 — 完善
- [ ] 暗色/亮色主题
- [ ] Incus 实例终端
- [ ] 通知
- [ ] 安装脚本 + systemd service
- [ ] 创建 GitHub 仓库并推送
- [ ] 发布 pipeline
