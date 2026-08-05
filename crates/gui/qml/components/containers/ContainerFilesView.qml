import QtQuick
import org.tuxstack.app

/** Container Files wrapper over the shared local FUSE browser. */
LocalFuseFilesView {
    id: root

    property bool localEndpoint: true
    signal hostPathRequested(string path)

    // Named-volume routes stay in TuxStack's Volumes page. Bind actions are
    // opened by LocalFuseFilesView through Qt.openUrlExternally(fileUrl).
}
