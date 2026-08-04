# TuxStack Docker Containers 模块完整实现 Prompt

你需要在当前 TuxStack 仓库中完整实现 Docker Containers 模块。

本次实现参考提供的界面截图，但必须遵循当前项目已经确定的技术架构和 KDE/Kirigami 视觉规范。

当前架构：

```text
Qt 6 + QML + Kirigami
        │
      CXX-Qt
        │
   docker-core
        │
     Bollard
        │
 Docker Engine
```

本次实现范围：

```text
Containers 页面
├── 容器列表与 Compose 分组
├── 容器和分组操作
├── Info
├── Stats
├── Logs
├── Terminal
├── Files
├── 创建容器
├── Docker Events 实时同步
├── 缓存与快速加载
└── 完整测试
```

禁止引入：

```text
自定义 daemon
REST API
JSON-RPC
docker CLI shell 调用
mock Docker 数据
旧版和新版并行实现
兼容壳
未接通的占位按钮
假终端
假日志
假文件列表
```

所有 Docker 数据和操作必须通过 `docker-core` 和 Bollard 完成。

---

# 一、必须先完成仓库审计

修改代码前执行：

```bash
git status
git diff
git log --oneline --decorate -20

cargo metadata --no-deps

find . -maxdepth 4 -type f \
  \( -name "*.rs" -o -name "*.qml" -o -name "qmldir" -o -name "CMakeLists.txt" \) \
  | sort
```

重点查找：

```text
ContainersPage
ContainersController
ContainerModel
DockerClient
DockerDataStore
RequestCoordinator
DockerEventMonitor
FilesystemTable
VolumeFiles
ImageFiles
Terminal
Logs
Stats
PropertyRow
PropertySection
KeyValueTable
```

首先回答以下问题，再开始修改：

1. 当前 Containers 页面是否已有 placeholder 或旧实现。
2. 当前 Qt Model 基类和 CXX-Qt 注册方式。
3. Images、Volumes、Networks 的状态管理方式。
4. 当前是否已有统一的 Docker repository、缓存和事件层。
5. Volume Files 与 Image Files 中哪些 UI 组件可以复用。
6. Commands 模块是否已有终端组件。
7. 当前是否已有日志查看组件。
8. 当前是否已有图表组件。
9. 当前 Compose 标签和项目分组逻辑是否已有实现。
10. 当前包名、构建命令和 QML 资源注册位置。

不要根据本 Prompt 中的示例路径盲目创建重复目录。

发现已有职责相同的实现时，直接完善或替换。

替换旧实现时必须在同一批改动中删除：

* 旧 Controller；
* 旧 Model；
* 旧 QML；
* 无引用的 service；
* 旧 signal；
* 旧注册代码；
* dead code；
* compatibility wrapper。

不保留两条实现路径。

---

# 二、子 Agent 派发方案

主 Agent 可以派发子 Agent 并行实施，但必须控制文件所有权。

## Agent A：仓库和架构审计

只读，不修改代码。

交付：

* 当前模块图；
* 可复用组件；
* 需要删除的旧代码；
* Bollard 版本及对应 API；
* 文件所有权建议；
* 风险清单。

## Agent B：docker-core Container 领域层

只允许修改 Containers 相关的 `docker-core` 新文件或已有 Container service 文件。

负责：

* Container models；
* list / inspect；
* create；
* start / stop / restart / kill；
* pause / unpause；
* rename；
* remove；
* stats；
* logs；
* exec；
* filesystem snapshot；
* Compose grouping data；
* error mapping；
* 单元测试。

不得修改：

* 主应用导航；
* QML；
* `qmldir`；
* 中央模块注册文件；
* Cargo 依赖，除非主 Agent批准。

## Agent C：列表、分组、Info 和创建对话框

负责：

* Containers 列表 QML；
* Compose Group QML；
* Container Row；
* Info 页面；
* Create Container Dialog；
* 操作确认对话框；
* 搜索和排序 UI。

不得实现 Docker 请求。

## Agent D：Stats、Logs、Terminal

负责：

* Stats 后端和 QML；
* Logs 后端和 QML；
* Terminal 后端和 QML；
* 流生命周期；
* 取消、重连、切换选择；
* 相关测试。

## Agent E：Container Files

负责：

* 容器文件系统 snapshot/index；
* Files Controller；
* 与通用文件表格整合；
* 文件预览和 Save As；
* mount overlay 表示；
* 测试。

## Agent F：测试和回归审查

在其他 Agent 完成后进行只读审查和补测试：

* 竞态；
* 状态泄漏；
* 缓存错误；
* Qt 主线程阻塞；
* 资源清理；
* 错误状态；
* Light/Dark；
* 旧代码残留。

## 主 Agent保留的共享文件

以下共享文件只允许主 Agent最终修改：

```text
Cargo.toml / workspace dependencies
中央 mod.rs
CXX-Qt bridge 注册
QML qmldir
应用主导航
ContainersPage 根组件
主 Controller 聚合
资源注册文件
```

每个 Agent 使用独立 commit。

主 Agent整合前逐项审查，禁止直接接受未经测试的变更。

子 Agent机制不可用时，按同样阶段串行执行。

---

# 三、最终页面信息架构

保持应用三栏结构：

```text
┌────────────────┬────────────────────────┬────────────────────────────────────┐
│ Main Sidebar   │ Container List         │ Container Detail                   │
│                │                        │                                    │
│ Containers     │ Toolbar                │ Info | Stats | Logs | Terminal     │
│ Volumes        │ Running                │ | Files                            │
│ Images         │ Compose Groups         │                                    │
│ Networks       │ Individual Containers  │ Current tab content                │
│                │ Stopped                │                                    │
└────────────────┴────────────────────────┴────────────────────────────────────┘
```

