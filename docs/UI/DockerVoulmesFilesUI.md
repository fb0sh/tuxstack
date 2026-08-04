
核心要求：

* 继续使用当前三栏布局
* 第三栏顶部增加 `Info / Files` 标签切换
* `Info` 保留现有卷详情
* `Files` 实现真实的 Docker Volume 文件浏览
* 通过受控 helper container 访问卷内容
* 只读预览优先
* 不直接读取宿主机 `/var/lib/docker/volumes`
* 不调用 `docker` CLI
* 不使用 mock 文件
* 不把完整卷打包后再一次性载入内存

---

# TuxStack Docker Volumes 第二阶段：Files Preview 完整实现 Prompt

你需要在当前 `tuxstack` 项目中完成 Docker Volumes 模块第二阶段。

本阶段重点：

> 在 Volumes 页面第三栏顶部增加 `Info / Files` 标签切换，并实现真实、安全、可取消的 Docker Volume 文件浏览与预览功能。

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

当前已经存在：

* Volumes 页面
* Volume 列表
* In Use / Unused 分组
* Volume 详情
* 创建、删除、导出、克隆等基础逻辑
* 固定三栏布局
* `VolumesController`
* `VolumeListModel`
* Qt/Tokio 异步框架
* KDE Breeze 风格

本次不要重构整体架构。

---

# 一、最终页面结构

第三栏调整为：

```text
VolumeDetailArea
├── TopTabBar
│   ├── Info
│   └── Files
│
└── StackLayout
    ├── VolumeInfoView
    └── VolumeFilesView
```

整体页面：

```text
┌──────────────┬─────────────────────┬─────────────────────────────────┐
│ App Sidebar  │ Volume List         │ Info / Files                    │
│              │                     │                                 │
│ Volumes      │ In Use              │ Info：卷详情                    │
│              │ Unused              │                                 │
│              │                     │ Files：卷文件浏览器              │
└──────────────┴─────────────────────┴─────────────────────────────────┘
```

第三栏顶部标签只在存在有效选中卷时显示。

没有选中卷时：

```text
第三栏完全 blank
```

不要显示：

* Info / Files 标签
* Select a volume
* No volume selected
* PlaceholderMessage
* 空文件表格
* loading skeleton

---

# 二、第三栏顶部标签设计

标签：

```text
Info
Files
```

标签切换只控制第三栏内容，不改变当前卷选择。

推荐使用：

```qml
QQC2.TabBar
QQC2.TabButton
```

或符合当前 Kirigami 版本的等价标准组件。

结构示例：

```qml
ColumnLayout {
    anchors.fill: parent

    QQC2.TabBar {
        id: detailTabs

        visible: volumesController.hasSelection
        Layout.alignment: Qt.AlignHCenter
        currentIndex: volumesController.detailTabIndex

        QQC2.TabButton {
            text: qsTr("Info")
        }

        QQC2.TabButton {
            text: qsTr("Files")
        }

        onCurrentIndexChanged: {
            volumesController.setDetailTab(currentIndex)
        }
    }

    StackLayout {
        Layout.fillWidth: true
        Layout.fillHeight: true
        currentIndex: detailTabs.currentIndex

        VolumeInfoView {}
        VolumeFilesView {}
    }
}
```

要求：

* 使用系统主题颜色
* 使用 Breeze 标准 Hover、Pressed、Checked 状态
* 不复制截图中的 macOS SegmentedControl
* 不使用固定白色背景
* 不使用固定灰色选中背景
* 不使用大圆角胶囊
* 支持键盘导航
* 支持 Breeze Light / Dark
* 支持系统强调色
* 标签宽度由内容决定
* 标签区域保持紧凑

默认打开：

```text
Info
```

用户切换到 Files 后，再选择其他卷：

```text
继续保持 Files 标签
```

应用重新启动后是否记忆标签不是本阶段要求。

---

# 三、Files 功能范围

本阶段必须实现：

* 自动读取所选卷根目录
* 文件和目录列表
* 进入目录
* 返回上级目录
* 面包屑路径
* 刷新当前目录
* 文件名排序
* 修改时间排序
* 大小排序
* 类型排序
* 目录优先排序
* 文件类型识别
* 文本文件预览
* 图片文件预览
* JSON 文件格式化预览
* 二进制文件信息预览
* 文件下载/另存为
* 文件属性
* 加载状态
* 目录为空状态
* 错误状态
* 任务取消
* 大文件保护
* 符号链接安全处理
* 切换卷后清理旧文件状态
* 切换标签后合理暂停或保留状态

第一阶段 Files 保持只读。

暂不实现：

```text
上传文件
创建文件
创建目录
删除文件
重命名
移动
复制
编辑并保存
权限修改
所有者修改
压缩/解压
拖放写入
终端
```

