#include "engine/plugin/audio_plugin_manager.hpp"
#include "settings_manager.hpp"
#include <QCoreApplication>
#include <QDir>
#include <QElapsedTimer>
#include <QFile>
#include <QFileInfo>
#include <QScopeGuard>
#include <QTest>
#include <QTextStream>
#include <algorithm>

using AviQtl::Core::SettingsManager;
using AviQtl::Engine::Plugin::AudioPluginManager;

namespace {
constexpr int kPluginFileCount = 40;

auto runFakeDiscovery(const QString &target) -> int {
    if (target == QStringLiteral("__probe__")) {
        return 0;
    }
    const QFileInfo info(target);
    if (info.completeBaseName().contains(QStringLiteral("corrupt"))) {
        QTextStream(stdout) << "malformed discovery output\n";
        return 0;
    }

    QString suffix = info.completeBaseName().section(QLatin1Char('_'), -1);
    bool ok = false;
    const qlonglong uniqueId = suffix.toLongLong(&ok);
    QTextStream(stdout) << "carla-discovery::init::\n"
                        << "carla-discovery::name::" << info.completeBaseName() << '\n'
                        << "carla-discovery::label::" << info.completeBaseName() << '\n'
                        << "carla-discovery::maker::AviQtl test\n"
                        << "carla-discovery::uniqueId::" << (ok ? uniqueId : 0) << '\n'
                        << "carla-discovery::category::filter\n"
                        << "carla-discovery::audio.ins::2\n"
                        << "carla-discovery::audio.outs::2\n"
                        << "carla-discovery::end::\n";
    return 0;
}
} // namespace

class TestAudioPluginScan : public QObject {
    Q_OBJECT

  private slots:
    void scansLargeDirectoryAndIgnoresMalformedPlugins();
};

void TestAudioPluginScan::scansLargeDirectoryAndIgnoresMalformedPlugins() {
    const QString relativeRoot = QStringLiteral("plugin-scan-fixture");
    const QString root = QCoreApplication::applicationDirPath() + QLatin1Char('/') + relativeRoot;
    QDir(root).removeRecursively();
    QVERIFY(QDir().mkpath(root + QStringLiteral("/nested")));
    const auto removeFixture = qScopeGuard([root]() { QDir(root).removeRecursively(); });

    for (int i = 0; i < kPluginFileCount; ++i) {
        const QString name = (i == kPluginFileCount / 2) ? QStringLiteral("plugin_corrupt.so") : QStringLiteral("plugin_%1.so").arg(i, 3, 10, QLatin1Char('0'));
        QFile file(root + QStringLiteral("/nested/") + name);
        QVERIFY(file.open(QIODevice::WriteOnly));
        QCOMPARE(file.write("fixture"), 7);
    }

    SettingsManager &settings = SettingsManager::instance();
    const QVariantMap originalSettings = settings.settings();
    const auto restoreSettings = qScopeGuard([&settings, originalSettings]() { settings.setSettings(originalSettings); });
    for (const QString &format : {QStringLiteral("LV2"), QStringLiteral("VST2"), QStringLiteral("VST3"), QStringLiteral("CLAP"), QStringLiteral("DSSI"), QStringLiteral("SF2"), QStringLiteral("SFZ")}) {
        settings.setValue(QStringLiteral("pluginEnable") + format, format == QStringLiteral("VST2"));
    }
    settings.setValue(QStringLiteral("pluginPathsVST2"), QStringList{relativeRoot});
    settings.setValue(QStringLiteral("_pluginDiscoveryToolPath"), QCoreApplication::applicationFilePath());
    settings.setValue(QStringLiteral("pluginDiscoveryThreads"), 8);
    settings.setValue(QStringLiteral("pluginDiscoveryTimeoutMs"), 5000);

    QElapsedTimer timer;
    timer.start();
    AudioPluginManager::instance().scanPlugins();
    const qint64 firstScanMs = timer.elapsed();
    const QVariantList first = AudioPluginManager::instance().getPluginList();
    QCOMPARE(first.size(), kPluginFileCount - 1);
    QVERIFY(std::all_of(first.cbegin(), first.cend(), [](const QVariant &entry) {
        const QVariantMap plugin = entry.toMap();
        return plugin.value(QStringLiteral("format")) == QStringLiteral("VST2") && plugin.value(QStringLiteral("category")) == QStringLiteral("Filter");
    }));

    timer.restart();
    AudioPluginManager::instance().scanPlugins();
    const qint64 repeatScanMs = timer.elapsed();
    QCOMPARE(AudioPluginManager::instance().getPluginList().size(), kPluginFileCount - 1);

    QTextStream(stdout) << "audio_plugin_scan targets=" << kPluginFileCount << " valid=" << first.size() << " first_ms=" << firstScanMs << " repeat_ms=" << repeatScanMs << " threads=8" << Qt::endl;
}

int main(int argc, char **argv) {
    if (argc == 3 && argv[1][0] != '-') {
        return runFakeDiscovery(QString::fromLocal8Bit(argv[2]));
    }
    QCoreApplication app(argc, argv);
    TestAudioPluginScan test;
    return QTest::qExec(&test, argc, argv);
}

#include "test_audio_plugin_scan.moc"