第三栏始终保留布局空间。

无选择时：

```text
第三栏内容完全 blank
```

不要显示：

```text
No Selection
Select a container
No container selected
```

标签栏可以保留但必须禁用，也可以在无选择时隐藏。选择一种与 Images、Volumes、Networks 一致的行为。

第三栏不能因为 selection 为空而从 `RowLayout` 中移除。

---

# 四、选择模型

必须支持两种选择：

```rust
pub enum ContainerSelection {
    None,
    Group {
        group_id: ContainerGroupId,
    },
    Container {
        container_id: String,
    },
}
```

不要分别维护：

```text
selectedContainerId
selectedGroupId
isGroupSelected
```

并产生互相矛盾的状态。

选择变化时统一执行：

```text
selection_generation += 1
取消旧 UI 订阅
重置 tab-specific state
加载新 selection summary
按当前 tab 加载对应数据
```

---

# 五、Containers 列表快速加载

进入页面：

```text
读取内存缓存
    ↓
读取持久化 summary 快照
    ↓
立即显示已有列表
    ↓
后台 list_containers(all=true)
    ↓
原位增量更新
```

缓存只用于显示加速。

所有危险操作和最终状态判断必须基于实时 Docker Engine。

基础列表不能等待：

* inspect 每个容器；
* stats；
* logs；
* filesystem index；
* image inspect；
* volume size；
* network inspect。

Docker `list containers all=true` 返回的数据应立即映射为 summary。

---

# 六、Container Summary 模型

定义或完善：

```rust
pub struct ContainerSummary {
    pub id: String,
    pub short_id: String,
    pub names: Vec<String>,
    pub display_name: String,

    pub image_name: String,
    pub image_id: String,

    pub command: String,
    pub created_at: Option<DateTime<Utc>>,

    pub state: ContainerRuntimeState,
    pub status_text: String,

    pub ports: Vec<ContainerPortSummary>,
    pub mounts: Vec<ContainerMountSummary>,
    pub networks: Vec<ContainerNetworkSummary>,

    pub labels: BTreeMap<String, String>,

    pub compose: Option<ComposeContainerMetadata>,
    pub health: Option<ContainerHealthSummary>,

    pub operation_state: ContainerOperationState,
}
```

运行状态：

```rust
pub enum ContainerRuntimeState {
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

操作状态：

```rust
pub enum ContainerOperationState {
    Idle,
    Starting,
    Stopping,
    Restarting,
    Killing,
    Pausing,
    Unpausing,
    Removing,
    Renaming,
}
```

不要通过多个 bool 表示操作状态。

---

# 七、Compose 和 Dev Container 分组

通过 Docker labels 识别 Compose 项目。

核心标签：

```text
com.docker.compose.project
com.docker.compose.service
com.docker.compose.container-number
com.docker.compose.project.working_dir
com.docker.compose.project.config_files
com.docker.compose.version
com.docker.compose.oneoff
```

Dev Container 可补充识别：

```text
devcontainer.local_folder
devcontainer.config_file
```

分组 ID：

```rust
pub struct ContainerGroupId {
    pub endpoint_key: DockerEndpointKey,
    pub project_name: String,
}
```

分组模型：

```rust
pub struct ContainerGroupSummary {
    pub id: ContainerGroupId,
    pub project_name: String,
    pub display_name: String,

    pub containers: Vec<String>,

    pub total_count: usize,
    pub running_count: usize,
    pub paused_count: usize,
    pub stopped_count: usize,
    pub unhealthy_count: usize,

    pub working_directory: Option<PathBuf>,
    pub config_files: Vec<PathBuf>,
    pub compose_version: Option<String>,

    pub operation_state: GroupOperationState,
}
```

没有 Compose project 标签的容器作为独立容器展示。

不要根据容器名称中的下划线猜测 Compose 分组。

---

# 八、列表分区和排序

默认分区：

```text
Running
Paused
Restarting
Stopped
```

Compose Group 根据组内状态放入对应区域：

* 至少一个 running：Running；
* 没有 running 且有 paused：Paused；
* 有 restarting：Restarting；
* 全部停止：Stopped。

也可以简化为：

```text
Running
Stopped
```

但必须准确显示组内 `n/m running`。

默认排序：

```text
Running first
Compose groups before ungrouped containers
Name A–Z
```

至少提供：

```text
Name A–Z
Name Z–A
Newest First
Oldest First
Running First
Stopped First
Compose Groups First
Individual Containers First
```

---

# 九、列表顶部工具栏

显示：

```text
Containers
1 running · 6 total

[Sort] [Search] [Create]
```

可选刷新按钮。

普通刷新只刷新 summary，不启动：

* stats；
* filesystem export；
* logs；
* inspect all；
* volume size；
* image filesystem index。

搜索范围：

* 容器名称；
* ID；
* short ID；
* image 名称；
* image ID；
* Compose project；
* Compose service；
* state；
* published port；
* network；
* label key；
* label value。

搜索大小写不敏感，使用 150–250ms debounce。

---

# 十、Compose Group Row

组行显示：

```text
展开箭头
Group 图标
项目名称
运行数量
主要操作
删除按钮
```

示例：

```text
▼ floatctf-develop_devcontainer
  1 / 4 running
