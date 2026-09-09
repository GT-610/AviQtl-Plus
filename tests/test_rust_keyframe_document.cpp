#include "rust_core_abi.hpp"
#include "rust_keyframe_document.hpp"
#include <QJsonDocument>
#include <QTest>
#include <array>

using namespace AviQtl::Core::RustKeyframeDocument;

class TestRustKeyframeDocument : public QObject {
    Q_OBJECT

  private slots:
    void undersizedBufferDoesNotReceivePartialJson() {
        const QByteArray request = QJsonDocument::fromVariant(QVariantMap{
            {QStringLiteral("operation"), QStringLiteral("normalize")},
            {QStringLiteral("track"), QVariantList{}},
            {QStringLiteral("fallback"), 1.0},
            {QStringLiteral("duration"), 10},
        }).toJson(QJsonDocument::Compact);
        std::size_t required = 0;
        QCOMPARE(aviqtl_keyframe_document_apply_json(
                     reinterpret_cast<const std::uint8_t *>(request.constData()),
                     static_cast<std::size_t>(request.size()), nullptr, 0, &required),
                 std::uint32_t{AVIQTL_RUST_CORE_STATUS_BUFFER_TOO_SMALL});
        QVERIFY(required > 1);

        QByteArray output(static_cast<qsizetype>(required - 1), '\x5a');
        const QByteArray original = output;
        std::size_t reported = 0;
        QCOMPARE(aviqtl_keyframe_document_apply_json(
                     reinterpret_cast<const std::uint8_t *>(request.constData()),
                     static_cast<std::size_t>(request.size()),
                     reinterpret_cast<std::uint8_t *>(output.data()),
                     static_cast<std::size_t>(output.size()), &reported),
                 std::uint32_t{AVIQTL_RUST_CORE_STATUS_BUFFER_TOO_SMALL});
        QCOMPARE(reported, required);
        QCOMPARE(output, original);
    }
};

QTEST_MAIN(TestRustKeyframeDocument)
#include "test_rust_keyframe_document.moc"
