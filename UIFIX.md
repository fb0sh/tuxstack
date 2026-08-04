# TuxStack Docker Images Regression Fix Prompt

当前 Docker Images 模块在最近一次 UI 优化后出现严重回归。

现在不要继续优化视觉。

第一目标：

**恢复 Images 模块正确工作状态。**

当前问题：

1. 进入 Images 页面不会自动加载 Docker Images。
2. 左侧显示：

```
0 images
0 B total image size
```

即使 Docker 中存在镜像。
3. 页面出现残留 skeleton。
4. 右侧 Detail Panel 整体消失。
5. 三栏布局被破坏。
6. 没有选中 Image 时的 blank 逻辑实现错误。
7. Detail loading、empty、error、none 状态混乱。

本次任务：

恢复正确架构，再继续 KDE 风格优化。

---

# 一、停止继续修改 UI

当前阶段禁止修改：

* PropertyRow 样式
* Section 样式
* KDE spacing
* 颜色
* 卡片设计
* 字体
* 圆角
* 表格布局

先恢复：

* 数据流
* Controller 生命周期
* Model 状态
* Layout 结构
* Detail 生命周期

---

# 二、恢复固定三栏布局

当前错误：

没有选中 Image 时：

```text
Sidebar | Image List | 空白
```

右侧区域完全消失。

正确：

```text
+-------------+----------------+----------------------+
| Sidebar     | Image List     | Image Detail         |
|             |                |                      |
|             |                | blank/detail/error   |
+-------------+----------------+----------------------+
```

右侧 Detail Panel 必须永久存在。

结构：

```qml
RowLayout {

    Sidebar {}

    ImageListPanel {
    }

    ImageDetailPanel {
        Layout.fillWidth: true
    }

}
```

禁止：

```qml
Loader {
    active: selectedImageId != ""
}
```

因为这会导致整个布局节点消失。

正确：

```qml
ImageDetailPanel {
    visible: true

    DetailContent {
        visible: selectedImageId !== ""
    }
}
```

规则：

```
DetailPanel 永远存在

DetailContent 根据状态显示
```

---

# 三、恢复正确页面状态模型

现在的问题：

List 状态和 Detail 状态混在一起。

重新拆分。

## Images List State

Rust：

```rust
pub enum ImagesListState {
    Loading,
    Ready,
    Empty,
    Error,
}
```

表示：

Docker Images 列表状态。

---

## Image Detail State

Rust：

```rust
pub enum ImageDetailState {
    None,
    Loading,
    Ready,
    Error,
}
```

表示：

当前选中的 Image 详情状态。

两个状态完全独立。

---

# 四、正确状态行为

## 状态 1：页面首次打开

流程：

```
ImagesPage created

↓

ImagesController.initialize()

↓

Docker list images

↓

更新 ImageListModel

↓

自动选择第一张 image

↓

inspect image

↓

显示 Detail
```

不能等待：

```
用户点击刷新按钮
```

---

# 五、Images 页面自动初始化

增加：

```rust
ImagesController::initialize()
```

要求：

只执行一次。

例如：

```rust
pub async fn initialize(&mut self) {

    if self.initialized {
        return;
    }

    self.initialized = true;

    self.load_images().await;

}
```

QML：

```qml
Component.onCompleted: {
    imagesController.initialize()
}
```

---

# 六、恢复 Docker Images 加载

检查：

```
ImagesController
Docker service
Bollard service
ImageListModel
```

确认：

调用：

```
docker.images.list()
```

而不是：

```
refresh button
```

触发。

增加调试日志：

启动页面必须看到：

```
ImagesPage created

ImagesController initialized

Loading docker images

Docker returned N images

Updating model

Selecting first image
```

---

# 七、自动选择第一张 Image

列表加载完成：

如果：

```rust
selected_image_id == None
```

执行：

```rust
select_first_image()
```

选择规则：

优先：

```
In Use 第一张
```

否则：

```
Unused 第一张
```

然后：

自动：

```rust
load_image_detail(image_id)
```

---

# 八、正确处理无 Image 状态

Docker 没有镜像：

左侧：

显示：

```
No Docker images found.

Pull an image to get started.
```

右侧：

必须：

