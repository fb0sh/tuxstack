你需要重构 TuxStack 当前的 Image Files 和 Volume Files 浏览实现，建立一套统一、可靠、可扩展的文件系统浏览基础设施。

请先完整检查仓库中与以下内容有关的代码，再开始修改：

* `image_files`
* `volume_files`
* `protocol.rs`
* `ImagePreviewSession`
* `VolumePreviewSession`
* `exec_collect`
* `LIST_SCRIPT`
* `STAT_SCRIPT`
* `PREVIEW_HEAD_SCRIPT`
* Docker 容器创建、启动、删除、Archive API、镜像检查相关代码
* Session 池、TTL、LRU、缓存、孤儿容器清理相关代码
* 前端或 IPC 中与 Image Files、Volume Files 相关的请求和响应模型

不要只在现有实现上继续增加 Shell 脚本。目标是完成正式架构迁移。

# 一、当前背景

目前 Image Files 和 Volume Files 使用相似的 Docker Exec 协议，但运行环境不同。

## Image Files 当前方式

* 直接使用目标镜像创建预览容器；
* 容器 rootfs 就是要浏览的镜像文件系统；
* 使用镜像内部的 `sh`、`find`、`stat`、`base64`、`readlink` 等工具；
* 通过 `docker exec sh -c <script>` 实现：

  * 列目录；
  * 文件属性；
  * 文本预览；
  * 下载等操作；
* 目标镜像没有 shell 或相关命令时，浏览失败。

## Volume Files 当前方式

* 卷本身不能直接启动；
* 使用固定的 `alpine:3.20` 作为 Helper 镜像；
* 将卷只读挂载到 `/volume`；
* 通过 Alpine 中的 shell 和工具执行与 Image Files 相同的脚本；
* 如果本地没有 `alpine:3.20`，提示用户手动 Pull；
* Session 按卷名缓存，支持 TTL、LRU、目录缓存和孤儿容器清理。

# 二、最终目标

实现下面的统一架构：

```text
                       tuxstack-fs-helper
                    同一个静态 Rust 二进制
                              │
             ┌────────────────┴────────────────┐
             │                                 │
        Image Files                       Volume Files
             │                                 │
使用目标镜像创建容器                     使用专用 Helper 镜像
将 helper 注入容器可写层                 镜像内已包含相同 helper
浏览根目录：/                            卷挂载到 /mnt/data
```

统一的内容包括：

* Helper 二进制；
* Helper 协议；
* 文件类型和响应模型；
* Docker Exec 客户端；
* JSON Lines 解析；
* 路径 Token；
* 非 UTF-8 文件名处理；
* 列目录、属性、预览、Hash、符号链接处理；
* 错误码；
* 超时和取消；
* Session 公共生命周期；
* 目录缓存模型。

不同的内容只包括：

* Image Session 如何创建；
* Volume Session 如何创建；
* 文件系统根目录；
* Session 缓存键；
* Helper 的分发方式。

不要错误地尝试把 Image rootfs 挂载到另一个 Helper 容器中。Docker 没有稳定、跨存储驱动的通用接口可以把任意镜像 rootfs 当普通卷挂载。

# 三、代码模块设计

请结合仓库当前结构调整实际路径，但最终职责应接近：

```text
crates/
├── tuxstack-fs-protocol/
│   └── src/
│       └── lib.rs
│
└── tuxstack-fs-helper/
    └── src/
        ├── main.rs
        ├── hold.rs
        ├── list.rs
        ├── stat.rs
        ├── preview.rs
        ├── hash.rs
        ├── path.rs
        ├── metadata.rs
        └── error.rs

src/docker/filesystem/
├── mod.rs
├── client.rs
├── types.rs
├── session.rs
├── session_pool.rs
├── image_provider.rs
├── volume_provider.rs
├── helper_image.rs
├── archive.rs
├── cache.rs
└── error.rs
```

如果仓库已有更合适的模块结构，可以适配现有结构，但必须保持职责清晰。

## `tuxstack-fs-protocol`

这是一个纯协议 crate，同时被主程序和 Helper 使用。

不得依赖 Docker、Qt、Tokio 或平台 UI。

应包含：

* 协议版本；
* 请求类型；
* 响应类型；
* 文件类型枚举；
* 错误码；
* Base64 字节字段类型；
* JSON Lines 序列化和反序列化所需结构。

建议：