```

展开后显示成员容器。

组行支持：

* 单击选中 Group；
* 点击箭头展开或折叠；
* 状态和选择相互独立；
* 展开状态跨刷新保留；
* 搜索命中子项时自动展示对应组。

组操作：

```text
Start All
Stop All
Restart All
Pause Running
Unpause Paused
Delete Containers…
Open Project Folder
Copy Project Name
```

Group 删除只删除容器。

禁止删除：

* Compose 配置文件；
* 项目目录；
* bind mount 源目录；
* named volumes，除非用户在独立明确流程中选择。

组操作使用有限并发：

```text
4–8
```

返回逐容器结果，允许部分成功。

例如：

```text
Started 3 containers.
1 container failed: db — port already in use.
```

---

# 十一、Container Row

每一行显示：

```text
图标
名称
Image
状态点
运行状态或时间
主要操作
删除
```

运行中：

```text
[Stop] [Delete]
```

已停止：

```text
[Start] [Delete]
```

Paused：

```text
[Resume] [Delete]
```

操作中：

```text
spinner
禁用冲突操作
```

状态颜色使用主题驱动颜色：

* running：positive；
* paused：warning；
* stopped：disabled/negative；
* restarting：warning；
* unhealthy：negative。

不要固定粉色、绿色或紫色。

选中使用：

```qml
Kirigami.Theme.highlightColor
Kirigami.Theme.highlightedTextColor
```

---

# 十二、右键菜单

容器右键菜单：

```text
Start / Stop
Restart
Kill
Pause / Resume
Delete…
────────────
Logs
Terminal
Files
Open in Browser
────────────
Mounts >
────────────
Copy ID
Copy Name
Copy Image
Copy…
```

显示规则：

## Running

```text
Stop
Restart
Kill
Pause
Logs
Terminal
Files
```

## Paused

```text
Resume
Kill
Delete
Logs
Files
```

Terminal 默认禁用，并提示需要先 Resume。

## Stopped

```text
Start
Delete
Logs
Files
```

Terminal 禁用并显示：

```text
Start the container to open a terminal.
```

菜单不能只改变标签。

例如点击 `Terminal` 必须：

1. 选中当前容器；
2. 切换第三栏 `Terminal`；
3. 创建真实 exec session。

---

# 十三、容器操作

实现真实 API：

```rust
pub trait ContainerService {
    async fn list_containers(...);
    async fn inspect_container(...);

    async fn start_container(...);
    async fn stop_container(...);
    async fn restart_container(...);
    async fn kill_container(...);

    async fn pause_container(...);
    async fn unpause_container(...);

    async fn rename_container(...);
    async fn remove_container(...);

    async fn create_container(...);
}
```

实际 Bollard 类型和方法必须按照仓库锁定版本校验。

Bollard 类型不能泄漏到 QML。

---

# 十四、操作一致性规则

缓存不得作为危险操作的最终依据。

例如删除容器：

```text
点击 Delete
    ↓
实时 inspect 或直接调用 Docker remove
    ↓
