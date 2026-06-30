#include "effect_registry.hpp"
#include <QCoreApplication>
#include <QDir>
#include <QFile>
#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QTest>

using namespace AviQtl::Core;

class TestTransitionEffects : public QObject {
    Q_OBJECT

  private slots:
    void dissolveJsonValid();
    void pushJsonValid();
    void zoomJsonValid();
    void wipeJsonValid();
    void noCrossFadeJson();

  private:
    QString transitionsDir() const;
    QJsonObject loadJson(const QString &fileName) const;
};

QString TestTransitionEffects::transitionsDir() const {
    QString srcDir = qgetenv("SRCDIR");
    if (!srcDir.isEmpty()) {
        QDir d(srcDir);
        if (d.exists(QStringLiteral("ui/qml/transitions")))
            return d.filePath(QStringLiteral("ui/qml/transitions"));
        if (d.exists(QStringLiteral("../../ui/qml/transitions")))
            return d.filePath(QStringLiteral("../../ui/qml/transitions"));
    }
#ifdef AVIQTL_SOURCE_DIR
    {
        QDir d(QStringLiteral(AVIQTL_SOURCE_DIR));
        if (d.exists(QStringLiteral("ui/qml/transitions")))
            return d.filePath(QStringLiteral("ui/qml/transitions"));
    }
#endif
    QString appDir = QCoreApplication::applicationDirPath();
    QDir d(appDir);
    if (d.exists(QStringLiteral("../../ui/qml/transitions")))
        return d.filePath(QStringLiteral("../../ui/qml/transitions"));
    if (d.exists(QStringLiteral("../../../ui/qml/transitions")))
        return d.filePath(QStringLiteral("../../../ui/qml/transitions"));
    if (d.exists(QStringLiteral("../../../../ui/qml/transitions")))
        return d.filePath(QStringLiteral("../../../../ui/qml/transitions"));
    return QStringLiteral("%1/../../../../ui/qml/transitions").arg(appDir);
}

QJsonObject TestTransitionEffects::loadJson(const QString &fileName) const {
    QFile file(transitionsDir() + QLatin1Char('/') + fileName);
    if (!file.open(QIODevice::ReadOnly))
        return {};
    return QJsonDocument::fromJson(file.readAll()).object();
}

void TestTransitionEffects::dissolveJsonValid() {
    QJsonObject obj = loadJson("dissolve.json");
    QVERIFY(!obj.isEmpty());
    QCOMPARE(obj["id"].toString(), QStringLiteral("dissolve"));
    QCOMPARE(obj["kind"].toString(), QStringLiteral("transition"));
    QCOMPARE(obj["version"].toString(), QStringLiteral("1.0.0"));
    QVERIFY(obj["categories"].toArray().size() > 0);
    QVERIFY(obj.contains("params"));
    QVERIFY(obj.contains("ui"));
}

void TestTransitionEffects::pushJsonValid() {
    QJsonObject obj = loadJson("push.json");
    QVERIFY(!obj.isEmpty());
    QCOMPARE(obj["id"].toString(), QStringLiteral("push"));
    QCOMPARE(obj["kind"].toString(), QStringLiteral("transition"));
    QCOMPARE(obj["version"].toString(), QStringLiteral("1.0.0"));
    QJsonObject params = obj["params"].toObject();
    QVERIFY(params.contains("direction"));
}

void TestTransitionEffects::zoomJsonValid() {
    QJsonObject obj = loadJson("zoom.json");
    QVERIFY(!obj.isEmpty());
    QCOMPARE(obj["id"].toString(), QStringLiteral("zoom"));
    QCOMPARE(obj["kind"].toString(), QStringLiteral("transition"));
    QJsonObject params = obj["params"].toObject();
    QVERIFY(params.contains("zoomScale"));
}

void TestTransitionEffects::wipeJsonValid() {
    QJsonObject obj = loadJson("wipe.json");
    QVERIFY(!obj.isEmpty());
    QCOMPARE(obj["id"].toString(), QStringLiteral("wipe"));
    QCOMPARE(obj["kind"].toString(), QStringLiteral("transition"));
    QJsonObject params = obj["params"].toObject();
    QVERIFY(params.contains("direction"));
}

void TestTransitionEffects::noCrossFadeJson() {
    QString dir = transitionsDir();
    QFile file(dir + QStringLiteral("/cross_fade.json"));
    QVERIFY2(!file.exists(), qPrintable(file.fileName()));
    QFile qml(dir + QStringLiteral("/CrossFade.qml"));
    QVERIFY2(!qml.exists(), qml.fileName().toUtf8());
}

#include "test_transition_effects.moc"
QTEST_MAIN(TestTransitionEffects)