```rust
pub const FS_HELPER_PROTOCOL_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HelperMessage {
    Hello {
        protocol: u32,
        helper_version: String,
    },
    Entry {
        name_b64: String,
        path_token: String,
        file_type: HelperFileType,
        size: Option<u64>,
        mtime: Option<i64>,
        mode: Option<u32>,
        uid: Option<u32>,
        gid: Option<u32>,
        symlink_target_b64: Option<String>,
        readable: bool,
    },
    Stat {
        path_token: String,
        file_type: HelperFileType,
        size: Option<u64>,
        mtime: Option<i64>,
        mode: Option<u32>,
        uid: Option<u32>,
        gid: Option<u32>,
        symlink_target_b64: Option<String>,
        readable: bool,
    },
    PreviewChunk {
        data_b64: String,
        offset: u64,
        eof: bool,
        truncated: bool,
    },
    Hash {
        algorithm: String,
        value: String,
    },
    End {
        truncated: bool,
        next_cursor: Option<String>,
    },
    Error {
        code: HelperErrorCode,
        message: String,
    },
}
```

协议中不要直接传递未经编码的 Linux 文件名。

# 四、统一 Helper 二进制

实现一个静态 Rust 二进制：

```text
tuxstack-fs-helper
```

至少支持：

```text
tuxstack-fs-helper hold
tuxstack-fs-helper hello
tuxstack-fs-helper list
tuxstack-fs-helper stat
tuxstack-fs-helper preview
tuxstack-fs-helper hash
```

可选支持：

```text
tuxstack-fs-helper readlink
```

未来写操作可以扩展：

```text
mkdir
rename
remove
write
chmod
chown
```

本次先确保只读功能完整，不要为了未来功能过度设计。

## `hold`

作为容器 PID 1 保活：

```text
tuxstack-fs-helper hold
```

要求：

* 正确处理 SIGTERM；
* 收到停止信号后干净退出；
* 不忙轮询；
* 不产生无意义日志；
* 不启动网络服务；
* 不监听 Socket；
* 不修改被浏览的文件系统。

## `hello`

输出单行 JSON：

```json
{"kind":"hello","protocol":1,"helper_version":"0.1.0"}
```

主程序创建 Session 后必须验证：

* 协议版本匹配；
* Helper 可执行；
* 输出能正确解析。

版本不兼容时返回明确错误，不允许继续执行。

## `list`

建议参数：

```text
tuxstack-fs-helper list \
  --root /mnt/data \
  --path-token <token> \
  --show-hidden false \
  --limit 1000 \
  --cursor <optional>
```

Image Files 使用：

```text
--root /
```

Volume Files 使用：

```text
--root /mnt/data
```

要求：

* 只读取当前目录一层；
* 不递归；
* 不跟随符号链接；
* 使用 `symlink_metadata`；
* 目录大小返回 `None`；
* 对普通文件返回大小；
* 返回 mode、uid、gid、mtime、readable；
* 返回符号链接目标的原始字节编码；
* 支持隐藏文件过滤；
* 支持最大条目数；
* 支持稳定排序；
* 支持分页 Cursor；
* 最后一行必须输出 `End`；
* 单个条目失败时不要静默丢弃。

输出示例：

```json
{"kind":"entry","name_b64":"Y29uZmln","path_token":"...","file_type":"directory","size":null,"mtime":1722000000,"mode":493,"uid":0,"gid":0,"symlink_target_b64":null,"readable":true}
{"kind":"entry","name_b64":"YXBwLmxvZw==","path_token":"...","file_type":"file","size":2048,"mtime":1722000001,"mode":420,"uid":1000,"gid":1000,"symlink_target_b64":null,"readable":true}
{"kind":"end","truncated":false,"next_cursor":null}
```

## `stat`

读取路径本身的 metadata，不跟随最终符号链接。

应明确区分：

* 不存在；
* 权限不足；
* 非法 Token；
* 路径逃逸；
* I/O 错误。

## `preview`

支持按偏移量读取有限字节：

```text
tuxstack-fs-helper preview \
  --root / \
  --path-token <token> \
  --offset 0 \
  --limit 65536
```

要求：

* 不一次性读取完整文件；
* 限制最大读取量；
* 返回 Base64 内容；
* 返回 `eof`；
* 返回 `truncated`；
* 目录返回明确错误；
* 特殊设备、FIFO、Socket 默认拒绝预览；
* 避免读取可能永久阻塞的文件类型。