UI 中不要放不可用的写操作按钮。

---

# 四、Docker Volume 文件访问方式

Docker Engine 没有原生的 Volume 文件浏览 API。

不得直接访问：

```text
/var/lib/docker/volumes/<name>/_data
```

原因：

* Docker Root Dir 可能改变
* rootless Docker 路径不同
* 远程 Docker Host 无法直接访问
* 普通用户可能没有宿主路径权限
* Volume Driver 可能不是 local
* Desktop/VM 环境中宿主 mountpoint 不可直接读取

必须通过受控 helper container 挂载目标卷。

架构：

```text
VolumeFilesController
        │
        ▼
VolumeFileService
        │
        ▼
创建临时 helper container
        │
        ├── volume:/volume:ro
        ├── NetworkMode=none
        ├── no privileged
        ├── no Docker socket
        └── 受限资源
        │
        ▼
exec 受控命令
        │
        ▼
结构化输出
```

源 Volume 必须只读挂载：

```text
/volume:ro
```

---

# 五、Helper Image

定义统一配置：

```rust
pub struct VolumeHelperConfig {
    pub image: String,
    pub mount_path: String,
    pub memory_limit_bytes: i64,
    pub nano_cpus: i64,
    pub operation_timeout: Duration,
}
```

默认 helper image 必须满足：

* 镜像小
* 支持 POSIX shell 或指定工具
* 支持 `find`
* 支持 `stat`
* 支持 `cat`
* 支持 `readlink`
* 支持安全的 NUL 分隔输出
* 版本固定
* 不使用 `latest`

示例可以选择经过验证的固定版本 Alpine：

```text
alpine:3.x
```

实际版本必须固定为项目验证过的版本。

禁止：

```text
alpine:latest
```

如果 helper image 不存在：

1. 显示需要拉取 helper image。
2. 用户确认后拉取。
3. 显示真实拉取进度。
4. 拉取成功后继续。
5. 用户拒绝时保持 Files 错误/未配置状态。

可以在设置中隐藏高级 helper image 配置，本阶段无需开放 UI。

---

# 六、Helper Container 生命周期

推荐两种模式，优先选择简单可靠的一种。

## 推荐方案：每个选中卷一个短生命周期会话容器

进入 Files 标签：

```text
创建 helper container
    ↓
挂载当前 volume:ro
    ↓
启动 sleep/wait 进程
    ↓
通过 exec 执行目录和文件读取
```

离开当前卷或应用退出：

```text
停止 helper
    ↓
强制删除 helper
```

helper 名称：

```text
tuxstack-volume-preview-<uuid>
```

标签：

```text
io.github.tuxstack.managed=true
io.github.tuxstack.purpose=volume-preview
io.github.tuxstack.volume=<volume-name>
io.github.tuxstack.session=<uuid>
```

安全配置：

```text
ReadonlyRootfs=true
NetworkMode=none
Privileged=false
AutoRemove=false
CapDrop=ALL
NoNewPrivileges=true
```

卷挂载：

```text
Type=volume
Source=<selected-volume>
Target=/volume
ReadOnly=true
```

临时目录：

```text
/tmp
```

如果 helper 工具需要写临时数据，可使用 tmpfs：

```text
/tmp: rw,noexec,nosuid,size=16m
```

资源限制建议：

```text
Memory: 64–128 MiB
PIDs limit: 64
CPU: 0.25–0.5 core
```

禁止挂载：

```text
/var/run/docker.sock
宿主根目录
Docker Root Dir
```

---

# 七、孤儿 Helper 清理

应用异常退出可能留下 helper container。

应用启动时检查：

```text
label=io.github.tuxstack.managed=true
label=io.github.tuxstack.purpose=volume-preview
```

对于明显属于旧会话且已停止或超时的 helper：

* 自动删除
* 记录 debug 日志
* 不影响主页面启动

不要删除其他软件或用户创建的容器。

当前运行中的有效 session 不应被错误删除。

---

# 八、目录列举协议

禁止使用：

```text
ls -l
```

解析人类可读输出。

禁止根据语言环境解析日期、权限和文件名。

必须输出稳定、结构化数据。

推荐在 helper 中使用固定脚本，以 NUL 或 JSON Lines 输出。

目录项模型：

```rust
pub struct VolumeFileEntry {
    pub name: String,
    pub path: String,
    pub entry_type: VolumeFileType,
    pub size_bytes: Option<u64>,
    pub modified_at: Option<DateTime<Utc>>,
    pub mode: Option<u32>,
    pub uid: Option<u32>,
    pub gid: Option<u32>,
    pub symlink_target: Option<String>,
    pub mime_type: Option<String>,
    pub hidden: bool,
    pub readable: bool,
}
```

类型：

