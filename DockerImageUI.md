
你需要在当前 `tuxstack` 项目中完整实现 Docker Images 管理功能。

当前项目架构已经确定：

```text
Qt 6 + QML + Kirigami GUI
          │
       CXX-Qt
          │
    tuxstack-docker-core
          │
       Bollard
          │
    Docker Engine
```

本次任务只实现 Images 功能，不重构整体架构，不引入 daemon、REST API、JSON-RPC、Incus 或 Kubernetes。

参考用户提供的界面截图完成页面的信息架构、资源列表、详情展示和操作流程，同时使用 KDE Plasma 原生设计语言。

## 一、核心要求

必须真实实现：

* 查询 Docker 镜像列表
* 区分正在使用和未使用的镜像
* 搜索镜像
* 排序镜像
* 查看镜像详情
* 删除镜像
* 拉取镜像
* 导出镜像
* 查看镜像标签
* 查看镜像环境变量
* 查看镜像配置
* 查看镜像 Label
* 查看哪些容器正在使用镜像
* 统计镜像总占用空间
* loading、empty、error、permission denied、Docker unavailable 状态
* Light/Dark KDE 主题适配
* 异步操作和取消
* 操作完成后的真实刷新

禁止使用任何 mock 数据。

---

# 二、页面布局

Images 页面采用三段式结构：

```text
┌──────────────┬─────────────────────┬──────────────────────────────┐
│ P1+r6B4E=1B5B367E\P0+r4B31\App Sidebar  │ Images Resource List│ Image Detail                 │
│              │                     │                              │
│ Containers   │ Title / Total Size  │ ID                           │
│ Images       │ Sort Search Pull    │ Tags                         │
│ Volumes      │                     │ Created                      │
│ Networks     │ In Use              │ Size                         │
│ ...          │ image rows          │ Platform                     │
│              │                     │                              │
│              │ Unused              │ Export                       │
│              │ image rows          │ Config                       │
│              │                     │ Environment                  │
P0+r4B33\│              │                     │ Labels                       │
│              │                     │ Used By                      │
└──────────────┴─────────────────────┴──────────────────────────────┘
```

继续使用现有应用侧边栏，不修改已经完成的侧边栏视觉。

Images 页面内部包含：

```text
ImageListPanel
ImageDetailPanel
```

建议桌面宽屏比例：

```text
资源列表宽度：280–340 px
右侧详情：占据剩余空间
```

使用 Kirigami 和 Qt Layout 自适应尺寸，不直接写死截图中的像素值。

---

# 三、明确删除右侧顶部栏

截图右侧顶部存在：

```text
Info | Terminal | Files
```

本项目 Images 页面中完全删除这一区域。

右侧详情面板直接从镜像基础信息开始：

```text
ID
Tags
Created
Size
Platform
```

不要创建：

* `Info` Tab
* `Terminal` Tab
* `Files` Tab
* SegmentedControl
* 顶部空白占位
* 隐藏但仍占空间的组件

右侧详情页结构：

```text
ImageDetailPanel
└── ScrollView
    └── ColumnLayout
        ├── BasicInfoSection
        ├── ExpoP0+r4B34\rtAction
        ├── ConfigSection
        ├── EnvironmentSection
        ├── LabelsSection
        └── UsedBySection
```

---

# 四、Images 页面顶部工具栏

截图中间资源列表顶部的功能保留：

```text
Images
总占用空间

排序按钮
搜索按钮或搜索框
拉取镜像按钮
```

KDE 风格布局建议：

```text
┌─────────────────────────────────┐
│ Images                          │
│ 4.39 GiB total                  │
│                                 │
│ [排序] [搜索]             [拉取] │
└─────────────────────────────────┘
```

也可以在宽度允许时使用单行：

```text
Images / 4.39 GiB total    Sort Search Pull
```

使用：

```qml
Kirigami.Heading
QQC2.ToolButton
QQC2.SearchField
Kirigami.Action
```

根据项目当前 Kirigami 版本选择实际可用控件。

