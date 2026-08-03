你需要对当前 `tuxstack` 项目实施一次完整、彻底的架构重构。

本次重构的核心目标是将项目收敛为：

> 面向 Linux KDE Plasma 桌面的原生 Docker 管理应用，使用 Rust、Qt 6、QML、Kirigami 和 Bollard 构建。

本次重构后：

* 不再开发自有 daemon
* 不再使用 REST API
* 不再使用 JSON-RPC
* 不再通过额外 Unix socket 在 GUI、CLI 和后端之间通信
* GUI 和 CLI 直接复用同一个 Rust Docker 核心库
* Docker 核心库通过 Bollard 连接 Docker Engine
* 当前版本只支持 Docker
* Incus 相关代码全部删除
* 未来增加 Incus 时，再独立增加 Incus crate
* 不提前设计 Docker/Incus 通用 backend trait
* 不保留旧架构兼容层

---

# 一、项目定位

`tuxstack` 是一个面向 Linux KDE Plasma 桌面的原生 Docker 管理应用。

产品目标：

* 提供轻量、快速、原生的 Docker 桌面管理体验
* 深度适配 KDE Plasma
* 使用 Breeze、系统图标、系统字体和系统配色
* 管理本地 Docker Engine
* 支持容器、镜像、网络、卷和 Compose 项目
* GUI 与 CLI 共享同一套核心逻辑
* 保持架构简单、清晰、可测试

项目当前处于 alpha 阶段。

README 中的项目描述应改为：

```text
TuxStack is a native Docker management application for Linux desktops,
designed for KDE Plasma and built with Rust, Qt, Kirigami, and Bollard.
```

当前版本不再声明支持 Incus。

---

# 二、最终技术栈

使用以下技术栈：

```text
语言：Rust
GUI 框架：Qt 6
界面技术：Qt Quick / QML
KDE 组件：Kirigami
Rust 与 Qt 桥接：CXX-Qt
Docker 客户端：Bollard
异步运行时：Tokio
CLI：Clap
序列化：Serde
错误处理：thiserror
日志：tracing
配置：serde + TOML 或轻量配置结构
```

开发和运行目标：

```text
Linux only
KDE Plasma 优先
Wayland 优先
X11 可兼容
```

请基于当前实际兼容版本选择依赖。

遇到 CXX-Qt、Qt、Kirigami 或 Bollard API 不确定时，查询对应官方文档和上游示例。

禁止凭记忆猜测已经变化的 API。

---

# 三、删除现有架构

必须删除以下内容：

* 自研 daemon crate
* daemon socket server
* Unix socket RPC 通信
* 手写 JSON-RPC 2.0 协议
* newline-delimited JSON
* REST API 设计或实现
* Axum、Hyper 服务端相关代码
* daemon client
* Slint GUI
* Slint 组件和构建脚本
* Incus client
* Incus DTO
* Incus UI
* Incus 状态检测
* Incus socket 检测
* Incus README 说明
* `prototype_logic`
* `prototype_ui` 相关说明
* mock 容器
* mock 镜像
* mock 日志
* mock stats
* 所有假数据生成函数
* 所有旧架构兼容层
* 废弃代码
* 注释掉的大段旧代码
* 重复实现
* 无效 feature flag

不要保留以下形式的目录或模块：

```text
legacy/
compat/
old/
v0/
jsonrpc/
rpc/
daemon/
incus/
slint/
prototype/
```

如果 `daemon` 目录中存在少量有价值的 Docker 映射或 Bollard 调用逻辑，将这些逻辑迁移到新的 `docker-core` crate，然后删除 daemon crate。

---

# 四、最终架构

最终架构：

```text
┌─────────────────────────────────────┐
│            tuxstack-gui             │
│                                     │
│ QML + Kirigami                      │
│       │                             │
│ CXX-Qt QObject / Qt Models          │
│       │                             │
│ GUI Controllers / App State         │
└───────┬─────────────────────────────┘
        │ Rust crate API
        ▼
┌─────────────────────────────────────┐
│          tuxstack-docker-core       │
│                                     │
│ Application services               │
│ Docker models                       │
│ Docker operations                   │
│ Stats/logs/event streams            │
│ Bollard type mapping                │
└───────┬─────────────────────────────┘
        │ Bollard
        ▼
┌─────────────────────────────────────┐
│           Docker Engine             │
│ /var/run/docker.sock or DOCKER_HOST │
└─────────────────────────────────────┘
```

CLI 结构：

```text
tuxstack-cli
      │
      ▼
tuxstack-docker-core
      │
      ▼
Bollard
      │
      ▼
Docker Engine
```

GUI 和 CLI 必须直接依赖 `docker-core`。

GUI 和 CLI 禁止重复封装 Docker API。

---

# 五、Workspace 结构

将 workspace 调整为：