```rust
pub enum VolumeFileType {
    Directory,
    RegularFile,
    SymbolicLink,
    Socket,
    Fifo,
    BlockDevice,
    CharacterDevice,
    Unknown,
}
```

列表请求：

```rust
pub struct ListVolumeDirectoryRequest {
    pub volume_name: String,
    pub path: VolumePath,
    pub show_hidden: bool,
}
```

服务：

```rust
pub async fn list_directory(
    &self,
    session: &VolumePreviewSession,
    request: ListVolumeDirectoryRequest,
) -> Result<Vec<VolumeFileEntry>, DockerError>;
```

---

# 九、路径模型和安全

不要在业务层直接传递未经验证的任意字符串路径。

定义：

```rust
pub struct VolumePath {
    components: Vec<String>,
}
```

逻辑根路径：

```text
/
```

映射到 helper：

```text
/volume
```

安全规则：

* 禁止 `..`
* 禁止 NUL 字节
* 禁止绝对宿主路径
* 禁止组件包含 `/`
* 空组件忽略或拒绝
* 路径标准化后必须位于 `/volume`
* 不允许通过符号链接逃逸
* 跟随符号链接前检查最终路径
* 默认不进入指向卷外部的 symlink
* 目录项名称保留原始 Unicode
* 非 UTF-8 文件名需要安全处理

Rust 内部尽量用字节路径或 OsString。

由于 QML 字符串是 Unicode，遇到非法 UTF-8 文件名时：

* 显示安全替代文本
* 保留内部原始字节标识
* 不允许替代文本导致错误操作到另一个文件
* 可以将该项标记为不可预览

---

# 十、Files 页面 UI

目标类似截图中的文件列表，但使用 KDE 风格。

结构：

```text
VolumeFilesView
├── FilesToolbar
│   ├── Back
│   ├── Up
│   ├── Breadcrumb
│   ├── Refresh
│   ├── Search
│   ├── Show Hidden
│   └── Sort
│
├── FileTable
│   ├── Name
│   ├── Date Modified
│   ├── Size
│   └── Kind
│
└── PreviewPane / PreviewDialog
```

对于当前三栏架构，Files 页面已经占据整个第三栏。

预览推荐采用：

* 双击文件打开预览对话框，或
* 在 Files 页右侧/下方打开自适应预览区

第一阶段优先使用：

```text
双击文件 → Preview Dialog / Overlay Sheet
```

这样避免第三栏再拆成过窄的第四栏。

---

# 十一、FilesToolbar

工具栏至少包含：

```text
Back
Up
Breadcrumb path
Refresh
Search
Show Hidden Files
Sort
```

图标建议：

```text
go-previous
go-up
view-refresh
edit-find
view-hidden
view-sort-ascending
```

必须验证实际图标名称。

Back：

* 返回浏览历史中的上一目录
* 没有历史时禁用

Up：

* 返回父目录
* 根目录时禁用

Refresh：

* 重新读取当前目录
* 保持当前路径
* 尽量保持当前选中项

Search：

* 当前目录本地过滤
* 本阶段不做全卷递归搜索
* 文案明确为：

  ```text
  Search this folder…
  ```

Show Hidden：

* 默认关闭或遵循设置
* 切换后重新过滤或请求
* 隐藏项判定为名称以 `.` 开头

---

# 十二、面包屑导航

显示：

```text
Volume / base / pg_wal
```

根节点使用卷名或：

```text
/
```

推荐：

```text
<volume-name> / path / to / directory
```

每个组件可点击。

要求：

* 点击任意层级直接导航
* 超长路径中间折叠
* Tooltip 显示完整路径
* 支持复制当前逻辑路径
* 不显示 helper 内部路径 `/volume`
* 不显示宿主 mountpoint

---

# 十三、文件表格

列：

```text
Name
Date Modified
Size
Kind
```

可选高级列放进列设置：

```text
Permissions
Owner
Group
```

第一阶段主界面只保留截图中的四列。

使用 QML `TableView` 或适合当前 Qt 版本的标准表格实现。

必须避免：

* 使用大量 Row Rectangle 手工模拟表格
* 固定背景颜色
* 固定白色交替行
* macOS 蓝色文件夹图标
* 巨大圆角行

使用 KDE/Breeze：

* 系统文件图标
* 标准选中背景
* 标准 Hover
* 交替行背景可使用主题色
* 表头使用标准控件
* 键盘上下移动选择
* Enter 打开
* Backspace 返回上级，避免在文本框焦点时触发
* Delete 不做任何写操作

---

# 十四、文件图标和 Kind

根据文件类型和 MIME 使用系统图标。

示例：

```text
Directory       folder
Text            text-plain
JSON            application-json
Image           image-x-generic
Archive         package-x-generic
Executable      application-x-executable
Symlink         emblem-symbolic-link
Socket          network-server
Unknown         unknown
```