顶部工具栏功能：

* 显示镜像数量
* 显示镜像总大小
* 搜索
* 排序
* 拉取镜像
* 手动刷新

建议按钮：

```text
view-sort-ascending
view-filter
edit-find
view-refresh
list-addP0+r4B35\
download
```

必须验证实际 KDE 图标主题中存在的名称。

---

# 五、资源列表分组

镜像列表划分为：

```text
In Use
Unused
```

判定规则：

## In Use

镜像被至少一个 Docker 容器引用，包括：

* running container
* stopped container
* created container
* paused container
* exited container

只要容器的镜像 ID 与镜像 ID 匹配，就属于 `In Use`。

## Unused

没有任何现存容器引用的镜像。

不要直接依赖镜像摘要中的不稳定或含义不清晰字段判断使用状态。

建议实现：

1. 获取所有镜像。
2. 获取所有容器，包含 stopped 容器。
3. 收集容器的 image ID。
4. 通过规范化后的完整 image ID 建立关联。
5. 计算每个镜像的使用容器列表。
6. 根据列表是否为空分组。

需要处理：

```text
sha256:<digest>
短 ID
完整 ID
RepoTag
RepoDigest
dangling image
多个 tag 指向同一 image
```

---

# 六、镜像列表行

每一行至少展示：

```text
镜像图标
主显示名称
辅助信息
架构标签
删除按钮
```

示例：

```text
[icon] postgres:17
       476.2 MiB, 2 weeks ago        [amd64] [delete]
```

主显示名称规则：

1. 优先使用第一个有效 RepoTag。
2. 有多个 RepoTag 时显示主要 tag，并显P0+r6B42\示 `+N`。
3. 没有 RepoTag 时显示：

   ```text
   <none>:<none>
   ```
4. 辅助信息显示：

   ```text
   格式化大小 · 相对创建时间
   ```

架构标签：

```text
amd64
arm64
arm/v7
386
ppc64le
s390x
unknown
```

平台信息来源应以 inspect 结果为准。

列表行组件建议：

```text
ImageListItem.qml
```

公开属性：

```qml
property string imageId
property string displayName
property string secondaryText
property string architecture
property string iconName
property bool selected
property bool inUse
property bool busy
property int usedByCount

signal selectedRequested(string imageId)
signal removeRequested(string imageId)
```

---

# 七、镜像图标策略

不要联网下载 Docker Hub 图标。

推荐策略：

* 默认统一使用容器镜像图标
* 可根据常见仓库名映射少量本地图标
* 映射失败使用通用系统图标
* 不影响真实功能

推荐默认图标：

```text
package-x-generic
application-x-archive
docker
system-run
```

必须使用当前系统实际可用图标。
P0+r5053\
图标不是功能依赖。

禁止：

* 为每个镜像请求外部网站
* 根据 registry URL 下载 favicon
* 内嵌大量第三方商标
* 因为图标缺失阻止列表加载

---

# 八、镜像选中状态

默认行为：

* 加载完成后选择第一张镜像
* 优先选择 `In Use` 组第一项
* 列表为空时不选择
* 删除当前镜像后选择相邻镜像
* 刷新后尽量保持当前 image ID 的选择
* 当前镜像消失时选择第一项

选中样式使用 KDE 系统强调色：

```qml
Kirigami.Theme.highlightColor
Kirigami.Theme.highlightedTextColor
```

Hover 使用当前已经改好的 KDE Breeze 风格：

* 浅强调色背景
* 1px 强调色边框
* 选中状态优先
* Hover 不引起布局抖动

禁止恢复固定紫色。

---

# 九、搜索功能

搜索范围包括：

* RepoTag
* RepoDigest
* image ID
* short ID
* Label
* architecture
* 使用该镜像的容器名称

搜索规则：

* 大小写不敏感
* 前后空格自动去除
* 输入变化后即时过滤
* 可增加 150–250ms debounce
* 清空搜索后恢复完整列表
* 搜索只改变展示，不重新请求 Docker Engine

