//! CXX-Qt bridge for read-only image file browsing.

use std::path::PathBuf;
use std::pin::Pin;

use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::{QList, QMap, QModelIndex, QString, QVariant};
use tokio_util::sync::CancellationToken;
use tuxstack_docker_core::{
    FilesystemError, FilesystemPathToken, FilesystemSession, FilesystemSource,
    ListDirectoryRequest, PreviewRequest, StatRequest, VolumePath, filesystem_decode_base64,
};

use crate::app_state::{get_services, get_store};
use crate::bridge::resource_bridges::qobject;
use crate::controllers::image_files::{ImageFilesControllerState, ImageFilesState};
use crate::controllers::volume_files::VolumeFileSortColumn;
use crate::models::volume_file_model::{VolumeFileRow, map_filesystem_row};

type QVariantList = QList<QVariant>;
type QVariantMap = QMap<cxx_qt_lib::QMapPair_QString_QVariant>;

const ROLE_NAME: i32 = 257;
const ROLE_DISPLAY_NAME: i32 = 258;
const ROLE_PATH: i32 = 259;
const ROLE_ENTRY_TYPE: i32 = 260;
const ROLE_ICON_NAME: i32 = 261;
const ROLE_SIZE_BYTES: i32 = 262;
const ROLE_SIZE_KNOWN: i32 = 263;
const ROLE_SIZE_TEXT: i32 = 264;
const ROLE_MODIFIED_AT: i32 = 265;
const ROLE_MODIFIED_TEXT: i32 = 266;
const ROLE_KIND_TEXT: i32 = 267;
const ROLE_HIDDEN: i32 = 268;
const ROLE_READABLE: i32 = 269;
const ROLE_SYMLINK_TARGET: i32 = 270;
const ROLE_SELECTED: i32 = 271;
const ROLE_MODE_TEXT: i32 = 272;
const ROLE_OWNER_TEXT: i32 = 273;

#[derive(Default)]
pub struct ImageFileListModelRust {
    pub(crate) state: ImageFilesControllerState,
    pub(crate) session: Option<FilesystemSession>,

    pub(crate) files_state: QString,
    pub(crate) error_kind: QString,
    pub(crate) error_message: QString,
    pub(crate) image_id: QString,
    pub(crate) current_path: QString,
    pub(crate) can_go_back: bool,
    pub(crate) can_go_up: bool,
    pub(crate) show_hidden: bool,
    pub(crate) search_query: QString,
    pub(crate) sort_column: QString,
    pub(crate) sort_descending: bool,
    pub(crate) directories_first: bool,
    pub(crate) selected_entry_path: QString,
    pub(crate) loading: bool,
    pub(crate) count: i32,
    pub(crate) truncated: bool,
    pub(crate) breadcrumb_model: QVariantList,
    pub(crate) active: bool,

    pub(crate) preview_loading: bool,
    pub(crate) preview_name: QString,
    pub(crate) preview_path: QString,
    pub(crate) preview_kind: QString,
    pub(crate) preview_text: QString,
    pub(crate) preview_mime: QString,
    pub(crate) preview_size_text: QString,
    pub(crate) preview_truncated: bool,
    pub(crate) preview_is_image: bool,
    pub(crate) preview_is_text: bool,
    pub(crate) preview_is_binary: bool,
    pub(crate) preview_parse_error: QString,
    pub(crate) preview_image_path: QString,
    pub(crate) preview_error: QString,

    pub(crate) download_in_progress: bool,
    pub(crate) download_bytes_written: i64,
    pub(crate) download_progress_text: QString,
    pub(crate) download_error: QString,

    pub(crate) properties_model: QVariantList,

    pub(crate) visible_rows: Vec<VolumeFileRow>,
    pub(crate) preview_temp_file: Option<PathBuf>,

    pub(crate) session_cancel: Option<CancellationToken>,
    pub(crate) list_cancel: Option<CancellationToken>,
    pub(crate) preview_cancel: Option<CancellationToken>,
    pub(crate) download_cancel: Option<CancellationToken>,

    pub(crate) session_bridge_generation: u64,
    pub(crate) list_bridge_generation: u64,
}

impl ImageFileListModelRust {
    fn cancel_all(&mut self) {
        cancel(&mut self.session_cancel);
        cancel(&mut self.list_cancel);
        cancel(&mut self.preview_cancel);
        cancel(&mut self.download_cancel);
    }

    fn clear_preview_temp(&mut self) {
        if let Some(path) = self.preview_temp_file.take() {
            let _ = std::fs::remove_file(path);
        }
        self.preview_image_path = QString::default();
    }
}

impl qobject::ImageFileListModel {
    pub(crate) fn row_count(&self, _parent: &QModelIndex) -> i32 {
        self.visible_rows.len() as i32
    }

    pub(crate) fn data(&self, index: &QModelIndex, role: i32) -> QVariant {
        let Some(row) = self.visible_rows.get(index.row() as usize) else {
            return QVariant::default();
        };
        let selected = self.selected_entry_path.to_string() == row.path;
        match role {
            ROLE_NAME => qv(&row.name),
            ROLE_DISPLAY_NAME => qv(&row.display_name),
            ROLE_PATH => qv(&row.path),
            ROLE_ENTRY_TYPE => qv(&row.entry_type),
            ROLE_ICON_NAME => qv(&row.icon_name),
            ROLE_SIZE_BYTES => QVariant::from(&row.size_bytes),
            ROLE_SIZE_KNOWN => QVariant::from(&row.size_known),
            ROLE_SIZE_TEXT => qv(&row.size_text),
            ROLE_MODIFIED_AT => qv(&row.modified_at),
            ROLE_MODIFIED_TEXT => qv(&row.modified_text),
            ROLE_KIND_TEXT => qv(&row.kind_text),
            ROLE_HIDDEN => QVariant::from(&row.hidden),
            ROLE_READABLE => QVariant::from(&row.readable),
            ROLE_SYMLINK_TARGET => qv(&row.symlink_target),
            ROLE_SELECTED => QVariant::from(&selected),
            ROLE_MODE_TEXT => qv(&row.mode_text),
            ROLE_OWNER_TEXT => qv(&row.owner_text),
            _ => QVariant::default(),
        }
    }

