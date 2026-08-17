#include "rust_timeline_domain.hpp"
#include "selection_service.hpp"
#include "timeline_service.hpp"
#include <QTest>
#include <array>
#include <cstdint>
#include <limits>
#include <utility>
#include <vector>

using AviQtl::RustCore::TimelineDomainStatus;
using namespace AviQtl::UI;

namespace {

template <typename T>
std::vector<T> applyPermutation(const std::vector<T> &values,
                                const std::vector<std::int32_t> &permutation) {
    std::vector<T> reordered;
    reordered.reserve(permutation.size());
    for (const std::int32_t index : permutation) {
        reordered.push_back(values.at(static_cast<std::size_t>(index)));
    }
    return reordered;
}

} // namespace

class TestRustTimelineDomain : public QObject {
    Q_OBJECT

  private slots:
    void allocationSkipsCollisions() {
        const std::array<std::int32_t, 4> existing{1, 2, 4, 5};
        AviQtl::RustCore::IdAllocation allocation{};
        QCOMPARE(AviQtl::RustCore::allocateId(existing, 1, 1, allocation),
                 TimelineDomainStatus::Ok);
        QCOMPARE(allocation.allocated_id, 3);
        QCOMPARE(allocation.next_id, 4);
    }

    void sceneSettingsAreNormalized() {
        const AviQtl::RustCore::SceneSettings input{
            .width = 0,
            .height = 50'000,
            .fps = std::numeric_limits<double>::infinity(),
            .total_frames = -1,
            .grid_mode = 99,
            .grid_bpm = 5'000.0,
            .grid_offset = -1.0,
            .grid_interval = 0,
            .grid_subdivision = 500,
            .enable_snap = 9,
            .magnetic_snap_range = 500,
        };
        AviQtl::RustCore::SceneSettings output{};
        QCOMPARE(AviQtl::RustCore::normalizeSceneSettings(input, output),
                 TimelineDomainStatus::Ok);
        QCOMPARE(output.width, 1920);
        QCOMPARE(output.height, 1080);
        QCOMPARE(output.fps, 60.0);
        QCOMPARE(output.total_frames, 300);
        QCOMPARE(output.grid_mode, std::uint32_t{0});
        QCOMPARE(output.grid_bpm, 120.0);
        QCOMPARE(output.grid_offset, 0.0);
        QCOMPARE(output.grid_interval, 10);
        QCOMPARE(output.grid_subdivision, 4);
        QCOMPARE(output.enable_snap, std::uint32_t{1});
        QCOMPARE(output.magnetic_snap_range, 10);
    }

    void selectionPreservesOrderAndPromotesPrimary() {
        const std::array<std::int32_t, 4> requested{7, 3, 7, -1};
        std::vector<std::int32_t> selected;
        std::int32_t primary = -1;
        QCOMPARE(AviQtl::RustCore::replaceSelection(requested, 7, selected, primary),
                 TimelineDomainStatus::Ok);
        QCOMPARE(selected, std::vector<std::int32_t>({7, 3}));
        QCOMPARE(primary, 7);

        std::vector<std::int32_t> nextSelection;
        QCOMPARE(AviQtl::RustCore::toggleSelection(selected, primary, 7, nextSelection, primary),
                 TimelineDomainStatus::Ok);
        selected = std::move(nextSelection);
        QCOMPARE(selected, std::vector<std::int32_t>({3}));
        QCOMPARE(primary, 3);

        QCOMPARE(AviQtl::RustCore::toggleSelection(selected, primary, 5, nextSelection, primary),
                 TimelineDomainStatus::Ok);
        selected = std::move(nextSelection);
        QCOMPARE(selected, std::vector<std::int32_t>({3, 5}));
        QCOMPARE(primary, 5);
    }

    void undersizedSelectionBufferIsNotPartiallyWritten() {
        const std::array<std::int32_t, 2> requested{4, 8};
        std::array<std::int32_t, 1> output{12345};
        std::size_t required = 0;
        std::int32_t primary = 6789;
        QCOMPARE(aviqtl_selection_replace(requested.data(), requested.size(), 4, output.data(),
                                           output.size(), &required, &primary),
                 std::uint32_t{AVIQTL_RUST_CORE_STATUS_BUFFER_TOO_SMALL});
        QCOMPARE(required, std::size_t{2});
        QCOMPARE(output.front(), 12345);
        QCOMPARE(primary, 6789);
    }

    void removalIndicesAreUniqueAndDescending() {
        const std::array<std::int32_t, 6> requested{3, 1, 3, 0, 8, -1};
        std::vector<std::int32_t> normalized;
        QCOMPARE(AviQtl::RustCore::normalizeRemovalIndices(6, requested, 1, normalized),
                 TimelineDomainStatus::Ok);
        QCOMPARE(normalized, std::vector<std::int32_t>({3, 1}));
    }