```text
tuxstack/
├── Cargo.toml
├── Cargo.lock
├── README.md
├── LICENSE
├── .gitignore
│
├── crates/
│   ├── docker-core/
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── client.rs
│   │       ├── config.rs
│   │       ├── error.rs
│   │       ├── models/
│   │       │   ├── mod.rs
│   │       │   ├── container.rs
│   │       │   ├── image.rs
│   │       │   ├── network.rs
│   │       │   ├── volume.rs
│   │       │   ├── compose.rs
│   │       │   ├── stats.rs
│   │       │   └── event.rs
│   │       ├── services/
│   │       │   ├── mod.rs
│   │       │   ├── containers.rs
│   │       │   ├── images.rs
│   │       │   ├── networks.rs
│   │       │   ├── volumes.rs
│   │       │   ├── compose.rs
│   │       │   └── system.rs
│   │       ├── mapping/
│   │       │   ├── mod.rs
│   │       │   ├── containers.rs
│   │       │   ├── images.rs
│   │       │   ├── networks.rs
│   │       │   ├── volumes.rs
│   │       │   └── stats.rs
│   │       └── streams/
│   │           ├── mod.rs
│   │           ├── logs.rs
│   │           ├── stats.rs
│   │           └── events.rs
│   │
│   ├── gui/
│   │   ├── Cargo.toml
│   │   ├── build.rs
│   │   ├── src/
│   │   │   ├── main.rs
│   │   │   ├── lib.rs
│   │   │   ├── runtime.rs
│   │   │   ├── app_state.rs
│   │   │   ├── settings.rs
│   │   │   ├── error.rs
│   │   │   ├── controllers/
│   │   │   │   ├── mod.rs
│   │   │   │   ├── app.rs
│   │   │   │   ├── overview.rs
│   │   │   │   ├── containers.rs
│   │   │   │   ├── images.rs
│   │   │   │   ├── networks.rs
│   │   │   │   ├── volumes.rs
│   │   │   │   └── settings.rs
│   │   │   ├── models/
│   │   │   │   ├── mod.rs
│   │   │   │   ├── container_model.rs
│   │   │   │   ├── image_model.rs
│   │   │   │   ├── network_model.rs
│   │   │   │   └── volume_model.rs
│   │   │   └── bridge/
│   │   │       ├── mod.rs
│   │   │       ├── app_bridge.rs
│   │   │       └── container_bridge.rs
│   │   └── qml/
│   │       ├── Main.qml
│   │       ├── qmldir
│   │       ├── pages/
│   │       │   ├── OverviewPage.qml
│   │       │   ├── ContainersPage.qml
│   │       │   ├── ContainerDetailsPage.qml
│   │       │   ├── ImagesPage.qml
│   │       │   ├── NetworksPage.qml
│   │       │   ├── VolumesPage.qml
│   │       │   ├── ComposePage.qml
│   │       │   └── SettingsPage.qml
│   │       ├── components/
│   │       │   ├── AppSidebar.qml
│   │       │   ├── PageHeader.qml
│   │       │   ├── LoadingView.qml
│   │       │   ├── EmptyState.qml
│   │       │   ├── ErrorBanner.qml
│   │       │   ├── StatusBadge.qml
│   │       │   ├── ContainerActions.qml
│   │       │   ├── ResourceSummaryCard.qml
│   │       │   └── SearchField.qml
│   │       └── dialogs/
│   │           ├── ConfirmRemoveDialog.qml
│   │           ├── ContainerLogsDialog.qml
│   │           ├── ContainerInspectDialog.qml
│   │           └── ErrorDetailsDialog.qml
│   │
│   └── cli/
│       ├── Cargo.toml
│       └── src/
│           ├── main.rs
│           ├── error.rs
│           ├── output.rs
│           └── commands/
│               ├── mod.rs
│               ├── ps.rs
│               ├── inspect.rs
│               ├── logs.rs
│               ├── start.rs
│               ├── stop.rs
│               ├── restart.rs
│               ├── remove.rs
│               ├── images.rs
│               ├── networks.rs
│               ├── volumes.rs
│               └── info.rs
│
├── docs/
│   ├── architecture.md
│   ├── development.md
│   ├── docker-core.md
│   ├── gui.md
│   └── roadmap.md
│
├── packaging/
│   ├── desktop/
│   │   ├── io.github.tuxstack.TuxStack.desktop
│   │   └── io.github.tuxstack.TuxStack.metainfo.xml
│   ├── icons/
│   ├── flatpak/
│   ├── rpm/
│   └── arch/
│
└── tests/
    └── integration/
        ├── docker.rs
        ├── containers.rs
        └── cli.rs
```

当前没有实现的 Compose 页面可以显示真实的“计划中”状态。

禁止填充 Compose mock 数据。

---

# 六、Workspace 配置

使用：

```toml
[workspace]
resolver = "2"
members = [
    "crates/docker-core",
    "crates/gui",
    "crates/cli",
]
```

统一使用 edition 2024。

根 `Cargo.toml` 统一管理通用依赖版本。

依赖大致包括：

```toml
[workspace.dependencies]
anyhow = "1"
bollard = "..."
bytes = "1"
chrono = { version = "0.4", features = ["serde"] }
clap = { version = "4", features = ["derive"] }
futures-util = "0.3"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
thiserror = "2"
tokio = { version = "1", features = ["full"] }
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
uuid = { version = "1", features = ["v4", "serde"] }
```

Qt 和 CXX-Qt 相关依赖根据上游官方文档配置。

不要机械使用本 Prompt 中的版本号。

需要验证：

* Rust MSRV
* CXX-Qt 支持的 Qt 版本
* Kirigami 版本
* Bollard 当前 API
* KDE Frameworks 和 Qt 的兼容关系

---

# 七、`docker-core` 职责

`docker-core` 是整个项目的核心。

它负责：

* Docker 连接
* Docker Engine 状态检测
* 容器查询
* 容器生命周期操作
* 容器详情
* 日志读取
* 实时日志流
* stats
* stats 流
* Docker events
* 镜像管理
* 网络管理
* 卷管理
* Bollard 类型映射
* 超时
* 错误转换
* 配置
* 业务模型

