#include "core/include/document_model.hpp"
#include "core/include/effect_registry.hpp"
#include "core/include/performance_metrics.hpp"
#include "core/include/settings_manager.hpp"
#include "engine/timeline/bake_controller.hpp"
#include "engine/timeline/ecs.hpp"
#include <QSignalSpy>
#include <QTest>
#include <algorithm>
#include <bitset>
#include <limits>

using namespace AviQtl::Core;
using namespace AviQtl::Engine::Timeline;

class TestBakeController : public QObject {
    Q_OBJECT

  private slots:
    void init() {
        DocumentModel::instance().clear();

        // Isolate BakeController from DocumentModel::structureChanged
        // to prevent accidental rebakes during test setup.
        QObject::disconnect(&DocumentModel::instance(), nullptr, &BakeController::instance(), nullptr);

        // Clear ECS residual entities by syncing with an empty alive set.
        std::bitset<MAX_CLIP_ID> empty;
        ECS::instance().syncClipIds(empty);
        ECS::instance().commit();

        SettingsManager::instance().setValue(QStringLiteral("bakeStrategy"), QStringLiteral("FullBake"));
        SettingsManager::instance().setValue(QStringLiteral("onDemandPrefetchFrames"), 30);
        PerformanceMetrics::instance().reset();

        EffectRegistry::instance().registerEffect({
            .id = QStringLiteral("cache-test"),
            .name = QStringLiteral("Cache Test"),
            .kind = QStringLiteral("effect"),
            .categories = {QStringLiteral("test")},
            .defaultParams = {{QStringLiteral("value"), 0.0}},
        });
    }

    void cleanup() {
        // Restore default bake strategy.
        SettingsManager::instance().setValue(QStringLiteral("bakeStrategy"), QStringLiteral("FullBake"));
        SettingsManager::instance().setValue(QStringLiteral("onDemandPrefetchFrames"), 30);
    }

    void fullBakeAllClips() {
        SceneSettings scene;
        scene.id = 1;
        scene.name = QStringLiteral("Test Scene");

        Clip c1;
        c1.id = 10;
        c1.layer = 2;
        c1.startFrame = 0;
        c1.durationFrames = 100;
        c1.type = QStringLiteral("video");
        scene.clips.push_back(c1);

        Clip c2;
        c2.id = 20;
        c2.layer = 3;
        c2.startFrame = 50;
        c2.durationFrames = 150;
        c2.type = QStringLiteral("audio");
        scene.clips.push_back(c2);

        DocumentModel::instance().addScene(scene);

        BakeController::instance().bake(1, 60);

        auto &state = ECS::instance().editState();
        QVERIFY(state.renderStates.contains(10));
        QVERIFY(state.renderStates.contains(20));
        QVERIFY(!state.renderStates.contains(99));

        // Verify render state fields
        auto *t = state.renderStates.find(10);
        QCOMPARE(t->layer, 2);
        QCOMPARE(t->startFrame, 0);
        QCOMPARE(t->durationFrames, 100);

        // Audio clip should have audio state
        QVERIFY(state.audioStates.contains(20));
        auto *a = state.audioStates.find(20);
        QCOMPARE(a->clipId, 20);
        QVERIFY(!a->mute);
    }

    void onDemandIncludesInRange() {
        SettingsManager::instance().setValue(QStringLiteral("bakeStrategy"), QStringLiteral("OnDemand"));
        SettingsManager::instance().setValue(QStringLiteral("onDemandPrefetchFrames"), 10);

        SceneSettings scene;
        scene.id = 2;

        Clip c;
        c.id = 5;
        c.startFrame = 40;
        c.durationFrames = 30; // 40..70
        c.type = QStringLiteral("video");
        scene.clips.push_back(c);

        DocumentModel::instance().addScene(scene);

        // currentFrame=50, prefetch=10 => range [40, 60]
        // clip 40..70 overlaps => should be included
        BakeController::instance().bake(2, 50);

        auto &state = ECS::instance().editState();
        QVERIFY(state.renderStates.contains(5));
    }

