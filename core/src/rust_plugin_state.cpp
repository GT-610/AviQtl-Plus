#include "rust_plugin_document.hpp"
#include <QJsonArray>
#include <limits>
#include <utility>

namespace AviQtl::RustCore::Plugin {
namespace {

QByteArray encode(const QVariantMap &value) {
    return QJsonDocument(QJsonObject::fromVariantMap(value)).toJson(QJsonDocument::Compact);
}

template <typename Function> Status collectJson(Function function, QByteArray &output) {
    output.clear();
    std::size_t required = 0;
    auto status = static_cast<Status>(function(nullptr, 0, &required));
    if (status != Status::BufferTooSmall || required == 0 ||
        required > static_cast<std::size_t>(std::numeric_limits<qsizetype>::max())) {
        return status == Status::Ok ? Status::InvalidArgument : status;
    }
    output.resize(static_cast<qsizetype>(required));
    std::size_t written = 0;
    status = static_cast<Status>(function(reinterpret_cast<std::uint8_t *>(output.data()),
                                          required, &written));
    if (status != Status::Ok || written != required) {
        output.clear();
        return status == Status::Ok ? Status::InvalidArgument : status;
    }
    return status;
}

} // namespace

CatalogState::CatalogState() {
    const auto status =
        static_cast<Status>(aviqtl_script_plugin_catalog_state_create(&m_handle));
    if (status != Status::Ok)
        m_handle = nullptr;
}

CatalogState::CatalogState(CatalogState &&other) noexcept
    : m_handle(std::exchange(other.m_handle, nullptr)) {}

CatalogState &CatalogState::operator=(CatalogState &&other) noexcept {
    if (this != &other) {
        aviqtl_script_plugin_catalog_state_destroy(m_handle);
        m_handle = std::exchange(other.m_handle, nullptr);
    }
    return *this;
}

CatalogState::~CatalogState() { aviqtl_script_plugin_catalog_state_destroy(m_handle); }

Status CatalogState::clear() {
    return static_cast<Status>(aviqtl_script_plugin_catalog_state_clear(m_handle));
}

Status CatalogState::store(const QVariantMap &plugin) {
    const QByteArray input = encode(plugin);
    return static_cast<Status>(aviqtl_script_plugin_catalog_state_store_json(
        m_handle, reinterpret_cast<const std::uint8_t *>(input.constData()),
        static_cast<std::size_t>(input.size())));
}

Status CatalogState::snapshot(QVariantList &plugins) const {
    plugins.clear();
    QByteArray output;
    const auto status = collectJson(
        [this](std::uint8_t *data, std::size_t capacity, std::size_t *length) {
            return aviqtl_script_plugin_catalog_state_snapshot_json(m_handle, data, capacity,
                                                                    length);
        },
        output);
    if (status != Status::Ok)
        return status;
    const QJsonDocument document = QJsonDocument::fromJson(output);
    if (!document.isArray())
        return Status::InvalidJson;
    plugins = document.array().toVariantList();
    return Status::Ok;
}

Status CatalogState::find(const QString &id, QVariantMap &plugin) const {
    plugin.clear();
    const QByteArray encodedId = id.toUtf8();
    QByteArray output;
    const auto status = collectJson(
        [this, &encodedId](std::uint8_t *data, std::size_t capacity, std::size_t *length) {
            return aviqtl_script_plugin_catalog_state_find_json(
                m_handle, reinterpret_cast<const std::uint8_t *>(encodedId.constData()),
                static_cast<std::size_t>(encodedId.size()), data, capacity, length);
        },
        output);
    if (status != Status::Ok)
        return status;
    const QJsonDocument document = QJsonDocument::fromJson(output);
    if (!document.isObject())
        return Status::InvalidJson;
    plugin = document.object().toVariantMap();
    return Status::Ok;
}

} // namespace AviQtl::RustCore::Plugin