    pub(crate) fn role_names(&self) -> qobject::QHash_i32_QByteArray {
        let mut roles = qobject::QHash_i32_QByteArray::default();
        for (role, name) in [
            (ROLE_NAME, "name"),
            (ROLE_DISPLAY_NAME, "displayName"),
            (ROLE_PATH, "path"),
            (ROLE_ENTRY_TYPE, "entryType"),
            (ROLE_ICON_NAME, "iconName"),
            (ROLE_SIZE_BYTES, "sizeBytes"),
            (ROLE_SIZE_KNOWN, "sizeKnown"),
            (ROLE_SIZE_TEXT, "sizeText"),
            (ROLE_MODIFIED_AT, "modifiedAt"),
            (ROLE_MODIFIED_TEXT, "modifiedText"),
            (ROLE_KIND_TEXT, "kindText"),
            (ROLE_HIDDEN, "hidden"),
            (ROLE_READABLE, "readable"),
            (ROLE_SYMLINK_TARGET, "symlinkTarget"),
            (ROLE_SELECTED, "selected"),
            (ROLE_MODE_TEXT, "modeText"),
            (ROLE_OWNER_TEXT, "ownerText"),
        ] {
            roles.insert(role, name.into());
        }
        roles
    }

    pub(crate) fn set_connection_state(
        mut self: Pin<&mut Self>,
        docker_status: i32,
        _message: &QString,
    ) {
        if docker_status == 1 {
            return;
        }
        // Engine is gone: stop the helper container and reset the browser.
        self.as_mut().teardown_session();
        let pool = get_store().image_preview_sessions.clone();
        crate::runtime::spawn(async move {
            let sessions = pool.drain_all().await;
            if let Some(services) = get_services() {
                for session in sessions {
                    let _ = services.filesystem.stop_session(&session).await;
                }
            }
        });
        self.as_mut().rust_mut().state.clear_image();
        self.as_mut().clear_preview_temp();
        self.as_mut().reset_preview_fields();
        self.as_mut().publish_state();
    }

    pub(crate) fn update_active(mut self: Pin<&mut Self>, active: bool) {
        tracing::info!(active, "Image Files tab activated");
        self.as_mut().rust_mut().state.set_active(active);
        if self.active != active {
            self.as_mut().rust_mut().active = active;
            self.as_mut().active_changed();
        }
        if active {
            let image = self.state.image_id.clone();
            if !image.is_empty() {
                self.open_image(&QString::from(&image));
            } else {
                self.as_mut().publish_state();
            }
        } else {
            // Leaving Files: keep the helper alive in the session pool so
            // re-entry reuses it instead of rebuilding the container.
            self.as_mut().release_session_to_pool();
            self.as_mut().publish_state();
        }
    }

