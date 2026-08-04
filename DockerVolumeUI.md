
# TuxStack Docker Volumes 模块完整实现 Prompt

你需要在当前 `tuxstack` 项目中完整实现 Docker Volumes 模块。

当前项目架构：

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

本次任务只实现 Docker Volumes。

继续复用当前已经完成的：

* 应用侧边栏
* KDE Breeze Hover / Selected 样式
* Qt 与 Tokio 异步框架
* `docker-core`
* 统一错误模型
* 页面导航机制
* Images / Networks 模块中已经验证正确的页面状态模式

禁止引入：

```text
daemon
REST API
JSON-RPC
Incus
Kubernetes
mock 数据
shell 调用 docker CLI
sudo
额外数据库
```

所有卷数据和操作必须通过 Bollard 调用真实 Docker Engine。

---

# 一、目标页面结构

参考用户提供的截图保留三栏信息架构：

```text
┌──────────────┬──────────────────────┬───────────────────────────────┐
│ App Sidebar  │ Volume List          │ Volume Detail                 │
│              │                      │                               │
│ Containers   │ Volumes              │ General                       │
│ Images       │ count / total size   │ Name                          │
│ Volumes      │ search / sort        │ Driver                        │
│ Networks     │ refresh / create     │ Scope                         │
│              │                      │ Mountpoint                    │
│              │ In Use              │ Created                       │
│              │ volume rows          │ Size                          │
│              │                      │                               │
│              │ Unused              │ Actions                       │
│              │ volume rows          │ Export                        │
│              │                      │ Clone                         │
│              │                      │                               │
│              │                      │ Used By                       │
│              │                      │ Labels                        │
│              │                      │ Options                       │
└──────────────┴──────────────────────┴───────────────────────────────┘
```

继续使用现有应用侧边栏，不修改其结构和视觉。

Volumes 页面内部建议：

```text
VolumesPage
├── VolumeListPanel
└── VolumeDetailPanel
```

桌面宽屏下：

```text
VolumeListPanel：280–340 dp
VolumeDetailPanel：占据剩余宽度
```

使用 `RowLayout`、`Layout.preferredWidth` 和 `Layout.fillWidth`。

不要复制截图中的固定像素尺寸。

---

# 二、功能范围

本次必须真实实现：

* 页面进入时自动加载 Docker volumes
* 卷列表
* In Use / Unused 分组
* 卷搜索
* 卷排序
* 创建卷
* 删除卷
* 查看卷详情
* 查看使用该卷的容器
* 查看卷 Labels
* 查看卷 Driver Options
* 展示卷挂载路径
* 展示创建时间
* 展示卷大小，无法获取时显示 Unknown
* 导出卷数据
* 克隆卷
* 刷新后保持当前选择
* 删除后选择相邻卷
* Docker unavailable 状态
* permission denied 状态
* loading / empty / error 状态
* Light / Dark KDE 主题适配
* 键盘导航
* 操作取消
* 真实错误处理

本阶段暂不实现：

```text
直接浏览卷文件
在线编辑卷内容
卷压缩策略配置
远程 Docker Host
Swarm Volume Plugin 管理
备份计划
定时快照
卷加密
Compose 项目编辑
```

---

# 三、页面生命周期

进入 Volumes 页面时必须自动加载，不能依赖用户点击 Refresh。

正确流程：

```text
VolumesPage created
    ↓
VolumesController.initialize()
    ↓
并行请求：
- list volumes
- list all containers
- system disk usage（用于卷大小和 RefCount）
    ↓
构建 volume → containers 关联
    ↓
更新 VolumeListModel
    ↓
自动选择第一个卷
    ↓
显示 Volume Detail
```

QML：

```qml
Component.onCompleted: {
    volumesController.initialize()
}
```

Rust Controller：

```rust
pub fn initialize(self: Pin<&mut Self>) {
    if self.initialized() {
        return;
    }

    self.set_initialized(true);
    self.refresh();
}
```

要求：

* 初始化只执行一次
* 页面重新进入时复用已有数据
* 用户点击 Refresh 时执行真实刷新
* 不重复创建 Tokio runtime
* 不重复创建 Bollard client
* 不吞掉初始化错误

---

# 四、状态模型

列表状态与详情状态必须独立。

## Volume List State

```rust
pub enum VolumesListState {
    Idle,
    Loading,
    Ready,
    Empty,
    Error,
    DockerUnavailable,
    PermissionDenied,
}
```

## Volume Detail State

