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
    void reportsRequiredDeltaMoveCapacityWithoutPartialWrite();
    void variablePlannerRetriesWithReportedCapacity();
    void plansClipboardPlacementAndRejectsInternalOverlap();
    void splitsWithoutPartialWrites();
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

void TestRustTimelineEdit::reportsRequiredDeltaMoveCapacityWithoutPartialWrite() {
    const std::array clips{clip(1, 0, 0, 10), clip(2, 1, 20, 10)};
    const std::array movingIds{std::int32_t{1}, std::int32_t{2}};
    std::array output{clip(99, 99, 99, 99)};
    std::size_t required = 0;

    const auto status = static_cast<TimelineEditStatus>(aviqtl_timeline_plan_delta_move(clips.data(), clips.size(), movingIds.data(), movingIds.size(), nullptr, 0, 1, 5, output.data(), output.size(), &required));
    QCOMPARE(status, TimelineEditStatus::BufferTooSmall);
    QCOMPARE(required, std::size_t{2});
    QCOMPARE(output[0].clip_id, 99);
    QCOMPARE(output[0].layer, 99);
    QCOMPARE(output[0].start_frame, 99);
    QCOMPARE(output[0].duration_frames, 99);
}

void TestRustTimelineEdit::variablePlannerRetriesWithReportedCapacity() {
    const std::array clips{clip(1, 0, 0, 10)};
    std::vector<TimelineClipGeometry> output;
    int calls = 0;
    const auto status = AviQtl::RustCore::planVariable(
        clips, output,
        [&](TimelineClipGeometry *data, std::size_t capacity, std::size_t *written) {
            ++calls;
            *written = 2;
            if (capacity < 2) {
                return AVIQTL_RUST_CORE_STATUS_BUFFER_TOO_SMALL;
            }
            data[0] = clip(1, 1, 0, 10);
            data[1] = clip(2, 2, 10, 10);
            return AVIQTL_RUST_CORE_STATUS_OK;
        });
    QCOMPARE(status, TimelineEditStatus::Ok);
    QCOMPARE(calls, 2);
    QCOMPARE(output.size(), std::size_t{2});
    QCOMPARE(output[1].clip_id, 2);
}

void TestRustTimelineEdit::plansClipboardPlacementAndRejectsInternalOverlap() {
    const std::array existing{clip(3, 0, 15, 10)};
    const std::array clipboard{clip(1, 2, 20, 10), clip(2, 4, 40, 5)};
    std::vector<TimelineClipGeometry> output;
    std::int32_t safeFrame = -1;

    QCOMPARE(AviQtl::RustCore::planClipboardPlacement(existing, clipboard, 10, 0, output, safeFrame), TimelineEditStatus::Ok);
    QCOMPARE(safeFrame, 25);
    QCOMPARE(output.size(), std::size_t{2});
    QCOMPARE(output[0].layer, 0);
    QCOMPARE(output[0].start_frame, 25);
    QCOMPARE(output[1].layer, 2);
    QCOMPARE(output[1].start_frame, 45);

    const std::array overlapping{clip(1, 0, 0, 10), clip(2, 0, 5, 10)};
    safeFrame = 123;
    QCOMPARE(AviQtl::RustCore::planClipboardPlacement({}, overlapping, 0, 0, output, safeFrame), TimelineEditStatus::InvalidArgument);
    QVERIFY(output.empty());
    QCOMPARE(safeFrame, 123);
}

void TestRustTimelineEdit::splitsWithoutPartialWrites() {
    const auto source = clip(1, 2, 10, 20);
    auto first = clip(98, 98, 98, 98);
    auto second = clip(99, 99, 99, 99);

    QCOMPARE(AviQtl::RustCore::splitClip(source, 10, first, second), TimelineEditStatus::InvalidArgument);
    QCOMPARE(first.clip_id, 98);
    QCOMPARE(second.clip_id, 99);

    QCOMPARE(AviQtl::RustCore::splitClip(source, 18, first, second), TimelineEditStatus::Ok);
    QCOMPARE(first.duration_frames, 8);
    QCOMPARE(second.start_frame, 18);
    QCOMPARE(second.duration_frames, 12);
}

QTEST_MAIN(TestRustTimelineEdit)
#include "test_rust_timeline_edit.moc"