    pub(crate) fn open_image(mut self: Pin<&mut Self>, image_id: &QString) {
        let id = image_id.to_string();
        let id = id.trim().to_string();
        if id.is_empty() {
            self.close_image();
            return;
        }

        // Opening Files always marks the controller active.
        if !self.state.active {
            self.as_mut().rust_mut().state.set_active(true);
        }
        if !self.active {
            self.as_mut().rust_mut().active = true;
            self.as_mut().active_changed();
        }

        // Idempotent: same image with a live session must not rebuild the helper.
        if self.state.image_id == id && self.session.is_some() {
            tracing::debug!(image = %id, "Reusing existing image preview session");
            match self.state.state {
                ImageFilesState::Idle | ImageFilesState::Error | ImageFilesState::Unsupported => {
                    self.load_current_directory();
                }
                ImageFilesState::StartingSession | ImageFilesState::Loading => {
                    self.as_mut().publish_state();
                }
                ImageFilesState::Ready | ImageFilesState::Empty => {
                    self.as_mut().publish_state();
                }
            }
            return;
        }

        tracing::info!(image = %id, "Opening image preview");
        // Keep the previous helper alive in the session pool for reuse;
        // never stop it just because the user switched images/tabs.
        self.as_mut().release_session_to_pool();
        let session_generation = {
            let mut rust = self.as_mut().rust_mut();
            rust.state.begin_image(&id)
        };
        let bridge_generation = bump(&mut self.as_mut().rust_mut().session_bridge_generation);
        self.as_mut().publish_state();

        let Some(services) = get_services() else {
            tracing::warn!("Failed to create helper session: Docker Engine is unavailable");
            let mut state = self.as_mut().rust_mut().state.clone();
            state.apply_error(
                session_generation,
                "engine_unavailable",
                "Docker Engine is unavailable",
            );
            self.as_mut().rust_mut().state = state;
            self.as_mut().publish_state();
            return;
        };

        // Try the session pool first: a live helper from a previous visit
        // (same image, earlier tab) is reused without any Docker calls.
        let pool = get_store().image_preview_sessions.clone();
        let qt = self.qt_thread();
        let image = id.clone();
        let token = CancellationToken::new();
        self.as_mut().rust_mut().session_cancel = Some(token.clone());
        crate::runtime::spawn(async move {
            let pooled = tokio::select! {
                _ = token.cancelled() => None,
                result = pool.acquire(&image) => result,
            };
            if let Some(session) = pooled {
                qt.queue(move |mut model| {
                    if bridge_generation != model.session_bridge_generation {
                        return;
                    }
                    tracing::info!(
                        image = %image,
                        "Reusing pooled image preview session"
                    );
                    model.as_mut().rust_mut().session = Some(session);
                    model.as_mut().load_current_directory();
                })
                .ok();
                return;
            }
            if token.is_cancelled() {
                return;
            }

            tracing::debug!("Creating fresh helper session");
            let result = tokio::select! {
                _ = token.cancelled() => return,
                result = services.filesystem.start_image_session(&image, token.clone()) => result,
            };
            qt.queue(move |mut model| {
                if bridge_generation != model.session_bridge_generation {
                    if let Ok(session) = result {
                        let services = get_services();
                        if let Some(services) = services {
                            crate::runtime::spawn(async move {
                                let _ = services.filesystem.stop_session(&session).await;
                            });
                        }
                    }
                    return;
                }
                match result {
                    Ok(session) => {
                        tracing::info!(
                            container_id = %session.container_id,
                            "Helper container started"
                        );
                        // Register the fresh helper so tab/image switches
                        // reuse it instead of rebuilding.
                        let pool = get_store().image_preview_sessions.clone();
                        let pooled = session.clone();
                        crate::runtime::spawn(async move {
                            pool.insert(pooled).await;
                        });
                        model.as_mut().rust_mut().session = Some(session);
                        model.as_mut().load_current_directory();
                    }
                    Err(error) => {
                        tracing::warn!(error = %error, "Failed to create helper session");
                        let (kind, message) = map_filesystem_error(&error);
                        let mut state = model.as_mut().rust_mut().state.clone();
                        state.apply_error(session_generation, kind, &message);
                        model.as_mut().rust_mut().state = state;
                        model.as_mut().publish_state();
                    }
                }
            })
            .ok();
        });
    }

    pub(crate) fn close_image(mut self: Pin<&mut Self>) {
        // Keep the helper alive in the pool so reopening the same image
        // reuses it instead of rebuilding the container.
        self.as_mut().release_session_to_pool();
        self.as_mut().rust_mut().state.clear_image();
        self.as_mut().clear_preview_temp();
        self.as_mut().reset_preview_fields();
        self.as_mut().publish_state();
    }

    pub(crate) fn refresh(self: Pin<&mut Self>) {
        self.load_current_directory();
    }

    pub(crate) fn open_entry(mut self: Pin<&mut Self>, path: &QString) {
        let Ok(logical) = VolumePath::parse(&path.to_string()) else {
            return;
        };
        let entry_type = self
            .visible_rows
            .iter()
            .find(|row| row.path == logical.display())
            .map(|row| row.entry_type.clone())
            .unwrap_or_default();
        if entry_type == "directory" {
            // When entering a directory, store its path token for the next list call.
            if let Some(fs_entry) = self
                .state
                .entries
                .iter()
                .find(|e| e.path_display() == logical.display())
            {
                self.as_mut().rust_mut().state.current_path_token = fs_entry.path_token.clone();
            }
            self.as_mut().rust_mut().state.navigate_to(logical, true);
            self.as_mut().load_current_directory();
            return;
        }
        if entry_type == "symlink" {
            self.as_mut().open_symlink(&logical);
            return;
        }
        // Files open with the host’s default application — no in-app preview.
        self.open_with_system_default(&logical);
    }

    pub(crate) fn load_more(mut self: Pin<&mut Self>) {
        let Some(session) = self.session.clone() else {
            return;
        };
        let Some(services) = get_services() else {
            return;
        };
        let Some(cursor) = self.state.next_cursor.clone() else {
            return;
        };
        if !self.state.truncated {
            return;
        }
        cancel(&mut self.as_mut().rust_mut().list_cancel);
        let list_generation = self.state.list_generation;
        let bridge_generation = bump(&mut self.as_mut().rust_mut().list_bridge_generation);
        let path_token = self.state.current_path_token.clone();
        let show_hidden = self.state.show_hidden;
        let token = CancellationToken::new();
        self.as_mut().rust_mut().list_cancel = Some(token.clone());
        let qt = self.qt_thread();
        crate::runtime::spawn(async move {
            let request = ListDirectoryRequest {
                path_token,
                show_hidden,
                limit: Some(1000),
                cursor: Some(cursor),
            };
            let result = tokio::select! {
                _ = token.cancelled() => return,
                result = services.filesystem.list_directory(&session, &request, token.clone()) => result,
            };
            qt.queue(move |mut model| {
                if bridge_generation != model.list_bridge_generation {
                    return;
                }
                let mut state = model.as_mut().rust_mut().state.clone();
                match result {
                    Ok(list_result) => {
                        tracing::info!(
                            count = list_result.entries.len(),
                            "Received more directory entries"
                        );
                        state.apply_more(list_generation, list_result);
                    }
                    Err(error) => {
                        tracing::warn!(error = %error, "Failed to load more entries");
                        let (kind, message) = map_filesystem_error(&error);
                        let _ = state.apply_error(list_generation, kind, &message);
                    }
                }
                model.as_mut().rust_mut().state = state;
                model.as_mut().publish_state();
            })
            .ok();
        });
    }

