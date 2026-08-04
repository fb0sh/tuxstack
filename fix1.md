

# TuxStack Docker 数据获取、缓存与增量刷新性能优化 Prompt

当前 TuxStack 的 Images、Volumes、Networks 等模块存在明显的数据加载和交互延迟。

从实际界面可见：

## Images

* 镜像列表已经出现；
* `architecture/platform` 等字段只有选中镜像并执行 inspect 后才显示；
* 未选中行长期显示 `unknown`；
* 切换选中时才加载详情，造成明显延迟；
* 同一镜像可能被重复 inspect；
* 重启应用后所有字段重新加载。

## Volumes

* 页面首次加载明显缓慢；
* 列表基础数据、容量、使用关系似乎被串行获取；
* `Files` 每次选择卷后都重新显示：

  ```text
  Preparing read-only volume access…
  ```
* 切换卷时重复创建 helper session；
* 已经读取过的数据没有复用；
* 返回页面后可能再次执行完整扫描；
* UI 等待所有数据完成后才更新。

本次任务需要重新设计 Docker 数据获取链路，实现：

> 缓存优先显示、基础数据快速加载、重数据后台补全、Docker Events 驱动失效、有限并发、请求去重和持久化快照。

本次重点是性能与数据生命周期。
持久化缓存和内存缓存只用于展示加速。所有危险操作、可用性判断和最终提交必须基于实时 Docker Engine 响应；任何由应用成功执行的操作必须同步更新内存模型，并立即使相关缓存失效或写入新状态。

不要继续修改页面视觉。

---

# 一、性能目标

需要达到以下目标。

## 应用冷启动

存在有效本地缓存时：

```text
进入 Images / Volumes / Networks
→ 50–150 ms 内显示缓存列表
→ 后台与 Docker Engine 同步
```

缓存不可用时：

```text
进入页面
→ 立即显示 loading
→ 首批基础列表尽量在 300 ms 内显示
→ 详情和昂贵字段后台补全
```

以上时间是本地 Docker Engine 的目标，不得写成绝对 SLA。

## 页面热切换

用户从其他页面返回：

```text
立即显示内存中的数据
```

不要重新执行完整加载。

## 选中详情

已经缓存详情时：

```text
立即显示
```

未缓存时：

```text
显示 detail skeleton
→ 后台 inspect
```

## Volume Files

同一个卷的有效 session 仍存在时：

```text
切回 Files
→ 立即显示上次目录
→ 后台刷新当前目录
```

不要每次切换标签都创建 helper。

---

# 二、整体缓存架构

新增统一的数据层：

```text
DockerDataStore
├── ImageRepository
├── VolumeRepository
├── NetworkRepository
├── ContainerRepository
├── DockerEventMonitor
├── PersistentCache
└── RequestCoordinator
```

数据流：

```text
                 ┌─────────────────────┐
                 │ Persistent snapshot │
                 └──────────┬──────────┘
                            │ startup
                            ▼
UI ◄──── in-memory store ◄──── Docker repositories
           ▲                         ▲
           │                         │
           └──── Docker events ──────┘
```

Controller 不应直接管理所有 Docker 请求。

Controller 只负责：

* 页面状态；
* 用户选择；
* 搜索排序；
* 触发 repository refresh；
* 把 repository 数据暴露给 Qt Model。

---

# 三、三层数据模型

每种资源都分为三层。

## 1. Summary

列表立即需要的数据。

例如 Image：

```rust
pub struct ImageSummary {
    pub id: String,
    pub short_id: String,
    pub display_name: String,
    pub repo_tags: Vec<String>,
    pub repo_digests: Vec<String>,
    pub created_at: Option<DateTime<Utc>>,
    pub size_bytes: u64,
    pub shared_size_bytes: Option<i64>,
    pub container_count: usize,
    pub in_use: bool,
    pub architecture: Option<String>,
    pub os: Option<String>,
    pub variant: Option<String>,
}
```

## 2. Detail

选中后展示的数据。

```rust
pub struct ImageDetail {
    pub summary: ImageSummary,
    pub config: ImageConfigDetail,
    pub environment: Vec<EnvironmentVariable>,
    pub labels: BTreeMap<String, String>,
    pub layers: Vec<ImageLayer>,
}
```

## 3. Derived Data

需要跨资源计算或昂贵请求的数据：

```text
Image → used by containers
Volume → used by containers
Volume → actual size
Network → connected containers
```