优先使用 MIME 类型对应的系统图标。

Kind 显示：

```text
Folder
Text Document
JSON Document
PNG Image
Symbolic Link
Socket
Unknown
```

不得把 Linux 类型直接显示为 Rust Debug：

```text
RegularFile
Some(...)
```

---

# 十五、排序

至少支持点击表头排序：

```text
Name
Date Modified
Size
Kind
```

规则：

* 目录始终优先，可提供开关，默认开启
* 文件名自然排序
* 大小写不敏感作为主排序
* 相同名称用原始字节稳定排序
* Size 未知排最后
* Date 未知排最后
* 点击同一表头切换升序/降序
* 当前排序列显示箭头

默认：

```text
Directories First
Name Ascending
```

---

# 十六、文件列表状态

定义：

```rust
pub enum VolumeFilesState {
    Idle,
    StartingSession,
    Loading,
    Ready,
    Empty,
    Error,
    HelperImageRequired,
}
```

行为：

## 首次切换 Files

```text
StartingSession
    ↓
Loading /
    ↓
Ready 或 Empty
```

## 切换目录

```text
保留旧列表
显示轻量 loading
新列表替换旧列表
```

避免整个 Files 页闪白。

## 空目录

显示：

```text
This folder is empty.
```

使用紧凑空状态。

## 错误

显示真实原因和 Retry：

```text
Folder could not be loaded.
Permission denied while reading this volume.
[Retry]
```

---

# 十七、选中卷切换行为

用户在 Files 标签中选择另一个 Volume：

正确流程：

```text
取消当前目录请求
    ↓
销毁旧 helper session
    ↓
清空旧文件列表
    ↓
创建新 helper session
    ↓
加载新卷根目录
```

切换过程中不得短暂显示前一个卷的文件。

可以先立即：

```text
filesState = StartingSession
currentEntries.clear()
currentPath = /
```

然后加载新卷。

必须使用 generation ID 防止旧结果覆盖新卷：

```rust
volume_session_generation += 1;
```

只有当前 generation 的结果允许更新 UI。

---

# 十八、进入目录

单击：

```text
只选中
```

双击或 Enter：

```text
Directory → 进入
File → 打开预览
Symlink → 根据安全策略处理
```

目录进入时：

* 加入 history
* 更新 breadcrumb
* loading
* 保持排序方式
* 清空搜索文本，或明确保留当前目录搜索；推荐清空
* 完成后焦点返回表格

---

# 十九、符号链接处理

符号链接可能指向：

* 卷内文件
* 卷内目录
* 卷外路径
* 不存在目标
* 循环链接

默认行为：

* 显示为 Symbolic Link
* 显示 target
* 不自动跟随
* 用户双击时解析最终路径
* 最终路径仍在卷根内才允许打开
* 指向卷外时显示：

  ```text
  This symbolic link points outside the volume and cannot be opened.
  ```
* 循环链接显示错误
* 不因为 symlink 逃逸访问 helper rootfs

禁止使用简单字符串前缀判断。

必须通过 helper 内的规范化路径与卷根校验。

---

# 二十、文件预览

双击普通文件打开：

```text
FilePreviewDialog.qml
```

预览类型：

```text
Text
JSON
Image
Binary/Unsupported
```

模型：

```rust
pub struct VolumeFilePreview {
    pub path: VolumePath,
    pub name: String,
    pub mime_type: Option<String>,
    pub size_bytes: Option<u64>,
    pub preview_kind: FilePreviewKind,
    pub content: FilePreviewContent,
    pub truncated: bool,
}
```

类型：

```rust
pub enum FilePreviewKind {
    Text,
    Json,
    Image,
    Binary,
    Unsupported,
}
```

---

# 二十一、文本预览

支持：

```text
.txt
.log
.conf
.ini
.toml
.yaml
.yml
.xml
.csv
.env
Dockerfile
无扩展名但可识别为文本的文件
```

限制：

```text
默认最多读取 1 MiB
```

可配置范围：

```text
256 KiB – 4 MiB
```

超过限制：

* 只读取前 N 字节
* 显示：

  ```text
  Preview truncated. Showing the first 1 MiB.
  ```
* 提供 Download
* 不自动读取完整文件

处理：

* UTF-8 正常显示
* UTF-8 BOM 移除
* 检测 NUL 字节后按二进制处理
* 非 UTF-8 文本可尝试安全替换显示
* 不因为超长单行阻塞 UI
* 使用等宽字体
* 支持选择和复制
* 默认只读
* 支持查找
* 可选行号，第一阶段可省略

禁止把文件正文写入 tracing 日志。

---

# 二十二、JSON 预览

文件识别为 JSON 时：