    pub(crate) fn go_back(mut self: Pin<&mut Self>) {
        if self.as_mut().rust_mut().state.go_back().is_some() {
            self.load_current_directory();
        }
    }

    pub(crate) fn go_up(mut self: Pin<&mut Self>) {
        if self.as_mut().rust_mut().state.go_up().is_some() {
            self.load_current_directory();
        }
    }

    pub(crate) fn navigate_to(mut self: Pin<&mut Self>, path: &QString) {
        let Ok(logical) = VolumePath::parse(&path.to_string()) else {
            return;
        };
        if logical == self.state.current_path {
            return;
        }
        self.as_mut().rust_mut().state.navigate_to(logical, true);
        self.load_current_directory();
    }

    pub(crate) fn update_search_query(mut self: Pin<&mut Self>, query: &QString) {
        self.as_mut()
            .rust_mut()
            .state
            .set_search_query(&query.to_string());
        self.as_mut().publish_state();
    }

    pub(crate) fn update_show_hidden(mut self: Pin<&mut Self>, show: bool) {
        let changed = self.state.show_hidden != show;
        self.as_mut().rust_mut().state.set_show_hidden(show);
        if changed {
            // Hidden filtering for already-loaded non-hidden lists needs a reload
            // when enabling hidden; when disabling, local filter is enough if we
            // loaded with hidden. Always reload for correctness.
            self.load_current_directory();
        } else {
            self.as_mut().publish_state();
        }
    }

    pub(crate) fn update_sort(mut self: Pin<&mut Self>, column: &QString, descending: bool) {
        self.as_mut()
            .rust_mut()
            .state
            .set_sort(VolumeFileSortColumn::parse(&column.to_string()), descending);
        self.as_mut().publish_state();
    }

    pub(crate) fn toggle_sort(mut self: Pin<&mut Self>, column: &QString) {
        self.as_mut()
            .rust_mut()
            .state
            .toggle_sort(VolumeFileSortColumn::parse(&column.to_string()));
        self.as_mut().publish_state();
    }

    pub(crate) fn select_entry(mut self: Pin<&mut Self>, path: &QString) {
        let parsed = VolumePath::parse(&path.to_string()).ok();
        self.as_mut().rust_mut().state.select_path(parsed);
        self.as_mut().publish_state();
    }

    /// Kept as a QML invokable for compatibility; opens with the system default
    /// application instead of an in-app preview pane.
    pub(crate) fn preview_entry(self: Pin<&mut Self>, path: &QString) {
        let Ok(logical) = VolumePath::parse(&path.to_string()) else {
            return;
        };
        self.open_with_system_default(&logical);
    }

    pub(crate) fn cancel_preview(mut self: Pin<&mut Self>) {
        cancel(&mut self.as_mut().rust_mut().preview_cancel);
        self.as_mut().rust_mut().preview_loading = false;
        self.as_mut().clear_preview_temp();
        self.as_mut().reset_preview_fields();
    }

    pub(crate) fn download_entry(mut self: Pin<&mut Self>, path: &QString, destination: &QString) {
        let Ok(logical) = VolumePath::parse(&path.to_string()) else {
            return;
        };
        let dest = match local_destination(&destination.to_string()) {
            Ok(path) => path,
            Err(message) => {
                self.as_mut().rust_mut().download_error = qstring(&message);
                self.as_mut().download_failed(qstring(&message));
                return;
            }
        };
        let Some(session) = self.session.clone() else {
            return;
        };
        let Some(services) = get_services() else {
            return;
        };
        cancel(&mut self.as_mut().rust_mut().download_cancel);
        self.as_mut().rust_mut().download_in_progress = true;
        self.as_mut().rust_mut().download_bytes_written = 0;
        self.as_mut().rust_mut().download_progress_text = qstring("0 B");
        self.as_mut().rust_mut().download_error = QString::default();
        let token = CancellationToken::new();
        self.as_mut().rust_mut().download_cancel = Some(token.clone());
        let qt = self.qt_thread();
        crate::runtime::spawn(async move {
            let path_token = volume_path_to_token(&logical);
            let request = StatRequest { path_token };
            // Get file size first for progress tracking.
            let stat_result = tokio::select! {
                _ = token.cancelled() => {
                    qt.queue(move |mut model| {
                        model.as_mut().rust_mut().download_in_progress = false;
                    }).ok();
                    return;
                },
                result = services.filesystem.stat(&session, &request, token.clone()) => result,
            };
            let stat_entry = match stat_result {
                Ok(entry) => entry,
                Err(error) => {
                    qt.queue(move |mut model| {
                        model.as_mut().rust_mut().download_in_progress = false;
                        let (_, message) = map_filesystem_error(&error);
                        model.as_mut().rust_mut().download_error = qstring(&message);
                        model.as_mut().download_failed(qstring(&message));
                    })
                    .ok();
                    return;
                }
            };
            // Create parent directory.
            if let Some(parent) = dest.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let mut file = std::fs::File::create(&dest).ok();
            let mut offset = 0u64;
            let chunk_limit = 4 * 1024 * 1024; // 4 MiB per chunk
            loop {
                let preview_request = PreviewRequest {
                    path_token: stat_entry.path_token.clone(),
                    offset,
                    limit: chunk_limit,
                };
                let chunk_result = tokio::select! {
                    _ = token.cancelled() => break,
                    result = services.filesystem.preview(&session, &preview_request, token.clone()) => result,
                };
                match chunk_result {
                    Ok(preview) => {
                        for chunk in &preview.chunks {
                            if let Ok(data) = filesystem_decode_base64(&chunk.data_b64) {
                                if let Some(ref mut f) = file {
                                    use std::io::Write;
                                    let _ = f.write_all(&data);
                                }
                                offset += data.len() as u64;
                            }
                        }
                        if preview.chunks.is_empty() || preview.truncated {
                            break;
                        }
                        // Check if we got an EOF.
                        let eof = preview.chunks.last().map(|c| c.eof).unwrap_or(false);
                        if eof {
                            break;
                        }
                    }
                    Err(error) => {
                        let _ = std::fs::remove_file(&dest);
                        qt.queue(move |mut model| {
                            model.as_mut().rust_mut().download_in_progress = false;
                            let (_, message) = map_filesystem_error(&error);
                            model.as_mut().rust_mut().download_error = qstring(&message);
                            model.as_mut().download_failed(qstring(&message));
                        })
                        .ok();
                        return;
                    }
                }
            }
            qt.queue(move |mut model| {
                model.as_mut().rust_mut().download_in_progress = false;
                model.as_mut().rust_mut().download_bytes_written = offset as i64;
                model
                    .as_mut()
                    .download_completed(qstring(&dest.display().to_string()));
            })
            .ok();
        });
    }

