//! Image, network, and volume list models.
//!
//! These are read-mostly list models with a single `refresh` invokable
//! each; Docker I/O runs on the Tokio runtime.

use std::pin::Pin;

use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::{QModelIndex, QString, QVariant};

use crate::app_state::{ImageRow, NetworkRow, VolumeRow, get_services, map_docker_error};

/// Build a QVariant from a string (String → QString → QVariant).
fn qv(s: &str) -> QVariant {
    QVariant::from(&QString::from(s))
}

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!(< QAbstractListModel >);
        type QAbstractListModel;

        include!("cxx-qt-lib/qmodelindex.h");
        type QModelIndex = cxx_qt_lib::QModelIndex;

        include!("cxx-qt-lib/qvariant.h");
        type QVariant = cxx_qt_lib::QVariant;

        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;

        include!("cxx-qt-lib/qhash.h");
        type QHash_i32_QByteArray = cxx_qt_lib::QHash<cxx_qt_lib::QHashPair_i32_QByteArray>;
    }

    impl cxx_qt::Threading for ImageListModel {}

    unsafe extern "RustQt" {
        /// Image list model.
        #[qobject]
        #[qml_element]
        #[base = QAbstractListModel]
        #[qproperty(QString, search_text)]
        #[qproperty(i32, status)]
        #[qproperty(QString, status_text)]
        type ImageListModel = super::ImageListModelRust;

        #[cxx_override]
        #[rust_name = "row_count"]
        fn rowCount(&self, parent: &QModelIndex) -> i32;

        #[cxx_override]
        fn data(&self, index: &QModelIndex, role: i32) -> QVariant;

        #[cxx_override]
        #[rust_name = "role_names"]
        fn roleNames(&self) -> QHash_i32_QByteArray;

        #[inherit]
        #[rust_name = "begin_reset_model"]
        fn beginResetModel(self: Pin<&mut Self>);

        #[inherit]
        #[rust_name = "end_reset_model"]
        fn endResetModel(self: Pin<&mut Self>);

        /// Reload the image list.
        #[qinvokable]
        #[rust_name = "refresh"]
        fn refresh(self: Pin<&mut Self>);
    }

    impl cxx_qt::Threading for NetworkListModel {}

    unsafe extern "RustQt" {
        /// Network list model.
        #[qobject]
        #[qml_element]
        #[base = QAbstractListModel]
        #[qproperty(QString, search_text)]
        #[qproperty(i32, status)]
        #[qproperty(QString, status_text)]
        type NetworkListModel = super::NetworkListModelRust;

        #[cxx_override]
        #[rust_name = "row_count"]
        fn rowCount(&self, parent: &QModelIndex) -> i32;

        #[cxx_override]
        fn data(&self, index: &QModelIndex, role: i32) -> QVariant;

        #[cxx_override]
        #[rust_name = "role_names"]
        fn roleNames(&self) -> QHash_i32_QByteArray;

        #[inherit]
        #[rust_name = "begin_reset_model"]
        fn beginResetModel(self: Pin<&mut Self>);

        #[inherit]
        #[rust_name = "end_reset_model"]
        fn endResetModel(self: Pin<&mut Self>);

        /// Reload the network list.
        #[qinvokable]
        #[rust_name = "refresh"]
        fn refresh(self: Pin<&mut Self>);
    }

    impl cxx_qt::Threading for VolumeListModel {}

    unsafe extern "RustQt" {
        /// Volume list model.
        #[qobject]
        #[qml_element]
        #[base = QAbstractListModel]
        #[qproperty(QString, search_text)]
        #[qproperty(i32, status)]
        #[qproperty(QString, status_text)]
        type VolumeListModel = super::VolumeListModelRust;

        #[cxx_override]
        #[rust_name = "row_count"]
        fn rowCount(&self, parent: &QModelIndex) -> i32;

        #[cxx_override]
        fn data(&self, index: &QModelIndex, role: i32) -> QVariant;

        #[cxx_override]
        #[rust_name = "role_names"]
        fn roleNames(&self) -> QHash_i32_QByteArray;

        #[inherit]
        #[rust_name = "begin_reset_model"]
        fn beginResetModel(self: Pin<&mut Self>);

        #[inherit]
        #[rust_name = "end_reset_model"]
        fn endResetModel(self: Pin<&mut Self>);

        /// Reload the volume list.
        #[qinvokable]
        #[rust_name = "refresh"]
        fn refresh(self: Pin<&mut Self>);
    }
}

/// Rust state for [`qobject::ImageListModel`].
#[derive(Default)]
pub struct ImageListModelRust {
    pub(crate) rows: Vec<ImageRow>,
    search_text: QString,
    status: i32,
    status_text: QString,
}