搜索无结果时显示：

```text
No images match “xxx”
```

使用 KDE PlaceholderMessage。

---

# 十、排序功能

排序菜单至少包含：

```text
Name A–Z
Name Z–A
Newest First
Oldest FirsP0+r5045\t
Largest First
Smallest First
Used First
Unused First
```

默认排序：

```text
In Use 在前
每组内按创建时间从新到旧
```

排序菜单使用：

```qml
QQC2.Menu
Kirigami.Action
```

当前排序项显示选中标记。

排序状态保存在 GUI 内存中，后续可以写入设置。

---

# 十一、拉取镜像

顶部加号或下载按钮打开 `PullImageDialog.qml`。

对话框字段：

```text
Image reference
Registry authentication，可选
Platform，可选
```

第一阶段必须支持：

```text
ubuntu:24.04
postgres:17
nginx:latest
ghcr.io/owner/image:tag
registry.example.com/project/image:tag
```

镜像名称为空时禁止提交。

平台可选值：

```text
自动
linux/amd64
linux/arm64
自定义
```

认证字段：

```text
Username
Password / token
Registry server
```

认证信息：

* 仅在当前请求期间保存在内存
* 不写日志
* 不存入配置文件
* 不通过 QML debug 输出
* 不保存为明文历史

拉取过程必须显示真实进度。

Bollard 会返回 pull stream，应解析：

* status
* id
* progress
* progress_detail.current
* progress_detail.total
* error
* error_detail

GUI 至少显示：

```text
Resolving
Pulling fs layer
Downloading
Extracting
Verifying checksum
Pull complete
Downloaded newer image
Image is up to date
```

进度状态：

```rust
pub struct ImagePullProgress {
    pub image_reference: String,
    pub layer_id: Option<String>,
    pub status: String,
    pub current: Option<u64>,
    pub total: Option<u64>,
    pub percent: Option<f64>,
    pub completed: bool,
}
```

拉取期间：

* 禁止重复提交相同 reference
* 支持取消
* 关闭窗口时取消任务
* 完成后刷新镜像列表
* 自动选中新拉取的镜像
* Docker 错误以结构化消息显示

不要使用假进度条。

---

# 十二、删除镜像

每一行的删除按钮必须调用真实 Docker API。

点击后弹出确认对话框：

```text
Remove image “postgres:17”?
```

显示：

* image ID
* tags
* size
* 使用它的容器数量
* 删除风险

选项：

```text
Force removal
Prune untagged parent images
```

如果镜像正在被容器使用：

* 默认禁用普通删除确认按钮或明确提示风险
* 允许用户主动选择 Force
* 不自动删除容器
* Docker 返回冲突时显示真实原因

删除成功后：

* 从列表中移除
* 更新总大小
* 更新选中项
* 刷新分组
* 显示 Kirigami passive notification

删除错误应区分：

```text
Image not found
Image is being used
Permission denied
Docker unavailable
Operation timeout
Docker API error
```

删除操作期间：

* 当前行显示 busy
* 禁用重复删除
* 保持其他镜像可操作
* 禁止整页卡死

---

# 十三、镜像基础详情

右侧第一部分显示：

```text
ID
Tags
Repo Digests
Created
Size
Virtual Size，如 Docker 提供
Platform
Architecture
OS
Author
Docker Version
Comment
```

页面视觉参考截图的信息表：

```text
Key                              Value
```

使用 KDE 风格的属性列表。

可以创建：

```text
PropertyGroup.qml
PropertyRow.qml
```

视觉要求：

* 使用主题背景
* 组内行之间使用轻微 separator
* 不固定浅灰色
* 不使用巨大圆角卡片
* 保持 KDE 设置页风格
* Key 左对齐
* Value 右侧或第二列
* 长文本可选择、复制和省略
* Hover 可显示完整 Tooltip

ID 旁边增加复制操作：

```text
edit-copy
```

Tags 和 Digests 支持复制。

---

