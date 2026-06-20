#include "preset_manager.hpp"
#include <QCoreApplication>
#include <QDir>
#include <QFile>
#include <QJsonDocument>
#include <QJsonObject>
#include <QSignalSpy>
#include <QTest>

using namespace AviQtl::Core;

class TestPresetManager : public QObject {
    Q_OBJECT

  private:
    QString m_testDir;

  private slots:
    void init() {
        m_testDir = QCoreApplication::applicationDirPath() + QStringLiteral("/presets");
        QDir().mkpath(m_testDir);
    }

    void cleanup() {
        QDir(m_testDir).removeRecursively();
    }

    void saveAndLoad() {
        const QString effectId = QStringLiteral("test_effect");
        const QString name = QStringLiteral("Warm Look");

        QVariantMap params;
        params[QStringLiteral("brightness")] = 110.0;
        params[QStringLiteral("contrast")] = 95.0;
        params[QStringLiteral("saturation")] = 1.2;

        QVariantMap keyframes;
        QVERIFY(PresetManager::instance().savePreset(effectId, name, params, keyframes, true));

        auto names = PresetManager::instance().presetNames(effectId);
        QVERIFY(names.contains(name));

        auto loaded = PresetManager::instance().loadPreset(effectId, name);
        QCOMPARE(loaded.value(QStringLiteral("effectId")).toString(), effectId);
        QCOMPARE(loaded.value(QStringLiteral("name")).toString(), name);
        QVERIFY(loaded.value(QStringLiteral("enabled")).toBool());

        auto loadedParams = loaded.value(QStringLiteral("params")).toMap();
        QCOMPARE(loadedParams.value(QStringLiteral("brightness")).toDouble(), 110.0);
        QCOMPARE(loadedParams.value(QStringLiteral("contrast")).toDouble(), 95.0);
        QCOMPARE(loadedParams.value(QStringLiteral("saturation")).toDouble(), 1.2);
    }

    void deletePreset() {
        const QString effectId = QStringLiteral("test_effect");
        const QString name = QStringLiteral("To Delete");

        QVariantMap params;
        params[QStringLiteral("x")] = 10.0;

        PresetManager::instance().savePreset(effectId, name, params, {}, false);
        QVERIFY(PresetManager::instance().presetNames(effectId).contains(name));

        QVERIFY(PresetManager::instance().deletePreset(effectId, name));
        QVERIFY(!PresetManager::instance().presetNames(effectId).contains(name));
    }

    void renamePreset() {
        const QString effectId = QStringLiteral("test_effect");
        const QString oldName = QStringLiteral("Old Name");
        const QString newName = QStringLiteral("New Name");

        QVariantMap params;
        PresetManager::instance().savePreset(effectId, oldName, params, {}, true);

        QVERIFY(PresetManager::instance().renamePreset(effectId, oldName, newName));
        QVERIFY(!PresetManager::instance().presetNames(effectId).contains(oldName));
        QVERIFY(PresetManager::instance().presetNames(effectId).contains(newName));
    }

    void loadNonexistent() {
        auto loaded = PresetManager::instance().loadPreset(QStringLiteral("nonexistent"), QStringLiteral("none"));
        QVERIFY(loaded.isEmpty());
    }

    void presetNamesEmpty() {
        auto names = PresetManager::instance().presetNames(QStringLiteral("nonexistent_effect"));
        QVERIFY(names.isEmpty());
    }

    void signalsEmitted() {
        QSignalSpy spy(&PresetManager::instance(), &PresetManager::presetsChanged);

        const QString effectId = QStringLiteral("signal_test");
        QVariantMap params;

        PresetManager::instance().savePreset(effectId, QStringLiteral("test"), params, {}, true);
        QCOMPARE(spy.count(), 1);
        QCOMPARE(spy.at(0).at(0).toString(), effectId);

        PresetManager::instance().deletePreset(effectId, QStringLiteral("test"));
        QCOMPARE(spy.count(), 2);
    }
};

QTEST_MAIN(TestPresetManager)
#include "test_preset_manager.moc"