    pub(crate) fn cancel_download(mut self: Pin<&mut Self>) {
        cancel(&mut self.as_mut().rust_mut().download_cancel);
        self.as_mut().rust_mut().download_in_progress = false;
    }

    pub(crate) fn load_properties(mut self: Pin<&mut Self>, path: &QString) {
        let Ok(logical) = VolumePath::parse(&path.to_string()) else {
            return;
        };
        let row = self
            .visible_rows
            .iter()
            .find(|row| row.path == logical.display())
            .cloned();
        let mut props = QVariantList::default();
        if let Some(row) = row {
            push_prop(&mut props, "Name", &row.name);
            push_prop(&mut props, "Logical Path", &row.path);
            push_prop(&mut props, "Kind", &row.kind_text);
            push_prop(&mut props, "Size", &row.size_text);
            push_prop(&mut props, "Modified", &row.modified_text);
            push_prop(&mut props, "Permissions", &row.mode_text);
            push_prop(&mut props, "Owner", &row.owner_text);
            if !row.symlink_target.is_empty() {
                push_prop(&mut props, "Symlink Target", &row.symlink_target);
            }
        } else {
            push_prop(&mut props, "Logical Path", &logical.display());
        }
        self.as_mut().rust_mut().properties_model = props;
        self.as_mut().properties_ready();
    }

    pub(crate) fn retry(self: Pin<&mut Self>) {
        let image = self.state.image_id.clone();
        if image.is_empty() {
            return;
        }
        if self.session.is_none() {
            self.open_image(&QString::from(&image));
        } else {
            self.load_current_directory();
        }
    }

    pub(crate) fn shutdown(mut self: Pin<&mut Self>) {
        self.as_mut().teardown_session();
        // Drain the session pool: every pooled helper container must be
        // stopped before the app exits.
        let pool = get_store().image_preview_sessions.clone();
        crate::runtime::spawn(async move {
            let sessions = pool.drain_all().await;
            if let Some(services) = get_services() {
                for session in sessions {
                    let _ = services.filesystem.stop_session(&session).await;
                }
            }
        });
        self.as_mut().clear_preview_temp();
        self.as_mut().rust_mut().state.clear_image();
        self.as_mut().publish_state();
    }

    fn open_symlink(self: Pin<&mut Self>, path: &VolumePath) {
        let Some(session) = self.session.clone() else {
            return;
        };
        let Some(services) = get_services() else {
            return;
        };
        let token = CancellationToken::new();
        let qt = self.qt_thread();
        let path = path.clone();
        let path_token = volume_path_to_token(&path);
        crate::runtime::spawn(async move {
            // Resolve the symlink to its target token.
            let resolve_request = StatRequest {
                path_token: path_token.clone(),
            };
            let result = services
                .filesystem
                .readlink(&session, &resolve_request, token.clone())
                .await;
            match result {
                Ok(resolved_token) => {
                    // Stat the resolved target to check if it's a directory.
                    let stat_request = StatRequest {
                        path_token: resolved_token.clone(),
                    };
                    let stat_result = services
                        .filesystem
                        .stat(&session, &stat_request, token.clone())
                        .await;
                    qt.queue(move |mut model| match stat_result {
                        Ok(entry) => {
                            let resolved_path = VolumePath::parse(&entry.path_display())
                                .unwrap_or_else(|_| VolumePath::root());
                            if entry.entry_type.is_directory() {
                                model.as_mut().rust_mut().state.current_path_token = resolved_token;
                                model
                                    .as_mut()
                                    .rust_mut()
                                    .state
                                    .navigate_to(resolved_path, true);
                                model.as_mut().load_current_directory();
                            } else {
                                model.as_mut().open_with_system_default(&resolved_path);
                            }
                        }
                        Err(error) => {
                            let (_, message) = map_filesystem_error(&error);
                            model.as_mut().symlink_blocked(qstring(&message));
                        }
                    })
                    .ok();
                }
                Err(error) => {
                    qt.queue(move |mut model| {
                        let (_, message) = map_filesystem_error(&error);
                        model.as_mut().symlink_blocked(qstring(&message));
                    })
                    .ok();
                }
            }
        });
    }