它不负责：

* Qt 类型
* QML
* CLI 参数解析
* 表格输出
* GUI loading 状态
* GUI 导航
* HTTP 服务
* Unix RPC
* 数据库
* daemon 生命周期

---

# 八、Docker 连接设计

支持以下连接方式：

```text
Unix socket
DOCKER_HOST
本地默认连接
```

优先级：

1. 显式传入的 `DockerConfig`
2. `DOCKER_HOST`
3. Bollard 的本地默认连接

配置结构示例：

```rust
pub struct DockerConfig {
    pub host: Option<String>,
    pub request_timeout: Duration,
    pub connect_timeout: Duration,
}
```

客户端：

```rust
pub struct DockerClient {
    docker: bollard::Docker,
    config: DockerConfig,
}
```

公开构造方法：

```rust
impl DockerClient {
    pub fn connect_default() -> Result<Self, DockerError>;

    pub fn connect_with_config(
        config: DockerConfig,
    ) -> Result<Self, DockerError>;

    pub async fn ping(&self) -> Result<(), DockerError>;

    pub async fn system_info(
        &self,
    ) -> Result<DockerSystemInfo, DockerError>;
}
```

连接失败时应明确区分：

```text
Docker socket 不存在
权限不足
Docker Engine 未运行
DOCKER_HOST 配置非法
连接超时
协议错误
不支持的连接方式
```

不要把全部连接错误压缩成单一字符串。

---

# 九、错误模型

定义统一错误类型：

```rust
#[derive(Debug, thiserror::Error)]
pub enum DockerError {
    #[error("Docker socket was not found")]
    SocketNotFound,

    #[error("Permission denied while accessing Docker")]
    PermissionDenied,

    #[error("Docker Engine is unavailable")]
    EngineUnavailable,

    #[error("Docker connection timed out")]
    ConnectionTimeout,

    #[error("Docker operation timed out")]
    OperationTimeout,

    #[error("Container was not found: {0}")]
    ContainerNotFound(String),

    #[error("Image was not found: {0}")]
    ImageNotFound(String),

    #[error("Network was not found: {0}")]
    NetworkNotFound(String),

    #[error("Volume was not found: {0}")]
    VolumeNotFound(String),

    #[error("Docker operation conflicts with current state: {0}")]
    Conflict(String),

    #[error("Invalid Docker response: {0}")]
    InvalidResponse(String),

    #[error("Docker API error: {0}")]
    Api(String),

    #[error("Internal error: {0}")]
    Internal(String),
}
```

根据 Bollard 返回值和 Docker HTTP 状态码进行精确映射。

错误中允许保存 source，但 GUI 默认只显示安全、简洁的用户信息。

完整底层错误可写入 debug 日志。

禁止记录：

* secret
* token
* 完整环境变量
* registry password
* 敏感挂载内容

---

# 十、领域模型

Bollard 类型不得泄漏到 GUI 和 CLI。

定义独立领域模型。

## 容器摘要

```rust
pub struct ContainerSummary {
    pub id: String,
    pub short_id: String,
    pub name: String,
    pub image: String,
    pub image_id: String,
    pub state: ContainerState,
    pub status: String,
    pub created_at: DateTime<Utc>,
    pub ports: Vec<PortBinding>,
    pub labels: BTreeMap<String, String>,
}
```

## 容器状态

```rust
pub enum ContainerState {
    Created,
    Running,
    Paused,
    Restarting,
    Removing,
    Exited,
    Dead,
    Unknown,
}
```

serde 格式使用 snake_case。

## 容器详情

```rust
pub struct ContainerDetail {
    pub summary: ContainerSummary,
    pub command: Vec<String>,
    pub entrypoint: Vec<String>,
    pub environment: Vec<EnvironmentVariable>,
    pub mounts: Vec<MountInfo>,
    pub networks: Vec<NetworkAttachment>,
    pub restart_policy: RestartPolicy,
    pub health: Option<HealthStatus>,
    pub platform: Option<String>,
    pub hostname: Option<String>,
    pub working_dir: Option<String>,
    pub resource_limits: ResourceLimits,
}
```

## 容器日志

```rust
pub enum LogStream {
    Stdout,
    Stderr,
    Console,
    Unknown,
}

pub struct LogLine {
    pub timestamp: Option<DateTime<Utc>>,
    pub stream: LogStream,
    pub message: String,
}
```

## 容器统计

```rust
pub struct ContainerStats {
    pub cpu_percent: f64,
    pub memory_usage_bytes: u64,
    pub memory_limit_bytes: u64,
    pub memory_percent: f64,
    pub network_rx_bytes: u64,
    pub network_tx_bytes: u64,
    pub block_read_bytes: u64,
    pub block_write_bytes: u64,
    pub pids: Option<u64>,
    pub sampled_at: DateTime<Utc>,
}
```

## 镜像摘要

至少包含：

```text
id
short_id
repository_tags
repository_digests
created_at
size_bytes
virtual_size_bytes
containers
labels
```

## 网络摘要

至少包含：

```text
id
name
driver
scope
internal
attachable
ingress
ipv6
labels
```

## 卷摘要

至少包含：

```text
name
driver
mountpoint
scope
created_at
labels
options
```

---

# 十一、Docker 服务设计

不要创建巨大 `DockerService` 文件。

按资源拆分 service：

