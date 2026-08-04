# tuxstack-fs-helper 统一迁移实施记录

依据 docs/imagehelper.md 执行。当前状态：迁移进行中。

## 仓库审计（第 1 步）

### Image Files 调用链
- `crates/gui/src/bridge/image_file_bridge.rs` (ImageFileListModelRust) → `ImageFileService` (`crates/docker-core/src/services/image_files.rs`)
- 服务: start_session → create container (entrypoint 清空, cmd sleep) → exec `LIST_SCRIPT/STAT_SCRIPT/PREVIEW_HEAD_SCRIPT` (共享自 volume protocol.rs)
- 模型: `crates/docker-core/src/models/image_file.rs` (ImagePreviewSession/Config/Requests)
- 会话池: `PreviewSessionPool<ImagePreviewSession>` (cache/session_pool.rs), 键 = image_id
- 依赖目标镜像内的 sh/find/stat/base64/readlink/sleep —— 待删除

### Volume Files 调用链
- `crates/gui/src/bridge/volume_file_bridge.rs` → `VolumeFileService` (`crates/docker-core/src/services/volume_files/mod.rs`)
- 服务: ensure alpine:3.20 → create helper container → 卷只读挂到 /volume → exec 同一套脚本
- 协议: `crates/docker-core/src/services/volume_files/protocol.rs` (LIST_SCRIPT/STAT_SCRIPT/PREVIEW_HEAD_SCRIPT, parse_list_line, decode_name, is_known_text_name)
- 模型: `crates/docker-core/src/models/volume_file.rs` (VolumePreviewSession/VolumeFileEntry/VolumePath/VolumeFileType/VolumeHelperConfig)
- 会话池: `PreviewSessionPool<VolumePreviewSession>`, 键 = volume_name
- 依赖 alpine:3.20 且要求用户手动 pull —— 待删除

### 共享
- 会话池/目录缓存: `crates/docker-core/src/cache/session_pool.rs`
- GUI 控制器: `controllers/volume_files.rs` (sort_entries, VolumeFileSortColumn), `controllers/image_files.rs`
- GUI 行模型: `models/volume_file_model.rs` (VolumeFileRow, map_file_row, role ids 257-273)
- GUI 桥声明: `bridge/resource_bridges.rs` (VolumeFileListModel/ImageFileListModel qobject)
- QML: `components/VolumeFilesView.qml` (viewKind volume/image, 字符串覆写), `ImageFilesView.qml`
- 应用状态: `app_state.rs` (两个 PreviewSessionPool), `app_bridge.rs` (孤儿清理)
- 集成测试: `tests/integration/image_files.rs`, `volume_files.rs`
- smoke: `crates/gui/src/smoke_test.rs` (含 fake filesModel fixtures, helper_image_required 状态)

## 目标架构（按 imagehelper.md 二、三节）

- `crates/tuxstack-fs-protocol` — 纯协议 crate（serde, JSON Lines, 路径 token, 错误码）
- `crates/tuxstack-fs-helper` — 静态 musl 二进制 (hold/hello/list/stat/preview/hash/readlink), 依赖 protocol crate
- `crates/docker-core/src/services/filesystem/` — client.rs / session.rs / session_pool.rs / image_provider.rs / volume_provider.rs / helper_image.rs / archive.rs / cache.rs / types.rs / error.rs / mod.rs
- Volume: 内置 scratch Helper 镜像归档（运行时由嵌入字节构造 → docker load），卷挂 /mnt/data
- 路径 token: 版本化 b64, 不透明, UI 不拼接路径
- 取消/超时 → 会话失效 + 删除容器

## 实施顺序
1. ✅ protocol crate (tuxstack-fs-protocol) — JSON Lines 消息, 路径 token, 错误码, base64, 测试全通过
2. ✅ helper 二进制 (tuxstack-fs-helper) — hold/hello/list/stat/preview/hash/readlink, CLI 集成测试 17/17
3. ✅ docker-core filesystem 模块 — types/error/client/session/image_provider/volume_provider/mod.rs
4. ✅ GUI 迁移 — bridges/controllers/models/QML/smoke 已迁移到 FilesystemService
5. ✅ 删除旧实现 — services/image_files.rs, services/volume_files/, models/image_file.rs 已删除; 旧 error variants 已清理
6. ✅ 测试验证 — 101 GUI + 131 docker-core + 22 protocol/helper = 全通过

## 已删除
- ✅ `services/image_files.rs` — shell-based image 浏览服务
- ✅ `services/volume_files/` — shell-based volume 浏览服务 (protocol.rs + mod.rs)
- ✅ `models/image_file.rs` — ImagePreviewSession/ImagePreviewConfig 等旧模型
- ✅ `models/volume_file.rs` 中的旧类型 — VolumeFileEntry/VolumeFileType/VolumePreviewSession/请求类型
- ✅ 旧 error variants — 13 个 Volume-related + 所有 Image-related 旧变体
- ✅ GUI bridge 中的死代码 — map_error 函数和 DockerError 引用
- ✅ 保留: VolumePath (GUI 导航仍需要)
- 保留: PreviewSessionPool (GUI bridges 仍用于缓存 FilesystemSession)