## `hash`

支持：

```text
sha256
```

可选支持：

```text
sha1
md5
```

要求流式计算，禁止把整个文件读入内存。

# 五、路径和非 UTF-8 文件名

Linux 文件名不保证是 UTF-8，因此不要继续把文件名和真实路径全部建模成 `String`。

必须区分：

```rust
pub struct FileName {
    pub raw: Vec<u8>,
    pub display: String,
}
```

其中：

```rust
display = String::from_utf8_lossy(&raw).into_owned();
```

UI 只展示 `display`，后续操作必须依赖原始字节或不透明 Token。

推荐使用不透明路径 Token：

```rust
pub struct FilesystemPathToken(String);
```

原则：

* UI 不自行拼接真实路径；
* UI 不传 `/parent/name`；
* Helper 返回每个条目的 `path_token`；
* 用户打开、预览、下载某个条目时，原样传回该 Token；
* Token 必须包含路径原始字节；
* Token 必须经过版本化编码；
* Helper 解码后进行根目录限制校验。

至少防止：

* `..` 路径穿越；
* 绝对路径覆盖 root；
* NUL 字节；
* 符号链接逃逸；
* 非 UTF-8 丢失；
* 文件名中的换行和控制字符造成协议混乱。

对于只读操作，可以使用规范化的组件 Token。

对于未来写操作，优先考虑基于根目录 fd 的：

```text
openat
fstatat
readlinkat
unlinkat
renameat2
```

Linux 可用时，可进一步使用：

```text
openat2
RESOLVE_BENEATH
RESOLVE_NO_MAGICLINKS
```

不要直接信任从 UI 传入的字符串路径。

# 六、Image Session Provider

实现：

```rust
ImageSessionProvider
```

职责仅包括：

* 检查目标 Image；
* 获取不可变 Image ID；
* 检查 Image 架构；
* 选择匹配架构的 Helper；
* 创建预览容器；
* 注入 Helper；
* 启动 Helper；
* 返回统一的 `FilesystemSession`。

流程：

```text
1. inspect image
2. 获取 immutable image ID
3. 获取 architecture/os
4. 创建目标镜像容器
5. 覆盖原 EntryPoint 和 Cmd
6. 创建后、启动前通过 put_archive 注入 helper
7. 启动容器
8. 执行 hello
9. 验证协议
10. Session 可用
```

容器中 Helper 路径建议：

```text
/.tuxstack/tuxstack-fs-helper
```

创建容器时：

```rust
entrypoint: Some(vec![
    "/.tuxstack/tuxstack-fs-helper".into(),
]),
cmd: Some(vec!["hold".into()]),
```

Docker 允许先创建容器，再通过 Archive API 写入可写层，然后启动。

注入目录：

```text
/.tuxstack/
```

必须在 Image Files UI 中隐藏，防止用户误认为它属于原始镜像。

Image Session 的浏览根目录：

```text
/
```

注意 Docker 运行时可能注入：

```text
/proc
/sys
/dev
/run
/etc/hosts
/etc/hostname
/etc/resolv.conf
```

至少要做到：

* 隐藏 `/.tuxstack`；
* 对运行时挂载和运行时生成文件建立明确策略；
* 不要把 `/proc`、`/sys`、`/dev` 当作镜像真实内容递归浏览；
* 可以在后端维护默认排除列表；
* 代码中写清楚这些路径属于容器运行时视图，不完全等于原始 Image Layer。

不要依赖目标镜像中的：

```text
/bin/sh
find
stat
base64
readlink
sleep
```

迁移完成后，Image Files 必须能支持：

* scratch；
* distroless；
* 没有 Shell 的静态应用镜像；
* 极简镜像。

前提是目标架构能够在当前 Docker daemon 上执行 Helper。

# 七、Volume Session Provider

实现：

```rust
VolumeSessionProvider
```

Volume 使用专用 Helper 镜像，但镜像中包含的必须是与 Image 注入相同的二进制。

Helper 镜像：

```dockerfile
FROM scratch

COPY tuxstack-fs-helper /usr/bin/tuxstack-fs-helper

ENTRYPOINT ["/usr/bin/tuxstack-fs-helper"]
CMD ["hold"]
```

卷挂载：

```rust
Mount {
    typ: Some(MountType::VOLUME),
    source: Some(volume_name.into()),
    target: Some("/mnt/data".into()),
    read_only: Some(true),
}
```