Derived Data 独立刷新，不阻塞 Summary。

---

# 四、禁止等待所有字段后才显示列表

当前可能存在类似逻辑：

```rust
let images = list_images().await;

for image in images {
    let detail = inspect_image(&image.id).await?;
    rows.push(map(detail));
}

model.replace(rows);
```

必须移除。

正确方式：

```text
list resources
    ↓
立即生成 summary rows
    ↓
立即更新 Qt Model
    ↓
后台有限并发补充缺失字段
    ↓
逐项 patch model
```

列表首次出现不能依赖全部 inspect 完成。

---

# 五、Images 优化

## 5.1 基础列表

首次使用 Docker image list API 获取：

```text
ID
RepoTags
RepoDigests
Created
Size
SharedSize，若请求支持
Labels，若摘要包含
```

立即显示：

```text
名称
大小
创建时间
In Use / Unused
```

不要等待 inspect。

---

## 5.2 Architecture 与 Platform

截图中的 `unknown` 说明列表模型缺少 architecture。

architecture 通常需要 image inspect。

优化策略：

### 首选：读取持久化详情缓存

```text
image ID 没变化
→ 直接复用 architecture/os/variant
```

Image ID 是内容寻址标识，同一 ID 的基本详情可视为稳定数据。

因此：

```rust
ImageMetadataCache {
    image_id -> {
        architecture,
        os,
        variant,
        config_digest,
        inspected_at,
    }
}
```

对同一个 image ID：

* 应用重启后可直接使用缓存；
* 不需要每次重新 inspect；
* 只有缓存缺失时才 inspect。

### 后台预取

列表完成后：

```text
找出 architecture 缺失的 Image IDs
→ 有限并发 inspect
→ 每完成一项立即 patch 对应行
```

并发限制：

```text
4–8
```

不要一次并发数百个 inspect。

### UI

缓存缺失且正在加载时显示：

```text
…
```

或隐藏 badge。

不要显示永久 `unknown`，因为这看起来像真实平台值。

只有 Docker 真正返回未知时才显示：

```text
Unknown architecture
```

---

## 5.3 Image inspect 去重

新增 SingleFlight：

```rust
inspect_image(image_id)
```

同一时刻多个调用请求同一 image ID：

```text
只执行一次 Docker 请求
其他调用等待同一 Future
```

示意：

```rust
HashMap<ImageId, Shared<BoxFuture<Result<Arc<ImageDetail>>>>>
```

或者实现统一的：

```rust
RequestCoordinator::run_once(key, future)
```

要求：

* 请求完成后移除 in-flight 项；
* 成功结果写入 cache；
* 失败不得永久缓存；
* 短时间错误可有 1–3 秒退避；
* 取消一个调用方不能取消其他调用方共用的请求。

---

## 5.4 Image 与 Container 的使用关系

不要为了判断 Image `In Use` 而逐个 inspect 镜像。

一次获取全部容器摘要：

```text
list containers(all=true)
```

建立：

```rust
HashMap<ImageId, usize>
```

然后批量 patch：

```text
container_count
in_use
```

不要每张镜像单独查询容器。

---

# 六、Volumes 首屏优化

Volumes 慢的主要原因通常是把以下操作串行放在首屏：

```text
list volumes
list containers
inspect containers
system df
helper session
volume size scan
```

这些操作必须拆开。

## 首屏只执行

```text
GET volumes
```

将返回的基础信息立即映射到列表：

```text
Name
Driver
Scope
CreatedAt
Labels
Options
Mountpoint
可能存在的 UsageData
```

Docker 的 volume list 支持返回 summary volume data；`UsageData` 可能未知，因此不能等待或假设一定存在。([Docker Documentation][1])

UI 第一阶段立即显示：

```text
卷名称
Driver
Created
大小已知则显示大小
未知则显示 Calculating… 或 Unknown
```

---

# 七、Volumes 分阶段加载

采用以下 Pipeline：

## Stage A：基础卷列表

优先级最高：

```text
list volumes
→ 立即更新 VolumeListModel
```

目标：

```text
100–300 ms 内显示名字列表
```

## Stage B：容器引用关系

并行执行：

```text
list containers(all=true)
→ 从 Mounts 建立 volume_name → containers
→ patch usedByCount
→ 更新 In Use / Unused 分组
```

如果 container list 摘要已包含足够 mount 数据：

```text
禁止额外 inspect
```