    void onDemandExcludesOutOfRange() {
        SettingsManager::instance().setValue(QStringLiteral("bakeStrategy"), QStringLiteral("OnDemand"));
        SettingsManager::instance().setValue(QStringLiteral("onDemandPrefetchFrames"), 10);

        SceneSettings scene;
        scene.id = 3;

        Clip c;
        c.id = 6;
        c.startFrame = 0;
        c.durationFrames = 20; // 0..20
        c.type = QStringLiteral("video");
        scene.clips.push_back(c);

        DocumentModel::instance().addScene(scene);

        // currentFrame=50, prefetch=10 => range [40, 60]
        // clip 0..20 does NOT overlap => excluded
        BakeController::instance().bake(3, 50);

        auto &state = ECS::instance().editState();
        QVERIFY(!state.renderStates.contains(6));
    }

    void clipIdOutOfRangeIgnored() {
        SceneSettings scene;
        scene.id = 4;

        Clip c;
        c.id = MAX_CLIP_ID + 10; // out of bounds
        c.startFrame = 0;
        c.durationFrames = 100;
        c.type = QStringLiteral("video");
        scene.clips.push_back(c);

        DocumentModel::instance().addScene(scene);

        // Should not crash; clip silently skipped.
        BakeController::instance().bake(4, 0);

        auto &state = ECS::instance().editState();
        QVERIFY(!state.renderStates.contains(MAX_CLIP_ID + 10));
    }

    void removesDeadClips() {
        SceneSettings scene;
        scene.id = 5;

        Clip c1;
        c1.id = 30;
        c1.layer = 0;
        c1.startFrame = 0;
        c1.durationFrames = 100;
        c1.type = QStringLiteral("video");
        scene.clips.push_back(c1);

        DocumentModel::instance().addScene(scene);
        BakeController::instance().bake(5, 0);

        {
            auto &state = ECS::instance().editState();
            QVERIFY(state.renderStates.contains(30));
        }

        // Now remove the clip and rebake
        DocumentModel::instance().setClips(5, {});
        BakeController::instance().bake(5, 0);

        {
            auto &state = ECS::instance().editState();
            QVERIFY(!state.renderStates.contains(30));
        }
    }

    void triggerRebakeOnStructureChanged() {
        // Reconnect signal for this specific test
        QObject::connect(&DocumentModel::instance(), SIGNAL(structureChanged()), &BakeController::instance(), SLOT(onStructureChanged()));

        SceneSettings scene;
        scene.id = 6;

        Clip c;
        c.id = 40;
        c.layer = 0;
        c.startFrame = 0;
        c.durationFrames = 100;
        c.type = QStringLiteral("video");
        scene.clips.push_back(c);

        DocumentModel::instance().addScene(scene);
        BakeController::instance().bake(6, 0);

        {
            auto &state = ECS::instance().editState();
            QVERIFY(state.renderStates.contains(40));
        }

        // Add another clip → structureChanged → triggerRebake
        QSignalSpy spy(&DocumentModel::instance(), &DocumentModel::structureChanged);

        Clip c2;
        c2.id = 41;
        c2.layer = 1;
        c2.startFrame = 10;
        c2.durationFrames = 50;
        c2.type = QStringLiteral("video");
        {
            SceneSettings scene = *DocumentModel::instance().findScene(6);
            scene.clips.push_back(c2);
            DocumentModel::instance().setClips(6, std::move(scene.clips));
        }

        // Ensure rebake happened by checking ECS now has both clips
        auto &state = ECS::instance().editState();
        QVERIFY(state.renderStates.contains(40));
        QVERIFY(state.renderStates.contains(41));
    }

    void relTimeComputation() {
        SceneSettings scene;
        scene.id = 7;

        Clip c;
        c.id = 50;
        c.layer = 0;
        c.startFrame = 20;
        c.durationFrames = 100;
        c.type = QStringLiteral("video");
        scene.clips.push_back(c);

        DocumentModel::instance().addScene(scene);
        BakeController::instance().bake(7, 40); // 2 frames past start

        auto &state = ECS::instance().editState();
        auto *t = state.renderStates.find(50);
        QVERIFY(t != nullptr);
        QCOMPARE(t->timePosition, 20.0); // 40 - 20 = 20
    }