* 读取受大小限制的内容
* 尝试解析
* 成功后 pretty print
* 失败时按普通文本显示，并注明解析失败
* 保留 Download

禁止因为 JSON 巨大而在 Qt 主线程格式化。

解析和格式化放在 Tokio 后台。

---

# 二十三、图片预览

支持常见格式：

```text
PNG
JPEG
GIF 静态首帧即可
WebP
BMP
SVG 需要安全处理
```

大小限制建议：

```text
最大预览读取 16 MiB
```

超过限制：

* 显示文件信息
* 提供下载
* 不加载预览

安全要求：

* 在后台读取
* 对尺寸异常的图片设置像素限制
* 避免解码炸弹
* SVG 默认可使用 Qt Image provider，但需要禁用外部资源访问
* 不允许 SVG 加载网络资源或宿主文件
* 临时预览文件退出时清理

实现可采用：

1. 从 helper 通过 archive/download stream 读取有限内容
2. 写入应用私有临时目录
3. QML Image 读取该临时文件
4. 关闭预览时删除临时文件

不得暴露 helper 内部文件路径给 QML。

---

# 二十四、二进制文件预览

二进制或不支持类型显示：

```text
File name
Kind
MIME type
Size
Modified
Permissions
Owner
Group
```

操作：

```text
Download
Copy Path
```

可以增加有限十六进制预览，本阶段不要求。

不要把二进制按文本乱码显示。

---

# 二十五、读取文件实现

禁止：

```text
cat <untrusted-path>
```

通过 shell 字符串拼接执行。

必须：

* 使用 exec argv
* 路径作为独立参数
* 或使用固定 helper 脚本接收安全编码路径
* 校验路径
* 限制读取长度

服务 API：

```rust
pub async fn preview_file(
    &self,
    session: &VolumePreviewSession,
    request: PreviewVolumeFileRequest,
    cancellation: CancellationToken,
) -> Result<VolumeFilePreview, DockerError>;
```

请求：

```rust
pub struct PreviewVolumeFileRequest {
    pub volume_name: String,
    pub path: VolumePath,
    pub max_bytes: u64,
}
```

流读取必须及时停止，达到上限后取消或关闭 exec stream。

---

# 二十六、下载 / 另存为

文件预览中提供：

```text
Save As…
```

也可以在右键菜单中提供：

```text
Save As…
Copy Path
Properties
```

下载流程：

```text
选择目标路径
    ↓
创建临时目标文件
    ↓
从 helper 流式读取文件
    ↓
写入临时文件
    ↓
fsync 条件允许时执行
    ↓
原子重命名
```

要求：

* 不一次性加载到内存
* 支持取消
* 显示已写入字节
* 已知总大小时显示真实进度
* 覆盖现有文件前确认
* 失败清理临时文件
* 磁盘不足显示明确错误
* 权限不足显示明确错误
* 文件内容不写入日志

服务：

```rust
pub async fn download_file(
    &self,
    session: &VolumePreviewSession,
    request: DownloadVolumeFileRequest,
    progress: ProgressSender,
    cancellation: CancellationToken,
) -> Result<(), DockerError>;
```

---

# 二十七、文件属性

右键或预览中打开：

```text
FilePropertiesDialog.qml
```

显示：

```text
Name
Logical Path
Kind
MIME Type
Size
Modified
Permissions
UID
GID
Symlink Target
```

路径必须是卷内逻辑路径：

```text
/base/PG_VERSION
```

不要显示：

```text
/volume/base/PG_VERSION
/var/lib/docker/volumes/...
```

权限格式：

```text
0644
-rw-r--r--
```

条件允许时同时显示。

---

# 二十八、Volume Files Service

在 `docker-core` 中新增模块：

```text
operations/volume_files/
```

推荐结构：

```text
crates/docker-core/src/operations/volume_files/
├── mod.rs
├── session.rs
├── helper.rs
├── path.rs
├── listing.rs
├── preview.rs
├── download.rs
├── protocol.rs
└── cleanup.rs
```

公开服务：

```rust
pub struct VolumeFileService {
    client: Arc<DockerClient>,
    config: VolumeHelperConfig,
}
```

公开方法：