    /// Stream the image file to a unique temp path, then hand it to the host
    /// desktop's default handler (`xdg-open` / `open` / `cmd start`). TuxStack
    /// does not implement an in-app file preview.
    fn open_with_system_default(mut self: Pin<&mut Self>, path: &VolumePath) {
        let Some(session) = self.session.clone() else {
            self.as_mut()
                .preview_failed(qstring("Image preview session is not available."));
            return;
        };
        let Some(services) = get_services() else {
            self.as_mut()
                .preview_failed(qstring("Docker Engine is unavailable."));
            return;
        };

        let file_name = path
            .components()
            .last()
            .cloned()
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| "image-file".into());
        let destination = match unique_open_temp_path(&file_name) {
            Ok(path) => path,
            Err(message) => {
                self.as_mut().preview_failed(qstring(&message));
                return;
            }
        };

        cancel(&mut self.as_mut().rust_mut().preview_cancel);
        self.as_mut().rust_mut().preview_loading = true;
        self.as_mut().rust_mut().preview_path = qstring(&path.display());
        self.as_mut().rust_mut().preview_name = qstring(&file_name);
        self.as_mut().rust_mut().preview_error = QString::default();
        let token = CancellationToken::new();
        self.as_mut().rust_mut().preview_cancel = Some(token.clone());
        let logical = path.clone();
        let qt = self.qt_thread();
        tracing::info!(path = %logical.display(), "Opening image file with system default app");