```rust
pub enum VolumeDetailState {
    None,
    Loading,
    Ready,
    Error,
}
```

## Volume Operation State

```rust
pub enum VolumeOperation {
    Creating,
    Removing,
    Exporting,
    Cloning,
}
```

每个卷独立保存操作状态：

```rust
HashMap<String, VolumeOperation>
```

页面级操作：

```rust
pub enum GlobalVolumeOperation {
    Creating,
    Pruning,
}
```

---

# 五、状态行为

## 有卷

```text
List = Ready
Detail = Ready
```

加载完成后自动选择第一项。

## 无卷

左侧显示：

```text
No Docker volumes found.
Create a volume to get started.
```

右侧保持完全空白。

禁止显示：

```text
Select a volume
No volume selected
PlaceholderMessage
```

## 列表加载失败

左侧显示明确错误和 Retry。

右侧保持空白。

## 详情加载失败

左侧列表继续正常。

右侧显示：

```text
Volume details unavailable
<真实原因>
[Retry]
```

## 删除当前卷

* 删除成功后更新列表
* 自动选择相邻卷
* 没有其他卷时，右侧变为空白
* 不显示残留详情
* 不显示永久 skeleton

---

# 六、固定三栏布局

右侧详情栏必须永久保留布局空间。

禁止：

```qml
Loader {
    active: selectedVolumeName !== ""
}
```

这会让第三栏从布局中消失。

正确结构：

```qml
RowLayout {
    AppSidebar {}

    VolumeListPanel {
        Layout.preferredWidth: Kirigami.Units.gridUnit * 16
        Layout.fillHeight: true
    }

    Kirigami.Separator {
        Layout.fillHeight: true
    }

    VolumeDetailPanel {
        Layout.fillWidth: true
        Layout.fillHeight: true
    }
}
```

`VolumeDetailPanel` 内：

```text
DetailState.None    → blank
DetailState.Loading → skeleton
DetailState.Ready   → content
DetailState.Error   → error
```

---

# 七、Volumes 列表顶部

中间栏顶部显示：

```text
Volumes
8 volumes
1.42 GiB known volume data

[Sort] [Search] [Refresh] [Create]
```

注意大小文案。

Docker 无法获取全部卷大小时，不允许显示误导性的总大小。

建议状态：

### 全部大小已知

```text
1.42 GiB total volume data
```

### 部分大小未知

```text
1.42 GiB known · 3 volumes unknown
```

### 全部未知

```text
Volume sizes unavailable
```

禁止：

```text
0 B total
```

来表示未知。

工具栏建议使用：

```qml
Kirigami.Heading
QQC2.ToolButton
QQC2.TextField
QQC2.Menu
Kirigami.Action
```

按钮图标建议：

```text
view-sort-ascending
edit-find
view-refresh
list-add
edit-clear-history
```

创建按钮可使用：

```text
list-add
```

Prune 后续可放进溢出菜单，不要做成容易误触的主按钮。

---

# 八、卷列表分组

分组：

```text
In Use
Unused
```

## In Use 定义

卷被至少一个现存容器引用。

包括容器状态：

```text
created
running
paused
restarting
exited
dead
```

停止的容器仍然算使用该卷。

## Unused 定义

当前没有任何容器 mount 引用该卷。

不能只依赖：

```text
UsageData.RefCount
```

因为该字段可能缺失或未知。

正确关联方式：

1. 调用 `list_containers(all=true)`
2. 对每个容器执行必要的 inspect
3. 读取 `Mounts`
4. 仅处理：

   ```text
   Type == volume
   ```
5. 通过 `Name` 与 volume name 关联
6. 建立：

   ```rust
   HashMap<VolumeName, Vec<VolumeContainerReference>>
   ```

为避免初次加载 N+1 请求，可以采用以下策略：

* `list_containers(all=true)` 返回的摘要字段足够时直接使用
* 如果摘要缺少 Mounts，再有限并发 inspect
* 限制 inspect 并发，例如 8 或 16
* 缓存结果
* 避免串行 inspect 全部容器

---

# 九、卷列表行

每行至少显示：

```text
卷图标
卷名称
大小或大小状态
容器数量
删除按钮
```

示例：

```text
[volume icon] postgres_data
              70.7 MiB · 1 container
```

未知大小：

```text
[volume icon] cache_data
              Unknown size · Unused
```

匿名卷：

```text
659f4ab7d5bb...
```

可使用短显示名称，同时 Tooltip 展示完整名称。

组件：

```text
VolumeListItem.qml
```

公开属性建议：