    void audioParamsBakedToEcs() {
        SceneSettings scene;
        scene.id = 8;
        scene.fps = AviQtl::kDefaultFps;

        Clip c;
        c.id = 60;
        c.layer = 0;
        c.startFrame = 10;
        c.durationFrames = 120;
        c.type = QStringLiteral("audio");

        Effect audio;
        audio.id = QStringLiteral("audio");
        audio.params.insert(QStringLiteral("playMode"), QStringLiteral("開始時間＋再生速度"));
        audio.params.insert(QStringLiteral("startTime"), 1.5);
        audio.params.insert(QStringLiteral("speed"), 150.0);
        audio.params.insert(QStringLiteral("directTime"), 3.0);
        audio.params.insert(QStringLiteral("volume"), 0.5);
        audio.params.insert(QStringLiteral("pan"), -0.25);
        audio.params.insert(QStringLiteral("mute"), true);
        c.effects.push_back(audio);

        scene.clips.push_back(c);
        DocumentModel::instance().addScene(scene);

        BakeController::instance().bake(8, 20);

        auto &state = ECS::instance().editState();
        auto *a = state.audioStates.find(60);
        QVERIFY(a != nullptr);
        QCOMPARE(a->clipId, 60);
        QCOMPARE(a->sourceStartTime, 1.5f);
        QCOMPARE(a->playbackSpeed, 1.5f);
        QCOMPARE(a->directTime, 3.0f);
        QCOMPARE(a->volume, 0.5f);
        QCOMPARE(a->pan, -0.25f);
        QVERIFY(a->mute);
        QVERIFY(!a->directMode);
    }

    void resolvedTracksAreReusedAndInvalidatedByRevision() {
        SceneSettings scene;
        scene.id = 9;

        Clip clip;
        clip.id = 70;
        clip.durationFrames = 120;
        Effect effect;
        effect.id = QStringLiteral("cache-test");
        effect.params.insert(QStringLiteral("value"), 1.0);
        clip.effects.push_back(effect);
        scene.clips.push_back(clip);
        DocumentModel::instance().addScene(scene);

        BakeController::instance().bake(scene.id, 0);
        PerformanceSnapshot first = PerformanceMetrics::instance().snapshot();
        QCOMPARE(first.value(PerformanceCounter::BakeTrackCacheMisses), quint64{1});
        QCOMPARE(first.value(PerformanceCounter::BakeTrackCacheHits), quint64{0});

        BakeController::instance().bake(scene.id, 1);
        PerformanceSnapshot second = PerformanceMetrics::instance().snapshot();
        QCOMPARE(second.value(PerformanceCounter::BakeTrackCacheMisses), quint64{1});
        QCOMPARE(second.value(PerformanceCounter::BakeTrackCacheHits), quint64{1});

        scene.clips.front().effects.front().params.insert(QStringLiteral("value"), 2.0);
        DocumentModel::instance().setClips(scene.id, std::move(scene.clips));
        BakeController::instance().bake(scene.id, 2);
        PerformanceSnapshot invalidated = PerformanceMetrics::instance().snapshot();
        QCOMPARE(invalidated.value(PerformanceCounter::BakeTrackCacheMisses), quint64{2});
        QCOMPARE(invalidated.value(PerformanceCounter::BakeTrackCacheHits), quint64{1});
    }

    void onDemandUsesTemporalBuckets() {
        SettingsManager::instance().setValue(QStringLiteral("bakeStrategy"), QStringLiteral("OnDemand"));
        SettingsManager::instance().setValue(QStringLiteral("onDemandPrefetchFrames"), 10);

        SceneSettings scene;
        scene.id = 10;
        constexpr int kClipCount = 1000;
        constexpr int kTargetIndex = 500;
        for (int i = 0; i < kClipCount; ++i) {
            Clip clip;
            clip.id = i;
            clip.startFrame = i * 240;
            clip.durationFrames = 30;
            scene.clips.push_back(std::move(clip));
        }
        DocumentModel::instance().addScene(scene);
        PerformanceMetrics::instance().reset();

        BakeController::instance().bake(scene.id, kTargetIndex * 240 + 5);

        const PerformanceSnapshot snapshot = PerformanceMetrics::instance().snapshot();
        QCOMPARE(snapshot.value(PerformanceCounter::BakeClipsVisited), quint64{1});
        QCOMPARE(snapshot.value(PerformanceCounter::BakeClipsActive), quint64{1});
        QVERIFY(ECS::instance().editState().renderStates.contains(kTargetIndex));
    }