```rust
impl VolumeFileService {
    pub async fn start_session(
        &self,
        volume_name: &str,
        cancellation: CancellationToken,
    ) -> Result<VolumePreviewSession, DockerError>;

    pub async fn stop_session(
        &self,
        session: VolumePreviewSession,
    ) -> Result<(), DockerError>;

    pub async fn list_directory(
        &self,
        session: &VolumePreviewSession,
        path: &VolumePath,
        cancellation: CancellationToken,
    ) -> Result<Vec<VolumeFileEntry>, DockerError>;

    pub async fn preview_file(
        &self,
        session: &VolumePreviewSession,
        request: PreviewVolumeFileRequest,
        cancellation: CancellationToken,
    ) -> Result<VolumeFilePreview, DockerError>;

    pub async fn download_file(
        &self,
        session: &VolumePreviewSession,
        request: DownloadVolumeFileRequest,
        progress: ProgressSender,
        cancellation: CancellationToken,
    ) -> Result<(), DockerError>;

    pub async fn file_properties(
        &self,
        session: &VolumePreviewSession,
        path: &VolumePath,
        cancellation: CancellationToken,
    ) -> Result<VolumeFileProperties, DockerError>;

    pub async fn cleanup_orphan_sessions(
        &self,
    ) -> Result<usize, DockerError>;
}
```

`VolumePreviewSession`：

```rust
pub struct VolumePreviewSession {
    pub id: Uuid,
    pub volume_name: String,
    pub container_id: String,
    pub started_at: DateTime<Utc>,
}
```

实现 `Drop` 时不要直接异步删除；由 controller/session manager 显式清理。

---

# 二十九、VolumesController 扩展

可以扩展现有 `VolumesController`，也可以新增：

```text
VolumeFilesController
```

推荐独立 Controller，避免 VolumesController 继续膨胀。

职责：

* 当前卷文件会话
* 当前路径
* 历史记录
* 目录列表
* 搜索
* 排序
* 文件选中
* 文件预览
* 下载
* 取消
* helper image 状态
* session cleanup

公开属性：

```text
state
errorMessage
currentVolumeName
currentPath
canGoBack
canGoUp
showHidden
searchQuery
sortColumn
sortDescending
selectedEntryPath
previewLoading
downloadInProgress
downloadProgress
```

Invokable：

```text
openVolume(volumeName)
closeVolume()
refresh()
openEntry(path)
goBack()
goUp()
navigateTo(path)
setSearchQuery(query)
setShowHidden(value)
setSort(column, descending)
previewEntry(path)
downloadEntry(path, destination)
cancelPreview()
cancelDownload()
retry()
```

标签切换：

```text
setActive(bool)
```

进入 Files：

```text
setActive(true)
```

离开 Files：

```text
setActive(false)
```

离开标签时是否立即销毁 session：

推荐策略：

* 短时间保留 session，例如 30–60 秒，提升标签切换体验
* 后台无操作超时后销毁
* 应用退出立即清理
* 切换卷立即销毁旧 session

如果当前实现复杂，第一阶段可在离开 Files 时立即销毁 session，优先正确性。

---

# 三十、Qt Models

新增：

```text
VolumeFileModel
BreadcrumbModel
```

`VolumeFileModel` roles：

```text
name
displayName
path
entryType
iconName
sizeBytes
sizeKnown
sizeText
modifiedAt
modifiedText
kindText
hidden
readable
symlinkTarget
selected
```

禁止将目录项作为大型 JSON 数组交给 QML。

目录更新可以：

* reset model，第一阶段可接受
* 后续优化增量 diff

保持排序和过滤在 Rust 或 Qt ProxyModel 中完成。

---

# 三十一、QML 文件建议

新增：

```text
crates/gui/qml/components/VolumeDetailTabs.qml
crates/gui/qml/components/VolumeInfoView.qml
crates/gui/qml/components/VolumeFilesView.qml
crates/gui/qml/components/VolumeFilesToolbar.qml
crates/gui/qml/components/VolumeBreadcrumb.qml
crates/gui/qml/components/VolumeFileTable.qml
crates/gui/qml/components/VolumeFileRow.qml

crates/gui/qml/dialogs/VolumeFilePreviewDialog.qml
crates/gui/qml/dialogs/VolumeFilePropertiesDialog.qml
crates/gui/qml/dialogs/DownloadVolumeFileDialog.qml
crates/gui/qml/dialogs/HelperImageDialog.qml
```

Rust：

```text
crates/gui/src/controllers/volume_files.rs
crates/gui/src/models/volume_file_model.rs
crates/gui/src/models/breadcrumb_model.rs
```

调整当前仓库实际目录结构，避免创建同职责重复文件。

---

# 三十二、右键菜单

文件行右键菜单：

```text
Open
Save As…
Copy Path
Properties
```

目录：

```text
Open
Copy Path
Properties
```

Symlink：

```text
Open Target，只有安全可访问时
Copy Path
Properties
```

不要显示：

```text
Delete
Rename
Cut
Paste
New Folder
```

因为本阶段只读。

---

# 三十三、键盘交互

必须支持：

```text
Up / Down          移动选择
Enter              打开目录或预览文件
Backspace          返回上级
Alt+Left           Back
Alt+Up             Up
Ctrl+R / F5        Refresh
Ctrl+F             聚焦搜索
Ctrl+L             聚焦或打开路径导航，条件允许
Ctrl+S             预览文件时 Save As
Escape             关闭预览
```