Volume 浏览根目录：

```text
/mnt/data
```

不再使用：

```text
alpine:3.20
```

不再要求用户手动执行：

```text
docker pull alpine:3.20
```

不再依赖 Alpine、BusyBox 或外部发行版。

# 八、Helper 镜像分发

不要把 Helper 镜像放到公开 Registry 作为唯一依赖。

推荐在 TuxStack 安装包中携带 Helper OCI 镜像归档：

```text
resources/helpers/
├── linux-amd64/
│   ├── tuxstack-fs-helper
│   └── tuxstack-fs-helper-image.tar.zst
└── linux-arm64/
    ├── tuxstack-fs-helper
    └── tuxstack-fs-helper-image.tar.zst
```

同一个编译出的二进制同时用于：

* Image Session 注入；
* Volume Helper OCI 镜像。

第一次浏览 Volume 时：

```text
1. inspect helper image
2. 检查标签和版本
3. 不存在或版本不匹配
4. 从 TuxStack 资源中解压 OCI/Docker archive
5. 通过 Docker images/load API 流式导入 daemon
6. 再创建 Volume Session
```

这不是网络操作，不需要用户提前 Pull。

Helper 镜像标签建议：

```text
io.tuxstack.internal=true
io.tuxstack.purpose=filesystem-helper
io.tuxstack.protocol=1
io.tuxstack.version=<version>
io.tuxstack.architecture=<arch>
```

内部镜像名称示例：

```text
tuxstack.internal/fs-helper:1-amd64
tuxstack.internal/fs-helper:1-arm64
```

不要使用 `latest` 作为唯一版本标识。

# 九、统一 Session 模型

建立公共模型：

```rust
pub struct FilesystemSession {
    pub container_id: String,
    pub source: FilesystemSource,
    pub root: String,
    pub helper_path: String,
    pub protocol_version: u32,
    pub helper_version: String,
    pub read_only: bool,
    pub created_at: Instant,
    pub last_used_at: Instant,
}
```

来源：

```rust
pub enum FilesystemSource {
    Image {
        image_id: String,
        platform: String,
    },
    Volume {
        volume_name: String,
    },
}
```

缓存键：

```rust
pub enum FilesystemSessionKey {
    Image {
        image_id: String,
        platform: String,
        helper_version: String,
    },
    Volume {
        volume_name: String,
        read_only: bool,
        helper_version: String,
    },
}
```

不要按 Image Tag 缓存：

```text
ubuntu:latest
```

必须使用不可变 Image ID：

```text
sha256:...
```

公共 Session 池负责：

* 最大 Session 数；
* TTL；
* LRU；
* 活跃引用；
* 健康检查；
* Docker daemon 断开后的失效；
* Docker context 切换后的清理；
* 取消或超时后的销毁；
* 应用启动时孤儿容器清理。

Image 和 Volume Provider 只实现：

```rust
async fn create_session(...)
async fn validate_source(...)
```

# 十、统一文件系统客户端

建立一个公共客户端，例如：

```rust
pub struct FilesystemHelperClient;
```

统一提供：

```rust
async fn list_directory(...)
async fn stat(...)
async fn preview(...)
async fn hash(...)
async fn hello(...)
```

入口模型：

```rust
pub struct ListDirectoryRequest {
    pub path_token: FilesystemPathToken,
    pub show_hidden: bool,
    pub limit: usize,
    pub cursor: Option<String>,
}
```

响应：

```rust
pub struct ListDirectoryResult {
    pub entries: Vec<FilesystemEntry>,
    pub truncated: bool,
    pub next_cursor: Option<String>,
}
```

不要再返回：

```rust
Result<Vec<VolumeFileEntry>, DockerError>
```

因为调用方必须知道是否截断，以及下一页 Cursor。

统一文件条目：

```rust
pub struct FilesystemEntry {
    pub name_raw: Vec<u8>,
    pub display_name: String,
    pub path_token: FilesystemPathToken,
    pub entry_type: FilesystemEntryType,
    pub size_bytes: Option<u64>,
    pub modified_at: Option<DateTime<Utc>>,
    pub mode: Option<u32>,
    pub uid: Option<u32>,
    pub gid: Option<u32>,
    pub symlink_target_raw: Option<Vec<u8>>,
    pub symlink_target_display: Option<String>,
    pub readable: bool,
    pub hidden: bool,
}
```

