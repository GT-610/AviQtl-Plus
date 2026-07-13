#include <QTest>
#include <bitset>
#include "engine/timeline/ecs.hpp"

using namespace AviQtl::Engine::Timeline;

class TestTripleBuffer : public QObject {
    Q_OBJECT

  private slots:
    void commitAndGetSnapshot_basic() {
        auto &e = ECS::instance();
        e.clearEffectParams();

        e.updateClipState(1, 0, 0.0, 0, 100);

        AudioComponent audio;
        audio.clipId = 1;
        audio.volume = 0.5f;
        e.updateAudioClipState(1, audio);

        std::bitset<MAX_CLIP_ID> alive;
        alive.set(1);
        e.syncClipIds(alive);
        e.commit();

        const auto *snap = e.getSnapshot();
        QVERIFY(snap != nullptr);
        QVERIFY(snap->renderStates.contains(1));
        QVERIFY(snap->audioStates.contains(1));

        auto *r = snap->renderStates.find(1);
        QVERIFY(r != nullptr);
        QCOMPARE(r->clipId, 1);
        QCOMPARE(r->startFrame, 0);
        QCOMPARE(r->durationFrames, 100);

        auto *a = snap->audioStates.find(1);
        QVERIFY(a != nullptr);
        QCOMPARE(a->volume, 0.5f);
    }

    void syncClipIds_removesDeadClips() {
        auto &e = ECS::instance();
        e.clearEffectParams();

        e.updateClipState(1, 0, 0.0, 0, 100);
        e.updateClipState(2, 1, 0.0, 10, 50);

        std::bitset<MAX_CLIP_ID> alive;
        alive.set(1);
        alive.set(2);
        e.syncClipIds(alive);
        e.commit();

        // Now only keep clip 2
        std::bitset<MAX_CLIP_ID> alive2;
        alive2.set(2);
        e.clearEffectParams();
        e.updateClipState(2, 1, 0.0, 10, 50);
        e.syncClipIds(alive2);
        e.commit();

        const auto *snap = e.getSnapshot();
        QVERIFY(snap != nullptr);
        QVERIFY(!snap->renderStates.contains(1));
        QVERIFY(snap->renderStates.contains(2));
    }

    void effectParamBuffer_commit() {
        auto &e = ECS::instance();
        e.clearEffectParams();

        EffectParamEntry entry;
        entry.clipId = 1;
        entry.effectIndex = 0;
        entry.paramType = ParamType::Float;
        qstrncpy(entry.paramName, "x", sizeof(entry.paramName));
        entry.value[0] = 42.0f;
        e.editState().effectParams.entries.push_back(entry);

        e.commit();

        const auto *snap = e.getSnapshot();
        QVERIFY(snap != nullptr);
        QCOMPARE(static_cast<int>(snap->effectParams.entries.size()), 1);
        QCOMPARE(snap->effectParams.entries[0].clipId, static_cast<uint32_t>(1));
        QCOMPARE(snap->effectParams.entries[0].value[0], 42.0f);
    }

    void updateRenderState_replacesComponent() {
        auto &e = ECS::instance();
        e.clearEffectParams();

        e.updateClipState(1, 0, 0.0, 0, 100);

        RenderComponent rc;
        rc.clipId = 1;
        rc.layer = 0;
        rc.startFrame = 0;
        rc.durationFrames = 100;
        rc.x = 100.0f;
        rc.y = 200.0f;
        rc.scaleX = 1.5f;
        rc.opacity = 0.8f;
        e.updateRenderState(1, rc);

        std::bitset<MAX_CLIP_ID> alive;
        alive.set(1);
        e.syncClipIds(alive);
        e.commit();

        const auto *snap = e.getSnapshot();
        auto *r = snap->renderStates.find(1);
        QVERIFY(r != nullptr);
        QCOMPARE(r->x, 100.0f);
        QCOMPARE(r->y, 200.0f);
        QCOMPARE(r->scaleX, 1.5f);
        QCOMPARE(r->opacity, 0.8f);
    }

    void getSnapshot_stableUntilNextCommit() {
        auto &e = ECS::instance();
        e.clearEffectParams();
        e.updateClipState(1, 0, 0.0, 0, 100);

        std::bitset<MAX_CLIP_ID> alive;
        alive.set(1);
        e.syncClipIds(alive);
        e.commit();

        const auto *snap1 = e.getSnapshot();
        const auto *snap2 = e.getSnapshot();
        QCOMPARE(snap1, snap2);
    }
};

QTEST_MAIN(TestTripleBuffer)
#include "test_triple_buffer.moc"
