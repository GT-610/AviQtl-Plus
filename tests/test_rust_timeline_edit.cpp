#include "rust_timeline_edit.hpp"
#include <QTest>
#include <array>
#include <cstdint>
#include <vector>

using AviQtl::RustCore::TimelineClipGeometry;
using AviQtl::RustCore::TimelineEditStatus;

namespace {

constexpr auto clip(std::int32_t id, std::int32_t layer, std::int32_t start, std::int32_t duration) -> TimelineClipGeometry { return {.clip_id = id, .layer = layer, .start_frame = start, .duration_frames = duration}; }

} // namespace

class TestRustTimelineEdit : public QObject {
    Q_OBJECT

  private slots:
    void plansDeltaMovesAndLockedLayers();
};

void TestRustTimelineEdit::plansDeltaMovesAndLockedLayers() {
    const std::array clips{clip(1, 0, 0, 10), clip(2, 0, 10, 10)};
    const std::array movingIds{std::int32_t{1}, std::int32_t{2}};
    std::vector<TimelineClipGeometry> output;

    QCOMPARE(AviQtl::RustCore::planDeltaMove(clips, movingIds, {}, 0, 5, output), TimelineEditStatus::Ok);
    QCOMPARE(output.size(), std::size_t{2});
    QCOMPARE(output[0].start_frame, 5);
    QCOMPARE(output[1].start_frame, 15);
    QCOMPARE(output[1].start_frame - output[0].start_frame, 10);

    const std::array lockedLayers{std::int32_t{0}};
    QCOMPARE(AviQtl::RustCore::planDeltaMove(clips, movingIds, lockedLayers, 0, 5, output), TimelineEditStatus::LockedLayer);
    QVERIFY(output.empty());
}

QTEST_MAIN(TestRustTimelineEdit)
#include "test_rust_timeline_edit.moc"