如果 UI 现有接口必须使用 `VolumeFileEntry`，请重命名为更通用的：

```text
FilesystemEntry
DockerFilesystemEntry
```

不要让 Image Files 继续使用名为 `VolumeFileEntry` 的公共模型。

# 十一、Docker Exec 和 JSON Lines

Helper stdout 只输出协议 JSON Lines。

stderr 只输出诊断日志，不得混入 stdout。

主程序必须逐行流式解析，不要先把无限输出完整收集到一个大字符串。

推荐：

```text
Docker Exec Stream
    ↓
按行读取 stdout
    ↓
serde_json::from_slice
    ↓
HelperMessage
```

要求：

* 限制单行最大长度；
* 限制响应总字节数；
* 限制最大 Entry 数；
* 检查必须收到 `End`；
* 收到未知协议版本时报错；
* JSON 解析错误包含上下文，但不要把任意二进制完整写入日志；
* stderr 只截取有限长度用于错误诊断；
* Docker multiplexed stream 必须正确区分 stdout/stderr。

取消或超时时，Docker Exec 缺少可靠的“只杀当前 exec 进程”接口，因此采用：

```text
取消或超时
    → 标记 Session 无效
    → 删除整个预览容器
    → 下次请求重新创建 Session
```

不要只取消 Rust Future 后保留失控的容器内命令。

# 十二、安全配置

Image 和 Volume Session 都要应用统一安全策略。

建议：

```text
network_mode = none
cap_drop = ALL
no-new-privileges = true
restart_policy = no
pids_limit = 64
memory = 128 MiB
cpu quota ≈ 0.25 CPU
nofile ulimit
auto_remove = false
```

可以按现有代码适配。

Volume Session：

* 卷默认只读；
* rootfs 可以只读；
* 必要目录使用 tmpfs；
* Helper 本身不需要修改卷。

Image Session：

* Helper 需要先写入容器可写层，因此不能在注入前设置成完全不可写；
* 镜像底层 Layer 本身仍是只读；
* 不允许执行镜像原始 EntryPoint；
* 不允许连接网络；
* 不允许继承镜像中的危险 capabilities；
* 不允许使用 privileged；
* 不允许挂载 Docker socket；
* 不允许挂载宿主机目录。

Helper 只负责文件读取，不执行目标文件系统中的任何程序。

# 十三、下载与大文件处理

目录列表、属性、小段预览和 Hash 使用 Helper Exec。

对于完整文件下载，不要继续使用：

```text
exec_collect
Base64 整个文件
```

完整下载优先使用 Docker Archive API：

```text
GET /containers/{id}/archive
```

路径：

* Image：目标路径；
* Volume：`/mnt/data/...`。

主程序应流式读取 tar：

* 不把整个 tar 放入内存；
* 验证返回条目；
* 单文件下载时提取正确条目；
* 目录下载时可以保存 tar 或流式解包；
* 支持取消；
* 限制异常 tar header；
* 防止解包路径穿越。

如果现阶段下载重构范围过大，可以先保留旧下载通道，但必须：

* 在代码中隔离；
* 不再依赖 Shell；
* 不能阻塞本次统一 Helper 迁移；
* 留下明确 TODO 和测试。

# 十四、目录缓存

Image 内容在 Session 生命周期内基本稳定，但容器可写层可能存在 Helper 自身文件。

Volume 可能同时被其他业务容器修改，所以缓存只能是短 TTL。

统一缓存 Key：

```rust
pub struct DirectoryCacheKey {
    pub source_key: FilesystemSourceKey,
    pub path_token: FilesystemPathToken,
    pub show_hidden: bool,
    pub limit: usize,
    pub cursor: Option<String>,
    pub sort: SortMode,
}
```

要求：

* 默认 TTL 可保持 5 秒；
* 提供强制刷新；
* Session 销毁时清空相关缓存；
* Volume 后续写操作完成后立即清空该卷缓存；
* Helper 版本变化时清空缓存；
* Image ID 变化时不能命中旧缓存。

# 十五、错误模型

建立明确错误类型，至少包含：

