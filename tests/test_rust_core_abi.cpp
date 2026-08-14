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
    }

    void exposesExpectedLayouts() {
        QCOMPARE(sizeof(AviQtlEasingParameters), std::size_t{16});
        QCOMPARE(alignof(AviQtlEasingParameters), std::size_t{8});
        QCOMPARE(sizeof(AviQtlAudioMixParameters), std::size_t{40});
        QCOMPARE(alignof(AviQtlAudioMixParameters), std::size_t{8});
        QCOMPARE(sizeof(AviQtlAudioMeter), std::size_t{16});
        QCOMPARE(alignof(AviQtlAudioMeter), std::size_t{4});
    }
};

QTEST_MAIN(TestRustCoreAbi)
#include "test_rust_core_abi.moc"