```qml
property string volumeName
property string displayName
property string driver
property string sizeText
property int usedByCount
property bool inUse
property bool anonymous
property bool selected
property bool busy
property string operation

signal selectedRequested(string volumeName)
signal removeRequested(string volumeName)
```

---

# 十、匿名卷识别

匿名卷通常使用较长随机名称，但不能仅靠长度武断判断。

建议判断依据按优先级：

1. 是否有 Compose 或用户 Labels
2. 名称是否符合 Docker 生成的长十六进制形式
3. 创建请求是否由本应用记录为匿名，仅限当前进程
4. 无法确认时标记为普通卷

可实现一个保守函数：

```rust
pub fn looks_anonymous_volume(name: &str) -> bool
```

例如：

* 64 位十六进制字符串可判定为可能匿名卷
* 其他名称不进行猜测

UI 文案使用：

```text
Anonymous volume
```

作为辅助信息，名称仍然显示。

---

# 十一、Hover 和选中样式

沿用当前侧边栏和 Images 模块已经完成的 KDE Breeze 状态：

Normal：

```text
透明背景
```

Hovered：

```text
浅系统强调色背景
1 dp 系统强调色边框
```

Selected：

```text
Kirigami.Theme.highlightColor
Kirigami.Theme.highlightedTextColor
```

Pressed：

```text
主题驱动的按压反馈
```

禁止：

```text
固定紫色
macOS 风格大圆角
发光
阴影堆叠
```

删除按钮只有 Hover 当前行时可提高可见性，但键盘焦点时同样必须可访问。

---

# 十二、搜索

搜索范围：

* volume name
* driver
* scope
* mountpoint
* label key
* label value
* option key
* option value
* connected container name
* connected container ID

规则：

* 大小写不敏感
* trim 前后空格
* 本地过滤
* 150–250 ms debounce
* 清空搜索恢复完整列表
* 搜索时保持 In Use / Unused 分组
* 当前选择被过滤掉时，选择第一条可见结果
* 清空搜索后尽量恢复原选择

无匹配结果：

```text
No volumes match “xxx”.
```

---

# 十三、排序

至少支持：

```text
Name A–Z
Name Z–A
Newest First
Oldest First
Largest First
Smallest First
Most Containers
Fewest Containers
In Use First
Unused First
```

大小未知时：

* Largest First：未知排最后
* Smallest First：未知仍排最后
* 不把 Unknown 当成 0 B

默认排序：

```text
In Use First
Name A–Z
```

---

# 十四、docker-core 领域模型

在 `docker-core` 中新增或完善：

```rust
pub struct VolumeSummary {
    pub name: String,
    pub driver: String,
    pub scope: String,
    pub mountpoint: Option<String>,
    pub created_at: Option<DateTime<Utc>>,
    pub labels: BTreeMap<String, String>,
    pub options: BTreeMap<String, String>,
    pub usage: VolumeUsage,
    pub used_by: Vec<VolumeContainerReference>,
    pub anonymous: bool,
}
```

卷使用数据：

```rust
pub struct VolumeUsage {
    pub size_bytes: Option<u64>,
    pub ref_count: Option<u64>,
}
```

要求：

* Docker 返回 `-1` 时映射为 `None`
* 缺失时映射为 `None`
* 不把负值转成巨大的 `u64`
* 不把未知值映射为 0

详情：

```rust
pub struct VolumeDetail {
    pub summary: VolumeSummary,
    pub status: BTreeMap<String, String>,
}
```

容器引用：

```rust
pub struct VolumeContainerReference {
    pub id: String,
    pub short_id: String,
    pub name: String,
    pub state: ContainerState,
    pub destination: String,
    pub read_only: bool,
    pub propagation: Option<String>,
}
```

创建请求：

```rust
pub struct CreateVolumeRequest {
    pub name: Option<String>,
    pub driver: Option<String>,
    pub driver_options: BTreeMap<String, String>,
    pub labels: BTreeMap<String, String>,
}
```

删除选项：

```rust
pub struct RemoveVolumeOptions {
    pub force: bool,
}
```

导出请求：

```rust
pub struct ExportVolumeRequest {
    pub volume_name: String,
    pub destination: PathBuf,
    pub compression: VolumeExportCompression,
}
```

压缩格式：

```rust
pub enum VolumeExportCompression {
    Tar,
    TarGzip,
    TarZstd,
}
```

克隆请求：