根据 Docker 实际响应决定成功或失败
```

操作成功后：

1. 更新内存 repository；
2. patch Qt Model；
3. 标记持久化缓存 dirty；
4. 后台执行确认刷新；
5. 等待 Docker event 时允许去重。

操作失败时：

* 恢复 `operation_state=Idle`；
* 保留原状态；
* 显示具体错误。

---

# 十五、Stop / Restart / Kill

## Stop

支持配置超时：

```text
默认 10 秒
```

用户可在确认或高级菜单选择：

```text
5s
10s
30s
60s
```

## Restart

支持超时。

## Kill

默认 signal：

```text
SIGKILL
```

高级对话框可选择：

```text
SIGTERM
SIGINT
SIGHUP
SIGUSR1
SIGUSR2
SIGKILL
```

第一阶段可以只实现默认 Kill，但错误文案必须准确。

---

# 十六、删除容器

确认对话框显示：

```text
Name
ID
Image
State
Compose Project
Mounts
```

选项：

```text
Force remove running container
Remove anonymous volumes
```

默认：

```text
Force = false
Remove anonymous volumes = false
```

删除 running container 时默认要求先停止。

不要默认删除 named volumes。

Compose Group 删除需要单独的 group confirmation，并列出所有目标容器。

---

# 十七、Open in Browser

根据容器 published ports 构建菜单。

示例：

```text
Open http://localhost:8080
Open https://localhost:8443
Copy localhost:8080
```

规则：

* 只使用 published host ports；
* `0.0.0.0` 和 `::` 在本地 endpoint 下映射为 `localhost`；
* 远程 Docker endpoint 使用远程 host；
* 不把 container internal port 当成宿主可访问端口；
* 多端口时显示子菜单；
* 80 默认 HTTP；
* 443 默认 HTTPS；
* 其他端口可以生成 HTTP 候选，但不要声称服务一定是 HTTP；
* 没有 published port 时禁用该操作。

使用 `QDesktopServices::openUrl` 或 portal。

禁止 shell 调用 `xdg-open`。

---

# 十八、Mounts 子菜单

Container mount 类型：

```rust
pub enum ContainerMountType {
    Volume,
    Bind,
    Tmpfs,
    NamedPipe,
    Cluster,
    Unknown,
}
```

菜单示例：

```text
Mounts
├── /var/lib/postgresql/data → postgres_data
├── /workspace → ~/Projects/app
└── /tmp → tmpfs
```

Volume mount：

* 点击后跳转 Volumes 页面；
* 选中对应 volume；
* 可以直接进入 Files 标签。

Bind mount：

只有满足以下条件时提供：

```text
Open Host Folder
```

条件：

* Docker endpoint 是本机；
* source path 存在；
* 当前用户可访问。

远程 Docker 只显示和复制 source path。

Tmpfs 只显示属性。

---

# 十九、Info 标签

Container 选中时，Info 至少包含：

```text
General
State
Health
Ports
Mounts
Networks
Configuration
Environment
Labels
```

## General

```text
Name
ID
Image
Image ID
Created
Platform
Hostname
Domain Name
Working Directory
User
```

## State

```text
Status
Running
Paused
Restarting
OOM Killed
Dead
Exit Code
Error
Started At
Finished At
Restart Count
```

## Health

```text
Status
Failing Streak
Last Check
Recent Health Logs
```

Health 不存在时隐藏 Section。

## Configuration

```text
Entrypoint
Command
Working Directory
User
Stop Signal
Stop Timeout
Restart Policy
Auto Remove
TTY
Open Stdin
Read-only RootFS
Privileged
```

不要显示：

```text
Some(...)
None
Vec(...)
```

---

# 二十、Mounts 表格

列：

```text
Type
Source
Destination
Access
Propagation
```

Volume：

```text
volume | postgres_data | /var/lib/postgresql/data | Read/Write
```

Bind：

```text
bind | ~/Projects/app | /workspace | Read/Write
```

需要：

* 长路径 elide；
* Tooltip；
* Copy；
* Volume 可点击跳转；
* 本地 Bind 可点击打开；
* read-only 清晰显示。

---

# 二十一、Ports 表格

列：

```text
Container Port
Protocol
Host IP
Host Port
Action
```

例如：

```text
80
tcp
0.0.0.0
8080
Open
```

区分：

* exposed but not published；
* published；
* host networking；
* 多 host binding。

---

# 二十二、Networks 表格

显示：

```text
Network
IP Address
Gateway
MAC Address
Aliases
Endpoint ID
```

Network 名称可点击跳转 Networks 页面并选中对应 network。

---

# 二十三、Environment

环境变量可能包含 secret。

UI 默认显示：

```text
KEY
••••••••
```

提供逐项：

```text
Reveal
Copy Key
Copy Value
```

可配置只对疑似敏感 key 隐藏：

```text
PASSWORD
TOKEN
SECRET
KEY
CREDENTIAL
AUTH
```

但更安全的默认行为是所有 values 隐藏。

不要将环境变量 value 写入持久化缓存或普通日志。

---

# 二十四、Labels

使用通用 KeyValueTable：

```text
Key | Value
```

支持：

* 搜索；
* 排序；
* Copy；
* Tooltip；
* 长值 elide。

少于 8 项时不显示搜索框。

---

# 二十五、Group Info

Compose Group 选中时，Info 显示：

```text
Group
├── Project Name
├── Status
├── Containers
├── Working Directory
├── Compose Files
├── Compose Version
└── Labels / Metadata
```

成员列表：

```text
Name
Service
State
Image
```

点击成员：

```text
切换为 Container selection
```

本地 working directory 可显示：

```text
Open Project Folder
```

在 KDE/Linux 中使用文案：

```text
Open Project Folder
```

不要硬编码：

```text
Show in Finder
```

远程 endpoint 或路径不存在时隐藏操作。

---

# 二十六、Stats 标签

Running Container 选中时启动真实 stats stream。

展示：

```text
CPU
Memory
Network I/O
Block I/O
PIDs
```

建议布局：

```text
Current Metrics
History Charts
Per-interface / Per-device details
```

至少显示：

```text
CPU %
Memory Used
Memory Limit
Memory %
Network RX
Network TX
Block Read
Block Write
PIDs
```

CPU 百分比必须基于连续采样计算，并正确处理：

* cpu delta；
* system cpu delta；
* online CPU 数；
* daemon 平台差异；
* counter reset。

内存值应准确标注：

```text
Raw usage
Working set
Limit
```

不要把自定义计算值错误标成 Docker 原始值。

---

# 二十七、Stats 生命周期

只在以下条件成立时保持 stats stream：

```text
选择为 Container
当前标签为 Stats
Container state = Running
页面可见
```

离开 Stats：

```text
取消 stream
```

切换容器：

```text
取消旧 stream
generation + 1
启动新 stream
```

UI 使用内存 ring buffer：

```text
最近 5–15 分钟
```

不默认持久化 stats 历史。

采样间隔：

```text
1 秒
```

Qt 主线程只接收整理后的 sample。

---

# 二十八、Group Stats

Group 选中时显示：

```text
Aggregate
Per-container
```

Aggregate：

```text
CPU 总和
Memory 总和
Network RX/TX 总和
Block I/O 总和
Running count
```

Container CPU 总和可能超过 100%，UI 应明确按主机逻辑 CPU 语义展示。

已停止成员不建立 stats stream。

限制并发并共享已有 container stats stream。

---

# 二十九、Logs 标签

实现真实 Docker logs。

功能：

```text
stdout / stderr
follow
timestamps
tail
since
search
pause display
copy
save
clear viewport
auto-scroll
wrap lines
```

默认：

```text
tail = 500 或 1000
follow = running container
timestamps = true
```

`Clear` 只清空当前 UI viewport。

禁止声称删除 Docker 历史日志。

---

# 三十、Logs UI

顶部：

```text
[Search logs…] [Follow] [Timestamps] [Wrap] [Copy] [Save As…] [Clear View]
```

日志区：

* 等宽字体；
* stdout / stderr 可用轻微主题差异；
* 不使用固定红绿；
* 支持文本选择；
* 支持复制；
* 支持跳转底部；
* 用户向上滚动时暂停自动滚动；
* 回到底部恢复。

避免每一行创建复杂 QML 对象。

大量日志使用虚拟化 model 或分块文本缓冲。

---

# 三十一、Logs 生命周期

只在 Logs 标签激活时保持 stream。

离开标签：

* 取消 follow stream；
* 保留当前 viewport；
* 短时间内切回可以重新 follow；
* 不继续无限积累日志。

内存上限必须设置，例如：

```text
10,000–50,000 行
或 16–64 MiB
```

超过上限丢弃最旧内容，并显示：

```text
Older log entries were discarded from this view.
```

---

# 三十二、Group Logs

Group 选中时提供合并日志：

```text
[container-name] log content
```

支持按成员筛选：

```text
All containers
db
dev
nginx
rustfs
```

日志合并按到达顺序展示。

不要伪造严格全局时间排序。

每个成员独立 stream，组选择变更时统一取消。

---

# 三十三、Terminal 标签

Terminal 只支持 running container。

实现真实 Docker exec：

```text
create_exec
start_exec
stdin
stdout
stderr
TTY
resize_exec
```

不要使用普通 TextArea 假装终端。

优先复用项目已有终端组件。

若 Commands 模块已有可交互终端 surface，应抽取通用组件。

如果当前仓库没有真正终端控件：

* 选择项目已经认可的终端组件；
* 不在 QML 中手写 ANSI 解析器；
* 不提交只支持纯文本的假实现；
* 主 Agent需在实现报告中说明依赖选择。

---

# 三十四、Shell 选择

不要假设容器有 Bash。

按顺序尝试：

```text
容器配置的 shell，若有
/bin/bash
/bin/sh
/bin/ash
/bin/zsh
/bin/dash
```

每次尝试创建并启动 exec。

找到可用 shell 后缓存到当前 container session。

全部失败时显示：

```text
No supported shell was found in this container.
```

并提供：

```text
Copy Container ID
View Logs
View Files
```

本阶段不实现注入式 Debug Shell。

不要把 `Debug Shell` 做成未实现占位按钮。

---

# 三十五、Terminal Exec 配置

建议默认：

```text
AttachStdin = true
AttachStdout = true
AttachStderr = true
Tty = true
Detach = false
```

工作目录：

* 优先 container configured working directory；
* 不存在时 `/`。

用户：

* 默认使用 container configured user；
* 可以提供高级选择；
* 不默认强制 root。

环境：

```text
TERM=xterm-256color
COLORTERM=truecolor
```

根据 terminal 控件能力配置。

---

# 三十六、Terminal 生命周期

Terminal session 状态：

```rust
pub enum ContainerTerminalState {
    Idle,
    Connecting,
    Ready,
    Exited,
    Error,
}
```

切换其他 tab：

* session 可以短时间保留；
* 不继续创建新 session；
* 关闭选中容器或容器停止时结束 session；
* 应用退出时关闭 stdin 和任务。

容器 stop/die event 到达：

```text
标记 terminal Exited
显示 exit 提示
```

Resize：

```text
QML terminal size change
→ debounce
→ Docker resize_exec
```

---

# 三十七、Files 标签语义

Container Files 展示：

```text
容器合并后的 root filesystem snapshot
```

容器文件系统是可变的。

必须明确使用 snapshot 语义：

```text
Snapshot updated 8 seconds ago
[Refresh Snapshot]
```

不要让用户误以为文件列表持续实时更新。

---

# 三十八、Container Files 获取方式

禁止依赖容器内部存在：

```text
sh
find
ls
stat
tar
```

通用实现应使用 Docker Engine 提供的容器 rootfs 导出或 archive/copy API。

建议：

```text
Container ID
    ↓