快捷键不得在搜索框编辑时错误触发。

---

# 三十四、KDE 视觉要求

使用：

```qml
Kirigami.Theme.backgroundColor
Kirigami.Theme.alternateBackgroundColor
Kirigami.Theme.textColor
Kirigami.Theme.disabledTextColor
Kirigami.Theme.highlightColor
Kirigami.Theme.highlightedTextColor
Kirigami.Theme.negativeTextColor

Kirigami.Units.smallSpacing
Kirigami.Units.mediumSpacing
Kirigami.Units.largeSpacing
Kirigami.Units.gridUnit
```

必须适配：

* Breeze Light
* Breeze Dark
* 系统强调色
* 系统字体
* 高 DPI
* Wayland
* 键盘
* 无障碍

文件列表视觉参考 KDE Dolphin：

* 紧凑表格
* 标准表头
* 主题驱动交替行
* 标准 Hover
* 标准 Selected
* 系统文件图标
* 不使用大卡片

---

# 三十五、错误模型

新增明确错误：

```rust
pub enum DockerError {
    VolumePreviewHelperImageMissing,
    VolumePreviewSessionFailed(String),
    VolumePreviewSessionClosed,
    VolumePathInvalid(String),
    VolumePathEscapesRoot,
    VolumeEntryNotFound(String),
    VolumeEntryUnreadable(String),
    VolumeSymlinkOutsideRoot(String),
    VolumeSymlinkLoop(String),
    VolumeFileTooLarge {
        size: u64,
        limit: u64,
    },
    VolumePreviewUnsupported(String),
    VolumeDownloadFailed(String),
    VolumeHelperProtocolError(String),
    OperationCancelled,
    OperationTimeout,
}
```

GUI 文案区分：

```text
Helper image is required
Volume could not be mounted for preview
Folder does not exist
Permission denied
Symbolic link points outside the volume
File is too large to preview
File type is not supported for preview
Docker Engine is unavailable
Operation timed out
Operation cancelled
```

不要统一显示：

```text
Failed
```

---

# 三十六、安全要求

必须严格满足：

1. Volume 只读挂载。
2. Helper 不使用 privileged。
3. Helper 不挂 Docker socket。
4. Helper 不挂宿主目录。
5. Helper 默认无网络。
6. Helper root filesystem 只读。
7. Drop all capabilities。
8. 启用 no-new-privileges。
9. 限制内存、CPU、PIDs。
10. 路径禁止 `..` 逃逸。
11. Symlink 最终路径必须位于卷根。
12. 所有用户路径通过 argv 或安全协议传递。
13. 禁止 shell 字符串拼接。
14. 文件读取有大小上限。
15. 文件下载采用流式。
16. 应用退出清理 helper。
17. 不记录文件正文。
18. 不记录可能敏感的文件名列表到 info 日志。
19. 不自动执行卷内程序。
20. 不解析卷内不可信脚本。

---

# 三十七、性能要求

* 不递归扫描整个 Volume。
* 只列出当前目录。
* 目录条目很多时支持分批更新或虚拟化表格。
* 目录超过 10,000 项时给出性能提示。
* 单次目录输出设置合理最大条目数，例如 50,000。
* 达到限制时提示结果被截断。
* 文件大小只使用 stat，不读取内容计算。
* MIME 检测优先通过有限头部读取。
* 切换目录取消旧请求。
* 预览和下载互相独立。
* 不在 QML 主线程处理大数据。

---

# 三十八、测试要求

## 路径测试

必须测试：

* 根路径
* 普通子目录
* `..`
* `.`
* 重复 `/`
* 空组件
* NUL
* Unicode
* 含空格
* 含引号
* 含换行
* 非 UTF-8
* symlink 卷内
* symlink 卷外
* symlink loop

## Protocol 测试

* 普通文件
* 目录
* 空目录
* hidden 文件
* socket
* fifo
* symlink
* 大文件
* 文件名包含 tab/newline
* 结构化输出解析失败
* exec 中断

## Controller 测试

* 进入 Files 自动启动 session
* 切换卷关闭旧 session
* 旧请求不能覆盖新卷
* 导航历史
* Back
* Up
* 根目录 Up 禁用
* 搜索
* 排序
* hidden 切换
* preview cancel
* download cancel
* 离开 Files 清理 session
* 应用退出清理 session

## Docker 集成测试

使用：

```rust
#[ignore]
```

或：

```text
docker-integration
```

流程：

1. 创建测试卷。
2. 用测试 helper 写入：

   * 目录
   * 文本
   * JSON
   * 图片
   * 二进制
   * hidden 文件
   * symlink