只有缺失时才有限并发 inspect。

## Stage C：磁盘使用量

后台调用：

```text
GET /system/df?type=volume
```

Docker Engine API 支持按对象类型请求磁盘使用数据，volume 可以单独计算，避免同时计算 images、containers 和 build cache。([Docker Documentation][2])

完成后 patch：

```text
size
ref_count
known_total_size
unknown_count
```

该请求可能较慢，所以：

* 不阻塞基础列表；
* 页面离开后可继续完成；
* 相同请求全局去重；
* 设置合理 TTL；
* 不在每次选择卷时调用；
* 不在每次刷新当前目录时调用。

## Stage D：选中卷详情

只有选中时执行：

```text
inspect volume
```

详情缓存后复用。

---

# 八、Volume Size 缓存策略

卷大小变化频率与卷内容有关，不能永久缓存。

定义：

```rust
pub struct CachedVolumeUsage {
    pub size_bytes: Option<u64>,
    pub ref_count: Option<u64>,
    pub measured_at: DateTime<Utc>,
    pub source: VolumeUsageSource,
}
```

来源：

```rust
pub enum VolumeUsageSource {
    SystemDf,
    VolumeList,
    VolumeInspect,
    PersistentCache,
}
```

TTL 建议：

```text
内存缓存：30–60 秒
持久化缓存：启动时立即展示
后台刷新：进入 Volumes 页面后触发
```

UI 可以显示旧缓存，同时后台刷新。

例如：

```text
2.4 MiB
```

无需因为数据正在后台重新验证就切回 skeleton。

只有完全没有缓存时显示：

```text
Calculating…
```

不要把未知显示为 `0 B`。

---

# 九、Volume Files Session 优化

当前截图每次选择卷都长时间停留在：

```text
Preparing read-only volume access…
```

需要实现 Session Pool。

定义：

```rust
pub struct VolumePreviewSessionCache {
    sessions: HashMap<VolumeName, CachedPreviewSession>,
}
```

```rust
pub struct CachedPreviewSession {
    pub session: VolumePreviewSession,
    pub last_used_at: Instant,
    pub state: SessionState,
    pub directory_cache: LruCache<VolumePath, CachedDirectory>,
}
```

## Session 生命周期

首次打开：

```text
创建 helper
→ 加载根目录
```

切换到 Info：

```text
不要立即销毁 helper
```

切回 Files：

```text
复用 helper
→ 立即显示缓存目录
→ 后台刷新
```

切换到另一个 Volume：

* 当前 helper 进入空闲状态；
* 可暂时保留；
* 总 session 数有限制；
* LRU 淘汰旧 session。

建议：

```text
最大活跃/空闲 session：2–4
空闲 TTL：60–180 秒
```

达到上限：

```text
停止并删除最久未使用的 session
```

应用退出：

```text
清理全部 helper
```

---

# 十、目录缓存

同一个卷内目录已经加载过时：

```text
立即显示缓存结果
→ 后台重新读取
→ 有变化时增量更新
```

定义：

```rust
pub struct CachedDirectory {
    pub entries: Arc<Vec<VolumeFileEntry>>,
    pub fetched_at: Instant,
    pub generation: u64,
}
```

TTL：

```text
2–10 秒
```

Files 浏览强调即时性，因此 TTL 不宜过长。

用户主动 Refresh：

```text
绕过 TTL
```

目录切换：

* 优先读内存缓存；
* 不落盘持久化文件名列表；
* 文件名可能敏感，默认仅内存缓存。

---

# 十一、启动预热

应用完成 Docker 连接后，可以低优先级预热：

```text
list containers
list images
list volumes
list networks
```

不要同时执行全部昂贵请求。

优先级：

```text
P0 当前页面的基础列表
P1 当前页面详情
P2 当前页面 Derived Data
P3 其他页面基础列表
P4 其他页面详情预取
```

使用全局并发限制：

```rust
DockerRequestScheduler {
    high_priority: Semaphore,
    background: Semaphore,
}
```

建议：

```text
高优先级并发：8
后台并发：2–4
```

当前页面请求不能被后台预热挤占。

---

# 十二、持久化缓存

新增轻量持久化存储。

推荐：

```text
SQLite
```

也可以使用当前项目已经存在且成熟的嵌入式存储方案。

不推荐为所有数据手工维护多个 JSON 文件。

缓存目录：

```text
$XDG_CACHE_HOME/tuxstack/docker-cache.sqlite3
```