export merged container filesystem
    ↓
流式解析 tar headers
    ↓
构建临时 filesystem index
    ↓
目录浏览从内存 index 查询
```

索引构建必须：

* 流式；
* 不将整个 tar 读入内存；
* 不执行容器内程序；
* 不阻塞 Qt 主线程。

文件预览和 Save As 使用 container archive/copy API。

实际使用哪个 Bollard API 必须根据锁定版本验证。

---

# 三十九、Container Files 与 Mounts

Docker container export 通常不能被视为 mounted volume 内容的可靠实时视图。

因此 Files UI 需要结合 inspect 中的 Mounts。

对于 mount destination：

```text
/var/lib/postgresql/data
/workspace
```

在文件索引中标记为：

```text
Mounted Volume
Bind Mount
Tmpfs
```

点击：

## Volume mount

跳转 Volumes Files。

## 本地 Bind mount

可打开宿主目录。

## 远程 Bind mount

显示源路径和复制操作。

## Tmpfs

展示属性，不能跳转。

不要把镜像底层被 mount 遮蔽的目录内容冒充为运行时实际内容。

---

# 四十、Container Files 缓存

Container rootfs 可变化，所以不要像 Image Files 一样长期缓存。

建议：

```text
内存 snapshot TTL：5–15 秒
页面切回：立即显示已有 snapshot
后台提示用户可 Refresh
容器 start/restart/die 事件：立即失效
应用重启：默认不持久化完整文件索引
```

同一个 container 同时发起多个 snapshot 请求时使用 SingleFlight。

用户主动 Refresh：

```text
绕过 TTL
重新构建 snapshot
```

---

# 四十一、Create Container

顶部 `+` 打开：

```text
CreateContainerDialog
```

必须支持：

```text
Basic
Command
Ports
Mounts
Environment
Networks
Resources
Restart Policy
Labels
Advanced
```

## Basic

```text
Container Name
Image
Create Only / Create and Start
```

Image 选择：

* 从本地 Images model 选择；
* 支持输入 image reference；
* image 不存在时询问是否 Pull；
* 不静默拉取。

## Command

```text
Entrypoint
Command
Working Directory
User
TTY
Keep stdin open
```

Command 和 Entrypoint 使用参数列表模型。

不要通过空格简单 split shell 字符串。

---

# 四十二、Create：Ports

每行：

```text
Container Port
Protocol
Host IP
Host Port
```

规则：

* container port 必填；
* protocol tcp/udp/sctp；
* host port 可空，允许 Docker 自动分配；
* 检查重复绑定；
* 显示格式化预览。

---

# 四十三、Create：Mounts

支持：

```text
Named Volume
Bind Mount
Tmpfs
```

Volume：

```text
Source Volume
Destination
Read Only
```

Bind：

```text
Host Path
Destination
Read Only
Propagation
```

远程 Docker：

* host path 指远程宿主；
* UI 必须明确提示；
* 不使用本机文件选择器假装路径属于远程宿主。

Tmpfs：

```text
Destination
Size
Mode
```

---

# 四十四、Create：Environment

Key / Value 编辑器。

支持：

* 添加；
* 删除；
* 从 `.env` 导入，本地 endpoint 条件下；
* 检测重复 key；
* value 可为空；
* 敏感 value 默认遮挡。

不要记录环境值。

---

# 四十五、Create：Networks

支持：

* 默认 bridge；
* none；
* host，平台支持时；
* 选择已有网络；
* aliases；
* IPv4 / IPv6 高级选项。

首个网络随 create config 设置。

额外网络在容器创建后通过 Docker network connect 完成。

任何后续 connect 失败时：

* 返回部分成功；
* 提示容器已创建；
* 列出网络失败；
* 不静默删除容器，除非用户明确选择 rollback。

---

# 四十六、Create：Resources

支持：

```text
CPU limit
CPU shares
Memory limit
Memory reservation
Swap
PIDs limit
Block I/O weight
```

第一阶段至少：

```text
CPU limit
Memory limit
PIDs limit
```

单位输入必须明确：

```text
MiB
GiB
CPU cores
```

不要要求用户填写底层 nano_cpus 数值。

---

# 四十七、Create：Restart Policy

支持：

```text
No
Always
Unless Stopped
On Failure
```

On Failure 支持：

```text
Maximum retry count
```

---

# 四十八、Create 验证和提交

提交前验证：

* name；
* image；
* ports；
* mount destinations；
* duplicate env keys；
* network；
* resource limits；
* restart policy。

流程：

```text
Validate
    ↓