3. 启动只读 preview session。
4. 列出根目录。
5. 进入目录。
6. 预览文本。
7. 预览 JSON。
8. 下载文件。
9. 验证下载内容。
10. 验证卷外 symlink 被阻止。
11. 停止并删除 helper。
12. 删除测试卷。
13. 确认没有残留 helper。

测试失败时也必须清理。

---

# 三十九、实施顺序

## Phase 1：标签与页面框架

* Info / Files 标签
* StackLayout
* 保留现有 Info
* Files 空框架
* 正确 selection 行为

## Phase 2：Helper Session

* Helper 配置
* helper image 检查
* container 创建
* 安全配置
* start / stop
* orphan cleanup
* tests

## Phase 3：目录浏览

* VolumePath
* list directory
* structured protocol
* VolumeFileModel
* toolbar
* breadcrumb
* table
* navigation

## Phase 4：排序和过滤

* 搜索当前目录
* show hidden
* directory first
* table sorting
* selection

## Phase 5：文件预览

* text
* JSON
* image
* binary info
* size limits
* cancellation

## Phase 6：下载和属性

* Save As
* streaming
* progress
* temp file
* cleanup
* properties

## Phase 7：安全、测试和文档

* symlink containment
* path validation
* resource limits
* integration tests
* Light / Dark
* keyboard
* docs

---

# 四十、构建验证

开始前执行：

```bash
git status
git log --oneline --decorate -20
find crates/docker-core -maxdepth 6 -type f | sort
find crates/gui -maxdepth 7 -type f | sort
cargo metadata --no-deps
cargo test --workspace
```

保留用户未提交修改。

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

# 四十一、人工验收

必须验证：

1. 选中 Volume 后第三栏显示 Info / Files。
2. 默认打开 Info。
3. 切换 Files 自动启动只读预览 session。
4. 根目录显示真实 Volume 内容。
5. 文件名、修改时间、大小、类型准确。
6. 目录优先排序。
7. 点击表头可排序。
8. 双击目录可进入。
9. Back 和 Up 正确。
10. Breadcrumb 可点击。
11. Refresh 正确。
12. Search 只过滤当前目录。
13. Hidden Files 切换正确。
14. 文本预览正确。
15. JSON 格式化正确。
16. 图片预览正确。
17. 二进制文件显示属性。
18. 大文件不完整载入内存。
19. 文件下载正确。
20. 切换 Volume 后旧文件不残留。
21. 无选中卷时第三栏 blank。
22. 离开 Files 后 session 正确清理。
23. 应用退出无 helper 残留。
24. Symlink 无法逃逸 Volume。
25. Helper 无网络、无特权、无 Docker socket。
26. Breeze Light 正常。
27. Breeze Dark 正常。
28. 高 DPI 正常。
29. 键盘导航正常。
30. 没有 mock 数据。
31. 没有调用 docker CLI。
32. 没有直接访问 Docker volume 宿主路径。

---

# 四十二、文档

更新：

```text
docs/volumes.md
docs/architecture.md
docs/security.md
README.md
```

说明：

* Volume Files 使用 helper container
* 只读模式
* helper image
* 安全限制
* 文件预览大小限制
* helper 生命周期
* 当前不支持编辑
* 不直接访问宿主 mountpoint
* 对远程 Docker Host 的后续限制

README 功能状态必须按真实实现更新。

---

# 四十三、Commit 建议

```text
feat(gui): add info and files tabs to volume details
feat(docker-core): add secure volume preview sessions
feat(docker-core): implement volume directory listing
feat(gui): add KDE volume file browser
feat(gui): add breadcrumb navigation and file sorting
feat(docker-core): implement bounded volume file preview
feat(gui): add text json and image previews
feat(docker-core): add streaming volume file downloads
feat(gui): add volume file properties and save-as
test(volumes): cover paths preview sessions and downloads
docs: document read-only volume file browsing
```

不要将全部功能压入一个 commit。

---

# 四十四、最终交付报告

完成后输出：

1. 修改文件列表
2. Info / Files 标签实现
3. Files 页面组件结构
4. Helper image 和 container 设计
5. Helper 安全参数
6. 目录列举协议
7. 路径和 symlink 安全措施
8. 文件预览类型
9. 大文件限制
10. 下载实现
11. Session 生命周期
12. 孤儿 helper 清理
13. Qt Model 结构
14. 单元测试结果
15. Docker 集成测试结果
16. Light / Dark 验证结果
17. Clippy 结果
18. 已知限制
19. 所有 commit hash

最终实现应形成：

```text
Volume selected
      │
      ├── Info
      │    └── Volume metadata
      │
      └── Files
           └── secure read-only helper session
                  ├── directory browsing
                  ├── file preview
                  ├── properties
                  └── streaming download
```

优先保证**只读、安全、真实、可清理**。文件编辑、删除和上传继续放在后续阶段。