```rust
pub struct CloneVolumeRequest {
    pub source_volume: String,
    pub target_name: String,
    pub target_driver: Option<String>,
    pub target_driver_options: BTreeMap<String, String>,
    pub copy_labels: bool,
}
```

---

# 十五、卷大小获取

这是本模块最容易做错的地方。

Docker Volume List / Inspect 的 UsageData 可能：

```text
Size = -1
RefCount = -1
```

含义为未知。

正确策略：

## 首选

使用 Docker system disk usage API：

```text
/system/df
```

从 Volumes 数据中读取：

```text
UsageData.Size
UsageData.RefCount
```

## 次选

Volume inspect 返回 UsageData 时使用。

## 无法获得

映射：

```rust
size_bytes: None
ref_count: None
```

UI：

```text
Unknown
```

禁止：

```text
-1 B
0 B
18446744073709551615 B
```

## 总大小

只对：

```rust
size_bytes: Some(size)
```

求和。

同时统计未知数量：

```rust
pub struct VolumeSizeSummary {
    pub known_total_bytes: u64,
    pub known_count: usize,
    pub unknown_count: usize,
}
```

展示：

```text
1.42 GiB known · 2 volumes unknown
```

---

# 十六、VolumeService

新增或完善：

```rust
pub struct VolumeService {
    client: Arc<DockerClient>,
}
```

公开方法至少包括：

```rust
impl VolumeService {
    pub async fn list_volumes(
        &self,
    ) -> Result<Vec<VolumeSummary>, DockerError>;

    pub async fn inspect_volume(
        &self,
        name: &str,
    ) -> Result<VolumeDetail, DockerError>;

    pub async fn create_volume(
        &self,
        request: CreateVolumeRequest,
    ) -> Result<VolumeDetail, DockerError>;

    pub async fn remove_volume(
        &self,
        name: &str,
        options: RemoveVolumeOptions,
    ) -> Result<(), DockerError>;

    pub async fn prune_volumes(
        &self,
        filters: PruneVolumeFilters,
    ) -> Result<VolumePruneResult, DockerError>;

    pub async fn export_volume(
        &self,
        request: ExportVolumeRequest,
        cancellation: CancellationToken,
    ) -> Result<(), DockerError>;

    pub async fn clone_volume(
        &self,
        request: CloneVolumeRequest,
        cancellation: CancellationToken,
    ) -> Result<VolumeDetail, DockerError>;
}
```

实际 Bollard API 名称和类型必须根据当前项目锁定版本及 docs.rs 校验。

Bollard 类型禁止泄漏到 GUI。

---

# 十七、创建卷

顶部 Create 按钮打开：

```text
CreateVolumeDialog.qml
```

字段：

```text
Name
Driver
Driver Options
Labels
```

## Name

* 可为空
* 为空时允许 Docker 生成名称
* 用户输入时 trim
* 不自行修改用户名称
* 显示 Docker 返回的校验错误

## Driver

默认：

```text
local
```

允许自定义 driver 名称。

## Driver Options

Key / Value 编辑表。

支持：

* 添加
* 删除
* 修改
* 防止空 key
* 防止重复 key
* value 可以为空

## Labels

Key / Value 编辑表。

规则相同。

创建流程：

```text
用户提交
    ↓
validate
    ↓
VolumeOperation::Creating
    ↓
Docker create volume
    ↓
刷新列表
    ↓
自动选中新卷
    ↓
显示成功通知
```

创建期间：

* 禁止重复提交
* 可以关闭对话框前确认取消
* 失败时保留输入
* 不记录 label value 和 driver option value 到普通日志

---

# 十八、删除卷

删除是真实危险操作，确认对话框必须清晰。

点击删除：

```text
Remove volume “postgres_data”?
```

显示：

```text
Name
Driver
Known size
Used by N containers
Mountpoint
```

## 正在使用的卷

如果 `used_by` 非空：

* 默认禁用删除主按钮
* 展示使用该卷的容器
* 不自动删除容器
* 不自动停止容器
* 不自动断开 mount
* 可以展示 Docker 原始冲突原因
* `force` 的语义必须跟随 Docker API，不能声称可绕过正在使用状态

## Unused 卷

允许删除。

删除成功后：

* 从模型移除
* 更新总大小
* 更新分组
* 选择相邻卷
* 显示 Kirigami passive notification

错误区分：

```text
Volume not found
Volume is in use
Permission denied
Docker unavailable
Plugin error
Operation timeout
```

---

# 十九、Prune Volumes

Prune 放在工具栏溢出菜单中：

```text
Remove unused volumes…
```