/// Rust state for [`qobject::NetworkListModel`].
#[derive(Default)]
pub struct NetworkListModelRust {
    pub(crate) rows: Vec<NetworkRow>,
    search_text: QString,
    status: i32,
    status_text: QString,
}

/// Rust state for [`qobject::VolumeListModel`].
#[derive(Default)]
pub struct VolumeListModelRust {
    pub(crate) rows: Vec<VolumeRow>,
    search_text: QString,
    status: i32,
    status_text: QString,
}

/// Shared refresh logic for the three read-only models.
pub(crate) enum ListKind {
    Images,
    Networks,
    Volumes,
}

impl qobject::ImageListModel {
    fn row_count(&self, _parent: &QModelIndex) -> i32 {
        self.rows.len() as i32
    }

    fn data(&self, index: &QModelIndex, role: i32) -> QVariant {
        let Some(row) = self.rows.get(index.row() as usize) else {
            return QVariant::default();
        };
        match role {
            0 => qv(&row.id),
            1 => qv(&row.short_id),
            2 => qv(&row.tags),
            3 => qv(&row.size),
            4 => qv(&row.created_at),
            _ => QVariant::default(),
        }
    }

    fn role_names(&self) -> qobject::QHash_i32_QByteArray {
        let mut hash = qobject::QHash_i32_QByteArray::default();
        hash.insert(0, "imageId".into());
        hash.insert(1, "shortId".into());
        hash.insert(2, "tags".into());
        hash.insert(3, "size".into());
        hash.insert(4, "createdAt".into());
        hash
    }

    /// Reload the image list.
    pub fn refresh(mut self: Pin<&mut Self>) {
        self.as_mut().refresh_kind(ListKind::Images);
    }
}

impl qobject::NetworkListModel {
    fn row_count(&self, _parent: &QModelIndex) -> i32 {
        self.rows.len() as i32
    }

    fn data(&self, index: &QModelIndex, role: i32) -> QVariant {
        let Some(row) = self.rows.get(index.row() as usize) else {
            return QVariant::default();
        };
        match role {
            0 => qv(&row.id),
            1 => qv(&row.name),
            2 => qv(&row.driver),
            3 => qv(&row.scope),
            4 => QVariant::from(&row.internal),
            5 => QVariant::from(&row.attachable),
            6 => QVariant::from(&row.ipv6),
            _ => QVariant::default(),
        }
    }

    fn role_names(&self) -> qobject::QHash_i32_QByteArray {
        let mut hash = qobject::QHash_i32_QByteArray::default();
        hash.insert(0, "networkId".into());
        hash.insert(1, "name".into());
        hash.insert(2, "driver".into());
        hash.insert(3, "scope".into());
        hash.insert(4, "internal".into());
        hash.insert(5, "attachable".into());
        hash.insert(6, "ipv6".into());
        hash
    }

    /// Reload the network list.
    pub fn refresh(mut self: Pin<&mut Self>) {
        self.as_mut().refresh_kind(ListKind::Networks);
    }
}

impl qobject::VolumeListModel {
    fn row_count(&self, _parent: &QModelIndex) -> i32 {
        self.rows.len() as i32
    }

    fn data(&self, index: &QModelIndex, role: i32) -> QVariant {
        let Some(row) = self.rows.get(index.row() as usize) else {
            return QVariant::default();
        };
        match role {
            0 => qv(&row.name),
            1 => qv(&row.driver),
            2 => qv(&row.mountpoint),
            3 => qv(&row.scope),
            4 => qv(&row.created_at),
            _ => QVariant::default(),
        }
    }

    fn role_names(&self) -> qobject::QHash_i32_QByteArray {
        let mut hash = qobject::QHash_i32_QByteArray::default();
        hash.insert(0, "name".into());
        hash.insert(1, "driver".into());
        hash.insert(2, "mountpoint".into());
        hash.insert(3, "scope".into());
        hash.insert(4, "createdAt".into());
        hash
    }

    /// Reload the volume list.
    pub fn refresh(mut self: Pin<&mut Self>) {
        self.as_mut().refresh_kind(ListKind::Volumes);
    }
}

/// Generic refresh pipeline shared by the three read-only models.
impl qobject::ImageListModel {
    fn refresh_kind(mut self: Pin<&mut Self>, _kind: ListKind) {
        let Some(services) = get_services() else {
            self.as_mut().set_status(5);
            self.as_mut()
                .set_status_text(QString::from("Not connected to Docker Engine."));
            return;
        };
        self.as_mut().set_status(1); // loading
        let search = self.search_text().to_string();
        let qt_thread = self.qt_thread();
        crate::runtime::spawn(async move {
            let options = tuxstack_docker_core::services::images::ListImagesOptions {
                search: if search.is_empty() {
                    None
                } else {
                    Some(search)
                },
            };
            let result = services.images.list_images(&options).await;
            qt_thread
                .queue(move |mut model| match result {
                    Ok(images) => {
                        let rows: Vec<ImageRow> = images
                            .into_iter()
                            .map(|i| ImageRow {
                                id: i.id,
                                short_id: i.short_id,
                                tags: i.repository_tags.join(", "),
                                size: tuxstack_docker_core::format::bytes(i.size_bytes),
                                created_at: i.created_at.format("%Y-%m-%d %H:%M").to_string(),
                            })
                            .collect();
                        model.as_mut().apply_images(rows);
                    }
                    Err(e) => {
                        model.as_mut().set_status(4);
                        model
                            .as_mut()
                            .set_status_text(QString::from(map_docker_error(&e).user_message()));
                    }
                })
                .expect("queue to Qt thread");
        });
    }