```rust
pub struct ContainerService {
    client: Arc<DockerClient>,
}

pub struct ImageService {
    client: Arc<DockerClient>,
}

pub struct NetworkService {
    client: Arc<DockerClient>,
}

pub struct VolumeService {
    client: Arc<DockerClient>,
}

pub struct SystemService {
    client: Arc<DockerClient>,
}
```

可提供聚合入口：

```rust
pub struct DockerServices {
    pub system: SystemService,
    pub containers: ContainerService,
    pub images: ImageService,
    pub networks: NetworkService,
    pub volumes: VolumeService,
}
```

所有 service 共享一个 `Arc<DockerClient>`。

不要为未来 Incus 创建：

```text
Backend
WorkloadBackend
ContainerBackend
RuntimeProvider
ProviderRegistry
```

等通用 trait。

Docker 本身直接建模。

---

# 十二、容器功能

至少实现以下容器操作：

```rust
list_containers
inspect_container
start_container
stop_container
restart_container
pause_container
unpause_container
kill_container
remove_container
rename_container
container_logs
container_stats
watch_logs
watch_stats
```

第一阶段 GUI 必须接入：

```text
list
inspect
start
stop
restart
remove
logs
stats
```

CLI 至少接入：

```text
list
inspect
start
stop
restart
remove
logs
```

## 列表选项

```rust
pub struct ListContainersOptions {
    pub all: bool,
    pub limit: Option<usize>,
    pub search: Option<String>,
    pub state: Option<ContainerState>,
}
```

搜索和状态过滤可以先在本地领域模型层完成。

若 Docker API 支持对应 filter，可同步下推。

## 停止选项

```rust
pub struct StopContainerOptions {
    pub timeout_seconds: Option<i64>,
}
```

## 删除选项

```rust
pub struct RemoveContainerOptions {
    pub force: bool,
    pub remove_volumes: bool,
    pub remove_links: bool,
}
```

## 日志选项

```rust
pub struct ContainerLogsOptions {
    pub stdout: bool,
    pub stderr: bool,
    pub timestamps: bool,
    pub follow: bool,
    pub tail: Option<usize>,
    pub since: Option<DateTime<Utc>>,
    pub until: Option<DateTime<Utc>>,
}
```

---

# 十三、流式数据

日志、stats 和 Docker events 通过 Rust stream 暴露。

示例：

```rust
pub type LogStreamResult =
    Pin<Box<dyn Stream<Item = Result<LogLine, DockerError>> + Send>>;

pub type StatsStreamResult =
    Pin<Box<dyn Stream<Item = Result<ContainerStats, DockerError>> + Send>>;

pub type EventStreamResult =
    Pin<Box<dyn Stream<Item = Result<DockerEvent, DockerError>> + Send>>;
```

流必须支持：

* 主动取消
* GUI 页面关闭后停止
* 容器删除后结束
* Docker 断开时返回明确错误
* 应用退出时统一取消
* 避免后台 task 泄漏

可以使用：

```text
tokio::sync::watch
tokio::sync::broadcast
tokio_util::sync::CancellationToken
```

根据现有依赖和必要性选择。

不要为简单任务引入复杂 actor 框架。

---

# 十四、GUI 技术方案

使用：

```text
Qt 6
Qt Quick
Qt Quick Controls
Qt Quick Layouts
Kirigami
Kirigami Addons
CXX-Qt
```

禁止继续使用 Slint。

QML 负责：

* 页面布局
* 控件
* 导航
* 主题
* 动画
* loading 展示
* empty 展示
* 用户交互
* 对话框
* 通知

Rust 负责：

* Docker 调用
* Tokio runtime
* 应用状态
* Qt Model 数据
* 并发
* 错误处理
* 取消任务
* 操作状态
* 数据转换

QML 禁止：

* 直接操作 Docker
* 直接创建 shell 命令
* 直接读 Docker socket
* 保存复杂业务状态
* 使用 mock 数据
* 硬编码 macOS 风格颜色
* 手写大量重复业务逻辑

---

# 十五、KDE 原生风格要求

应用必须遵循 KDE Plasma 设计习惯。

使用：

```qml
import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
```

根据实际环境增加 Kirigami Addons。

颜色使用：

```qml
Kirigami.Theme.backgroundColor
Kirigami.Theme.alternateBackgroundColor
Kirigami.Theme.textColor
Kirigami.Theme.disabledTextColor
Kirigami.Theme.highlightColor
Kirigami.Theme.negativeTextColor
Kirigami.Theme.positiveTextColor
```

间距使用：

```qml
Kirigami.Units.smallSpacing
Kirigami.Units.mediumSpacing
Kirigami.Units.largeSpacing
Kirigami.Units.gridUnit
```

图标通过系统图标名称：

```text
view-refresh
media-playback-start
media-playback-stop
view-list-details
edit-delete
document-properties
utilities-terminal
dialog-information
network-server
drive-harddisk
folder
```

使用实际存在的 FreeDesktop/KDE 图标名。

不要嵌入大量自绘 SVG 替代系统图标。

必须支持：

* Breeze Light
* Breeze Dark
* Plasma 系统强调色
* 系统字体
* 高 DPI
* Wayland
* 键盘导航
* 无障碍标签
* 窗口尺寸变化

禁止写死：

```text
#F6F6F7
#A13BDA
macOS-style
OrbStack-style
```

视觉可以保持现代、简洁，但必须由 KDE 主题控制。

---

# 十六、GUI 主结构

使用 KDE 桌面应用结构：