不要放在配置目录。

---

# 十三、缓存隔离键

缓存不能只按资源 ID 存储。

必须按 Docker endpoint 隔离：

```rust
pub struct DockerEndpointKey {
    pub endpoint_fingerprint: String,
    pub daemon_id: Option<String>,
    pub context_name: Option<String>,
}
```

Fingerprint 可以来自：

* Unix socket 路径；
* Docker context endpoint；
* TLS endpoint；
* daemon ID，若可获取；
* API host。

防止：

```text
本地 Docker 的镜像缓存
被错误显示在远程 Docker 页面
```

不要把 credential 存入 fingerprint。

---

# 十四、持久化表结构建议

```sql
CREATE TABLE docker_cache_metadata (
    endpoint_key TEXT PRIMARY KEY,
    schema_version INTEGER NOT NULL,
    daemon_id TEXT,
    api_version TEXT,
    last_connected_at INTEGER NOT NULL
);

CREATE TABLE image_summaries (
    endpoint_key TEXT NOT NULL,
    image_id TEXT NOT NULL,
    payload BLOB NOT NULL,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY (endpoint_key, image_id)
);

CREATE TABLE image_details (
    endpoint_key TEXT NOT NULL,
    image_id TEXT NOT NULL,
    payload BLOB NOT NULL,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY (endpoint_key, image_id)
);

CREATE TABLE volume_summaries (
    endpoint_key TEXT NOT NULL,
    volume_name TEXT NOT NULL,
    payload BLOB NOT NULL,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY (endpoint_key, volume_name)
);

CREATE TABLE volume_usage (
    endpoint_key TEXT NOT NULL,
    volume_name TEXT NOT NULL,
    size_bytes INTEGER,
    ref_count INTEGER,
    measured_at INTEGER NOT NULL,
    PRIMARY KEY (endpoint_key, volume_name)
);

CREATE TABLE network_summaries (
    endpoint_key TEXT NOT NULL,
    network_id TEXT NOT NULL,
    payload BLOB NOT NULL,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY (endpoint_key, network_id)
);
```

Payload 可使用：

* MessagePack；
* CBOR；
* JSON；
* 独立字段。

优先复用项目现有序列化依赖。

SQLite 操作不得阻塞 Qt 主线程。

---

# 十五、哪些数据应该持久化

允许持久化：

```text
Image summary
Image architecture/os/variant
Image detail metadata
Volume summary
Volume size与测量时间
Network summary
Container summary
资源关联数量
页面最近选择
```

默认不要持久化：

```text
Volume 文件名列表
Volume 文件内容
环境变量明文，尤其可能包含 secret
Registry credential
Container secret
Docker auth config
日志正文
```

Image environment 可能含敏感信息，持久化前需要明确策略。

建议：

```text
默认不持久化 environment values
```

详情打开时从内存或 Docker 获取。

---

# 十六、Stale-While-Revalidate

所有页面统一采用：

```text
先显示缓存
→ 标记数据可能较旧
→ 后台刷新
→ 原位更新
```

不需要在正常 UI 中显示刺眼的 “stale”。

可以在：

* 刷新按钮 Tooltip；
* 调试信息；
* 数据更新时间；

展示：

```text
Updated 12 seconds ago
```

刷新失败时：

```text
保留缓存数据
显示非阻塞错误提示
```

不要清空整个页面。

例如：

```text
Could not refresh Docker data. Showing cached information.
```

---

# 十七、Docker Events 增量更新

启动一个全局 Docker Event Monitor。

监听至少：

```text
image
container
volume
network
daemon
```

Volume 会产生 create、mount、unmount、destroy、prune 等事件；Image 也会产生 create、delete、pull、tag、untag、prune 等事件，可据此刷新或失效对应缓存。([Docker Documentation][3])

处理策略：

## Image event

```text
pull/create/load/tag/untag/delete/prune
→ debounce 100–500ms
→ refresh image summaries
```

## Container event

```text
create/start/stop/die/destroy/rename
→ 更新 container summaries
→ 重新计算 image in-use
→ 重新计算 volume used-by
→ 重新计算 network connected containers
```

## Volume event

```text
create/destroy/prune
→ refresh volume list
```

```text
mount/unmount
→ invalidate used-by/refcount
→ 延迟刷新 volume usage
```

## Network event

```text
create/destroy/connect/disconnect
→ refresh network list或单项详情
```

