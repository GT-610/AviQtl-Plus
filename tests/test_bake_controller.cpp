#include "core/include/document_model.hpp"
#include "core/include/settings_manager.hpp"
#include "engine/timeline/bake_controller.hpp"
#include "engine/timeline/ecs.hpp"
#include <QSignalSpy>
#include <QTest>
#include <bitset>

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
};

QTEST_MAIN(TestBakeController)
#include "test_bake_controller.moc"