不要做成醒目的单击按钮。

Docker 对 volume prune 的行为需要准确展示：

* 只处理未被容器引用的本地卷
* 根据 Docker API 和当前版本应用过滤条件
* 用户必须确认
* 先显示候选卷列表
* 显示已知可回收大小
* 未知大小明确标记

建议对话框：

```text
Remove unused volumes?

The following 4 volumes are not referenced by any container:

cache-a       120 MiB
build-data    Unknown
...

Known reclaimable size: 120 MiB

[Cancel] [Remove Volumes]
```

不要把 system prune 与 volume prune 混为一谈。Docker 的常规 system prune 默认不会自动清理 volumes，而 volume prune 专门面向未使用卷。([Docker Documentation][2])

---

# 二十、右侧详情设计

右侧采用 KDE System Settings 风格：

```text
General
Actions
Used By
Labels
Driver Options
Status
```

不使用：

```text
Info / Files 顶部 SegmentedControl
macOS card
固定浅灰色
巨大圆角
固定紫色
Web Dashboard 卡片
大量阴影
```

右侧使用：

```qml
QQC2.ScrollView
```

内容最大宽度建议：

```text
900–1000 dp
```

超宽窗口居中。

---

# 二十一、General Section

显示：

```text
Name
Driver
Scope
Mountpoint
Created
Size
Reference Count
Anonymous
```

示例：

```text
Name                postgres_data
Driver              local
Scope               local
Mountpoint          /var/lib/docker/volumes/postgres_data/_data
Created             Mar 19, 2026 15:10 UTC
Size                70.7 MiB
Reference Count     1
Anonymous           No
```

要求：

* `Mountpoint` 可复制
* 长文本 elide
* Hover Tooltip 显示完整值
* 创建时间缺失显示 `—`
* 大小未知显示 `Unknown`
* RefCount 未知显示 `Unknown`
* 不显示 Rust Debug 格式

禁止显示：

```text
Some(...)
None
-1
```

---

# 二十二、Actions Section

包含：

```text
Export Volume
Clone Volume
```

可以使用 KDE 设置页风格的操作行：

```text
[icon] Export Volume                  >
[icon] Clone Volume                   >
```

或使用标准按钮。

不要恢复截图中的 macOS 灰色大卡片。

图标建议：

```text
document-save
document-export
edit-copy
```

使用实际存在的系统图标。

---

# 二十三、Used By Section

显示所有引用该卷的容器。

每个容器至少显示：

```text
Name
Short ID
State
Destination
Access
```

示例：

```text
postgres
a81c92d7f032
running
/var/lib/postgresql/data
Read/Write
```

只读 mount：

```text
Read Only
```

点击容器：

```text
navigateToContainer(containerId)
```

主导航接收后：

1. 切换 Containers 页面
2. 选中对应容器
3. 打开详情

无使用者：

```text
No containers are using this volume.
```

内容紧凑展示，不保留巨大空白区域。

---

# 二十四、Labels Section

展示全部 Labels：

```text
Key | Value
```

要求：

* key 按字母排序
* 支持复制
* 长值 Tooltip
* value 可以为空
* 不输出 JSON blob
* 不在 tracing 中记录值
* 空时显示：

  ```text
  No labels.
  ```

如果 Labels 少于 8 条，不显示搜索框。

---

# 二十五、Driver Options Section

展示 Docker volume options：

```text
Key | Value
```

例如：

```text
type          nfs
o             addr=10.0.0.2,rw
device        :/exports/data
```

安全要求：

* 普通 UI 可展示真实值
* 不在普通日志中输出
* 复制操作明确
* 空时显示：

  ```text
  No driver options.
  ```

---

# 二十六、Status Section

部分 volume plugin 会返回：

```text
Status: HashMap<String, String>
```

如果存在，显示：

```text
Key | Value
```

不存在时整个 Section 隐藏。

不要显示空标题。

---

# 二十七、导出卷

Docker Volume API 没有直接的“导出卷为 tar”端点，因此导出必须通过受控的临时容器完成。

实现思路：

```text
创建临时只读 helper container
    ↓
把源 volume 挂载到 /source:ro
    ↓
把宿主目标目录 bind mount 到 /output
    ↓
在 helper 中执行 tar
    ↓
等待容器完成
    ↓
检查 exit code
    ↓
删除 helper container
```

安全要求：