# 十四、Config 区域

显示真实镜像配置：

```text
Command
Entrypoint
Working Directory
User
Stop Signal
Hostname
Domain Name
Shell
```

字段缺失时显示：

```text
—
```

Command 和 Entrypoint 使用可靠格式化方式：

* 保留参数边界
* 支持复制
* 长内容换行或省略
* 点击可以打开完整值对话框

禁止简单地用空格拼接导致含空格参数失真。

可以使用 JSON 数组形式显示：

```json
["dockerd", "--host=unix:///var/run/docker.sock"]
```

---

# 十五、Environment 区域

显示镜像 Config 中真实环境变量。

表格：

```text
Key | Value
```

解析规则：

```text
KEY=value
KEY=
KEY
```

第一个 `=` 作为键值分隔点，Value 中后续 `=` 必须保留。

支持：

* 按 key 排序
* 搜索
* 复制 key
* 复制 value
* 复制整行
* 长值省略并显示 Tooltip
* 无环境变量时显示空状态

注意安全：

* 默认展示 Docker 镜像中已有环境变量
* 不写入应用日志
* 不在 tracing 中输出 Value
* README 中说明镜像环境变量可能包含敏感信息
* 可以为后续敏感值遮罩预留逻辑，但当前不得伪造脱敏结果

---

# 十六、Labels 区域

显示镜像真实 Labels：

```text
Key | Value
```

支持：

* 排序
* 搜索
* 复制
* 长值显示
* 空状态

常见 Compose Labels 需要正常展示，例如：

```text
com.docker.compose.project
com.docker.compose.service
org.opencontainers.image.source
org.opencontainers.image.version
```

不要仅显示固定白名单。

---

# 十七、Used By 区域

真实显示所有引用当前镜像的容器。

每个容器至少显示：

```text
容器名称
short ID
状态
创建时间
```

示例：

```text
floatctf-dev       running       abcd12345678
postgres-test      exited        efgh12345678
```

点击容器：

* 切换到 Containers 页面
* 选中对应容器
* 打开容器详情

如果跨页面选择机制尚未实现：

* 先发出结构化导航 signal
* 在现有页面导航层接入
* 不使用 shell 命令
* 不创建假跳转

没有容器使用时显示：

```text
This image is not used by any container.
```

---

# 十八、导出镜像

截图中有 `Export` 操作，本次必须真实实现。

点击 Export 后打开文件保存对话框。

默认文件名：

```text
<repository>-<tag>.tar
```

需要清理非法路径字符。

无 tag 时：

```text
image-<short-id>.tar
```

使用 Docker Engine 的镜像导出能力，将镜像内容流式写入目标文件。

要求：

* 流式写文件
* 禁止一次性把完整镜像读入内存
* 显示进度状态
* 支持取消
* 临时文件写入
* 成功后原子重命名
* 失败时删除不完整临时文件
* 磁盘空间不足时显示明确错误
* 用户取消时清理临时文件
* 文件覆盖前确认
* 不在 Qt 主线程写文件

导出进度无法从 Docker API获得精确总大小时：

* 使用 indeterminate progress
* 显示已写入字节数
* 不伪造百分比

导出成功后显示：

```text
Image exported to /path/file.tar
```

可以提供“打开所在目录”操作，使用 Qt DesktopServices 或 KDE 支持方式。

---

# 十九、总大小统计

页面标题下显示：

```text
4.39 GiB total
```

计算规则：

* 对唯一 image ID 求和
* 不因多个 tag 重复计算
* 使用镜像 Size
* 清晰标识逻辑大小
* 避免把共享 layer 空间宣称为实际磁盘独占空间

建议文案：

```text
4.39 GiB total image size
```

如 Docker Engine 提供 disk usage API，可以同时展示：

```text
Logical image size
Reclaimable size
```

第一阶段至少实现唯一镜像 Size 求和。

字节格式化使用 IEC：

```text
KiB
MiB
GiB
TiB
```

---

# 二十、docker-core 数据模型