Create container
    ↓
Connect extra networks
    ↓
Start，若选择
    ↓
更新 repository
    ↓
选中新容器
```

创建失败时保留用户输入。

禁止重复提交。

---

# 四十九、Docker Events

全局监听：

```text
container
image
volume
network
daemon
```

Containers 至少处理：

```text
create
start
stop
die
kill
pause
unpause
restart
rename
destroy
health_status
oom
attach
detach
```

事件到达：

```text
针对性更新 summary
使 detail/live streams 缓存失效
更新分组
更新 running count
```

Compose 批量启动时进行 debounce：

```text
100–500 ms
```

不要每个事件都完整刷新所有模块。

---

# 五十、事件与操作去重

TuxStack 发起 Start：

```text
API success
→ 立即 patch operation/state
→ 后续 Docker event 作为确认
```

Event 到达时发现状态已一致：

```text
不重复 reset model
```

使用 resource generation 或 version 防止旧请求覆盖。

---

# 五十一、状态模型

建议：

```rust
pub enum ContainersListState {
    Idle,
    Loading,
    Ready,
    Empty,
    Error,
    DockerUnavailable,
    PermissionDenied,
}

pub enum ContainerDetailState {
    None,
    Loading,
    Ready,
    Error,
}

pub enum ContainerTab {
    Info,
    Stats,
    Logs,
    Terminal,
    Files,
}
```

每个 live tab 有自己的状态：

```text
StatsState
LogsState
TerminalState
ContainerFilesState
```

不要把所有状态塞进一个全局 `loading`。

---

# 五十二、Repository 和请求去重

建议：

```text
ContainerRepository
├── Summary Store
├── Detail Cache
├── Compose Group Index
├── Stats Sessions
├── Logs Sessions
├── Terminal Sessions
└── Files Snapshot Cache
```

请求键：

```rust
pub enum ContainerRequestKey {
    ListContainers,
    InspectContainer(String),
    Stats(String),
    Logs(String, LogsOptionsKey),
    FilesSnapshot(String),
}
```

相同 inspect 和 filesystem snapshot 使用 SingleFlight。

---

# 五十三、持久化缓存

允许持久化：

```text
Container summary
Container detail 非敏感字段
Compose group metadata
最近 selection
列表排序与展开状态
```

默认不持久化：

```text
Environment values
Logs
Terminal output
Container file index
Container file contents
Secrets
Credentials
```

缓存按 Docker endpoint 和 daemon identity 隔离。

页面显示缓存后后台刷新。

危险操作永远不依赖缓存判断。

---

# 五十四、Qt Model 增量更新

不要频繁完整 reset。

实现：

```text
insert rows
remove rows
move rows
patch roles
dataChanged
```

完整 reset 仅用于：

* endpoint 切换；
* 大规模不可避免重建；
* schema 变化。

必须保持：

* selection；
* group expand state；
* scroll position；
* search；
* sort；
* active tab。

---

# 五十五、异步与竞态

Qt 主线程禁止：

```text
Docker API
stream 读取
tar 解析
日志处理
stats 计算
SQLite
文件下载
同步等待
block_on
thread::sleep
```

使用共享 Tokio runtime。

Generation：

```rust
list_generation
selection_generation
detail_generation
stats_generation
logs_generation
terminal_generation
files_generation
```

旧结果可以进入共享 cache，但不能更新当前 UI。

---

# 五十六、错误模型

至少区分：

```rust
pub enum ContainerError {
    NotFound(String),
    AlreadyRunning(String),
    NotRunning(String),
    Paused(String),
    RemovalInProgress(String),

    PortAlreadyAllocated(String),
    NameAlreadyInUse(String),
    ImageNotFound(String),
    ImagePullRequired(String),

    VolumeNotFound(String),
    NetworkNotFound(String),

    StartFailed(String),
    StopFailed(String),
    RestartFailed(String),
    KillFailed(String),
    PauseFailed(String),
    UnpauseFailed(String),
    RemoveFailed(String),
    RenameFailed(String),