        crate::runtime::spawn(async move {
            let path_token = volume_path_to_token(&logical);
            // Download file by streaming preview chunks to disk.
            let mut offset = 0u64;
            let chunk_limit = 4 * 1024 * 1024; // 4 MiB per chunk
            if let Some(parent) = destination.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let mut file = match std::fs::File::create(&destination) {
                Ok(f) => f,
                Err(e) => {
                    qt.queue(move |mut model| {
                        model.as_mut().rust_mut().preview_loading = false;
                        model.as_mut().rust_mut().preview_error = qstring(&e.to_string());
                        model.as_mut().preview_failed(qstring(&e.to_string()));
                    })
                    .ok();
                    return;
                }
            };
            loop {
                let preview_request = PreviewRequest {
                    path_token: path_token.clone(),
                    offset,
                    limit: chunk_limit,
                };
                let chunk_result = tokio::select! {
                    _ = token.cancelled() => break,
                    result = services.filesystem.preview(&session, &preview_request, token.clone()) => result,
                };
                match chunk_result {
                    Ok(preview) => {
                        for chunk in &preview.chunks {
                            if let Ok(data) = filesystem_decode_base64(&chunk.data_b64) {
                                use std::io::Write;
                                let _ = file.write_all(&data);
                                offset += data.len() as u64;
                            }
                        }
                        if preview.chunks.is_empty() || preview.truncated {
                            break;
                        }
                        let eof = preview.chunks.last().map(|c| c.eof).unwrap_or(false);
                        if eof {
                            break;
                        }
                    }
                    Err(error) => {
                        let _ = std::fs::remove_file(&destination);
                        qt.queue(move |mut model| {
                            model.as_mut().rust_mut().preview_loading = false;
                            let (_, message) = map_filesystem_error(&error);
                            model.as_mut().rust_mut().preview_error = qstring(&message);
                            model.as_mut().preview_failed(qstring(&message));
                        })
                        .ok();
                        return;
                    }
                }
            }
            qt.queue(move |mut model| {
                model.as_mut().rust_mut().preview_loading = false;
                model.as_mut().rust_mut().preview_temp_file = Some(destination.clone());
                match open_path_with_default_app(&destination) {
                    Ok(()) => {
                        tracing::debug!(
                            path = %destination.display(),
                            "Launched system default application"
                        );
                        model.as_mut().preview_ready();
                    }
                    Err(message) => {
                        let _ = std::fs::remove_file(&destination);
                        model.as_mut().rust_mut().preview_temp_file = None;
                        model.as_mut().rust_mut().preview_error = qstring(&message);
                        model.as_mut().preview_failed(qstring(&message));
                    }
                }
            })
            .ok();
        });
    }

    fn load_current_directory(mut self: Pin<&mut Self>) {
        let Some(session) = self.session.clone() else {
            return;
        };
        let Some(services) = get_services() else {
            let mut state = self.as_mut().rust_mut().state.clone();
            let generation = state.list_generation;
            state.apply_error(
                generation,
                "engine_unavailable",
                "Docker Engine is unavailable",
            );
            self.as_mut().rust_mut().state = state;
            self.as_mut().publish_state();
            return;
        };
        cancel(&mut self.as_mut().rust_mut().list_cancel);
        let list_generation = {
            let mut rust = self.as_mut().rust_mut();
            rust.state.begin_list()
        };
        let bridge_generation = bump(&mut self.as_mut().rust_mut().list_bridge_generation);
        let path = self.state.current_path.clone();
        let path_token = self.state.current_path_token.clone();
        tracing::info!(path = %path.display(), "Listing directory");
        self.as_mut().publish_state();
        let token = CancellationToken::new();
        self.as_mut().rust_mut().list_cancel = Some(token.clone());
        let show_hidden = self.state.show_hidden;
        let qt = self.qt_thread();
        crate::runtime::spawn(async move {
            let request = ListDirectoryRequest {
                path_token: path_token.clone(),
                show_hidden,
                limit: Some(1000),
                cursor: None,
            };
            let result = tokio::select! {
                _ = token.cancelled() => return,
                result = services.filesystem.list_directory(&session, &request, token.clone()) => result,
            };
            qt.queue(move |mut model| {
                if bridge_generation != model.list_bridge_generation {
                    return;
                }
                let mut state = model.as_mut().rust_mut().state.clone();
                match result {
                    Ok(list_result) => {
                        tracing::info!(
                            count = list_result.entries.len(),
                            "Received directory entries"
                        );
                        let _ = state.apply_list(list_generation, path, path_token, list_result);
                    }
                    Err(error) => {
                        tracing::warn!(error = %error, "Failed to exec listing command");
                        let (kind, message) = map_filesystem_error(&error);
                        let _ = state.apply_error(list_generation, kind, &message);
                    }
                }
                model.as_mut().rust_mut().state = state;
                tracing::debug!("Updating ImageFileModel");
                model.as_mut().publish_state();
            })
            .ok();
        });
    }

    fn teardown_session(mut self: Pin<&mut Self>) {
        self.as_mut().cancel_all();
        bump(&mut self.as_mut().rust_mut().session_bridge_generation);
        bump(&mut self.as_mut().rust_mut().list_bridge_generation);
        if let Some(session) = self.as_mut().rust_mut().session.take() {
            if let Some(services) = get_services() {
                crate::runtime::spawn(async move {
                    let _ = services.filesystem.stop_session(&session).await;
                });
            }
        }
    }

    /// Hand the current helper session back to the pool (kept alive for
    /// reuse) instead of stopping it. Cancels in-flight work first.
    fn release_session_to_pool(mut self: Pin<&mut Self>) {
        self.as_mut().cancel_all();
        bump(&mut self.as_mut().rust_mut().session_bridge_generation);
        bump(&mut self.as_mut().rust_mut().list_bridge_generation);
        if let Some(session) = self.as_mut().rust_mut().session.take() {
            let pool_key = session_pool_key(&session);
            let pool = get_store().image_preview_sessions.clone();
            crate::runtime::spawn(async move {
                pool.release(&pool_key).await;
            });
        }
    }

    fn reset_preview_fields(mut self: Pin<&mut Self>) {
        let mut rust = self.as_mut().rust_mut();
        rust.preview_loading = false;
        rust.preview_name = QString::default();
        rust.preview_path = QString::default();
        rust.preview_kind = QString::default();
        rust.preview_text = QString::default();
        rust.preview_mime = QString::default();
        rust.preview_size_text = QString::default();
        rust.preview_truncated = false;
        rust.preview_is_image = false;
        rust.preview_is_text = false;
        rust.preview_is_binary = false;
        rust.preview_parse_error = QString::default();
        rust.preview_error = QString::default();
    }

    fn publish_state(mut self: Pin<&mut Self>) {
        let state = self.state.clone();
        let visible = state.visible_entries();
        let rows: Vec<VolumeFileRow> = visible.into_iter().map(map_filesystem_row).collect();
        let count = rows.len() as i32;
        let loading = matches!(
            state.state,
            ImageFilesState::StartingSession | ImageFilesState::Loading
        );
        let selected = state
            .selected_path
            .as_ref()
            .map(|path| path.display())
            .unwrap_or_default();
        let breadcrumbs = breadcrumb_list(&state);

        // Model rows are not Q_PROPERTY values; update them under reset.
        self.as_mut().begin_reset_model();
        self.as_mut().rust_mut().visible_rows = rows;
        self.as_mut().end_reset_model();

        // Writable qproperties must go through generated setters so QML
        // bindings receive NOTIFY signals. Direct rust_mut writes leave the
        // Files page stuck on idle and hide the table forever.
        self.as_mut().set_files_state(qstring(state.state.as_str()));
        self.as_mut().set_error_kind(qstring(&state.error_kind));
        self.as_mut()
            .set_error_message(qstring(&state.error_message));
        self.as_mut().set_image_id(qstring(&state.image_id));
        self.as_mut()
            .set_current_path(qstring(&state.current_path.display()));
        self.as_mut().set_can_go_back(state.can_go_back());
        self.as_mut().set_can_go_up(state.can_go_up());
        self.as_mut()
            .set_sort_column(qstring(state.sort_column.as_str()));
        self.as_mut().set_sort_descending(state.sort_descending);
        self.as_mut().set_directories_first(state.directories_first);
        self.as_mut().set_selected_entry_path(qstring(&selected));
        self.as_mut().set_loading(loading);
        self.as_mut().set_count(count);
        self.as_mut().set_truncated(state.truncated);
        self.as_mut().set_breadcrumb_model(breadcrumbs);

        // READ + NOTIFY properties have no QML write path; emit change
        // signals after updating the backing fields.
        if self.show_hidden != state.show_hidden {
            self.as_mut().rust_mut().show_hidden = state.show_hidden;
            self.as_mut().show_hidden_changed();
        }
        let search = qstring(&state.search_query);
        if self.search_query != search {
            self.as_mut().rust_mut().search_query = search;
            self.as_mut().search_query_changed();
        }
        if self.active != state.active {
            self.as_mut().rust_mut().active = state.active;
            self.as_mut().active_changed();
        }
    }
}

impl qobject::ImageFileListModel {
    fn cancel_all(mut self: Pin<&mut Self>) {
        self.as_mut().rust_mut().cancel_all();
    }

    fn clear_preview_temp(mut self: Pin<&mut Self>) {
        self.as_mut().rust_mut().clear_preview_temp();
    }
}