在 `docker-core` 中增加或完善：

```rust
pub struct ImageSummary {
    pub id: String,
    pub short_id: String,
    pub repo_tags: Vec<String>,
    pub repo_digests: Vec<String>,
    pub display_name: String,
    pub created_at: Option<DateTime<Utc>>,
    pub size_bytes: u64,
    pub shared_size_bytes: Option<u64>,
    pub virtual_size_bytes: Option<u64>,
    pub labels: BTreeMap<String, String>,
    pub containers: Vec<ImageContainerReference>,
    pub in_use: bool,
}
```

详情：

```rust
pub struct ImageDetail {
    pub summary: ImageSummary,
    pub architecture: Option<String>,
    pub os: Option<String>,
    pub variant: Option<String>,
    pub author: Option<String>,
    pub docker_version: Option<String>,
    pub comment: Option<String>,
    pub command: Vec<String>,
    pub entrypoint: Vec<String>,
    pub environment: Vec<EnvironmentVariable>,
    pub working_dir: Option<String>,
    pub user: Option<String>,
    pub stop_signal: Option<String>,
    pub shell: Vec<String>,
    pub labels: BTreeMap<String, String>,
    pub root_fs_layers: Vec<String>,
}
```

容器引用：

```rust
pub struct ImageContainerReference {
    pub id: String,
    pub short_id: String,
    pub name: String,
    pub state: ContainerState,
    pub status: String,
}
```

环境变量：

```rust
pub struct EnvironmentVariable {
    pub key: String,
    pub value: String,
}
```

拉取参数：

```rust
pub struct PullImageOptions {
    pub reference: String,
    pub platform: Option<String>,
    pub registry_auth: Option<RegistryAuth>,
}
```

删除参数：

```rust
pub struct RemoveImageOptions {
    pub force: bool,
    pub prune_children: bool,
}
```

---

# 二十一、ImageService

在 `docker-core` 中提供清晰的镜像服务：

```rust
pub struct ImageService {
    client: Arc<DockerClient>,
}
```

公开方法至少包括：

```rust
impl ImageService {
    pub async fn list_images(
        &self,
        options: ListImagesOptions,
    ) -> Result<Vec<ImageSummary>, DockerError>;

    pub async fn inspect_image(
        &self,
        id: &str,
    ) -> Result<ImageDetail, DockerError>;

    pub async fn remove_image(
        &self,
        id: &str,
        options: RemoveImageOptions,
    ) -> Result<Vec<ImageDeleteResult>, DockerError>;

    pub fn pull_image(
        &self,
        options: PullImageOptions,
    ) -> ImagePullStream;

    pub fn export_image(
        &self,
        id: &str,
    ) -> ImageExportStream;
}
```

实际 Bollard API 名称与签名必须查询当前官方文档。

不要根据旧版本记忆机械编写。

Bollard 类型不得暴露到 GUI。

---

# 二十二、Docker 数据请求策略

Images 页面首次加载建议：

```text
并行请求：
- list images
- list all containers
- Docker system info 或 disk usage
```

之后：

1. 构建 image ID 到 containers 的映射。
2. 生成 In Use / Unused 分组。
3. 计算总大小。
4. 更新 Qt Models。
5. 选择默认镜像。
6. 异步获取选中镜像 inspect 信息。

不要对列表中的每张镜像立即调用 inspect，避免 N+1 请求。

只有以下情况调用 inspect：

* 用户选中镜像
* 列表摘要缺少必要字段
* 用户打开详情
* 导出、删除等操作需要确认信息

可以增加有限详情缓存：

```rust
HashMap<ImageId, Arc<ImageDetail>>
```

缓存仅存在于应用进程内。

刷新后根据 image ID 和更新时间失效。

---

# 二十三、Qt Model 设计

使用两个 Qt Model 或一个带 section role 的模型。

推荐一个统一模型：

```text
ImageListModel
```

roles 至少包括：