    StatsUnavailable(String),
    LogsUnavailable(String),
    ExecFailed(String),
    ShellNotFound,
    TerminalDisconnected,

    FilesSnapshotFailed(String),
    FileNotFound(String),
    FilePreviewFailed(String),

    PermissionDenied,
    DockerUnavailable,
    OperationTimeout,
    OperationCancelled,
}
```

UI 不允许全部显示：

```text
Operation failed
```

---

# 五十七、KDE 视觉规范

必须使用：

```qml
Kirigami.Theme.backgroundColor
Kirigami.Theme.alternateBackgroundColor
Kirigami.Theme.textColor
Kirigami.Theme.disabledTextColor
Kirigami.Theme.highlightColor
Kirigami.Theme.highlightedTextColor
Kirigami.Theme.positiveTextColor
Kirigami.Theme.negativeTextColor

Kirigami.Units.smallSpacing
Kirigami.Units.mediumSpacing
Kirigami.Units.largeSpacing
Kirigami.Units.gridUnit
```

禁止复制参考截图中的：

```text
macOS traffic lights
固定紫色 selection
Finder 文案
固定浅灰背景
Web 卡片阴影
大胶囊 tab
```

参考图只用于信息架构和交互范围。

最终视觉必须与当前 TuxStack 的 Images、Volumes、Networks 一致。

---

# 五十八、建议文件结构

按实际仓库调整。

docker-core：

```text
models/container.rs
models/compose_group.rs

services/containers/mod.rs
services/containers/list.rs
services/containers/inspect.rs
services/containers/actions.rs
services/containers/create.rs
services/containers/stats.rs
services/containers/logs.rs
services/containers/exec.rs
services/containers/files.rs
services/containers/grouping.rs
```

GUI Rust：

```text
controllers/containers.rs
controllers/container_stats.rs
controllers/container_logs.rs
controllers/container_terminal.rs
controllers/container_files.rs
controllers/create_container.rs

models/container_list_model.rs
models/container_group_model.rs
models/container_mount_model.rs
models/container_network_model.rs
models/container_port_model.rs
models/container_log_model.rs
models/container_file_model.rs
```

QML：

```text
pages/ContainersPage.qml

components/containers/ContainerListPanel.qml
components/containers/ContainerGroupItem.qml
components/containers/ContainerListItem.qml
components/containers/ContainerDetailTabs.qml
components/containers/ContainerInfoView.qml
components/containers/ContainerGroupInfoView.qml
components/containers/ContainerStatsView.qml
components/containers/ContainerLogsView.qml
components/containers/ContainerTerminalView.qml
components/containers/ContainerFilesView.qml
components/containers/ContainerContextMenu.qml