fn breadcrumb_list(state: &ImageFilesControllerState) -> QVariantList {
    let mut list = QVariantList::default();
    for (label, path) in state.breadcrumb_components() {
        let mut map = QVariantMap::default();
        map.insert(QString::from("label"), qv(&label));
        map.insert(QString::from("path"), qv(&path.display()));
        list.append(QVariant::from(&map));
    }
    list
}

fn push_prop(list: &mut QVariantList, label: &str, value: &str) {
    let mut map = QVariantMap::default();
    map.insert(QString::from("label"), qv(label));
    map.insert(QString::from("value"), qv(value));
    list.append(QVariant::from(&map));
}

fn map_filesystem_error(error: &FilesystemError) -> (&'static str, String) {
    match error {
        FilesystemError::DockerUnavailable => {
            ("engine_unavailable", "Docker Engine is unavailable.".into())
        }
        FilesystemError::ImageNotFound(id) | FilesystemError::ImageNotFoundVariant(id) => {
            ("image_missing", format!("Image not found: {id}"))
        }
        FilesystemError::VolumeNotFound(name) => {
            ("volume_missing", format!("Volume not found: {name}"))
        }
        FilesystemError::UnsupportedPlatform(msg) => ("unsupported", msg.clone()),
        FilesystemError::HelperBinaryUnavailable(msg) => ("unsupported", msg.clone()),
        FilesystemError::HelperHandshakeFailed(msg) => ("session_failed", msg.clone()),
        FilesystemError::HelperProtocolMismatch { .. } => ("protocol", error.to_string()),
        FilesystemError::HelperProtocolError(msg) => ("protocol", msg.clone()),
        FilesystemError::PathNotFound(path) => {
            ("not_found", format!("Folder or file was not found: {path}"))
        }
        FilesystemError::PermissionDenied(msg) => ("permission", msg.clone()),
        FilesystemError::SessionClosed | FilesystemError::SessionInvalidated => (
            "session_closed",
            "The preview session is no longer available.".into(),
        ),
        FilesystemError::SessionFailed(msg) => ("session_failed", msg.clone()),
        FilesystemError::Timeout | FilesystemError::OperationTimeout => {
            ("timeout", "Operation timed out.".into())
        }
        FilesystemError::Cancelled => ("cancelled", "Operation cancelled.".into()),
        FilesystemError::ExecFailed(msg) => ("error", msg.clone()),
        other => ("error", other.to_string()),
    }
}

/// Extract a pool key from a `FilesystemSession` for the session pool.
fn session_pool_key(session: &FilesystemSession) -> String {
    match &session.source {
        FilesystemSource::Image { image_id, .. } => image_id.clone(),
        FilesystemSource::Volume { volume_name } => volume_name.clone(),
    }
}

/// Convert a `VolumePath` to a `FilesystemPathToken` for service calls.
fn volume_path_to_token(path: &VolumePath) -> FilesystemPathToken {
    if path.is_root() {
        FilesystemPathToken::root_token()
    } else {
        let relative = path.components().join("/");
        FilesystemPathToken::from_relative(&relative)
            .unwrap_or_else(|_| FilesystemPathToken::root_token())
    }
}

fn cancel(slot: &mut Option<CancellationToken>) {
    if let Some(token) = slot.take() {
        token.cancel();
    }
}

fn bump(value: &mut u64) -> u64 {
    *value = value.saturating_add(1);
    *value
}

fn qv(value: &str) -> QVariant {
    QVariant::from(&QString::from(value))
}

fn qstring(value: &str) -> QString {
    QString::from(value)
}

fn unique_open_temp_path(file_name: &str) -> Result<PathBuf, String> {
    let safe_name = file_name
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_' | ' ') {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>();
    let safe_name = if safe_name.trim().is_empty() {
        "image-file".into()
    } else {
        safe_name
    };
    let mut path = std::env::temp_dir();
    path.push(format!(
        "tuxstack-open-{}-{}-{}",
        std::process::id(),
        uuid_simple(),
        safe_name
    ));
    Ok(path)
}

fn open_path_with_default_app(path: &std::path::Path) -> Result<(), String> {
    use std::process::{Command, Stdio};

    #[cfg(target_os = "linux")]
    let mut command = {
        let mut cmd = Command::new("xdg-open");
        cmd.arg(path);
        cmd
    };
    #[cfg(target_os = "macos")]
    let mut command = {
        let mut cmd = Command::new("open");
        cmd.arg(path);
        cmd
    };
    #[cfg(target_os = "windows")]
    let mut command = {
        let mut cmd = Command::new("cmd");
        cmd.args(["/C", "start", "", &path.display().to_string()]);
        cmd
    };
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        let _ = path;
        return Err(
            "Opening files with the system default application is not supported on this platform."
                .into(),
        );
    }

    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|error| {
            format!("Could not open the file with the system default application: {error}")
        })
}

fn local_destination(value: &str) -> Result<PathBuf, String> {
    let value = value.trim();
    if value.is_empty() {
        return Err("A download destination is required.".into());
    }
    let path = if let Some(rest) = value.strip_prefix("file://") {
        if rest.starts_with('/') {
            rest.to_string()
        } else if let Some(path) = rest.strip_prefix("localhost/") {
            format!("/{path}")
        } else {
            return Err("The download destination must be a local file URL.".into());
        }
    } else if let Some(path) = value.strip_prefix("file:") {
        path.to_string()
    } else if value.contains("://") {
        return Err("The download destination must be a local file path.".into());
    } else {
        value.to_string()
    };
    Ok(PathBuf::from(path))
}

fn uuid_simple() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{nanos:x}")
}