* 使用固定、可信的 helper image
* 不拼接未经转义的 shell 字符串
* 优先使用 argv 形式执行命令
* 源卷只读挂载
* 输出文件使用临时名称
* 成功后原子重命名
* 失败后清理临时文件
* 无论成功失败都删除 helper container
* 支持取消
* helper 容器使用唯一名称
* 不启用 privileged
* 不挂载 Docker socket
* 不访问网络，条件允许时使用：

  ```text
  NetworkMode=none
  ```
* 设置 CPU / memory 合理限制
* 不把 volume 内容加载进应用内存

支持格式：

```text
.tar
.tar.gz
.tar.zst
```

如果 helper image 不支持 zstd，第一阶段只实现 tar 和 gzip，并在 UI 中准确展示。

不要声称使用 Docker 原生导出卷 API。

---

# 二十八、克隆卷

克隆同样通过受控 helper container 完成：

```text
创建目标卷
    ↓
创建 helper container
    ↓
源卷挂载 /source:ro
目标卷挂载 /target:rw
    ↓
复制所有内容并尽量保留权限、符号链接和时间
    ↓
检查 exit code
    ↓
删除 helper
    ↓
刷新并选中目标卷
```

要求：

* 目标名称必填
* 目标已存在时默认拒绝
* 支持复制 Labels
* 支持选择目标 driver
* 支持 driver options
* 保留隐藏文件
* 保留符号链接
* 尽量保留 uid/gid、mode、mtime
* 避免 `cp /source/*` 导致遗漏 dotfiles
* 使用安全 argv
* 源卷只读
* 失败时询问或自动删除不完整目标卷
* 默认自动删除失败目标卷
* 明确记录清理结果
* 支持取消

---

# 二十九、Qt Models

建议：

```text
VolumeListModel
VolumeUsageModel
VolumeLabelModel
VolumeOptionModel
VolumeStatusModel
```

`VolumeListModel` roles：

```text
volumeName
displayName
driver
scope
sizeBytes
sizeKnown
sizeText
createdAt
createdText
inUse
usedByCount
anonymous
selected
busy
operation
section
```

`section`：

```text
in_use
unused
```

不要把整个 VolumeDetail 序列化为 JSON 交给 QML。

---

# 三十、VolumesController

新增或完善：

```text
VolumesController
```

职责：

* initialize
* refresh
* 搜索
* 排序
* 选择卷
* 加载详情
* 创建
* 删除
* prune
* 导出
* 克隆
* 任务取消
* 页面状态
* 错误状态

QML 属性建议：

```text
initialized
listState
detailState
errorMessage
volumeCount
inUseCount
unusedCount
knownTotalSizeText
unknownSizeCount
selectedVolumeName
searchQuery
sortMode
creating
pruning
```

Invokable：

```text
initialize()
refresh()
selectVolume(name)
setSearchQuery(query)
setSortMode(mode)
createVolume(...)
removeVolume(name, force)
pruneVolumes(...)
exportVolume(name, destination, compression)
cancelExport()
cloneVolume(source, target, ...)
cancelClone()
navigateToContainer(containerId)
```

---

# 三十一、异步和竞态

Qt 主线程禁止执行：

```text
Docker API
文件压缩
同步文件 I/O
block_on
thread::sleep
等待 helper container
```

使用共享 Tokio runtime。

刷新 generation：

```rust
refresh_generation += 1;
```

详情 generation：

```rust
detail_generation += 1;
```

旧请求不能覆盖新请求。

场景：

1. 用户选择卷 A
2. A detail 开始加载
3. 用户选择卷 B
4. B 先完成
5. A 后完成
6. A 的结果不得覆盖 B

导出、克隆、Prune 使用 `CancellationToken`。

关闭应用时取消所有后台任务，并尝试清理 helper containers。

---

# 三十二、错误模型

补充或映射：

```rust
pub enum DockerError {
    VolumeNotFound(String),
    VolumeInUse(String),
    VolumeAlreadyExists(String),
    VolumeDriverUnavailable(String),
    VolumePluginError(String),
    InvalidVolumeName(String),
    ExportFailed(String),
    CloneFailed(String),
    CleanupFailed(String),
    PermissionDenied,
    EngineUnavailable,
    OperationTimeout,
    OperationCancelled,
}
```

用户界面区分：

```text
Volume not found
Volume is still used by a container
Volume name already exists
Volume driver is unavailable
Volume plugin returned an error
Docker socket permission denied
Docker Engine unavailable
Export destination is not writable
Disk is full
Operation timed out
Operation cancelled
```

不要把所有错误显示成：

```text
Operation failed
```

---

# 三十三、KDE 原生视觉