    void reorderPlansShareOneRedoUndoConvention() {
        std::vector<std::int32_t> redo;
        std::vector<std::int32_t> undo;
        QCOMPARE(AviQtl::RustCore::planIndexMove(5, 1, 4, 1, redo, undo),
                 TimelineDomainStatus::Ok);
        const std::vector<int> original{10, 20, 30, 40, 50};
        const auto moved = applyPermutation(original, redo);
        QCOMPARE(moved, std::vector<int>({10, 30, 40, 50, 20}));
        QCOMPARE(applyPermutation(moved, undo), original);

        const std::array<std::int32_t, 4> selected{4, 2, 4, 0};
        std::size_t selectedCount = 0;
        QCOMPARE(AviQtl::RustCore::planMultiReorder(6, selected, 5, 1, redo, undo,
                                                    selectedCount),
                 TimelineDomainStatus::Ok);
        QCOMPARE(selectedCount, std::size_t{2});
        const std::vector<int> multiOriginal{0, 1, 2, 3, 4, 5};
        const auto multiMoved = applyPermutation(multiOriginal, redo);
        QCOMPARE(multiMoved, std::vector<int>({0, 1, 3, 2, 4, 5}));
        QCOMPARE(applyPermutation(multiMoved, undo), multiOriginal);
    }

    void timelineServiceUsesRustPermutationsForUndoRedo() {
        SelectionService selection;
        TimelineService timeline(&selection);
        ClipData clip{
            .id = 1,
            .sceneId = 0,
            .type = QStringLiteral("test"),
            .startFrame = 0,
            .durationFrames = 30,
            .layer = 0,
        };
        clip.effects = {
            new EffectModel(QStringLiteral("transform"), QStringLiteral("Transform"), {}, {},
                            {}, {}, {}, &timeline),
            new EffectModel(QStringLiteral("a"), QStringLiteral("A"), {}, {}, {}, {}, {},
                            &timeline),
            new EffectModel(QStringLiteral("b"), QStringLiteral("B"), {}, {}, {}, {}, {},
                            &timeline),
            new EffectModel(QStringLiteral("c"), QStringLiteral("C"), {}, {}, {}, {}, {},
                            &timeline),
        };
        clip.audioPlugins = {
            AudioPluginState{.id = QStringLiteral("p0")},
            AudioPluginState{.id = QStringLiteral("p1")},
            AudioPluginState{.id = QStringLiteral("p2")},
        };
        const bool clipAccepted = timeline.addClipDirectInternal(clip, false);
        QVERIFY(clipAccepted);

        const auto effectIds = [&timeline]() {
            QStringList ids;
            for (const auto *effect : timeline.clips().first().effects) {
                ids.append(effect->id());
            }
            return ids;
        };
        const auto pluginIds = [&timeline]() {
            QStringList ids;
            for (const auto &plugin : timeline.clips().first().audioPlugins) {
                ids.append(plugin.id);
            }
            return ids;
        };

        timeline.reorderEffects(1, 1, 3);
        QCOMPARE(effectIds(), QStringList({QStringLiteral("transform"), QStringLiteral("b"),
                                           QStringLiteral("c"), QStringLiteral("a")}));
        timeline.undo();
        QCOMPARE(effectIds(), QStringList({QStringLiteral("transform"), QStringLiteral("a"),
                                           QStringLiteral("b"), QStringLiteral("c")}));
        timeline.redo();
        QCOMPARE(effectIds(), QStringList({QStringLiteral("transform"), QStringLiteral("b"),
                                           QStringLiteral("c"), QStringLiteral("a")}));

        timeline.reorderAudioPlugins(1, 0, 2);
        QCOMPARE(pluginIds(), QStringList({QStringLiteral("p1"), QStringLiteral("p2"),
                                           QStringLiteral("p0")}));
        timeline.undo();
        QCOMPARE(pluginIds(), QStringList({QStringLiteral("p0"), QStringLiteral("p1"),
                                           QStringLiteral("p2")}));
        timeline.redo();
        QCOMPARE(pluginIds(), QStringList({QStringLiteral("p1"), QStringLiteral("p2"),
                                           QStringLiteral("p0")}));
    }

    void batchIdAllocationDoesNotAdvanceOnExhaustion() {
        SelectionService selection;
        TimelineService timeline(&selection);
        timeline.setNextClipId(std::numeric_limits<int>::max());

        QVERIFY(timeline.allocateClipIds(2).isEmpty());
        QCOMPARE(timeline.nextClipId(), std::numeric_limits<int>::max());
        QVERIFY(timeline.clips().isEmpty());
    }
};

QTEST_MAIN(TestRustTimelineDomain)
#include "test_rust_timeline_domain.moc"
