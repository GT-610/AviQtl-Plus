#include "core/include/document_model.hpp"
#include "core/include/effect_registry.hpp"
#include "core/include/performance_metrics.hpp"
#include "core/include/settings_manager.hpp"
#include "engine/timeline/bake_controller.hpp"
#include "engine/timeline/ecs.hpp"
#include "bridge/ecs_render_bridge.hpp"
#include <QSignalSpy>
#include <QTest>
#include <bitset>

using namespace AviQtl::Core;
using namespace AviQtl::Engine::Timeline;
using namespace AviQtl::UI;

class TestECSRender : public QObject {
    Q_OBJECT

  private slots:
    void init() {
        DocumentModel::instance().clear();
        QObject::disconnect(&DocumentModel::instance(), nullptr, &BakeController::instance(), nullptr);

        std::bitset<MAX_CLIP_ID> empty;
        ECS::instance().syncClipIds(empty);
        ECS::instance().commit();
        if (qstrcmp(QTest::currentTestFunction(), "accessorChangeIsLatchedUntilNotification") != 0)
            ECSRenderBridge::instance().notifyFrameReady();
        PerformanceMetrics::instance().reset();

        EffectRegistry::instance().registerEffect({
            .id = QStringLiteral("transform"),
            .name = QStringLiteral("Transform"),
            .kind = QStringLiteral("effect"),
            .categories = {QStringLiteral("standard")},
            .defaultParams = {
                {QStringLiteral("x"), 0}, {QStringLiteral("y"), 0}, {QStringLiteral("z"), 0},
                {QStringLiteral("scale"), 100}, {QStringLiteral("rotationX"), 0},
                {QStringLiteral("rotationY"), 0}, {QStringLiteral("rotationZ"), 0},
                {QStringLiteral("opacity"), 1.0}
            }
        });
    }

    void cleanup() {
        SettingsManager::instance().setValue(QStringLiteral("bakeStrategy"), QStringLiteral("FullBake"));
    }

    void accessorChangeIsLatchedUntilNotification() {
        SceneSettings scene;
        scene.id = 6;
        scene.fps = AviQtl::kDefaultFps;

        Clip clip;
        clip.id = 91;
        clip.durationFrames = 30;
        clip.type = QStringLiteral("text");
        scene.clips.push_back(clip);
        DocumentModel::instance().addScene(scene);
        BakeController::instance().bake(scene.id, 0);

        ECSRenderBridge &bridge = ECSRenderBridge::instance();
        QSignalSpy spy(&bridge, &ECSRenderBridge::renderStatesChanged);
        const quint64 initialRevision = bridge.renderRevision();
        QVERIFY(bridge.renderStateMap().contains(QStringLiteral("91")));
        QCOMPARE(spy.count(), 0);

        bridge.notifyFrameReady();
        QCOMPARE(spy.count(), 1);
        QCOMPARE(bridge.renderRevision(), initialRevision + 1);
    }

    void renderComponentPopulated() {
        SceneSettings scene;
        scene.id = 1;
        scene.fps = AviQtl::kDefaultFps;

        Clip clip;
        clip.id = 5;
        clip.layer = 1;
        clip.startFrame = 10;
        clip.durationFrames = 50;
        clip.type = QStringLiteral("text");

        Effect transformEffect;
        transformEffect.id = QStringLiteral("transform");
        transformEffect.enabled = true;
        transformEffect.params = {
            {QStringLiteral("x"), 100}, {QStringLiteral("y"), 200},
            {QStringLiteral("scale"), 150}, {QStringLiteral("opacity"), 0.8}
        };
        clip.effects.push_back(transformEffect);

        scene.clips.push_back(clip);
        DocumentModel::instance().addScene(scene);

        BakeController::instance().bake(1, 30);

        const auto *snapshot = ECS::instance().getSnapshot();
        QVERIFY(snapshot != nullptr);

        const auto *rc = snapshot->renderStates.find(5);
        QVERIFY(rc != nullptr);
        QCOMPARE(rc->clipId, 5);
        QCOMPARE(rc->layer, 1);
        QCOMPARE(rc->startFrame, 10);
        QCOMPARE(rc->durationFrames, 50);
        QCOMPARE(rc->x, 100.0f);
        QCOMPARE(rc->y, 200.0f);
        QCOMPARE(rc->scaleX, 1.5f);
        QCOMPARE(rc->scaleY, 1.5f);
        QCOMPARE(rc->opacity, 0.8f);
        QCOMPARE(rc->effectCount, static_cast<uint16_t>(1));
    }

