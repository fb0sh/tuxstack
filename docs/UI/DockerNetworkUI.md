Network 页面建议沿用 Images 的成熟模式：

* 左侧资源列表
* 中间 Network List
* 右侧 Network Detail
* KDE Plasma / Kirigami 风格
* 真实 Docker Engine 数据
* 不做 mock
* 不做 Kubernetes


---

# TuxStack Docker Network Management UI Implementation Prompt

当前 TuxStack 已完成 Docker Images 页面基础能力，现在实现第二个 Docker 资源模块：

# Docker Network 管理页面

目标：

实现一个接近 Docker Desktop / OrbStack / KDE System Settings 风格的 Network 管理界面。

参考用户提供截图的信息架构：

```text
Sidebar

Docker
 ├── Containers
 ├── Volumes
 ├── Images
 └── Networks


Network Page

┌──────────────┬──────────────────┬──────────────────────────┐
│ Sidebar      │ Network List     │ Network Detail            │
│              │                  │                           │
│              │ bridge           │ General                   │
│              │ br-awd           │ Options                   │
│              │ custom-network   │ Containers                │
│              │                  │                           │
└──────────────┴──────────────────┴──────────────────────────┘
```

---

# 一、实现范围

本次实现：

必须支持：

* 自动加载 Docker Networks
* Network 列表展示
* Network 搜索
* Network 排序
* Network 详情
* 创建 Network
* 删除 Network
* 查看 Network 配置
* 查看连接容器
* 查看 IPAM 信息
* 查看 Options
* 查看 Labels
* 刷新 Network

不实现：

* Kubernetes
* CNI
* Docker Swarm
* Network attach/detach
* 多 Docker Host
* daemon
* REST API

---

# 二、架构要求

继续使用当前架构：

```text
QML
 |
CXX-Qt Controller
 |
docker-core
 |
Bollard
 |
Docker Engine
```

不要新增：

* daemon
* JSON RPC
* REST backend

Docker API 调用必须集中在：

```text
docker-core
```

GUI 不直接调用 Bollard。

---

# 三、页面生命周期

Network 页面进入：

必须自动加载。

流程：

```
NetworksPage created

↓

NetworksController.initialize()

↓

Docker list networks

↓

更新 NetworkListModel

↓

自动选择第一条 Network

↓

加载 Network Detail
```

不要要求用户点击刷新。

QML：

```qml
Component.onCompleted: {
    networksController.initialize()
}
```

---

# 四、页面状态模型

和 Images 保持一致。

## Network List State

Rust：

```rust
pub enum NetworksListState {

    Loading,

    Ready,

    Empty,

    Error,

}
```

---

## Network Detail State

```rust
pub enum NetworkDetailState {

    None,

    Loading,

    Ready,

    Error,

}
```

两个状态独立。

---

# 五、三栏布局

保持固定：

```text
Sidebar

Network List

Network Detail
```

不要因为没有选中 Network 删除 Detail。

正确：

```
NetworkDetailPanel
    永久存在

DetailContent
    根据 selectedNetworkId 显示
```

没有 Network：

右侧：

blank。

不要：

```
No network selected
```

---

# 六、Network List UI

参考截图。

顶部：

```text
Networks

4 total
```

显示：

```text
4 networks
```

不要：

```
undefined network(s)
```

---

工具栏：

包含：

```
Sort
Search
Create Network
Refresh
```

布局：

```
Networks


[sort] [search] [+]
```

使用：

```qml
QQC2.ToolButton

Kirigami.Action
```

---

# 七、Network List Item

创建：

```text
NetworkListItem.qml
```

显示：

```
bridge

192.168.215.0/24
```

结构：

```
Icon

Name

Subnet / driver information
```

示例：

```
bridge
192.168.215.0/24
```

---

属性：

```qml
property string networkId

property string name

property string subnet

property string driver

property bool selected

signal clicked()
signal removeRequested()
```

---

# 八、Network 分组

Network 不需要 In Use / Unused。

按照：

Docker 返回顺序。

默认排序：

```
bridge
host
none
custom networks
```

或者：

```
Name A-Z
```

---

# 九、Network Detail 设计

继续使用 KDE Settings 风格。

不要：

* 大 Card
* Web Dashboard
* 巨大圆角

结构：

```
General


Options


IPAM


Containers


Labels
```

---

# 十、General Section

显示：

```
Name          bridge

ID            118b84da63a4

Created       Jul 22 2026

Driver        bridge

Scope         local

Internal      false

Attachable    false
```

使用：

```
PropertySection

PropertyRow
```

类似 KDE：

```
Key                         Value


Name                        bridge

Driver                      bridge

Scope                       local
```

---

# 十一、Network 数据模型

docker-core 增加：

## NetworkSummary

```rust
pub struct NetworkSummary {

    pub id: String,

    pub short_id: String,

    pub name: String,

    pub driver: String,

    pub scope: String,

    pub created_at: Option<DateTime<Utc>>,

    pub subnet: Option<String>,

    pub gateway: Option<String>,

    pub labels: HashMap<String,String>,

}
```

---

## NetworkDetail

```rust
pub struct NetworkDetail {


    pub summary: NetworkSummary,


    pub internal: bool,

    pub attachable: bool,

    pub ingress: bool,


    pub options: BTreeMap<String,String>,


    pub ipam: NetworkIPAM,


    pub containers: Vec<NetworkContainer>,


}
```

---

## IPAM