```text
imageId
shortId
displayName
repoTags
secondaryText
sizeBytes
sizeText
createdAt
createdText
architecture
inUse
usedByCount
selected
busy
operation
section
```

`section` 值：

```text
in_use
unused
```

QML 根据 section 显示分组标题。

也可以使用两个 model：

```text
InUseImagesModel
UnusedImagesModel
```

选择最符合当前项目 Qt Model 结构的方案，避免复制大量逻辑。

详情数据通过独立 QObject 或 detail model 暴露。

Environment、Labels、Used By 使用独立 model：

```text
EnvironmentModel
LabelModel
ImageUsageModel
```

禁止把整个 detail 序列化成 JSON 后交给 QML解析。

---

# 二十四、ImagesController

新增或完善：

```text
ImagesController
```

负责：

* 刷新镜像列表
* 搜索
* 排序
* 选中镜像
* 加载详情
* 删除镜像
* 拉取镜像
* 导出镜像
* 操作状态
* 错误状态
* 页面生命周期
* 取消任务

建议状态：

```rust
pub enum ImagesLoadState {
    Idle,
    Loading,
    Ready,
    Empty,
    Error,
    DockerUnavailable,
    PermissionDenied,
}
```

控制器公开给 QML 的属性建议：

```text
loading
state
errorMessage
totalImageCount
inUseCount
unusedCount
totalSizeText
searchQuery
sortMode
selectedImageId
detailLoading
operationInProgress
```

invokable：

```text
refresh()
selectImage(imageId)
setSearchQuery(query)
setSortMode(mode)
removeImage(imageId, force, pruneChildren)
pullImage(reference, platform, username, password, registry)
cancelPull()
exportImage(imageId, destinationPath)
cancelExport()
```

所有 Docker 操作在 Tokio 后台执行。

---

# 二十五、异步和竞态处理

禁止在 Qt 主线程调用：

```rust
block_on
std::thread::sleep
同步文件写入
Docker API
```

页面刷新使用 generation ID：

```rust
refresh_generation += 1;
```

只有最新请求结果允许更新 UI。

选中详情使用独立 generation：

```rust
detail_generation += 1;
```

场景：

1. 用户点击镜像 A。
2. A 的 inspect 请求开始。
3. 用户马上点击镜像 B。
4. B 请求完成。
5. A 请求后完成。
6. A 的旧结果不得覆盖 B。

删除、拉取和导出任务使用 CancellationToken。

应用退出时取消所有任务。

---

# 二十六、KDE 原生视觉要求

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

必须适配：

* Breeze Light
* Breeze Dark
* 系统强调色
* 系统字体
* 高 DPI
* Wayland
* 键盘导航
* 无障碍

禁止使用截图中的：

```text
macOS 红黄绿窗口按钮
固定紫色选中背景
固定灰白卡片
超大圆角
macOS Segmented Control
Personal use only 标签
右上角外链按钮
```

列表选中、Hover、Focus 继续沿用当前已完成的 KDE Breeze 交互效果。

---

# 二十七、右侧详情视觉

右侧详情使用 ScrollView。

建议结构：

```qml
QQC2.ScrollView {
    contentWidth: availableWidth

    ColumnLayout {
        width: parent.width

        BasicInfoGroup {}
        ExportActionRow {}
        ConfigGroup {}
        EnvironmentTable {}
        LabelsTable {}
        UsedByList {}
    }
}
```

内容最大宽度建议：

```text
800–1000 px
```

在超宽窗口中居中，避免属性表横跨整个屏幕。

使用：

```qml
Layout.maximumWidth
Layout.alignment: Qt.AlignHCenter
```

每个 Section：

```text
Section Heading
Property/Table content
```

标题使用：

```qml
Kirigami.Heading {
    level: 3
}
```

Section 之间使用足够垂直间距。

不要将所有内容包成一个巨大卡片。

---

# 二十八、详情空状态

未选中镜像时：

```text
Select an image to view its details.
```

列表为空时：

```text
No Docker images found.
Pull an image to get started.
```

Docker 不可用时：