    void dynamicTimeDoesNotInvalidateResolvedTracks() {
        SceneSettings scene;
        scene.id = 11;

        Clip clip;
        clip.id = 80;
        clip.startFrame = 10;
        clip.durationFrames = 100;
        Effect effect;
        effect.id = QStringLiteral("cache-test");
        effect.params.insert(QStringLiteral("value"), 4.0);
        clip.effects.push_back(effect);
        scene.clips.push_back(clip);
        DocumentModel::instance().addScene(scene);

        BakeController::instance().bake(scene.id, 15);
        BakeController::instance().bake(scene.id, 23);

        const auto &entries = ECS::instance().editState().effectParams.entries;
        const auto timeIt = std::find_if(entries.cbegin(), entries.cend(), [](const EffectParamEntry &entry) {
            return entry.clipId == 80 && QString::fromUtf8(entry.paramName) == QStringLiteral("time");
        });
        QVERIFY(timeIt != entries.cend());
        QCOMPARE(timeIt->value[0], 13.0f);
        const PerformanceSnapshot snapshot = PerformanceMetrics::instance().snapshot();
        QCOMPARE(snapshot.value(PerformanceCounter::BakeTrackCacheMisses), quint64{1});
        QCOMPARE(snapshot.value(PerformanceCounter::BakeTrackCacheHits), quint64{1});
    }

    void numericKeyframesUseOneRustBatchPerEffect() {
        SceneSettings scene;
        scene.id = 12;

        Clip clip;
        clip.id = 81;
        clip.durationFrames = 101;
        Effect effect;
        effect.id = QStringLiteral("cache-test");
        effect.params.insert(QStringLiteral("value"), 0.0);
        effect.params.insert(QStringLiteral("other"), 10.0);
        effect.keyframes[QStringLiteral("value")] = {
            {.frame = 0, .value = 0.0F, .interpolation = QStringLiteral("linear")},
            {.frame = 100, .value = 100.0F, .interpolation = QStringLiteral("linear")},
        };
        effect.keyframes[QStringLiteral("other")] = {
            {.frame = 0, .value = 10.0F, .interpolation = QStringLiteral("linear")},
            {.frame = 100, .value = 30.0F, .interpolation = QStringLiteral("linear")},
        };
        clip.effects.push_back(std::move(effect));
        scene.clips.push_back(std::move(clip));
        DocumentModel::instance().addScene(scene);
        PerformanceMetrics::instance().reset();

        BakeController::instance().bake(scene.id, 50);

        const auto &entries = ECS::instance().editState().effectParams.entries;
        const auto valueFor = [&entries](const QString &name) {
            const auto it = std::find_if(entries.cbegin(), entries.cend(), [&name](const EffectParamEntry &entry) {
                return entry.clipId == 81 && QString::fromUtf8(entry.paramName) == name;
            });
            return it == entries.cend() ? std::numeric_limits<float>::quiet_NaN() : it->value[0];
        };
        QCOMPARE(valueFor(QStringLiteral("value")), 50.0F);
        QCOMPARE(valueFor(QStringLiteral("other")), 20.0F);

        const PerformanceSnapshot snapshot = PerformanceMetrics::instance().snapshot();
        QCOMPARE(snapshot.value(PerformanceCounter::BakeRustKeyframeBatchCalls), quint64{1});
        QCOMPARE(snapshot.value(PerformanceCounter::BakeRustKeyframeTracks), quint64{2});
    }
};

QTEST_MAIN(TestBakeController)
#include "test_bake_controller.moc"
