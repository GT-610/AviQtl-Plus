#include "rust_core_abi.hpp"
#include <QTest>

class TestRustCoreAbi : public QObject {
    Q_OBJECT

  private slots:
    void reportsCompatibleVersionAndCapabilities() {
        QCOMPARE(aviqtl_core_abi_version(), AVIQTL_RUST_CORE_ABI_VERSION);
        const std::uint64_t capabilities = aviqtl_core_capabilities();
        QVERIFY(capabilities & AVIQTL_RUST_CORE_CAPABILITY_EASING);
        QVERIFY(capabilities & AVIQTL_RUST_CORE_CAPABILITY_AUDIO_DSP);
        QVERIFY(capabilities & AVIQTL_RUST_CORE_CAPABILITY_NUMERIC_KEYFRAME_BATCH);
        QVERIFY(capabilities & AVIQTL_RUST_CORE_CAPABILITY_TIMELINE_BAKE);
        QVERIFY(capabilities & AVIQTL_RUST_CORE_CAPABILITY_PROJECT_DOCUMENT);
        QVERIFY(capabilities & AVIQTL_RUST_CORE_CAPABILITY_AUDIO_BATCH_MIX);
        QVERIFY(capabilities & AVIQTL_RUST_CORE_CAPABILITY_TIMELINE_EDIT);
        QCOMPARE(aviqtl_project_current_version(), std::int32_t{3});
    }

    void exposesExpectedLayouts() {
        QCOMPARE(sizeof(AviQtlEasingParameters), std::size_t{16});
        QCOMPARE(alignof(AviQtlEasingParameters), std::size_t{8});
        QCOMPARE(sizeof(AviQtlAudioMixParameters), std::size_t{40});
        QCOMPARE(alignof(AviQtlAudioMixParameters), std::size_t{8});
        QCOMPARE(sizeof(AviQtlAudioMeter), std::size_t{16});
        QCOMPARE(alignof(AviQtlAudioMeter), std::size_t{4});
        if constexpr (sizeof(void *) == 8) {
            QCOMPARE(sizeof(AviQtlAudioBatchTrack), std::size_t{72});
            QCOMPARE(alignof(AviQtlAudioBatchTrack), std::size_t{8});
        }
        QCOMPARE(sizeof(AviQtlAudioBatchResult), std::size_t{24});
        QCOMPARE(alignof(AviQtlAudioBatchResult), std::size_t{4});
        QCOMPARE(sizeof(AviQtlNumericKeyframe), std::size_t{48});
        QCOMPARE(alignof(AviQtlNumericKeyframe), std::size_t{8});
        if constexpr (sizeof(void *) == 8) {
            QCOMPARE(sizeof(AviQtlNumericTrackView), std::size_t{40});
            QCOMPARE(alignof(AviQtlNumericTrackView), std::size_t{8});
        }
        QCOMPARE(sizeof(AviQtlRenderBakeInput), std::size_t{68});
        QCOMPARE(alignof(AviQtlRenderBakeInput), std::size_t{4});
        QCOMPARE(sizeof(AviQtlRenderBakeOutput), std::size_t{72});
        QCOMPARE(alignof(AviQtlRenderBakeOutput), std::size_t{8});
        QCOMPARE(sizeof(AviQtlAudioBakeInput), std::size_t{72});
        QCOMPARE(alignof(AviQtlAudioBakeInput), std::size_t{8});
        QCOMPARE(sizeof(AviQtlAudioBakeOutput), std::size_t{60});
        QCOMPARE(alignof(AviQtlAudioBakeOutput), std::size_t{4});
        QCOMPARE(sizeof(AviQtlTimelineClipGeometry), std::size_t{16});
        QCOMPARE(alignof(AviQtlTimelineClipGeometry), std::size_t{4});
        QCOMPARE(sizeof(AviQtlTimelineMoveInput), std::size_t{24});
        QCOMPARE(alignof(AviQtlTimelineMoveInput), std::size_t{4});
        QCOMPARE(sizeof(AviQtlTimelinePosition), std::size_t{8});
        QCOMPARE(alignof(AviQtlTimelinePosition), std::size_t{4});
    }
};

QTEST_MAIN(TestRustCoreAbi)
#include "test_rust_core_abi.moc"
