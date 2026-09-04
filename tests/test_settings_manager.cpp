#include "settings_manager.hpp"
#include <QCoreApplication>
#include <QFile>
#include <QFileInfo>
#include <QJsonDocument>
#include <QJsonObject>
#include <QScopeGuard>
#include <QSignalSpy>
#include <QTest>

using namespace AviQtl::Core;

class TestSettingsManager : public QObject {
    Q_OBJECT

  private slots:
    void defaultValue() {
        QVariant theme = SettingsManager::instance().value(QStringLiteral("theme"));
        QCOMPARE(theme.toString(), QStringLiteral("Dark"));
    }

    void defaultMaxImageSize() {
        QVariant val = SettingsManager::instance().value(QStringLiteral("maxImageSize"));
        QCOMPARE(val.toInt(), 8192);
    }

    void performanceDefaults() {
        SettingsManager &settings = SettingsManager::instance();
        QCOMPARE(settings.doubleValue(QStringLiteral("previewRenderScale"), 0.0), 1.0);
        QCOMPARE(settings.intValue(QStringLiteral("previewMsaaSamples"), -1), 0);
        QCOMPARE(settings.intValue(QStringLiteral("exportEncoderQueueMB"), 0), 128);
    }

    void typedValuesUseRustState() {
        SettingsManager &settings = SettingsManager::instance();
        settings.setValue(QStringLiteral("_test.intString"), QStringLiteral("42"));
        settings.setValue(QStringLiteral("_test.double"), 1.25);
        settings.setValue(QStringLiteral("_test.falseString"), QStringLiteral("false"));
        settings.setValue(QStringLiteral("_test.trueString"), QStringLiteral("enabled"));

        QCOMPARE(settings.intValue(QStringLiteral("_test.intString"), -1), 42);
        QCOMPARE(settings.doubleValue(QStringLiteral("_test.double"), -1.0), 1.25);
        QVERIFY(!settings.boolValue(QStringLiteral("_test.falseString"), true));
        QVERIFY(settings.boolValue(QStringLiteral("_test.trueString"), false));
        QCOMPARE(settings.intValue(QStringLiteral("_test.missing"), 77), 77);

        settings.removeValue(QStringLiteral("_test.intString"));
        settings.removeValue(QStringLiteral("_test.double"));
        settings.removeValue(QStringLiteral("_test.falseString"));
        settings.removeValue(QStringLiteral("_test.trueString"));
    }

    void setAndGetValue() {
        // Use underscore prefix to avoid disk save side-effect
        SettingsManager::instance().setValue(QStringLiteral("_test.integer"), 42);
        QCOMPARE(SettingsManager::instance().value(QStringLiteral("_test.integer")).toInt(), 42);
    }

    void removeValue() {
        SettingsManager::instance().setValue(QStringLiteral("_test.removed"), 42);
        SettingsManager::instance().removeValue(QStringLiteral("_test.removed"));
        QVERIFY(!SettingsManager::instance().settings().contains(QStringLiteral("_test.removed")));
    }

    void runtimeValuesAreNeverPersisted() {
        SettingsManager &settings = SettingsManager::instance();
        const QVariantMap originalSettings = settings.settings();
        const QString settingsPath = QCoreApplication::applicationDirPath() + QStringLiteral("/aviqtl_settings.json");
        QVERIFY(QFileInfo(QCoreApplication::applicationDirPath()).isWritable());

        QFile originalFile(settingsPath);
        const bool originalFileExisted = originalFile.exists();
        QByteArray originalPayload;
        if (originalFileExisted) {
            QVERIFY(originalFile.open(QIODevice::ReadOnly));
            originalPayload = originalFile.readAll();
            originalFile.close();
        }
        const auto restoreSettings = qScopeGuard([&settings, originalSettings, settingsPath, originalFileExisted, originalPayload]() -> void {
            settings.setSettings(originalSettings);
            if (originalFileExisted) {
                QFile file(settingsPath);
                if (file.open(QIODevice::WriteOnly | QIODevice::Truncate)) {
                    file.write(originalPayload);
                }
            } else {
                QFile::remove(settingsPath);
            }
        });

        const QString runtimeKey = QStringLiteral("_test.runtimeOnly");
        const QString persistentKey = QStringLiteral("showConfirmOnClose");
        const bool persistentValue = !settings.value(persistentKey).toBool();
        settings.setValue(runtimeKey, QStringLiteral("must-not-persist"));
        settings.setValue(persistentKey, persistentValue);

        QFile persistedFile(settingsPath);
        QVERIFY(persistedFile.open(QIODevice::ReadOnly));
        const QJsonDocument persistedDocument = QJsonDocument::fromJson(persistedFile.readAll());
        QVERIFY(persistedDocument.isObject());
        QVERIFY(!persistedDocument.object().contains(runtimeKey));
        QCOMPARE(persistedDocument.object().value(persistentKey).toBool(), persistentValue);

        settings.removeValue(runtimeKey);
        settings.load();
        QVERIFY(!settings.settings().contains(runtimeKey));
        QCOMPARE(settings.value(persistentKey).toBool(), persistentValue);
    }

    void valueWithDefault() {
        QVariant val = SettingsManager::instance().value(QStringLiteral("_test.nonexistent"), QStringLiteral("fallback"));
        QCOMPARE(val.toString(), QStringLiteral("fallback"));
    }

    void unchangedValueNoSignal() {
        QSignalSpy spy(&SettingsManager::instance(), &SettingsManager::settingsChanged);
        SettingsManager::instance().setValue(QStringLiteral("theme"), QStringLiteral("Dark")); // same as default
        QCOMPARE(spy.count(), 0);
    }

    void changedValueEmitsSignal() {
        QSignalSpy spy(&SettingsManager::instance(), &SettingsManager::settingsChanged);
        SettingsManager::instance().setValue(QStringLiteral("_test.signal"), QStringLiteral("A"));
        QCOMPARE(spy.count(), 1);

        SettingsManager::instance().setValue(QStringLiteral("_test.signal"), QStringLiteral("B"));
        QCOMPARE(spy.count(), 2);
    }

    void shortcutsDefaultExists() {
        QVariantMap map = SettingsManager::instance().shortcuts();
        QVERIFY(map.contains(QStringLiteral("project.new")));
        QVERIFY(map.contains(QStringLiteral("edit.undo")));
        QVERIFY(map.contains(QStringLiteral("transport.playPause")));
        QCOMPARE(map.value(QStringLiteral("project.new")).toString(), QStringLiteral("Ctrl+N"));
    }

    void shortcutLookup() {
        QString val = SettingsManager::instance().shortcut(QStringLiteral("project.save"), QStringLiteral("fallback"));
        QCOMPARE(val, QStringLiteral("Ctrl+S"));
    }

    void shortcutFallback() {
        QString val = SettingsManager::instance().shortcut(QStringLiteral("_nonexistent.action"), QStringLiteral("None"));
        QCOMPARE(val, QStringLiteral("None"));
    }
};

QTEST_MAIN(TestSettingsManager)
#include "test_settings_manager.moc"