```rust
pub struct NetworkIPAM {


    pub driver: Option<String>,


    pub subnets: Vec<NetworkSubnet>,


}
```

---

## NetworkSubnet

```rust
pub struct NetworkSubnet {

    pub subnet: String,

    pub gateway: Option<String>,

}
```

---

# 十二、Bollard 实现

使用：

Docker API：

```
list_networks

inspect_network

create_network

remove_network
```

不要自己解析：

```
docker network ls
```

不要调用 shell。

---

# 十三、Network Service

新增：

```
docker-core/src/services/networks.rs
```

结构：

```rust
pub struct NetworkService {

    client: Arc<DockerClient>

}
```

方法：

```rust
list_networks()

inspect_network()

create_network()

remove_network()
```

---

# 十四、Network Detail 数据加载

列表：

只调用：

```
docker network list
```

不要：

```
N 个 network inspect
```

避免：

N+1。

只有：

用户选择 Network

才：

```
inspect_network(id)
```

---

# 十五、Network Detail 页面

## General

显示：

```
Name

ID

Created

Driver

Scope

Subnet

Gateway
```

---

## Options

如果：

options 非空。

显示：

```
Key                         Value


com.docker.network.bridge.default_bridge

true


com.docker.network.bridge.name

docker0
```

如果为空：

显示：

```
No network options.
```

不要显示空表格。

---

# 十六、IPAM

显示：

```
IPAM


Driver

bridge


Subnets


192.168.215.0/24

Gateway:

192.168.215.1
```

支持多个 subnet。

---

# 十七、Containers

显示连接该 Network 的容器。

数据来源：

Network inspect:

```
Containers
```

显示：

```
Container Name

Container ID

IPv4 Address

IPv6 Address

Endpoint ID
```

例如：

```
web

a12345

172.18.0.2
```

---

空：

显示：

```
No containers attached.
```

---

# 十八、Labels

和 Images 保持一致。

表格：

```
Key                         Value
```

排序：

Key A-Z。

空：

```
No labels.
```

---

# 十九、创建 Network

点击：

```
+
```

打开：

```text
CreateNetworkDialog.qml
```

字段：

```
Name

Driver

Subnet

Gateway

IPv6

Internal

Attachable

Labels
```

---

默认：

Name:

必须填写。

Driver:

默认：

```
bridge
```

支持：

```
bridge

overlay

macvlan
```

如果 Docker 不支持：

显示错误。

---

创建：

调用：

```rust
create_network()
```

成功：

```
refresh list

select new network
```

---

# 二十、删除 Network

列表每项：

删除按钮。

点击：

确认：

```
Remove network bridge?
```

显示：

```
Connected containers:

3
```

如果：

network 被使用。

Docker 返回错误：

显示：

```
Network is currently in use.
Remove containers or disconnect them first.
```

不要自动删除容器。

---

# 二十一、搜索

搜索：

范围：

```
name

id

driver

subnet

labels
```

本地过滤。

不要每次请求 Docker。

---

# 二十二、排序

支持：

```
Name A-Z

Name Z-A

Created newest

Created oldest

Driver
```

---

# 二十三、错误处理

区分：

```
Docker unavailable

Permission denied

Network not found

Network in use

Invalid network config

Timeout
```

GUI 显示友好错误。

---

# 二十四、KDE 风格要求

严格使用：

```qml
Kirigami.Theme

Kirigami.Units
```

不要：

```
固定颜色

紫色

巨大卡片

阴影

Material Design
```

参考：

KDE System Settings。

---

Detail 页面：

使用：

```
Section

PropertyRow

Table
```

不要：

```
Card inside Card
```

---

# 二十五、文件规划

docker-core:

```
src/models/network.rs

src/services/networks.rs

src/mapping/network.rs
```

GUI:

```
qml/pages/NetworksPage.qml

qml/components/NetworkListItem.qml

qml/components/NetworkDetailPanel.qml

qml/dialogs/CreateNetworkDialog.qml

src/controllers/networks.rs

src/models/network_model.rs
```

根据当前项目结构调整。

---

# 二十六、测试

## Unit Test

测试：

* network mapping
* subnet parsing
* gateway parsing
* options mapping
* labels mapping
* container mapping
* error mapping

---

## Docker Integration Test

使用：

```
#[ignore]
```

流程：

1. 创建测试 network
2. inspect
3. 创建容器连接
4. 验证 Containers
5. 删除 network
6. 清理资源

---

# 二十七、验收

最终效果：

打开 Networks：

自动显示：

```
bridge

host

none

custom networks
```

点击：

bridge

右侧：

```
General

Name          bridge

ID            xxxx

Driver        bridge

Scope         local


Options

...


IPAM

...


Containers

...


Labels

...
```

---

验证：

* 不需要点击刷新
* 没有 mock
* 没有 placeholder 空详情
* 没有 skeleton 残留
* KDE 风格一致
* Light/Dark 正常
* Docker 数据真实

---

# 二十八、提交要求

完成后输出：

1. 修改文件列表
2. Network 数据模型
3. Docker API 使用情况
4. Controller 状态设计
5. UI 组件结构
6. 创建 Network 流程
7. 删除 Network 流程
8. 测试结果
9. commit hash

Commit:

```
feat(docker-core): add network service

feat(gui): add docker network management page

feat(gui): add create and remove network workflow

test(network): add network integration tests
```

---

目标：

实现一个真正可用的 Docker Network 管理页面，达到当前 Images 模块同等成熟度，并保持 TuxStack KDE 原生桌面应用定位。
