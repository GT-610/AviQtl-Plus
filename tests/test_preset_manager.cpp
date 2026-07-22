#include "preset_manager.hpp"
#include <QCoreApplication>
#include <QDir>
#include <QFile>
#include <QJsonDocument>
#include <QJsonObject>
#include <QSignalSpy>
#include <QTest>
#ifndef Q_OS_WIN
#include <unistd.h>
#endif

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

    void loadNonexistent() {
        auto loaded = PresetManager::instance().loadPreset(QStringLiteral("nonexistent"), QStringLiteral("none"));
        QVERIFY(loaded.isEmpty());
    }

    void presetNamesEmpty() {
        auto names = PresetManager::instance().presetNames(QStringLiteral("nonexistent_effect"));
        QVERIFY(names.isEmpty());
    }

    void rejectsUnsafeNames() {
        const QStringList unsafeNames = {
            QString(),
            QStringLiteral("."),
            QStringLiteral("../escape"),
            QStringLiteral("nested/name"),
            QStringLiteral("nested\\name"),
            QStringLiteral(".hidden"),
        };
        for (const QString &name : unsafeNames) {
            QVERIFY(!PresetManager::instance().savePreset(name, QStringLiteral("safe"), {}, {}, true));
            QVERIFY(!PresetManager::instance().savePreset(QStringLiteral("safe"), name, {}, {}, true));
        }
    }

    void overwritesPresetAtomically() {
        const QString effectId = QStringLiteral("atomic_effect");
        const QString name = QStringLiteral("Atomic");
        QVERIFY(PresetManager::instance().savePreset(effectId, name,
                                                     {{QStringLiteral("value"), 1}}, {}, true));
        QVERIFY(PresetManager::instance().savePreset(effectId, name,
                                                     {{QStringLiteral("value"), 2}}, {}, false));

        const QVariantMap loaded = PresetManager::instance().loadPreset(effectId, name);
        QCOMPARE(loaded.value(QStringLiteral("params")).toMap().value(QStringLiteral("value")).toInt(), 2);
        QVERIFY(!loaded.value(QStringLiteral("enabled")).toBool());

        QDir effectDir(m_testDir + QLatin1Char('/') + effectId);
        const QStringList temporaryFiles = effectDir.entryList({QStringLiteral("Atomic.json.*")}, QDir::Files);
        QVERIFY(temporaryFiles.isEmpty());
    }

    void rejectsSymlinkedEffectDirectory() {
        QTemporaryDir outside;
        QVERIFY(outside.isValid());

        const QString linkedDir = m_testDir + QStringLiteral("/linked_effect");
        if (!QFile::link(outside.path(), linkedDir))
            QSKIP("Directory symlinks are not supported in this environment");

        QVERIFY(!PresetManager::instance().savePreset(QStringLiteral("linked_effect"),
                                                      QStringLiteral("Escape"), {}, {}, true));
        QVERIFY(!QFile::exists(outside.filePath(QStringLiteral("Escape.json"))));
        QFile::remove(linkedDir);
    }

    void failedOverwritePreservesExistingPreset() {
#ifdef Q_OS_WIN
        QSKIP("Directory permission failure is not deterministic on Windows");
#else
        if (::geteuid() == 0)
            QSKIP("Directory permission failure cannot be tested as root");

        const QString effectId = QStringLiteral("readonly_effect");
        const QString name = QStringLiteral("Existing");
        QVERIFY(PresetManager::instance().savePreset(effectId, name,
                                                     {{QStringLiteral("value"), 1}}, {}, true));

        const QString effectDir = m_testDir + QLatin1Char('/') + effectId;
        const QFileDevice::Permissions originalPermissions = QFile::permissions(effectDir);
        struct PermissionRestore {
            QString path;
            QFileDevice::Permissions permissions;
            ~PermissionRestore() { QFile::setPermissions(path, permissions); }
        } restore{effectDir, originalPermissions};

        QVERIFY(QFile::setPermissions(effectDir,
                                      QFileDevice::ReadOwner | QFileDevice::ExeOwner));
        QVERIFY(!PresetManager::instance().savePreset(effectId, name,
                                                      {{QStringLiteral("value"), 2}}, {}, false));
        const QVariantMap loaded = PresetManager::instance().loadPreset(effectId, name);
        QCOMPARE(loaded.value(QStringLiteral("params")).toMap().value(QStringLiteral("value")).toInt(), 1);
#endif
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
