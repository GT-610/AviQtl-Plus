#include "rust_timeline_domain.hpp"
#include "selection_service.hpp"
#include "timeline_service.hpp"
#include <QTest>
#include <array>
#include <cstdint>
#include <limits>
#include <vector>

using namespace AviQtl::UI;

class TestRustTimelineDomain : public QObject {
    Q_OBJECT

  private slots:
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