```text
Docker Engine is unavailable.
```

权限不足时：

```text
TuxStack cannot access the Docker socket.
```

使用：

```qml
Kirigami.PlaceholderMessage
```

提供：

* Retry
* Pull Image
* 查看权限说明

不要自动执行 sudo 或修改 docker group。

---

# 二十九、错误处理

需要区分：

```text
Docker socket not found
Permission denied
Docker Engine unavailable
Image not found
Image is in use
Invalid image reference
Registry authentication failed
Registry unavailable
Pull failed
Export failed
Destination permission denied
Disk full
Operation timeout
Operation cancelled
Unknown Docker API error
```

GUI 展示简洁错误。

完整错误写入 debug 日志。

禁止日志输出：

* registry password
* token
* 环境变量值
* 镜像敏感 Labels
* 导出内容

---

# 三十、页面文件建议

建议创建或更新：

```text
crates/gui/qml/pages/ImagesPage.qml
crates/gui/qml/components/ImageListPanel.qml
crates/gui/qml/components/ImageListItem.qml
crates/gui/qml/components/ImageDetailPanel.qml
crates/gui/qml/components/PropertyGroup.qml
crates/gui/qml/components/PropertyRow.qml
crates/gui/qml/components/KeyValueTable.qml
crates/gui/qml/components/ImageUsedByList.qml
crates/gui/qml/dialogs/PullImageDialog.qml
crates/gui/qml/dialogs/RemoveImageDialog.qml
crates/gui/qml/dialogs/ExportImageDialog.qml
crates/gui/src/controllers/images.rs
crates/gui/src/models/image_model.rs
crates/gui/src/models/environment_model.rs
crates/gui/src/models/label_model.rs
crates/gui/src/models/image_usage_model.rs
```

docker-core：

```text
crates/docker-core/src/models/image.rs
crates/docker-core/src/services/images.rs
crates/docker-core/src/mapping/images.rs
crates/docker-core/src/streams/image_pull.rs
crates/docker-core/src/streams/image_export.rs
```

适配当前仓库实际结构，不制造重复模块。

---

# 三十一、测试要求

## docker-core 单元测试

必须测试：

* RepoTag 解析
* display name 选择
* dangling image
* short ID
* 多 tag
* 多 digest
* image size 格式化
* timestamp mapping
* architecture mapping
* Config mapping
* Entrypoint mapping
* Command mapping
* environment 解析
* labels mapping
* image/container 关联
* In Use 判断
* Unused 判断
* 多个容器引用同一镜像
* 同一镜像多个 tag 不重复计数
* Docker 404 到 ImageNotFound
* Docker 409 到 Conflict
* permission denied
* timeout

## GUI 状态测试

必须测试：

* loading → ready
* loading → empty
* loading → error
* 搜索过滤
* 排序
* 选择保持
* 删除后选择相邻项
* 旧详情结果不覆盖新选择
* 拉取进度更新
* 拉取取消
* 导出取消
* busy 状态清理
* Docker unavailable
* permission denied

## Docker 集成测试

真实 Docker 测试使用：

```rust
#[ignore]
```

或 feature：

```text
docker-integration
```

测试：

1. 拉取一个小型测试镜像。
2. 查询列表。
3. inspect。
4. 验证 tag。
5. 创建测试容器并验证 In Use。
6. 删除容器并验证 Unused。
7. 导出镜像。
8. 验证 tar 文件非空。
9. 删除镜像。
10. 清理所有测试资源。

测试镜像应尽量小，例如经验证可用的轻量镜像。

测试失败时仍要清理。

---

# 三十二、构建和验证

开始前执行：

```bash
git status
git log --oneline --decorate -20
find crates/docker-core -maxdepth 5 -type f | sort
find crates/gui -maxdepth 6 -type f | sort
cargo metadata --no-deps
cargo test --workspace
```

保留用户未提交修改。

完成后执行：

```bash
cargo fmt --all -- --check
cargo check --workspace
cargo test --workspace
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo build -p tuxstack
```