事件处理必须 debounce。

例如大量 Compose 启动时：

```text
50 个事件
→ 合并成一次 refresh
```

---

# 十八、事件断线恢复

Event stream 断开时：

* 指数退避重连；
* 显示 debug 日志；
* 不清空缓存；
* 重连后执行一次轻量全量同步；
* 使用 `since` 参数或记录最后时间，条件允许时补事件；
* daemon 重启后重新获取 daemon identity。

退避建议：

```text
1s → 2s → 5s → 10s → 30s
```

加少量 jitter。

---

# 十九、请求去重

必须实现全局请求协调。

容易重复的请求：

```text
list_images
inspect_image(id)
list_volumes
inspect_volume(name)
system_df(volume)
list_containers(all=true)
inspect_container(id)
list_networks
inspect_network(id)
start_volume_preview_session(name)
list_volume_directory(name, path)
```

同一个 key 的并发调用只执行一次。

示例 key：

```rust
pub enum RequestKey {
    ListImages,
    InspectImage(String),
    ListVolumes,
    InspectVolume(String),
    VolumeSystemDf,
    ListContainersAll,
    InspectContainer(String),
    ListNetworks,
    InspectNetwork(String),
    VolumePreviewSession(String),
    VolumeDirectory(String, VolumePath),
}
```

---

# 二十、TTL 策略

建议初始值：

| 数据                     |         内存 TTL | 持久化使用  |
| ---------------------- | -------------: | ------ |
| Image summary          |        10–30 秒 | 启动显示   |
| Image detail           | 直到 image ID 消失 | 长期缓存   |
| Container summary      |          2–5 秒 | 启动显示可选 |
| Volume summary         |        10–30 秒 | 启动显示   |
| Volume detail          |        30–60 秒 | 启动显示   |
| Volume usage           |        30–60 秒 | 启动显示   |
| Network summary        |        10–30 秒 | 启动显示   |
| Network detail         |        30–60 秒 | 启动显示   |
| Volume directory       |         2–10 秒 | 不持久化   |
| Preview helper session |     60–180 秒空闲 | 不持久化   |

Docker Events 到达时可以提前失效，不必等待 TTL。

TTL 必须集中配置，不要散落在 Controller。

---

# 二十一、Qt Model 增量更新

当前可能每完成一个字段就完整 reset model，导致闪烁和选中丢失。

必须支持按 ID patch。

例如：

```rust
pub enum ImageModelPatch {
    Architecture {
        image_id: String,
        architecture: String,
    },
    Usage {
        image_id: String,
        container_count: usize,
    },
}
```

Qt Model：

```text
找到对应 row
更新字段
emit dataChanged(row, roles)
```

只有资源集合整体变化时才 reset。

要求：

* 选中 ID 不变；
* 滚动位置尽量不变；
* 搜索排序状态不变；
* 补字段时不会整列表闪烁；
* 分组变化时执行最小必要更新。

---

# 二十二、UI 状态调整

列表字段分成：

```text
Known
Loading
Unavailable
Unknown
```

不要用一个字符串 `unknown` 表示全部情况。

例如 architecture：

```rust
pub enum FieldState<T> {
    Loading,
    Ready(T),
    Unavailable,
    Error(String),
}
```

QML：

```text
Loading       → “…”或隐藏
Ready(amd64)  → amd64
Unavailable   → —
Error         → tooltip 显示错误
```

截图中的灰色 `unknown` badge 应移除。

---

# 二十三、刷新行为

刷新按钮分两种。

## 普通刷新

```text
立即返回现有数据
后台绕过 TTL 请求最新 summary
```

## 强制刷新

可以在菜单中提供：

```text
Reload All Details
Recalculate Disk Usage
```

不要让普通刷新默认触发：

```text
inspect every image
inspect every volume
system df all types
重新创建所有 helper
```

Volumes 菜单可以单独提供：

```text
Recalculate Volume Sizes
```

因为该操作可能较慢。

---

# 二十四、并发和优先级

建议：

```rust
pub enum RequestPriority {
    Interactive,
    VisibleBackground,
    Background,
}
```

## Interactive

* 当前选中详情；
* 当前目录加载；
* 用户主动刷新；
* 用户操作结果。

## VisibleBackground

* 当前列表缺失字段；
* 当前页面 derived data。

## Background

* 其他页面预热；
* 持久化写入；
* 老缓存清理。