```
完全 blank
```

不要：

```
No image selected
```

不要：

```
PlaceholderMessage
```

不要：

```
Empty detail card
```

---

# 九、正确处理有 Image 但未加载详情

流程：

```
Image selected

↓

DetailState = Loading

↓

显示 Detail Skeleton

↓

DetailState = Ready

↓

显示详情
```

Skeleton 只能存在：

```
selectedImageId != null

AND

detailState == Loading
```

禁止：

页面打开时显示 skeleton。

---

# 十、恢复 Detail Panel 内容

确认：

文件：

```
ImageDetailPanel.qml
```

必须恢复。

结构：

```
ImageDetailPanel

 ├── General
 │
 ├── Actions
 │
 ├── Configuration
 │
 ├── Environment
 │
 ├── Labels
 │
 └── Used By
```

没有选中：

```
DetailContent hidden
```

有选中：

```
render detail
```

---

# 十一、修复 List 空白问题

当前：

```
0 images
```

但是没有正确 empty。

检查：

Image model。

确认：

不要：

```rust
unwrap_or_default()
```

吞掉错误。

错误：

```
Docker error
```

不能变成：

```
0 images
```

需要区分：

```
Docker returned empty list

Docker request failed
```

---

# 十二、修复 Total Size 和 Count

当前：

```
0 B total image size
```

检查：

不要依赖：

```
repo tags
```

正确：

唯一 image ID。

例如：

```
ubuntu:latest
ubuntu:22.04
```

同一个 image：

只计算一次。

实现：

```rust
HashSet<String>
```

逻辑：

```rust
for image in images {

    if seen.insert(image.id.clone()) {

        total_size += image.size;

    }

}
```

---

# 十三、恢复 Error 状态

Detail 加载失败：

不要：

整个页面错误。

正确：

左侧：

```
正常
```

右侧：

```
Image details unavailable

Reason

Retry
```

错误类型：

区分：

```
Docker unavailable

Permission denied

Image not found

Timeout
```

---

# 十四、Skeleton 修复

当前灰色块残留。

检查所有：

```
Skeleton
Placeholder
Loading
```

绑定。

规则：

List skeleton：

只绑定：

```
ImagesListState == Loading
```

Detail skeleton：

只绑定：

```
ImageDetailState == Loading
```

Empty:

不显示 skeleton。

Ready:

不显示 skeleton。

---

# 十五、恢复后再做 KDE Detail 优化

当前顺序错误。

正确顺序：

## Phase 1

恢复：

* 自动加载
* 列表
* 自动选择
* Detail
* Empty
* Error

## Phase 2

继续：

* KDE PropertyRow
* Section
* spacing
* table

不要同时修改。

---

# 十六、代码检查

开始修改前：

执行：

```bash
git diff

git status

git log --oneline -10
```

找到最近一次导致 regression 的 commit。

重点检查：

```
ImagesPage.qml

ImageDetailPanel.qml

ImagesController

ImageListModel

ImageDetailLoader
```

---

# 十七、验证要求

完成后必须验证：

## Case 1：Docker 有镜像

打开 Images：

结果：

```
左侧:
真实 image 列表

右侧:
自动显示第一张 image detail
```

---

## Case 2：Docker 无镜像

结果：

```
左侧:
Empty state

右侧:
blank
```

---

## Case 3：点击 Image

结果：

```
Detail loading

↓

Detail ready
```

---

## Case 4：删除当前 Image

结果：

```
刷新列表

自动选择下一张

没有下一张:

右侧 blank
```

---

## Case 5：Docker 服务关闭

结果：

```
左侧:
Docker error

右侧:
保持布局
```

---

# 十八、最终输出报告

完成后报告：

1. Regression 原因。
2. 修改文件。
3. 恢复的状态模型。
4. 页面生命周期。
5. 自动加载实现。
6. Detail Panel 生命周期。
7. Empty / Error / Loading 行为。
8. 数据修复点。
9. 测试结果。
10. commit hash。

---

目标：

恢复一个稳定的 Docker Images 页面。

当前阶段优先级：

```
功能正确性
>
状态模型正确
>
布局稳定
>
KDE 视觉优化
```

不要继续堆 UI 修改。