dialogs/containers/CreateContainerDialog.qml
dialogs/containers/RemoveContainerDialog.qml
dialogs/containers/RemoveContainerGroupDialog.qml
dialogs/containers/KillContainerDialog.qml
dialogs/containers/RenameContainerDialog.qml
```

优先复用：

```text
PropertySection
PropertyRow
KeyValueTable
FilesystemTable
Breadcrumb
FilePreviewDialog
SearchField
ErrorView
LoadingView
```

---

# 五十九、实施阶段

## Phase 0：审计和基准

* 仓库结构；
* 当前旧实现；
* Docker API；
* 可复用组件；
* 性能基准；
* 测试基准。

## Phase 1：Container Core

* models；
* list；
* inspect；
* grouping；
* actions；
* errors；
* tests。

## Phase 2：列表和选择

* ContainerListModel；
* Compose Group；
* search；
* sort；
* selection；
* quick actions；
* context menu。

## Phase 3：Info

* container detail；
* group detail；
* mounts；
* ports；
* networks；
* labels；
* environment。

## Phase 4：操作

* start；
* stop；
* restart；
* kill；
* pause；
* unpause；
* remove；
* rename；
* group operations。

## Phase 5：Stats

* stream；
* samples；
* charts；
* aggregate group stats；
* cancellation。

## Phase 6：Logs

* tail；
* follow；
* search；
* group merge；
* save；
* memory limit。

## Phase 7：Terminal

* exec；
* terminal component；
* shell fallback；
* input/output；
* resize；
* lifecycle。

## Phase 8：Files

* snapshot index；
* mount overlays；
* directory navigation；
* preview；
* Save As；
* cache。

## Phase 9：Create Container

* complete dialog；
* validation；
* create；
* network connect；
* start；
* partial failure。

## Phase 10：Events、缓存和性能

* events；
* singleflight；
* incremental model；
* stale-while-revalidate；
* persistent summary cache。

## Phase 11：清理和质量

* 删除旧实现；
* Light/Dark；
* keyboard；
* accessibility；
* tests；
* clippy；
* docs。

每个 Phase 完成后必须可构建、可测试。

---

# 六十、测试要求

## Summary 和分组

测试：

* running；
* paused；
* restarting；
* exited；
* dead；
* health；
* Compose group；
* ungrouped；
* multiple projects；
* one-off container；
* devcontainer labels；
* group counts；
* sorting；
* searching。

## Actions

测试：

* start stopped；
* start running；
* stop running；
* stop stopped；
* restart；
* kill；
* pause；
* unpause；
* remove stopped；
* remove running without force；
* force remove；
* rename conflict；
* partial group failure；
* operation state cleanup。

## Info

测试：

* ports；
* mounts；
* networks；
* restart policy；
* health；
* labels；
* environment masking；
* missing fields；
* malformed timestamps。

## Stats

测试：

* CPU delta；
* counter reset；
* memory；
* network；
* block I/O；
* PIDs；
* stopped container；
* stream cancellation；
* selection race；
* group aggregate。

## Logs

测试：

* stdout；
* stderr；
* timestamps；
* follow；
* tail；
* stopped container logs；
* memory limit；
* search；
* cancel；
* group merge；
* reconnect。

## Terminal

测试：

* `/bin/bash`；
* `/bin/sh` fallback；
* no shell；
* stdin；
* output；
* resize；
* exit；
* container stop；
* selection switch；
* cleanup。

## Files

测试：

* root snapshot；
* directory；
* symlink；
* hardlink；
* hidden files；
* mount destination；
* volume navigation；
* bind mount；
* preview；
* Save As；
* refresh；
* snapshot TTL；
* old result race。

## Create

测试：

* valid create；
* create and start；
* image missing；
* invalid name；
* duplicate name；
* ports；
* volume；
* bind；
* environment；
* network；
* resource limits；
* partial network failure；
* validation state retention。

## Events

测试：

* create；
* start；
* stop；
* die；
* pause；
* unpause；
* rename；
* destroy；
* health；
* event burst；
* reconnect；
* daemon restart；
* optimistic patch dedup。

---

# 六十一、Docker 集成测试

使用：

```rust
#[ignore]
```

或 feature：

```text
docker-integration
```

测试资源：

```text
tuxstack-test-container-<uuid>
tuxstack-test-network-<uuid>
tuxstack-test-volume-<uuid>
```

流程：

1. 创建测试 network；
2. 创建测试 volume；
3. 创建测试 container；
4. 验证 list；
5. 验证 grouping；
6. start；
7. stats；
8. logs；
9. terminal exec；
10. files snapshot；
11. pause；
12. unpause；
13. restart；
14. stop；
15. inspect；
16. rename；
17. remove；
18. 清理 network 和 volume；
19. 确认无残留资源。

任何测试失败也必须执行清理。

---

# 六十二、人工验收矩阵

必须逐项实测：

1. 页面打开后快速显示容器。
2. Running / Stopped 数量准确。
3. Compose Group 分组准确。
4. 展开和折叠保持。
5. 搜索准确。
6. 排序准确。
7. 容器选择不闪烁。
8. Group 选择显示 Group Info。
9. Container 选择显示真实 Info。
10. Start 成功后立即更新行。
11. Stop 成功后立即更新行。
12. Restart、Kill、Pause、Resume 正常。
13. Delete 确认准确。
14. Group 操作支持部分失败报告。
15. Mounts 可跨页面跳转。
16. Ports 可打开或复制。
17. Stats 实时更新。
18. 离开 Stats 后 stream 结束。
19. Logs 能 tail 和 follow。
20. 大量日志不无限占内存。
21. Terminal 可真实交互。
22. 容器停止后 Terminal 正确结束。
23. Files 显示真实 snapshot。
24. Volume mount 不冒充 rootfs 内容。
25. Files Refresh 生效。
26. Create Container 生效。
27. Docker CLI 外部操作后 UI 自动更新。
28. Docker Compose 批量事件没有刷新风暴。
29. Docker Engine 关闭后显示准确错误。
30. Engine 恢复后自动同步。
31. Breeze Light 正常。
32. Breeze Dark 正常。
33. 高 DPI 正常。
34. 键盘导航正常。
35. 没有 mock 数据。
36. 没有 docker CLI 调用。
37. 没有旧版 Container 实现残留。
38. 没有未实现按钮。
39. 没有 Qt 主线程阻塞。
40. 没有遗留 stats/logs/exec/background task。

---

# 六十三、构建验证

使用 `cargo metadata` 确认实际 package 名称后执行对应命令。

至少完成：

```bash
cargo fmt --all -- --check
cargo check --workspace
cargo test --workspace
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo build --workspace
```

GUI 启动命令必须根据仓库实际 package 名称执行。

如果某个 all-features 配置依赖系统环境无法构建，应：

1. 给出真实错误；
2. 执行项目支持的标准 feature 集；
3. 不虚报全部通过。

---

# 六十四、性能报告

必须测量优化前后：

```text
Containers first visible list
List refresh
Container inspect
Group construction
Info cache hit
Stats first sample
Logs first batch
Terminal connection
Files cold snapshot
Files warm snapshot
Create and start
Event-to-UI latency
```

报告示例：

```text
Containers page:
first visible list: xxx ms
complete summary enrichment: xxx ms

Container Info:
cold inspect: xxx ms
cache hit: xxx ms

Stats:
first sample: xxx ms

Terminal:
exec ready: xxx ms

Files:
cold snapshot: xxx ms
warm reuse: xxx ms
```

只能填写真实测量结果。

---

# 六十五、最终交付报告

完成后必须输出：

1. 仓库审计结果；
2. 删除的旧实现；
3. 新增文件；
4. 修改文件；
5. Container domain model；
6. Compose grouping 规则；
7. 列表和 selection 架构；
8. 操作一致性设计；
9. Info 实现；
10. Stats 生命周期；
11. Logs 生命周期；
12. Terminal 实现；
13. Files snapshot 语义；
14. Create Container 实现；
15. Docker Events 策略；
16. 缓存和 SingleFlight；
17. Qt Model 增量更新；
18. 安全措施；
19. 单元测试结果；
20. Docker 集成测试结果；
21. Light/Dark 验证；
22. 性能数据；
23. 已知限制；
24. 每个 Agent 的 commit；
25. 最终整合 commit hash。

最终要求：

```text
列表快速显示
操作基于实时 Docker
事件驱动更新
Compose 分组准确
Info 信息完整
Stats 不泄漏 stream
Logs 不无限占内存
Terminal 真实可交互
Files 语义准确
创建流程真实可用
旧实现彻底删除
```

不要只实现截图外观。最终交付必须形成完整、真实、可维护、可测试的 Docker Containers 管理模块。