```text
Kirigami.ApplicationWindow
├── Sidebar
│   ├── Overview
│   ├── Containers
│   ├── Images
│   ├── Networks
│   ├── Volumes
│   ├── Compose
│   └── Settings
└── Page Stack
```

不要继续使用固定的：

```text
210px Sidebar
340px ResourceList
DetailPanel
```

应采用响应式布局。

桌面宽屏时：

* 左侧导航
* 中间主资源列表
* 右侧可选详情抽屉或独立详情页

窄窗口时：

* 使用 Page Stack
* 资源详情推入下一页
* 导航可折叠

---

# 十七、Overview 页面

Overview 页面展示真实数据：

* Docker Engine 是否可用
* Docker Engine 版本
* Docker API 版本
* 操作系统
* 架构
* 运行中的容器数量
* 已停止容器数量
* 镜像数量
* 网络数量
* 卷数量
* Docker 数据根目录
* Docker 总内存
* Docker CPU 数量

页面状态：

```text
Loading
Ready
DockerUnavailable
PermissionDenied
Error
```

Docker 不可用时，应显示：

* 简洁错误原因
* 重新连接按钮
* Docker socket 路径或 DOCKER_HOST
* 权限问题提示
* 不要自动执行 sudo
* 不要自动修改 docker group

---

# 十八、Containers 页面

使用宽列表或表格。

至少显示：

```text
Name
State
Image
Ports
CPU
Memory
Created
Actions
```

功能：

* 搜索
* 状态筛选
* 显示全部/仅运行
* 刷新
* 启动
* 停止
* 重启
* 删除
* 暂停
* 恢复
* 打开详情
* 查看日志

第一阶段至少保证：

```text
搜索
状态筛选
刷新
启动
停止
重启
删除
查看详情
查看日志
查看 stats
```

状态类型：

```text
Idle
Loading
Ready
Empty
Error
DockerUnavailable
```

每个容器独立保存操作状态：

```rust
pub enum ContainerOperation {
    Starting,
    Stopping,
    Restarting,
    Pausing,
    Unpausing,
    Removing,
}
```

使用：

```rust
HashMap<String, ContainerOperation>
```

操作期间：

* 禁用冲突操作
* 显示 busy 状态
* 阻止重复点击
* 操作成功后局部更新或刷新
* 操作失败后恢复按钮
* 显示 KDE 风格 notification

不要每次操作都清空整个列表。

---

# 十九、容器详情页

Tabs：

```text
Overview
Logs
Stats
Inspect
Terminal
Files
```

当前实现要求：

## Overview

必须实现：

* 名称
* ID
* 镜像
* 状态
* 命令
* entrypoint
* 创建时间
* restart policy
* ports
* mounts
* networks
* health
* resource limits

## Logs

必须实现：

* 历史日志
* tail 数量
* stdout/stderr
* timestamps
* follow
* 清空视图
* 自动滚动开关
* 暂停显示
* 搜索

暂停显示只暂停 UI 消费或展示，不要错误地积累无限内存。

应设置最大日志行数，例如：

```text
默认 5000 行
可配置
超过后淘汰最旧内容
```

## Stats

必须实现：

* CPU
* 内存
* 网络 RX/TX
* Block I/O
* PIDs

可采用实时文本和简单趋势图。

第一阶段不需要复杂图表库。

保持较低刷新频率，例如：

```text
1 秒或 2 秒
```

用户离开页面后停止 stream。

## Inspect

显示结构化数据。

可以提供：

* 树形视图
* JSON 视图
* 复制按钮

不要将 Bollard 原始类型直接暴露给 QML。

## Terminal

当前可以显示“计划中”。

禁止实现假终端。

## Files

当前可以显示“计划中”。

禁止实现假文件管理器。

---

# 二十、Images 页面

必须使用真实 Docker 数据。

第一阶段实现：

* 镜像列表
* 搜索
* tags
* image ID
* 大小
* 创建时间
* 删除
* inspect

后续功能可以显示计划中：

* pull
* build
* tag
* push
* prune

不使用 mock progress。

删除镜像前必须确认。

如果镜像被容器引用，展示 Docker 返回的真实错误。

---

# 二十一、Networks 页面

必须使用真实 Docker 数据。

第一阶段实现：

* 网络列表
* 名称
* driver
* scope
* internal
* attachable
* subnet
* gateway
* 容器数量
* inspect

写操作可以后续实现。

---

# 二十二、Volumes 页面

必须使用真实 Docker 数据。

第一阶段实现：

* 卷列表
* 名称
* driver
* mountpoint
* scope
* 创建时间
* labels
* inspect

删除操作可以实现，但必须显示确认和使用风险。

不要假设 mountpoint 对普通用户始终可读。

---

# 二十三、Compose 页面

当前版本可以先保留页面骨架。

页面内容必须明确：

```text
Docker Compose project support is planned.
```

禁止：

* mock Compose 项目
* 伪造 compose 状态
* 声称已经支持 compose up/down
* 通过 shell 拼接任意命令实现临时版本

未来 Compose 支持应单独设计。

当前重构阶段不实现。

---

# 二十四、Qt Model

容器列表使用 CXX-Qt 支持的 Qt Model。

优先考虑：

* `QAbstractListModel`
* CXX-Qt 官方 model 示例
* 清晰的 roles

容器 role 至少包括：

```text
containerId
shortId
name
image
state
status
ports
cpuPercent
memoryUsage
memoryLimit
createdAt
running
busy
operation
```

