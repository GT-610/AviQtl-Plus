#include "rust_keyframe_document.hpp"
#include "rust_core_abi.hpp"
#include <QByteArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QJsonParseError>
#include <QLoggingCategory>
#include <limits>
#include <utility>

namespace AviQtl::Core::RustKeyframeDocument {
namespace {

using JsonOperation = std::uint32_t (*)(const std::uint8_t *, std::size_t, std::uint8_t *,
                                        std::size_t, std::size_t *);

QVariant encodeDomainValue(const QVariant &value) {
    const auto tagged = [](QString type, const QVariant &payload) {
        return QVariantMap{
            {QStringLiteral("$aviqtlType"), std::move(type)},
            {QStringLiteral("value"), payload},
        };
    };
    switch (value.typeId()) {
    case QMetaType::Int:
        return tagged(QStringLiteral("int"), value.toInt());
    case QMetaType::UInt:
        return tagged(QStringLiteral("uint"), value.toUInt());
    case QMetaType::LongLong:
        return tagged(QStringLiteral("longlong"), value.toLongLong());
    case QMetaType::ULongLong:
        return tagged(QStringLiteral("ulonglong"), value.toULongLong());
    case QMetaType::Float:
        return tagged(QStringLiteral("float"), value.toFloat());
    case QMetaType::QVariantList: {
        QVariantList encoded;
        const QVariantList values = value.toList();
        encoded.reserve(values.size());
        for (const QVariant &item : values) {
            encoded.append(encodeDomainValue(item));
        }
        return encoded;
    }
    case QMetaType::QVariantMap: {
        QVariantMap encoded;
        const QVariantMap values = value.toMap();
        for (auto it = values.cbegin(); it != values.cend(); ++it) {
            encoded.insert(it.key(), encodeDomainValue(it.value()));
        }
        return encoded;
    }
    default:
        return value;
    }
}

QVariant decodeDomainValue(const QVariant &value) {
    if (value.typeId() == QMetaType::QVariantList) {
        QVariantList decoded;
        const QVariantList values = value.toList();
        decoded.reserve(values.size());
        for (const QVariant &item : values) {
            decoded.append(decodeDomainValue(item));
        }
        return decoded;
    }
    if (value.typeId() != QMetaType::QVariantMap) {
        return value;
    }
    const QVariantMap map = value.toMap();
    const QString type = map.value(QStringLiteral("$aviqtlType")).toString();
    const QVariant payload = map.value(QStringLiteral("value"));
    if (type == QLatin1String("int"))
        return payload.toInt();
    if (type == QLatin1String("uint"))
        return payload.toUInt();
    if (type == QLatin1String("longlong"))
        return payload.toLongLong();
    if (type == QLatin1String("ulonglong"))
        return payload.toULongLong();
    if (type == QLatin1String("float"))
        return payload.toFloat();

    QVariantMap decoded;
    for (auto it = map.cbegin(); it != map.cend(); ++it) {
        decoded.insert(it.key(), decodeDomainValue(it.value()));
    }
    return decoded;
}

std::optional<QVariantMap> invoke(JsonOperation operation, const QVariantMap &request) {
    const QByteArray input = QJsonDocument::fromVariant(request).toJson(QJsonDocument::Compact);
    std::size_t required = 0;
    auto status = operation(
        reinterpret_cast<const std::uint8_t *>(input.constData()),
        static_cast<std::size_t>(input.size()), nullptr, 0, &required);
    if (status != AVIQTL_RUST_CORE_STATUS_BUFFER_TOO_SMALL || required == 0 ||
        required > static_cast<std::size_t>(std::numeric_limits<qsizetype>::max())) {
        qWarning() << "Rust keyframe document size query failed:" << status;
        return std::nullopt;
    }

    QByteArray output(static_cast<qsizetype>(required), Qt::Uninitialized);
    std::size_t written = 0;
    status = operation(
        reinterpret_cast<const std::uint8_t *>(input.constData()),
        static_cast<std::size_t>(input.size()),
        reinterpret_cast<std::uint8_t *>(output.data()), required, &written);
    if (status != AVIQTL_RUST_CORE_STATUS_OK || written != required) {
        qWarning() << "Rust keyframe document operation failed:" << status;
        return std::nullopt;
    }

    QJsonParseError parseError;
    const QJsonDocument document = QJsonDocument::fromJson(output, &parseError);
    if (parseError.error != QJsonParseError::NoError || !document.isObject()) {
        qWarning() << "Rust keyframe document returned invalid JSON:" << parseError.errorString();
        return std::nullopt;
    }
    return document.object().toVariantMap();
}

std::optional<TrackResult> apply(const QVariantMap &request) {
    const auto invoked = invoke(aviqtl_keyframe_document_apply_json, request);
    if (!invoked) {
        return std::nullopt;
    }
    const QVariantMap &response = *invoked;
    TrackResult result{
        .track = decodeDomainValue(response.value(QStringLiteral("track"))).toMap(),
        .flat = decodeDomainValue(response.value(QStringLiteral("flat"))).toList(),
        .inferredDuration = response.value(QStringLiteral("inferredDuration"), 1).toInt(),
        .accepted = response.value(QStringLiteral("accepted")).toBool(),
        .changed = response.value(QStringLiteral("changed")).toBool(),
    };
    if (response.contains(QStringLiteral("baseValue"))) {
        result.baseValue = decodeDomainValue(response.value(QStringLiteral("baseValue")));
    }
    if (response.contains(QStringLiteral("secondaryTrack"))) {
        result.secondaryTrack = decodeDomainValue(response.value(QStringLiteral("secondaryTrack"))).toMap();
    }
    return result;
}

QVariantMap baseRequest(QString operation, const QVariant &track, const QVariant &fallback,
                        int duration) {
    return {
        {QStringLiteral("operation"), std::move(operation)},
        {QStringLiteral("track"), encodeDomainValue(track)},
        {QStringLiteral("fallback"), encodeDomainValue(fallback)},
        {QStringLiteral("duration"), duration},
    };
}

} // namespace

std::optional<TrackResult> inspect(const QVariant &track, const QVariant &fallback, int duration) {
    return apply(baseRequest(QStringLiteral("inspect"), track, fallback, duration));
}

std::optional<TrackResult> normalize(const QVariant &track, const QVariant &fallback,
                                     int duration) {
    return apply(baseRequest(QStringLiteral("normalize"), track, fallback, duration));
}

std::optional<TrackResult> set(const QVariant &track, const QVariant &fallback, int duration,
                               int frame, const QVariant &value, const QVariantMap &options) {
    QVariantMap request = baseRequest(QStringLiteral("set"), track, fallback, duration);
    request.insert(QStringLiteral("frame"), frame);
    request.insert(QStringLiteral("value"), encodeDomainValue(value));
    request.insert(QStringLiteral("options"), encodeDomainValue(options));
    return apply(request);
}

std::optional<TrackResult> remove(const QVariant &track, const QVariant &fallback, int duration,
                                  int frame) {
    QVariantMap request = baseRequest(QStringLiteral("remove"), track, fallback, duration);
    request.insert(QStringLiteral("frame"), frame);
    return apply(request);
}

std::optional<TrackResult> move(const QVariant &track, const QVariant &fallback, int duration,
                                int oldFrame, int newFrame) {
    QVariantMap request = baseRequest(QStringLiteral("move"), track, fallback, duration);
    request.insert(QStringLiteral("old_frame"), oldFrame);
    request.insert(QStringLiteral("new_frame"), newFrame);
    return apply(request);
}

std::optional<TrackResult> sync(const QVariant &track, const QVariant &fallback, int oldDuration,
                                int newDuration) {
    QVariantMap request{
        {QStringLiteral("operation"), QStringLiteral("sync")},
        {QStringLiteral("track"), encodeDomainValue(track)},
        {QStringLiteral("fallback"), encodeDomainValue(fallback)},
        {QStringLiteral("old_duration"), oldDuration},
        {QStringLiteral("new_duration"), newDuration},
    };
    return apply(request);
}

std::optional<TrackResult> split(const QVariant &track, const QVariant &fallback,
                                 int firstHalfDuration, int originalDuration) {
    QVariantMap request{
        {QStringLiteral("operation"), QStringLiteral("split")},
        {QStringLiteral("track"), encodeDomainValue(track)},
        {QStringLiteral("fallback"), encodeDomainValue(fallback)},
        {QStringLiteral("first_half_duration"), firstHalfDuration},
        {QStringLiteral("original_duration"), originalDuration},
    };
    return apply(request);
}

std::optional<QVariant> evaluate(const QVariantList &track, int frame, const QVariant &fallback) {
    const QVariantMap request{
        {QStringLiteral("track"), encodeDomainValue(track)},
        {QStringLiteral("fallback"), encodeDomainValue(fallback)},
        {QStringLiteral("frame"), frame},
    };
    const auto response = invoke(aviqtl_keyframe_evaluate_json, request);
    if (!response || !response->contains(QStringLiteral("value"))) {
        return std::nullopt;
    }
    return decodeDomainValue(response->value(QStringLiteral("value")));
}

} // namespace AviQtl::Core::RustKeyframeDocument