    void effectParamBufferPopulated() {
        SceneSettings scene;
        scene.id = 2;
        scene.fps = AviQtl::kDefaultFps;

        Clip clip;
        clip.id = 10;
        clip.layer = 0;
        clip.startFrame = 0;
        clip.durationFrames = 30;
        clip.type = QStringLiteral("rect");

        Effect transformEffect;
        transformEffect.id = QStringLiteral("transform");
        transformEffect.enabled = true;
        transformEffect.params = {
            {QStringLiteral("x"), 50}, {QStringLiteral("y"), 75},
            {QStringLiteral("scale"), 200}, {QStringLiteral("opacity"), 0.5}
        };
        clip.effects.push_back(transformEffect);

        scene.clips.push_back(clip);
        DocumentModel::instance().addScene(scene);

        BakeController::instance().bake(2, 0);

        const auto *snapshot = ECS::instance().getSnapshot();
        QVERIFY(snapshot != nullptr);

        bool hasX = false, hasY = false, hasScale = false, hasOpacity = false;
        for (const auto &entry : snapshot->effectParams.entries) {
            if (entry.clipId != 10)
                continue;
            const QString name = QString::fromUtf8(entry.paramName);
            if (name == QStringLiteral("x")) {
                QCOMPARE(entry.value[0], 50.0f);
                hasX = true;
            } else if (name == QStringLiteral("y")) {
                QCOMPARE(entry.value[0], 75.0f);
                hasY = true;
            } else if (name == QStringLiteral("scale")) {
                QCOMPARE(entry.value[0], 200.0f);
                hasScale = true;
            } else if (name == QStringLiteral("opacity")) {
                QCOMPARE(entry.value[0], 0.5f);
                hasOpacity = true;
            }
        }
        QVERIFY(hasX);
        QVERIFY(hasY);
        QVERIFY(hasScale);
        QVERIFY(hasOpacity);
    }

    void defaultTransformWhenNoTransformEffect() {
        SceneSettings scene;
        scene.id = 3;
        scene.fps = AviQtl::kDefaultFps;

        Clip clip;
        clip.id = 15;
        clip.layer = 0;
        clip.startFrame = 0;
        clip.durationFrames = 30;
        clip.type = QStringLiteral("text");
        // No effects
        scene.clips.push_back(clip);
        DocumentModel::instance().addScene(scene);

        BakeController::instance().bake(3, 0);

        const auto *snapshot = ECS::instance().getSnapshot();
        QVERIFY(snapshot != nullptr);
        const auto *rc = snapshot->renderStates.find(15);
        QVERIFY(rc != nullptr);
        QCOMPARE(rc->x, 0.0f);
        QCOMPARE(rc->y, 0.0f);
        QCOMPARE(rc->z, 0.0f);
        QCOMPARE(rc->scaleX, 1.0f);
        QCOMPARE(rc->scaleY, 1.0f);
        QCOMPARE(rc->opacity, 1.0f);
    }

    void clipSyncRemovesDeletedClips() {
        SceneSettings scene;
        scene.id = 4;
        scene.fps = AviQtl::kDefaultFps;

        Clip c1;
        c1.id = 20;
        c1.startFrame = 0;
        c1.durationFrames = 30;
        c1.type = QStringLiteral("text");
        scene.clips.push_back(c1);

        Clip c2;
        c2.id = 21;
        c2.startFrame = 0;
        c2.durationFrames = 30;
        c2.type = QStringLiteral("text");
        scene.clips.push_back(c2);

        DocumentModel::instance().addScene(scene);
        BakeController::instance().bake(4, 0);

        {
            const auto *snapshot = ECS::instance().getSnapshot();
            QVERIFY(snapshot != nullptr);
            QVERIFY(snapshot->renderStates.contains(20));
            QVERIFY(snapshot->renderStates.contains(21));
        }

        scene.clips = {c1};
        DocumentModel::instance().removeScene(4);
        DocumentModel::instance().addScene(scene);
        BakeController::instance().bake(4, 0);

        {
            const auto *snapshot = ECS::instance().getSnapshot();
            QVERIFY(snapshot != nullptr);
            QVERIFY(snapshot->renderStates.contains(20));
            QVERIFY(!snapshot->renderStates.contains(21));
        }
    }

    void bridgeReusesUnchangedStatesAndSuppressesSignals() {
        SceneSettings scene;
        scene.id = 5;
        scene.fps = AviQtl::kDefaultFps;

        Clip clip;
        clip.id = 90;
        clip.durationFrames = 60;
        Effect transform;
        transform.id = QStringLiteral("transform");
        transform.params = {
            {QStringLiteral("x"), 12.0}, {QStringLiteral("y"), 24.0},
            {QStringLiteral("scale"), 100.0}, {QStringLiteral("opacity"), 1.0},
        };
        clip.effects.push_back(transform);
        scene.clips.push_back(clip);
        DocumentModel::instance().addScene(scene);

        BakeController::instance().bake(scene.id, 0);
        ECSRenderBridge &bridge = ECSRenderBridge::instance();
        bridge.notifyFrameReady();
        const QVariantMap initialMap = bridge.renderStateMap();
        QVERIFY(initialMap.contains(QStringLiteral("90")));
        QCOMPARE(initialMap.value(QStringLiteral("90")).toMap().value(QStringLiteral("x")).toFloat(), 12.0f);

        QSignalSpy spy(&bridge, &ECSRenderBridge::renderStatesChanged);
        PerformanceMetrics::instance().reset();
        BakeController::instance().bake(scene.id, 0);
        bridge.notifyFrameReady();
        QCOMPARE(spy.count(), 0);
        QCOMPARE(PerformanceMetrics::instance().snapshot().value(PerformanceCounter::EcsBridgeStatesReused), quint64{1});

        BakeController::instance().bake(scene.id, 1);
        bridge.notifyFrameReady();
        QCOMPARE(spy.count(), 1);
        QCOMPARE(bridge.getEffectParams(clip.id).value(QStringLiteral("time")).toFloat(), 1.0f);
    }
};

QTEST_MAIN(TestECSRender)
#include "test_ecs_render.moc"