镜像、网络和卷同样使用 Model。

禁止把大型 JSON 字符串作为整个 model row 传给 QML。

---

# 二十五、GUI 状态管理

定义应用状态：

```rust
pub enum LoadState<T> {
    Idle,
    Loading,
    Ready(T),
    Error(AppError),
}
```

应用状态示例：

```rust
pub struct AppState {
    pub docker_status: LoadState<DockerSystemInfo>,
    pub overview: LoadState<OverviewData>,
    pub containers: LoadState<Vec<ContainerSummary>>,
    pub images: LoadState<Vec<ImageSummary>>,
    pub networks: LoadState<Vec<NetworkSummary>>,
    pub volumes: LoadState<Vec<VolumeSummary>>,
    pub selected_container: Option<String>,
    pub operations: HashMap<String, ContainerOperation>,
}
```

不要把所有状态集中在一个超大 QObject 中。

按页面或领域拆分 controller：

```text
AppController
OverviewController
ContainerController
ImageController
NetworkController
VolumeController
```

---

# 二十六、异步线程模型

Qt 主线程禁止执行阻塞 Docker 操作。

推荐流程：

```text
QML Action
    ↓
CXX-Qt Controller
    ↓
Tokio Runtime
    ↓
docker-core
    ↓
Bollard
    ↓
Result / Stream
    ↓
Qt queued invocation
    ↓
更新 QObject / Qt Model
```

要求：

* 应用启动时创建一个共享 Tokio runtime
* 不在每次点击时创建 runtime
* 不在 Qt 主线程调用 `block_on`
* 后台任务不得长期持有无效 Qt 指针
* UI 更新必须回到 Qt event loop
* 应用关闭时取消后台任务
* 日志和 stats stream 必须支持取消
* 防止重复刷新
* 防止竞态覆盖新数据

可以使用 request generation 或 sequence ID 处理旧请求覆盖新请求的问题。

例如：

```rust
refresh_generation += 1;
```

只有最新 generation 的结果允许更新 UI。

---

# 二十七、CLI 设计

CLI binary 名称：

```text
tuxstack
```

GUI binary 可以使用：

```text
tuxstack-gui
```

CLI 命令：

```text
tuxstack info
tuxstack ps
tuxstack inspect <container>
tuxstack logs <container>
tuxstack start <container...>
tuxstack stop <container...>
tuxstack restart <container...>
tuxstack pause <container...>
tuxstack unpause <container...>
tuxstack rm <container...>
tuxstack images
tuxstack networks
tuxstack volumes
```

全局参数：

```text
--host
--timeout
--json
--debug
```

`ps` 参数：

```text
--all
--running
--filter
--format
```

`logs` 参数：

```text
--follow
--tail
--timestamps
--since
--until
```

`rm` 参数：

```text
--force
--volumes
```

CLI 和 GUI 直接使用 `docker-core`。

禁止 CLI 调用 GUI。

禁止 CLI 通过隐藏 daemon 工作。

表格输出示例：

```text
CONTAINER ID   NAME       IMAGE          STATE     STATUS       PORTS
abcdef123456   postgres   postgres:17    running   Up 2 hours   127.0.0.1:5432->5432/tcp
```

退出码：

```text
0 成功
1 通用错误
2 参数错误
3 Docker 不可用
4 权限不足
5 资源不存在
6 操作冲突
7 操作超时
```

---

# 二十八、配置

配置路径遵循 XDG：

```text
$XDG_CONFIG_HOME/tuxstack/config.toml
```

默认：

```text
~/.config/tuxstack/config.toml
```

配置项可以包括：

```toml
[docker]
host = ""
connect_timeout_seconds = 5
operation_timeout_seconds = 30

[ui]
auto_refresh_seconds = 5
stats_refresh_seconds = 2
log_line_limit = 5000
confirm_remove = true

[logging]
level = "info"
```

主题始终默认跟随系统。

不要实现自定义 Light/Dark 强制主题，除非 Kirigami 官方模式明确支持且实现成本很低。

配置读取失败时：

* 显示错误
* 使用安全默认值
* 不覆盖损坏配置
* 不静默丢弃用户配置

---

# 二十九、日志

使用 `tracing`。

环境变量：

```text
RUST_LOG
TUXSTACK_LOG
DOCKER_HOST
```

默认日志级别：

```text
info
```

记录：

* Docker 连接
* 操作类型
* 容器 ID 或 short ID
* 操作耗时
* 错误类型
* UI task lifecycle
* stream 启停

不要记录：

* 完整环境变量
* registry credential
* secret
* token
* 完整容器日志正文
* 敏感 mount 内容

---

# 三十、权限说明

应用直接访问 Docker Engine。

README 必须清楚说明：

* 本地默认 Docker socket 通常是 `/var/run/docker.sock`
* 用户需要有权访问 Docker socket
* Docker socket 权限等价于高权限主机控制能力
* 应谨慎管理 `docker` group
* 应用不会自动运行 sudo
* 应用不会自动修改用户组
* 应用不会静默提权

GUI 遇到权限错误时提供明确说明。

不要弹出要求输入 root 密码的自定义对话框。

---

# 三十一、桌面集成

提供：

```text
packaging/desktop/io.github.tuxstack.TuxStack.desktop
packaging/desktop/io.github.tuxstack.TuxStack.metainfo.xml
```

desktop file 至少包含：

