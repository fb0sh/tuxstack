#include "application_icon.h"

#include <QGuiApplication>
#include <QIcon>

bool set_tuxstack_application_icon() {
    const QIcon icon(QStringLiteral(
        ":/qt/qml/org/tuxstack/app/qml/assets/io.github.tuxstack.TuxStack.png"));
    QGuiApplication::setWindowIcon(icon);
    return !icon.isNull();
}