使用：

```qml
Kirigami.Theme.backgroundColor
Kirigami.Theme.alternateBackgroundColor
Kirigami.Theme.textColor
Kirigami.Theme.disabledTextColor
Kirigami.Theme.highlightColor
Kirigami.Theme.highlightedTextColor
Kirigami.Theme.negativeTextColor
Kirigami.Theme.positiveTextColor

Kirigami.Units.smallSpacing
Kirigami.Units.mediumSpacing
Kirigami.Units.largeSpacing
Kirigami.Units.gridUnit
```

遵循 KDE HIG 的标准控件和交互模式，优先使用 Qt Quick Controls / Kirigami 控件完成列表、按钮、表单和对话框。([Developer][3])

必须验证：

* Breeze Light
* Breeze Dark
* 系统强调色
* 高 DPI
* Wayland
* 键盘导航
* Tooltip
* Accessible name

---

# 三十四、建议文件结构

适配当前仓库实际结构，避免重复模块。

`docker-core`：

```text
crates/docker-core/src/models/volume.rs
crates/docker-core/src/services/volumes.rs
crates/docker-core/src/mapping/volumes.rs
crates/docker-core/src/operations/volume_export.rs
crates/docker-core/src/operations/volume_clone.rs
```

GUI：

```text
crates/gui/src/controllers/volumes.rs
crates/gui/src/models/volume_model.rs
crates/gui/src/models/volume_usage_model.rs
crates/gui/src/models/volume_label_model.rs
crates/gui/src/models/volume_option_model.rs

crates/gui/qml/pages/VolumesPage.qml
crates/gui/qml/components/VolumeListPanel.qml
crates/gui/qml/components/VolumeListItem.qml
crates/gui/qml/components/VolumeDetailPanel.qml
crates/gui/qml/components/VolumeUsedByList.qml
crates/gui/qml/dialogs/CreateVolumeDialog.qml
crates/gui/qml/dialogs/RemoveVolumeDialog.qml
crates/gui/qml/dialogs/PruneVolumesDialog.qml
crates/gui/qml/dialogs/ExportVolumeDialog.qml
crates/gui/qml/dialogs/CloneVolumeDialog.qml
```

复用已有的：

```text
PropertySection.qml
PropertyRow.qml
KeyValueTable.qml
LoadingView.qml
ErrorBanner.qml
```

不要复制 Images / Networks 中已有的通用组件。

---

# 三十五、测试要求

## Mapping 测试

必须测试：

* Volume summary mapping
* Volume inspect mapping
* Size = -1 → None
* RefCount = -1 → None
* 缺失 UsageData
* 创建时间 RFC3339
* 空 Labels
* 空 Options
* 空 Status
* Scope
* Driver
* Mountpoint
* 匿名名称识别

## 使用关系测试

必须测试：

* running 容器引用卷
* stopped 容器引用卷
* paused 容器引用卷
* bind mount 不算 volume 使用
* tmpfs 不算 volume 使用
* 多个容器引用同一卷
* 一个容器挂载多个卷
* read-only mount
* destination 映射
* 无容器时为 Unused

## 大小测试

必须测试：

* 已知大小求和
* 未知大小不参与求和
* Unknown 不显示 0
* 负数不会转换为 u64
* known / unknown 数量正确
* 排序时 Unknown 永远排最后

## Controller 状态测试

必须测试：

* initialize 自动加载
* loading → ready
* loading → empty
* loading → error
* 自动选择第一卷
* 刷新保持选择
* 删除后选择相邻卷
* 最后一卷删除后 detail=None
* 旧详情请求不覆盖新选择
* 创建成功选中新卷
* 导出取消
* 克隆取消
* operation busy 清理

## Docker 集成测试

使用：

```rust
#[ignore]
```

或 feature：

```text
docker-integration
```

流程：

1. 创建测试卷
2. inspect
3. 验证 Labels
4. 创建容器挂载测试卷
5. 验证 In Use
6. 停止容器后仍为 In Use
7. 删除容器
8. 验证 Unused
9. 向卷写入测试数据
10. 导出
11. 克隆
12. 验证克隆内容
13. 删除测试卷
14. 清理 helper container 和临时文件

测试资源命名：

```text
tuxstack-test-volume-<uuid>
tuxstack-helper-<uuid>
```

测试失败时仍必须清理。

---

# 三十六、实施顺序

## Phase 1：模型和映射

* VolumeSummary
* VolumeDetail
* VolumeUsage
* VolumeContainerReference
* Bollard mapping
* UsageData 正确处理
* tests