```rust
pub enum FilesystemError {
    SourceNotFound,
    ImageNotFound,
    VolumeNotFound,
    UnsupportedPlatform,
    HelperBinaryUnavailable,
    HelperImageLoadFailed,
    HelperContainerCreateFailed,
    HelperContainerStartFailed,
    HelperHandshakeFailed,
    HelperProtocolMismatch,
    HelperProtocolError,
    InvalidPathToken,
    PathEscapeRejected,
    PathNotFound,
    NotDirectory,
    IsDirectory,
    PermissionDenied,
    UnsupportedFileType,
    ResponseTooLarge,
    DirectoryEntryLimitExceeded,
    ExecFailed,
    Timeout,
    Cancelled,
    SessionInvalidated,
    DockerUnavailable,
}
```

错误应映射到稳定的 IPC 错误码，不要让前端依赖英文错误文本匹配。

删除旧错误：

```text
VolumePreviewHelperImageMissing(alpine:3.20)
ImageShellUnsupported
```

替换为统一且准确的错误，例如：

```text
helper_binary_unavailable
helper_image_load_failed
unsupported_platform
helper_protocol_mismatch
```

# 十六、多架构

至少支持：

```text
linux/amd64
linux/arm64
```

编译目标建议：

```text
x86_64-unknown-linux-musl
aarch64-unknown-linux-musl
```

创建 Session 前读取 Docker daemon 和目标 Image 架构。

Image Session：

* Helper 架构必须与容器可执行平台匹配；
* 如果 daemon 已配置 binfmt/QEMU，可以允许跨架构；
* 没有可执行支持时返回 `UnsupportedPlatform`；
* 不要出现模糊的 `exec format error` 直接暴露给 UI。

Volume Session：

* 根据 Docker daemon 架构选择对应 Helper 镜像。

构建系统中应保证：

* Helper 二进制；
* Helper OCI 镜像；
* 版本标签；
* SHA-256 校验；
* 应用内资源；

来自同一次构建。

# 十七、迁移要求

迁移完成后删除以下旧实现：

* `LIST_SCRIPT`
* `STAT_SCRIPT`
* `PREVIEW_HEAD_SCRIPT`
* Shell 分隔协议；
* `parse_list_line`
* `decode_name` 中以 UTF-8 String 为唯一结果的旧逻辑；
* Image Files 对目标镜像 Shell 的依赖；
* Volume Files 对 `alpine:3.20` 的依赖；
* 提示用户 Pull Alpine 的错误和 UI；
* Image 和 Volume 重复的 Exec 解析代码；
* Image 和 Volume 重复的 Session 生命周期代码。

不要保留：

```text
legacy
fallback_shell
old_protocol
compat_mode
```

除非某一项确实无法在本次改动中迁移，并且会阻塞已有功能。即便如此，也必须把兼容路径限制在单独模块，并在最终报告中明确说明原因。

优先完成完整替换，不要长期维护两套协议。

# 十八、实施顺序

按照以下顺序实施：

## 第 1 步：仓库审计

输出当前实现关系：

* Image Files 调用链；
* Volume Files 调用链；
* 共享协议；
* Session 池；
* IPC 模型；
* UI 依赖；
* 下载和预览路径；
* 测试覆盖。

在代码修改前形成简短实施记录，但不要停下来等待确认。

## 第 2 步：新增协议 crate

* 建立 `tuxstack-fs-protocol`；
* 定义版本、消息、错误码和文件类型；
* 加单元测试；
* 确保主程序和 Helper 都能依赖。

## 第 3 步：实现 Helper

先实现：

```text
hold
hello
list
stat
preview
hash
```

为每个命令增加单元测试和临时目录集成测试。

必须测试：

* 普通文件；
* 空目录；
* 隐藏文件；
* 空格；
* Tab；
* 换行；
* 非 UTF-8 文件名；
* 符号链接；
* 断链；
* 权限不足；
* 超大目录；
* Cursor；
* 截断；
* FIFO、Socket、设备文件拒绝；
* 路径穿越。

## 第 4 步：统一 Helper Client

* 实现 JSONL 流式解析；
* 实现 hello 握手；
* 实现 limit；
* 实现超时和取消；
* 实现 Session 失效；
* 替代 Shell 协议解析器。

## 第 5 步：Image Provider

* 创建目标镜像容器；
* 覆盖 Entrypoint；
* Archive 注入 Helper；
* 启动；
* hello；
* 支持 scratch/distroless 测试镜像。

## 第 6 步：Volume Provider

* 构建专用 scratch Helper 镜像；
* 内置镜像归档；
* 自动 Load；
* 卷挂载到 `/mnt/data`；
* 删除 Alpine 依赖。