运行：

```bash
cargo run
```

人工验证：

1. Images 页面加载真实 Docker 镜像。
2. In Use 与 Unused 分组正确。
3. 总大小不会因多个 tag 重复计算。
4. 搜索有效。
5. 所有排序方式有效。
6. 选择镜像显示真实详情。
7. Config 数据正确。
8. Environment 数据正确。
9. Labels 数据正确。
10. Used By 容器列表正确。
11. 删除镜像真实生效。
12. 拉取镜像显示真实进度。
13. 导出镜像生成可用 tar 文件。
14. Docker 不可用状态正确。
15. 权限不足状态正确。
16. Breeze Light 显示正常。
17. Breeze Dark 显示正常。
18. 系统强调色变化后选中状态同步。
19. 右侧不存在 `Info / Terminal / Files` 顶栏。
20. 页面不存在 mock 数据。

---

# 三十三、工作边界

本次只实现 Docker Images。

不要实现：

* Containers 新功能
* Volumes 新功能
* Networks 新功能
* Kubernetes
* Incus
* Machines
* Terminal
* Files
* Compose
* Registry 账号持久化
* Docker Hub 在线搜索
* 镜像 Build
* 镜像 Push
* 镜像 Prune
* 多 Docker Host
* daemon
* REST API
* JSON-RPC

Activity Monitor、Commands、Devices 保持现有页面状态。

---

# 三十四、实施顺序

## Phase 1：docker-core 镜像模型

* ImageSummary
* ImageDetail
* ImageContainerReference
* PullImageOptions
* RemoveImageOptions
* mapping
* error mapping
* tests

## Phase 2：查询与详情

* list images
* list all containers
* 构建使用关系
* inspect image
* GUI model
* Images 页面真实列表
* 详情面板

## Phase 3：搜索和排序

* 本地搜索
* 分组
* 排序
* 选择保持
* 空状态

## Phase 4：删除

* 确认对话框
* force
* prune children
* busy
* 错误处理
* 刷新

## Phase 5：拉取

* PullImageDialog
* 真实 pull stream
* progress
* cancel
* 刷新并选中新镜像

## Phase 6：导出

* 文件选择
* 流式写文件
* 临时文件
* 取消
* 错误清理
* 成功通知

## Phase 7：测试和文档

* 单元测试
* GUI 状态测试
* Docker 集成测试
* README 功能状态
* 架构文档

---

# 三十五、Commit 建议

使用小而清晰的 commit：

```text
feat(docker-core): add image domain models and mappings
feat(docker-core): implement image listing and inspection
feat(gui): add KDE image resource list and details
feat(gui): add image search grouping and sorting
feat(docker-core): implement image removal
feat(gui): add image removal workflow
feat(docker-core): add image pull progress stream
feat(gui): add image pull dialog and progress
feat(docker-core): implement streaming image export
feat(gui): add image export workflow
test(images): cover mappings operations and UI state
docs: document Docker image management
```

不要把全部功能压进单个 commit。

---

# 三十六、验收结果

最终页面应达到：

```text
侧边栏：
保留现有 KDE 导航栏

中间：
Images 标题
总镜像大小
排序
搜索
刷新
拉取镜像
In Use 分组
Unused 分组
真实镜像列表
删除操作

右侧：
无 Info/Terminal/Files 顶栏
镜像基础信息
Export
Config
Environment
Labels
Used By
```

所有内容来自 Docker Engine。

完成后报告：

1. 修改文件列表
2. docker-core 新增 API
3. 镜像和容器关联算法
4. GUI 组件结构
5. 搜索与排序实现
6. 删除行为
7. 拉取进度实现
8. 导出实现
9. Light/Dark 主题验证
10. 单元测试结果
11. Docker 集成测试结果
12. Clippy 结果
13. 已知限制
14. 所有 commit hash

最终实现应保留截图的高效信息架构，同时呈现 KDE Plasma 原生视觉和真实 Docker Images 管理能力。
