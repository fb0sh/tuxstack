import QtQuick
import org.tuxstack.app

/**
 * Read-only image file browser body.
 *
 * Reuses the volume file browser layout; only the resource-specific strings
 * differ (see VolumeFilesView's viewKind / string properties). The image
 * browsing backend runs a hardened temporary container from the selected
 * image and execs listing commands into it.
 */
VolumeFilesView {
    id: root

    viewKind: "image"
    filesErrorTitle: I18n.i18nd("tuxstack", "Image files could not be loaded.")
    unsupportedTitle: I18n.i18nd("tuxstack", "This image cannot be browsed.")
    unsupportedExplanation: I18n.i18nd("tuxstack",
        "The image does not include a shell or basic file utilities (for example scratch or distroless images), so its filesystem cannot be explored.")
    startingMessage: I18n.i18nd("tuxstack", "Preparing read-only image access…")
    saveAsTitle: I18n.i18nd("tuxstack", "Save Image File As")
}