## 第 7 步：统一 Session Pool

* 提取公共生命周期；
* 分别保留 Image/Volume 缓存键；
* 迁移 TTL、LRU、最大数量；
* 迁移孤儿清理。

## 第 8 步：迁移 IPC 和 UI

* 使用统一 `FilesystemEntry`；
* 返回 truncated 和 next_cursor；
* UI 继续区分 Image 和 Volume 页面；
* 删除 Alpine Pull 提示；
* 增加 Helper 安装或加载失败提示；
* 保持用户交互不倒退。

## 第 9 步：删除旧代码

确认没有调用后，彻底删除旧 Shell 脚本、旧解析器和旧错误。

## 第 10 步：文档与验证

更新：

* 架构文档；
* Helper 协议说明；
* 构建说明；
* 多架构说明；
* 安全模型；
* Session 生命周期；
* 故障排查说明。

# 十九、测试要求

至少增加以下测试。

## 协议测试

* 所有消息 JSON 往返；
* 未知字段兼容；
* 未知协议版本拒绝；
* Error Code 稳定；
* 超长行拒绝。

## Helper 测试

* 列目录；
* 属性；
* 预览；
* Hash；
* Cursor；
* 非 UTF-8；
* 符号链接；
* 权限；
* 路径逃逸；
* 特殊文件；
* 大目录截断。

## Image 集成测试

准备测试镜像：

```text
普通 Alpine 镜像
无 Shell 的 scratch 镜像
distroless 镜像
包含特殊文件名的镜像
包含符号链接的镜像
```

验证：

* 不依赖镜像内部命令；
* 能正确浏览；
* `/.tuxstack` 不出现在 UI；
* 网络关闭；
* 原 Entrypoint 不执行；
* Session 清理正确。

## Volume 集成测试

验证：

* 不安装 Alpine 也可浏览；
* Helper 镜像自动 Load；
* 卷只读挂载；
* 不能写入；
* 业务容器写入后刷新可见；
* TTL、LRU 和缓存正确；
* Helper 容器删除后卷内容不受影响。

## 取消和故障测试

* list 操作取消；
* Helper 卡住；
* Docker daemon 中断；
* Session 容器被外部删除；
* Helper 协议版本错误；
* 镜像架构不匹配；
* Helper 镜像损坏；
* 应用重启后孤儿容器清理。

# 二十、验收标准

只有满足以下条件才算完成：

1. Image Files 和 Volume Files 使用同一个 `tuxstack-fs-helper` 二进制。
2. 主程序和 Helper 使用同一个协议 crate。
3. Image Files 不依赖目标镜像中的 Shell 或 Unix 工具。
4. scratch 和 distroless 镜像可以浏览。
5. Volume Files 不依赖 `alpine:3.20`。
6. 用户无需手动 Pull Helper 镜像。
7. Helper 镜像可由应用内置资源自动导入 Docker daemon。
8. Image 和 Volume 只在 Session Provider 和 root 路径上存在差异。
9. 目录结果返回 `truncated` 和 `next_cursor`。
10. 非 UTF-8 文件名不会因为转换成 String 而丢失。
11. 路径无法逃逸指定 root。
12. Docker Exec 输出采用 JSON Lines。
13. 取消或超时会使 Session 失效并清理容器。
14. Session 池、TTL、LRU 和孤儿清理仍然有效。
15. Image 和 Volume 的重复文件协议代码被删除。
16. 旧 Shell 脚本、Alpine Helper 和旧解析器被删除。
17. 所有现有相关测试通过。
18. 新增协议、Helper、Image 和 Volume 集成测试通过。
19. `cargo fmt`、`cargo clippy` 和完整测试通过。
20. 不引入 privileged、Docker socket 挂载或宿主机直接卷路径访问。

# 二十一、最终输出要求

完成后给出：

1. 实际修改的文件列表；
2. 新架构说明；
3. Image Session 创建流程；
4. Volume Session 创建流程；
5. Helper 分发方式；
6. 协议消息示例；
7. 删除的旧实现；
8. 安全限制；
9. 测试命令和测试结果；
10. 尚未完成的事项。

不要只给设计建议。直接修改代码、完成迁移、运行测试并修复问题。

不要为了减少改动量保留旧实现。优先保证最终结构清晰、统一、可维护。
