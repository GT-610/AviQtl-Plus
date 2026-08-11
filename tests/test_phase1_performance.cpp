#include "bridge/ecs_render_bridge.hpp"
#include "document_model.hpp"
#include "effect_registry.hpp"
#include "performance_metrics.hpp"
#include "settings_manager.hpp"
#include "engine/timeline/bake_controller.hpp"

#include <QJsonDocument>
#include <QQmlComponent>
#include <QQmlEngine>
#include <QScopeGuard>
#include <QTest>
#include <QTextStream>
#include <memory>

using namespace AviQtl::Core;
using namespace AviQtl::Engine::Timeline;
using namespace AviQtl::UI;

class TestPhase1Performance : public QObject {
    Q_OBJECT

  private slots:
    void splashViewResourceLoadsWithoutWidgets() {
        QQmlEngine engine;
        QQmlComponent component(&engine, QUrl(QStringLiteral("qrc:/qt/qml/AviQtl/ui/qml/SplashView.qml")));
        QVERIFY2(component.isReady(), qPrintable(component.errorString()));
        std::unique_ptr<QObject> splash(component.create());
        QVERIFY2(splash != nullptr, qPrintable(component.errorString()));
    }

    void scrubsLargeObjectTimelineAndPrintsJson() {
        constexpr int kClipCount = 3'000;
        constexpr int kLastFrame = 600;
        constexpr int kFrameStep = 5;

        SettingsManager &settings = SettingsManager::instance();
        const QVariantMap originalSettings = settings.settings();
        const auto restoreSettings = qScopeGuard([&settings, originalSettings]() { settings.setSettings(originalSettings); });
        settings.setValue(QStringLiteral("bakeStrategy"), QStringLiteral("OnDemand"));
        settings.setValue(QStringLiteral("onDemandPrefetchFrames"), 30);

        DocumentModel::instance().clear();
        EffectRegistry::instance().registerEffect({
            .id = QStringLiteral("phase1-metric"),
            .name = QStringLiteral("Phase 1 Metric"),
            .kind = QStringLiteral("effect"),
            .categories = {QStringLiteral("test")},
            .defaultParams = {{QStringLiteral("value"), 0.0}},
        });

        SceneSettings scene;
        scene.id = 100;
        scene.clips.reserve(kClipCount);
        for (int i = 0; i < kClipCount; ++i) {
            Clip clip;
            clip.id = i;
            clip.startFrame = i * 3;
            clip.durationFrames = 45;
            Effect effect;
            effect.id = QStringLiteral("phase1-metric");
            effect.params.insert(QStringLiteral("value"), i);
            clip.effects.push_back(std::move(effect));
            scene.clips.push_back(std::move(clip));
        }
        DocumentModel::instance().addScene(scene);

        PerformanceMetrics::instance().reset();
        int bakeCalls = 0;
        for (int frame = 0; frame <= kLastFrame; frame += kFrameStep) {
            BakeController::instance().bake(scene.id, frame);
            ECSRenderBridge::instance().notifyFrameReady();
            ++bakeCalls;
        }

        const PerformanceSnapshot snapshot = PerformanceMetrics::instance().snapshot();
        QCOMPARE(snapshot.value(PerformanceCounter::BakeCalls), static_cast<quint64>(bakeCalls));
        QCOMPARE(snapshot.value(PerformanceCounter::BakeTrackCacheMisses), quint64{kClipCount});
        QVERIFY(snapshot.value(PerformanceCounter::BakeTrackCacheHits) > 0);
        QVERIFY(snapshot.value(PerformanceCounter::BakeClipsVisited) < static_cast<quint64>(bakeCalls * kClipCount / 10));
        QVERIFY(snapshot.value(PerformanceCounter::EcsBridgeStatesReused) > 0);

        const QJsonObject context{
            {QStringLiteral("clip_count"), kClipCount},
            {QStringLiteral("first_frame"), 0},
            {QStringLiteral("last_frame"), kLastFrame},
            {QStringLiteral("frame_step"), kFrameStep},
            {QStringLiteral("strategy"), QStringLiteral("OnDemand")},
        };
        const QByteArray json = QJsonDocument(PerformanceMetrics::instance().report(
            QStringLiteral("phase1-large-object-scrub"), context)).toJson(QJsonDocument::Compact);
        QTextStream(stdout) << json << Qt::endl;
    }
};

QTEST_MAIN(TestPhase1Performance)
#include "test_phase1_performance.moc"
