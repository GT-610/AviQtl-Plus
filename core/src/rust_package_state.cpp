#include "rust_package_document.hpp"
#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <limits>
#include <utility>

namespace AviQtl::RustCore::Package {
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

template <typename Function>
auto withUtf8(const QString &value, Function function) -> decltype(function(nullptr, 0)) {
    const QByteArray utf8 = value.toUtf8();
    return function(reinterpret_cast<const std::uint8_t *>(utf8.constData()),
                    static_cast<std::size_t>(utf8.size()));
}

} // namespace

CatalogState::CatalogState() {
    const auto status =
        static_cast<Status>(aviqtl_package_catalog_state_create(&m_handle));
    if (status != Status::Ok)
        m_handle = nullptr;
}

CatalogState::CatalogState(CatalogState &&other) noexcept
    : m_handle(std::exchange(other.m_handle, nullptr)) {}

CatalogState &CatalogState::operator=(CatalogState &&other) noexcept {
    if (this != &other) {
        aviqtl_package_catalog_state_destroy(m_handle);
        m_handle = std::exchange(other.m_handle, nullptr);
    }
    return *this;
}

CatalogState::~CatalogState() { aviqtl_package_catalog_state_destroy(m_handle); }

Status CatalogState::clear() {
    return static_cast<Status>(aviqtl_package_catalog_state_clear(m_handle));
}

Status CatalogState::merge(const QVariantList &packages, const QVariantMap &repository,
                           const QVariantList &repositories, const QVariantMap &installed,
                           const QString &language, const QString &appVersion) {
    const QByteArray input = encode({
        {QStringLiteral("packages"), packages},
        {QStringLiteral("repository"), repository},
        {QStringLiteral("repositories"), repositories},
        {QStringLiteral("installed"), installed},
        {QStringLiteral("language"), language},
        {QStringLiteral("appVersion"), appVersion},
    });
    return static_cast<Status>(aviqtl_package_catalog_state_merge_json(
        m_handle, reinterpret_cast<const std::uint8_t *>(input.constData()),
        static_cast<std::size_t>(input.size())));
}

Status CatalogState::snapshot(QVariantList &catalog) const {
    catalog.clear();
    QByteArray output;
    const auto status = collectJson(
        [this](std::uint8_t *data, std::size_t capacity, std::size_t *length) {
            return aviqtl_package_catalog_state_snapshot_json(m_handle, data, capacity, length);
        },
        output);
    if (status != Status::Ok)
        return status;
    const QJsonDocument document = QJsonDocument::fromJson(output);
    if (!document.isArray())
        return Status::InvalidJson;
    catalog = document.array().toVariantList();
    return Status::Ok;
}

bool CatalogState::hasUpdates() const {
    return aviqtl_package_catalog_state_has_updates(m_handle) != 0;
}

Status CatalogState::find(const QString &packageId, const QString &sourceRepository,
                          QVariantMap &package) const {
    package.clear();
    QByteArray output;
    const auto status = withUtf8(packageId, [this, &sourceRepository, &output](
                                                const std::uint8_t *id, std::size_t idLength) {
        return withUtf8(sourceRepository, [this, id, idLength, &output](
                                              const std::uint8_t *repository,
                                              std::size_t repositoryLength) {
            return collectJson(
                [this, id, idLength, repository, repositoryLength](
                    std::uint8_t *data, std::size_t capacity, std::size_t *length) {
                    return aviqtl_package_catalog_state_find_json(
                        m_handle, id, idLength, repository, repositoryLength, data, capacity,
                        length);
                },
                output);
        });
    });
    if (status != Status::Ok)
        return status;
    const QJsonDocument document = QJsonDocument::fromJson(output);
    if (!document.isObject())
        return Status::InvalidJson;
    package = document.object().toVariantMap();
    return Status::Ok;
}

Status CatalogState::filter(const QString &filterValue, QVariantList &catalog) const {
    catalog.clear();
    QByteArray output;
    const auto status = withUtf8(filterValue, [this, &output](const std::uint8_t *filter,
                                                             std::size_t filterLength) {
        return collectJson(
            [this, filter, filterLength](std::uint8_t *data, std::size_t capacity,
                                         std::size_t *length) {
                return aviqtl_package_catalog_state_filter_json(
                    m_handle, filter, filterLength, data, capacity, length);
            },
            output);
    });
    if (status != Status::Ok)
        return status;
    const QJsonDocument document = QJsonDocument::fromJson(output);
    if (!document.isArray())
        return Status::InvalidJson;
    catalog = document.array().toVariantList();
    return Status::Ok;
}

Status CatalogState::upgradeIds(QStringList &ids) const {
    ids.clear();
    QByteArray output;
    const auto status = collectJson(
        [this](std::uint8_t *data, std::size_t capacity, std::size_t *length) {
            return aviqtl_package_catalog_state_upgrade_ids_json(m_handle, data, capacity,
                                                                 length);
        },
        output);
    if (status != Status::Ok)
        return status;
    const QJsonDocument document = QJsonDocument::fromJson(output);
    if (!document.isArray())
        return Status::InvalidJson;
    for (const QJsonValue &id : document.array())
        ids.append(id.toString());
    return Status::Ok;
}

Status CatalogState::setInstalled(const QString &packageId,
                                  const std::optional<QString> &version, bool &changed) {
    changed = false;
    std::uint32_t changedValue = 0;
    const QString versionValue = version.value_or(QString());
    const auto status = withUtf8(packageId, [this, &versionValue, &version, &changedValue](
                                                const std::uint8_t *id, std::size_t idLength) {
        return withUtf8(versionValue, [this, id, idLength, &version, &changedValue](
                                          const std::uint8_t *encodedVersion,
                                          std::size_t versionLength) {
            return static_cast<Status>(aviqtl_package_catalog_state_set_installed(
                m_handle, id, idLength, encodedVersion, versionLength,
                version.has_value() ? 1U : 0U, &changedValue));
        });
    });
    if (status == Status::Ok)
        changed = changedValue != 0;
    return status;
}

} // namespace AviQtl::RustCore::Package