## Phase 2：列表和使用关系

* list volumes
* list all containers
* 建立 mount 关联
* 分组
* size summary
* GUI list model
* 自动加载和选择

## Phase 3：详情

* General
* Used By
* Labels
* Options
* Status
* KDE 属性布局

## Phase 4：创建和删除

* CreateVolumeDialog
* create API
* RemoveVolumeDialog
* in-use 保护
* 状态刷新

## Phase 5：Prune

* 候选卷预览
* 确认
* prune API
* 回收空间显示

## Phase 6：导出和克隆

* helper container abstraction
* tar export
* clone
* cancellation
* cleanup
* tests

## Phase 7：质量验证

* Light / Dark
* keyboard
* high DPI
* unit tests
* integration tests
* clippy
* docs

---

# 三十七、构建验证

开始前：

```bash
git status
git log --oneline --decorate -20
find crates/docker-core -maxdepth 5 -type f | sort
find crates/gui -maxdepth 6 -type f | sort
cargo metadata --no-deps
cargo test --workspace
```

完成后：

```bash
cargo fmt --all -- --check
cargo check --workspace
cargo test --workspace
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo build -p tuxstack-gui
```

运行：

```bash
cargo run -p tuxstack-gui
```

---

# 三十八、人工验收

必须验证：

1. 打开 Volumes 页面自动加载。
2. 显示真实卷数量。
3. In Use / Unused 分组准确。
4. stopped container 引用的卷仍属于 In Use。
5. bind mount 不计入 volume 使用。
6. 自动选择第一卷。
7. 右侧显示真实详情。
8. 无卷时右侧完全 blank。
9. 未知大小显示 Unknown。
10. 未知大小不显示 0 B。
11. 已知总大小计算准确。
12. 搜索有效。
13. 排序有效。
14. 创建卷真实生效。
15. 删除未使用卷真实生效。
16. 使用中的卷得到明确保护。
17. Labels 正确。
18. Driver Options 正确。
19. Used By 正确。
20. 点击容器能导航。
21. 导出生成有效归档。
22. 克隆卷包含完整数据。
23. Helper container 能可靠清理。
24. Docker 停止时显示准确错误。
25. 权限不足时显示准确错误。
26. Breeze Light 正常。
27. Breeze Dark 正常。
28. 没有 mock 数据。
29. 没有顶部 `Info / Files` 分段栏。
30. 没有 macOS 固定视觉元素。

---

# 三十九、工作边界

本次不要修改：

* Containers 模块
* Images 模块
* Networks 模块
* 应用侧边栏
* Activity Monitor
* Commands
* Devices
* CLI 整体架构

不要实现：

```text
Kubernetes
Incus
Docker Compose
卷文件浏览器
备份调度
远程存储配置向导
卷加密
快照系统
```

---

# 四十、Commit 建议

```text
feat(docker-core): add volume models and usage mapping
feat(docker-core): implement volume listing and container references
feat(gui): add KDE volume list and detail views
feat(gui): add volume search grouping and sorting
feat(docker-core): implement volume create and remove
feat(gui): add volume creation and removal workflows
feat(docker-core): add safe volume export operation
feat(docker-core): add volume clone operation
feat(gui): add volume export and clone dialogs
feat(docker-core): implement volume pruning
test(volumes): cover usage mapping operations and UI state
docs: document Docker volume management
```

不要将全部功能压入一个 commit。

---

# 四十一、最终报告

完成后输出：

1. 修改文件列表
2. 新增 docker-core API
3. 卷与容器关联算法
4. 卷大小来源和 Unknown 处理
5. GUI 组件结构
6. 创建流程
7. 删除保护
8. Prune 行为
9. 导出实现
10. 克隆实现
11. Helper container 安全措施
12. Light / Dark 验证
13. 单元测试结果
14. Docker 集成测试结果
15. Clippy 结果
16. 已知限制
17. 所有 commit hash

最终页面应保持截图中的高效信息架构，同时采用 KDE Plasma 原生视觉，并提供真实、安全、可验证的 Docker Volume 管理能力。

[1]: https://docs.rs/bollard/latest/bollard/struct.Docker.html?utm_source=chatgpt.com "Docker in bollard - Rust"
[2]: https://docs.docker.com/reference/cli/docker/system/prune/?utm_source=chatgpt.com "docker system prune"
[3]: https://develop.kde.org/hig/?utm_source=chatgpt.com "KDE Human Interface Guidelines"