```ini
[Desktop Entry]
Type=Application
Name=TuxStack
Comment=Native Docker management for KDE Plasma
Exec=tuxstack-gui
Icon=io.github.tuxstack.TuxStack
Categories=System;Utility;
Terminal=false
```

根据实际 Qt 窗口设置正确的 StartupWMClass 或 Wayland app ID。

AppStream 文件中：

* 说明 alpha 状态
* 不宣称 Incus 支持
* 不宣称未实现功能
* 使用真实截图路径或暂时不添加截图

---

# 三十二、未来 Incus 扩展策略

当前不得实现 Incus。

当前不得创建通用 backend abstraction。

在文档 `roadmap.md` 中说明未来可能增加：

```text
crates/incus-core/
```

未来架构可能为：

```text
gui
├── docker-core
└── incus-core
```

未来只有在 Docker 和 Incus 已经存在真实重复代码后，才评估是否抽取：

```text
shared resource model
common operation state
common UI contracts
```

当前禁止提前引入：

```text
WorkloadBackend
RuntimeBackend
ProviderRegistry
UniversalContainer
BackendCapabilities
```

GUI 当前使用 Docker 原生术语：

```text
Container
Image
Network
Volume
```

不要改名为：

```text
Workload
Resource
Runtime Unit
Instance
```

---

# 三十三、测试要求

必须为 `docker-core` 提供单元测试。

## 模型测试

* serde 序列化
* serde 反序列化
* enum snake_case
* short ID
* port 格式化
* byte size 格式化
* state mapping
* unknown state mapping

## Mapping 测试

使用构造的 Bollard DTO 测试：

* container summary mapping
* inspect mapping
* port mapping
* image mapping
* network mapping
* volume mapping
* stats 计算
* 缺失字段
* 非法时间
* 空名称
* 多名称处理

## 错误测试

* 404 映射为资源不存在
* 409 映射为冲突
* socket 不存在
* permission denied
* timeout
* Docker unavailable
* malformed response

## Service 测试

避免为测试引入大型通用 backend trait。

可以采用窄接口：

```rust
trait ContainerApi {
    ...
}
```

这个 trait 只能用于内部可测试边界，保持最小方法集。

也可以将 mapping 与 Docker 调用拆开，重点覆盖纯函数。

## Docker 集成测试

真实 Docker 测试使用：

```rust
#[ignore]
```

或 feature：

```text
docker-integration
```

集成测试流程：

1. 检查 Docker 是否可用
2. 启动轻量测试容器
3. list
4. inspect
5. stop
6. start
7. logs
8. stats
9. remove
10. 清理资源

即使测试失败，也必须清理测试容器。

测试资源名称使用唯一前缀：

```text
tuxstack-test-<uuid>
```

---

# 三十四、GUI 测试

至少完成：

* controller 状态转换测试
* loading → ready
* loading → error
* operation busy 状态
* 操作失败恢复
* 旧 refresh 结果不能覆盖新 refresh
* 日志行数上限
* stats task 取消
* Docker unavailable 状态

QML 层至少进行：

* QML 编译或加载测试
* 关键组件可实例化
* Main.qml 可加载
* 不引用不存在的 property
* 不引用不存在的 icon 或 module 时给出构建错误

---

# 三十五、代码质量要求

完成后必须运行：