调度规则：

```text
Interactive 不得等待 Background semaphore
```

---

# 二十五、取消策略

切换选择时：

* 当前详情请求允许继续进入共享缓存；
* 旧结果不得更新当前详情 UI；
* 不必取消可复用的 inspect；
* UI 使用 generation 检查。

Volume Files：

* 切换卷时取消当前目录 UI 请求；
* session 创建如果即将可复用，可以完成后进入缓存；
* 如果用户已删除卷，则立即清理。

---

# 二十六、持久化写入合并

不要每 patch 一个字段就同步写 SQLite。

使用 Write-Behind：

```text
内存更新
→ 标记 dirty
→ debounce 500–2000ms
→ 单事务批量写入
```

应用退出时：

* 尝试 flush；
* 设置最大等待时间；
* 不因缓存 flush 卡住退出数秒。

SQLite 建议：

```text
WAL mode
busy timeout
批量 transaction
```

缓存损坏时：

```text
删除或重建缓存
继续启动
```

不能让缓存错误导致应用无法打开。

---

# 二十七、缓存版本管理

增加：

```rust
const CACHE_SCHEMA_VERSION: u32 = 1;
```

领域模型变化时：

* 迁移；
* 或清除不兼容缓存；
* 不对旧 payload 直接 unwrap；
* 反序列化失败只丢弃单条数据。

---

# 二十八、监控和性能日志

增加 tracing span：

```text
docker.list_images
docker.inspect_image
docker.list_volumes
docker.system_df_volumes
docker.list_containers
docker.events
cache.load
cache.flush
volume_preview.start_session
volume_preview.list_directory
```

记录：

```text
duration_ms
result_count
cache_hit
cache_source
deduplicated
queue_wait_ms
```

不要记录：

* environment values；
* volume filenames；
* file contents；
* credentials；
* full sensitive labels。

启动 debug 日志示例：

```text
Loaded 4 image summaries from persistent cache in 3 ms
Listed 4 images from Docker in 18 ms
Reused 3 cached image details
Scheduled 1 missing image inspection
Loaded 4 volume summaries from cache in 2 ms
Listed 4 volumes from Docker in 11 ms
Volume disk usage completed in 740 ms
Reused volume preview session for <volume>
```

---

# 二十九、需要重点排查的当前慢点

开始前必须通过 tracing 确认实际耗时，不能盲猜。

检查：

```text
VolumesController.initialize()
VolumeService.list_volumes()
list_containers(all=true)
container inspect loop
system df
volume inspect loop
helper image inspect
helper image pull
helper container create
helper container start
directory list exec
Qt model reset
SQLite/cache I/O
```

特别检查是否存在：

```rust
for item in items {
    inspect(item).await;
}
```

串行 N+1。

改成：

```rust
stream::iter(items)
    .map(...)
    .buffer_unordered(CONCURRENCY)
```

并设置合理并发上限。

---

# 三十、实施阶段

## Phase 1：性能测量

* tracing spans；
* 请求计数；
* cache hit；
* 确认 Images/Volumes 实际瓶颈；
* 写入 baseline 报告。

## Phase 2：Repository 与内存缓存

* ImageRepository；
* VolumeRepository；
* NetworkRepository；
* RequestCoordinator；
* TTL；
* SingleFlight。

## Phase 3：快速 Summary

* 页面基础列表立即显示；
* inspect 后台化；
* volume usage 后台化；
* container relation 后台化。

## Phase 4：持久化缓存

* SQLite；
* endpoint isolation；
* startup hydration；
* write-behind；
* schema version。

## Phase 5：Docker Events

* event monitor；
* debounce；
* targeted invalidation；
* reconnect；
* daemon restart。

## Phase 6：Volume Files Session Cache

* session pool；
* directory LRU；
* TTL；
* helper cleanup；
* warm reuse。

## Phase 7：Qt 增量更新

* patch fields；
* dataChanged；
* preserve selection；
* remove permanent unknown badges。

---

# 三十一、测试要求

## Cache 测试

* cold miss；
* memory hit；
* persistent hit；
* expired cache；
* endpoint isolation；
* schema mismatch；
* corrupted payload；
* cache clear；
* write-behind；
* concurrent reads。

## Request Dedup 测试

* 10 个相同 inspect 只调用 Docker 一次；
* 不同 ID 可并发；
* 失败后允许重试；
* 一个等待者取消不影响其他等待者；
* 结果写入 cache。