    fn apply_images(mut self: Pin<&mut Self>, rows: Vec<ImageRow>) {
        self.as_mut().begin_reset_model();
        self.as_mut().rust_mut().rows = rows;
        self.as_mut().end_reset_model();
        let status = if self.rows.is_empty() { 3 } else { 2 };
        self.as_mut().set_status(status);
    }
}

impl qobject::NetworkListModel {
    fn refresh_kind(mut self: Pin<&mut Self>, _kind: ListKind) {
        let Some(services) = get_services() else {
            self.as_mut().set_status(5);
            self.as_mut()
                .set_status_text(QString::from("Not connected to Docker Engine."));
            return;
        };
        self.as_mut().set_status(1); // loading
        let search = self.search_text().to_string();
        let qt_thread = self.qt_thread();
        crate::runtime::spawn(async move {
            let options = tuxstack_docker_core::services::networks::ListNetworksOptions {
                search: if search.is_empty() {
                    None
                } else {
                    Some(search)
                },
            };
            let result = services.networks.list_networks(&options).await;
            qt_thread
                .queue(move |mut model| match result {
                    Ok(networks) => {
                        let rows: Vec<NetworkRow> = networks
                            .into_iter()
                            .map(|n| NetworkRow {
                                id: n.id.chars().take(12).collect(),
                                name: n.name,
                                driver: n.driver,
                                scope: n.scope,
                                internal: n.internal,
                                attachable: n.attachable,
                                ipv6: n.ipv6,
                            })
                            .collect();
                        model.as_mut().apply_networks(rows);
                    }
                    Err(e) => {
                        model.as_mut().set_status(4);
                        model
                            .as_mut()
                            .set_status_text(QString::from(map_docker_error(&e).user_message()));
                    }
                })
                .expect("queue to Qt thread");
        });
    }

    fn apply_networks(mut self: Pin<&mut Self>, rows: Vec<NetworkRow>) {
        self.as_mut().begin_reset_model();
        self.as_mut().rust_mut().rows = rows;
        self.as_mut().end_reset_model();
        let status = if self.rows.is_empty() { 3 } else { 2 };
        self.as_mut().set_status(status);
    }
}

impl qobject::VolumeListModel {
    fn refresh_kind(mut self: Pin<&mut Self>, _kind: ListKind) {
        let Some(services) = get_services() else {
            self.as_mut().set_status(5);
            self.as_mut()
                .set_status_text(QString::from("Not connected to Docker Engine."));
            return;
        };
        self.as_mut().set_status(1); // loading
        let search = self.search_text().to_string();
        let qt_thread = self.qt_thread();
        crate::runtime::spawn(async move {
            let options = tuxstack_docker_core::services::volumes::ListVolumesOptions {
                search: if search.is_empty() {
                    None
                } else {
                    Some(search)
                },
            };
            let result = services.volumes.list_volumes(&options).await;
            qt_thread
                .queue(move |mut model| match result {
                    Ok(volumes) => {
                        let rows: Vec<VolumeRow> = volumes
                            .into_iter()
                            .map(|v| VolumeRow {
                                name: v.name,
                                driver: v.driver,
                                mountpoint: v.mountpoint,
                                scope: v.scope,
                                created_at: v
                                    .created_at
                                    .map(|t| t.format("%Y-%m-%d %H:%M").to_string())
                                    .unwrap_or_default(),
                            })
                            .collect();
                        model.as_mut().apply_volumes(rows);
                    }
                    Err(e) => {
                        model.as_mut().set_status(4);
                        model
                            .as_mut()
                            .set_status_text(QString::from(map_docker_error(&e).user_message()));
                    }
                })
                .expect("queue to Qt thread");
        });
    }

    fn apply_volumes(mut self: Pin<&mut Self>, rows: Vec<VolumeRow>) {
        self.as_mut().begin_reset_model();
        self.as_mut().rust_mut().rows = rows;
        self.as_mut().end_reset_model();
        let status = if self.rows.is_empty() { 3 } else { 2 };
        self.as_mut().set_status(status);
    }
}