```bash
cargo fmt --all -- --check
cargo check --workspace
cargo test --workspace
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

GUI 构建环境允许时运行：

```bash
cargo build -p tuxstack-gui
```

如果当前环境缺失 Qt、Kirigami 或 CXX-Qt 构建依赖：

* 完成 Rust 端可实现部分
* 输出缺失的准确系统依赖
* 不伪造 GUI 构建成功
* 不切回 Slint
* 不跳过代码结构重构
* 不删除无法验证的 GUI 代码
* 清楚说明哪些步骤未验证

---

# 三十六、README 重写

README 必须包含：

1. 项目定位
2. alpha 状态
3. 当前功能
4. 未实现功能
5. 截图区域
6. 架构图
7. 技术栈
8. 系统要求
9. Docker 权限要求
10. Qt/Kirigami 依赖
11. 构建命令
12. GUI 运行方式
13. CLI 使用方式
14. 配置路径
15. 测试方式
16. Docker 集成测试
17. 安全说明
18. Roadmap

已实现和计划中必须分开。

当前功能表建议：

```text
Feature                 Status
Container list          Implemented
Container inspect       Implemented
Start/stop/restart      Implemented
Container logs          Implemented
Container stats         Implemented
Image list              Implemented
Network list            Implemented
Volume list             Implemented
Compose                  Planned
Terminal                 Planned
Files                    Planned
Incus                    Future consideration
```

只能根据真实实现填写状态。

---

# 三十七、文档

创建：

## `docs/architecture.md`

说明：

* 无 daemon 架构
* GUI 和 CLI 直接复用 docker-core
* Qt 与 Tokio 边界
* CXX-Qt 边界
* Bollard 数据映射
* stream 生命周期
* 错误传播
* 配置

## `docs/docker-core.md`

说明：

* 模块结构
* DockerClient
* services
* models
* mapping
* streams
* timeout
* cancellation

## `docs/gui.md`

说明：

* QML 页面结构
* controller
* Qt models
* runtime
* Kirigami 主题
* 响应式布局

## `docs/development.md`

说明：

* Fedora
* Arch Linux
* Ubuntu
* Qt 6
* KDE Frameworks
* Kirigami
* CXX-Qt
* Rust
* Docker
* build
* run
* tests

包名必须根据实际发行版验证。

不要凭空编写不存在的包名。

## `docs/roadmap.md`

说明后续阶段：

* Compose
* Terminal
* Files
* Image pull/build
* Registry login
* Docker contexts
* Remote engines
* Incus
* Podman 是否考虑

当前不实现这些功能。

---

# 三十八、实施顺序

严格按以下顺序实施。

## Phase 0：仓库检查

先执行：

```bash
git status
git log --oneline --decorate -20
find . -maxdepth 5 -type f | sort
cargo metadata --no-deps
cargo test --workspace
```

记录当前失败项。

不要先删除用户未提交的修改。

如果工作区有用户修改：

* 识别其内容
* 保留有价值修改
* 避免覆盖
* 在报告中说明

## Phase 1：删除旧架构

* 删除 daemon
* 删除 JSON-RPC
* 删除 Unix socket RPC
* 删除 Slint
* 删除 Incus
* 删除 mock
* 删除 prototype
* 删除旧 client
* 清理 README
* 清理 Cargo workspace
* 清理 `.gitignore`

## Phase 2：建立 docker-core

* DockerClient
* DockerConfig
* DockerError
* models
* mappings
* system service
* container service
* tests

## Phase 3：容器纵向闭环

* list
* inspect
* start
* stop
* restart
* remove
* logs
* stats
* streams
* integration tests

## Phase 4：CLI

* info
* ps
* inspect
* logs
* start
* stop
* restart
* rm
* JSON output
* exit codes

## Phase 5：Qt/Kirigami GUI 基础

* CXX-Qt
* Tokio runtime
* ApplicationWindow
* Sidebar
* app state
* Docker connection state
* Containers page

## Phase 6：GUI 容器功能

* real container model
* refresh
* operations
* details
* logs
* stats
* error states
* loading states

## Phase 7：其他 Docker 资源

* images
* networks
* volumes
* overview

## Phase 8：文档和打包

* README
* architecture docs
* desktop file
* AppStream
* packaging placeholders

---

# 三十九、第一阶段验收标准

完成后，以下命令应当有效：

```bash
cargo build --workspace
cargo test --workspace
```

CLI：

```bash
cargo run -p tuxstack-cli -- info
cargo run -p tuxstack-cli -- ps --all
cargo run -p tuxstack-cli -- inspect <container>
cargo run -p tuxstack-cli -- logs <container> --tail 100
```

GUI：

```bash
cargo run -p tuxstack-gui
```

GUI 必须：

* 连接真实 Docker Engine
* 显示真实容器
* 显示 Docker 不可用错误
* 显示权限错误
* 支持刷新
* 支持启动
* 支持停止
* 支持重启
* 支持删除
* 支持详情
* 支持历史日志
* 支持 stats

仓库中不得存在：

* daemon
* REST server
* JSON-RPC
* Slint
* Incus
* mock 数据
* prototype
* 旧通信 client

---

# 四十、实现原则

1. 当前只做 Docker。
2. GUI 和 CLI 直接复用 docker-core。
3. Bollard 直接连接 Docker Engine。
4. 不开发额外 daemon。
5. 不开发 REST API。
6. 不开发 JSON-RPC。
7. 不设计多 backend 插件系统。
8. 不提前抽象 Incus。
9. 不保留旧架构兼容层。
10. 不使用 mock 数据伪造完成度。
11. 不在 Qt 主线程执行阻塞操作。
12. 不让 Bollard 类型泄漏到 GUI。
13. 不让 QML 承担业务逻辑。
14. 不使用固定 macOS 配色。
15. 使用 Kirigami 和系统主题。
16. 所有 Docker 操作具有 timeout。
17. 所有流式任务支持取消。
18. README 必须与真实实现一致。
19. 优先完成可运行纵向闭环。
20. 避免为了未来需求增加当前复杂度。

---

# 四十一、Commit 要求

使用小而清晰的 commit。

建议：

```text
refactor: remove daemon incus slint and json-rpc
feat(docker-core): add docker connection and domain models
feat(docker-core): implement container lifecycle operations
feat(docker-core): add logs stats and event streams
feat(cli): implement docker management commands
feat(gui): migrate to CXX-Qt and Kirigami
feat(gui): connect container views to docker-core
feat(gui): add logs stats and container details
feat(docker-core): add image network and volume services
docs: rewrite architecture and development documentation
test: add docker mapping and integration coverage
```

每个 commit 前运行相应测试。

不要把整个重构压成一个 commit。

---

# 四十二、最终交付报告

完成后输出：

1. 新架构概览
2. 删除的旧模块
3. 最终 workspace 结构
4. docker-core 公开 API
5. GUI 页面和功能状态
6. CLI 命令
7. Docker 连接方式
8. 错误处理
9. 测试结果
10. 构建结果
11. Clippy 结果
12. Qt/Kirigami 验证情况
13. 已知限制
14. 后续开发建议
15. 所有 commit hash

必须执行并报告：

```bash
cargo fmt --all -- --check
cargo check --workspace
cargo test --workspace
cargo clippy --workspace --all-targets --all-features -- -D warnings
git status --short
```

如有命令失败：

* 给出准确错误
* 说明影响
* 列出已完成部分
* 不声称全部完成
* 不恢复旧架构作为绕过方案

最终仓库必须形成清晰的：

```text
Qt/Kirigami GUI
        +
Rust CLI
        ↓
docker-core
        ↓
Bollard
        ↓
Docker Engine
```

架构。