## Repository 测试

* summary 先返回；
* detail 后补；
* derived data 后补；
* refresh 不清空旧数据；
* event 触发失效；
* TTL 生效。

## Volumes 测试

* list volumes 不等待 system df；
* list volumes 不等待 container association；
* usage 后台 patch；
* unknown size 不变成 0；
* stopped container 仍计为 in use；
* system df 请求去重；
  -普通刷新不重复计算 volume size。

## Images 测试

* list 完成立即显示；
* architecture 缓存命中；
* 缓存缺失后台 inspect；
* 同一 ID 只 inspect 一次；
* image ID 消失时删除 detail cache；
* container event 更新 in-use。

## Volume Files 测试

* 再次进入复用 session；
* 切换 Info 后 session 暂时保留；
* session TTL 到期清理；
* LRU 上限；
* 当前目录缓存立即显示；
* Refresh 绕过目录 TTL；
* 应用退出清理全部 helper。

## Events 测试

* image create/delete；
* volume create/destroy；
* volume mount/unmount；
* container start/stop/destroy；
* network connect/disconnect；
* event burst debounce；
* stream reconnect；
* daemon restart。

---

# 三十二、人工验收

必须实测以下场景：

1. 冷启动进入 Images。
2. 缓存列表立即出现。
3. architecture 不再全部永久显示 unknown。
4. 缺失 architecture 在后台逐项补齐。
5. 选择已经缓存的镜像详情立即出现。
6. 返回 Images 不重新加载全部详情。
7. 冷启动进入 Volumes。
8. 名称列表先出现。
9. 大小和使用关系随后异步补齐。
10. UI 不等待 system df 才显示列表。
11. 返回 Volumes 不重新扫描。
12. 第一次打开 Files 创建 helper。
13. 切到 Info 再切回 Files，立即显示。
14. 同一个卷不重复创建 helper。
15. 切换卷后旧数据不串入新卷。
16. helper 超时后自动清理。
17. Docker create/delete 操作后列表自动更新。
18. Compose 批量事件不会触发几十次刷新。
19. Docker Engine 停止时保留缓存并显示错误。
20. Docker Engine恢复后自动重新同步。

---

# 三十三、性能报告

完成后必须提供优化前后数据：

```text
Application startup cache hydration
Images initial summary
Images missing-detail prefetch
Image detail cache hit
Volumes initial summary
Container association
Volume system df
Volume preview session cold start
Volume preview session warm reuse
Directory cache hit
```

示例格式：

```text
Images page:
before: 820 ms before first usable list
after cold cache miss: 95 ms
after persistent cache hit: 12 ms

Volumes page:
before: 2.4 s
after summary: 43 ms
usage enrichment: 720 ms background

Volume Files:
cold helper start: 680 ms
warm session reuse: 18 ms
```

必须使用真实测量结果，禁止虚构数据。

---

# 三十四、构建验证

执行：

```bash
cargo fmt --all -- --check
cargo check --workspace
cargo test --workspace
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo build -p tuxstack-gui
cargo run -p tuxstack-gui
```

---

# 三十五、最终报告

完成后输出：

1. 实际性能瓶颈；
2. 优化前测量结果；
3. Repository 架构；
4. 内存缓存设计；
5. 持久化缓存设计；
6. endpoint 隔离方式；
7. SingleFlight 实现；
8. Docker Events 处理；
9. Images architecture 预取方式；
10. Volumes 分阶段加载方式；
11. Volume Size 缓存策略；
12. Volume Files Session Pool；
13. Qt Model 增量更新；
14. 优化后测量结果；
15. 测试结果；
16. Clippy 结果；
17. 所有 commit hash。

最终要求：

```text
列表先出现
详情按需补全
昂贵数据后台刷新
缓存立即复用
Docker Events 保证新鲜度
```

Images 中未选中行不应长期显示假的 `unknown`；Volumes 页面也不应因为容量统计、容器关联或 helper 初始化阻塞基础列表。

[1]: https://docs.docker.com/reference/api/engine/version/v1.54/?utm_source=chatgpt.com "Docker Engine API v1.54 reference | Docker Docs"
[2]: https://docs.docker.com/reference/api/engine/version-history/?utm_source=chatgpt.com "Engine API version history | Docker Docs"
[3]: https://docs.docker.com/reference/cli/docker/system/events/?utm_source=chatgpt.com "docker system events | Docker Docs"

