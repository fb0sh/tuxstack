//! QML-facing terminal detection and preference model.

use cxx_qt::CxxQtType;
use cxx_qt_lib::{QModelIndex, QString, QVariant};
use std::pin::Pin;

use crate::terminal::{
    DetectedTerminal, TerminalConfigStore, TerminalDetector, TerminalId, TerminalLaunchError,
    TuxStackCliResolver,
};

const ROLE_TERMINAL_ID: i32 = 257;
const ROLE_DISPLAY_NAME: i32 = 258;
const ROLE_EXECUTABLE_PATH: i32 = 259;
const ROLE_DESKTOP_PREFERRED: i32 = 260;
const ROLE_AVAILABLE: i32 = 261;
const ROLE_SELECTED: i32 = 262;

#[derive(Default)]
pub struct TerminalApplicationModelRust {
    terminals: Vec<DetectedTerminal>,
    selected: TerminalId,
    count: i32,
    selected_terminal_id: QString,
    error_message: QString,
}

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!(<QAbstractListModel>);
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

    impl cxx_qt::Threading for TerminalApplicationModel {}

    unsafe extern "RustQt" {
        #[qobject]
        #[qml_element]
        #[base = QAbstractListModel]
        #[qproperty(i32, count)]
        #[qproperty(QString, error_message, cxx_name = "errorMessage")]
        #[qproperty(QString, selected_terminal_id, cxx_name = "selectedTerminalId")]
        type TerminalApplicationModel = super::TerminalApplicationModelRust;

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

        #[qinvokable]
        #[cxx_name = "refreshTerminals"]
        fn refresh_terminals(self: Pin<&mut Self>);
        #[qinvokable]
        #[cxx_name = "setDefaultTerminal"]
        fn set_default_terminal(self: Pin<&mut Self>, terminal_id: &QString);
        #[qinvokable]
        #[cxx_name = "testTerminal"]
        fn test_terminal(self: Pin<&mut Self>, terminal_id: &QString);
    }
}

impl qobject::TerminalApplicationModel {
    pub fn row_count(&self, _parent: &QModelIndex) -> i32 {
        self.terminals.len().min(i32::MAX as usize) as i32
    }

    pub fn data(&self, index: &QModelIndex, role: i32) -> QVariant {
        let Some(terminal) = self.terminals.get(index.row().max(0) as usize) else {
            return QVariant::default();
        };
        match role {
            ROLE_TERMINAL_ID => QVariant::from(&QString::from(terminal.id.as_str())),
            ROLE_DISPLAY_NAME => QVariant::from(&QString::from(&terminal.display_name)),
            ROLE_EXECUTABLE_PATH => QVariant::from(&QString::from(
                terminal.executable.to_string_lossy().as_ref(),
            )),
            ROLE_DESKTOP_PREFERRED => {
                let value = terminal.desktop_preferred;
                QVariant::from(&value)
            }
            ROLE_AVAILABLE => {
                let value = true;
                QVariant::from(&value)
            }
            ROLE_SELECTED => {
                let value = terminal.id == self.selected;
                QVariant::from(&value)
            }
            _ => QVariant::default(),
        }
    }

    pub fn role_names(&self) -> qobject::QHash_i32_QByteArray {
        let mut roles = qobject::QHash_i32_QByteArray::default();
        roles.insert(ROLE_TERMINAL_ID, "terminalId".into());
        roles.insert(ROLE_DISPLAY_NAME, "displayName".into());
        roles.insert(ROLE_EXECUTABLE_PATH, "executablePath".into());
        roles.insert(ROLE_DESKTOP_PREFERRED, "desktopPreferred".into());
        roles.insert(ROLE_AVAILABLE, "available".into());
        roles.insert(ROLE_SELECTED, "selected".into());
        roles
    }

    pub fn refresh_terminals(mut self: Pin<&mut Self>) {
        let selected = TerminalConfigStore::default_path()
            .and_then(|path| TerminalConfigStore::new(path).read_preference().ok())
            .unwrap_or(TerminalId::Auto);
        let terminals = TerminalDetector.detect();
        let effective =
            if selected != TerminalId::Auto && terminals.iter().any(|item| item.id == selected) {
                selected
            } else {
                TerminalId::Auto
            };
        self.as_mut().begin_reset_model();
        self.as_mut().rust_mut().terminals = terminals;
        self.as_mut().rust_mut().selected = effective;
        self.as_mut().end_reset_model();
        let count = self.terminals.len().min(i32::MAX as usize) as i32;
        self.as_mut().rust_mut().count = count;
        self.as_mut().set_count(count);
        self.as_mut()
            .set_selected_terminal_id(QString::from(effective.as_str()));
        self.as_mut().set_error_message(QString::default());
    }

    pub fn set_default_terminal(mut self: Pin<&mut Self>, terminal_id: &QString) {
        let Some(id) = TerminalId::parse(&terminal_id.to_string()) else {
            return;
        };
        if id != TerminalId::Auto && !self.terminals.iter().any(|item| item.id == id) {
            return;
        }
        let Some(path) = TerminalConfigStore::default_path() else {
            self.as_mut().set_error_message(QString::from(
                "Could not determine the configuration directory.",
            ));
            return;
        };
        match TerminalConfigStore::new(path).write_preference(id) {
            Ok(()) => {
                self.as_mut().rust_mut().selected = id;
                self.as_mut()
                    .set_selected_terminal_id(QString::from(id.as_str()));
                self.as_mut().set_error_message(QString::default());
            }
            Err(error) => self
                .as_mut()
                .set_error_message(QString::from(error.to_string())),
        }
    }

    pub fn test_terminal(mut self: Pin<&mut Self>, terminal_id: &QString) {
        let Some(id) = TerminalId::parse(&terminal_id.to_string()) else {
            return;
        };
        let result = TerminalApplicationController::test(id);
        if let Err(error) = result {
            self.as_mut()
                .set_error_message(QString::from(error.to_string()));
        } else {
            self.as_mut().set_error_message(QString::default());
        }
    }
}

struct TerminalApplicationController;

impl TerminalApplicationController {
    fn test(id: TerminalId) -> Result<(), TerminalLaunchError> {
        let Some(path) = TerminalConfigStore::default_path() else {
            return Err(TerminalLaunchError::SettingsReadFailed(
                "configuration directory unavailable".into(),
            ));
        };
        let launcher = crate::terminal::SystemTerminalLauncher {
            detector: TerminalDetector,
            settings: TerminalConfigStore::new(path),
            cli_resolver: TuxStackCliResolver,
        };
        launcher.launch_test_terminal(id)
    }
}
