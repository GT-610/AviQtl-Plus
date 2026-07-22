#include "package_manager.hpp"
#include "package_url_utils.hpp"
#include "effect_registry.hpp"
#include "settings_manager.hpp"
#include "shader_compiler.hpp"
#include "version.hpp"
#include <QCoreApplication>
#include <QCryptographicHash>
#include <QDateTime>
#include <algorithm>
#include <limits>
#include <QDir>
#include <QDirIterator>
#include <QFile>
#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QNetworkAccessManager>
#include <QNetworkReply>
#include <QSaveFile>
#include <QTemporaryDir>
#include <QTimer>
#include <QXmlStreamReader>
#include <QtCore/private/qzipreader_p.h>

namespace AviQtl::Core {

namespace {
constexpr qint64 kMaxPackageDownloadBytes = 256LL * 1024LL * 1024LL;
constexpr qint64 kMaxRepositoryResponseBytes = 16LL * 1024LL * 1024LL;
constexpr qint64 kMaxPackageExtractedBytes = 1024LL * 1024LL * 1024LL;
constexpr qsizetype kMaxPackageArchiveEntries = 10000;
constexpr int kNetworkTransferTimeoutMs = 30000;

bool isValidPackageId(const QString &packageId) {
    if (packageId.isEmpty() || packageId == QStringLiteral(".") || packageId == QStringLiteral(".."))
        return false;
    for (const QChar ch : packageId) {
        if (!ch.isLetterOrNumber() && ch != QLatin1Char('.') && ch != QLatin1Char('-') && ch != QLatin1Char('_'))
            return false;
    }
    return true;
}

bool isValidPackageType(const QString &packageType) {
    return packageType == QStringLiteral("mod") || packageType == QStringLiteral("effect") ||
           packageType == QStringLiteral("object") || packageType == QStringLiteral("transition");
}

bool writeJsonAtomically(const QString &path, const QJsonDocument &document) {
    QSaveFile file(path);
    if (!file.open(QIODevice::WriteOnly))
        return false;
    if (file.write(document.toJson(QJsonDocument::Indented)) < 0)
        return false;
    return file.commit();
}

QNetworkRequest packageNetworkRequest(const QUrl &url) {
    QNetworkRequest request(url);
    request.setTransferTimeout(kNetworkTransferTimeoutMs);
    request.setAttribute(QNetworkRequest::RedirectPolicyAttribute, QNetworkRequest::NoLessSafeRedirectPolicy);
    return request;
}

void enforceReplySizeLimit(QNetworkReply *reply, qint64 maxBytes) {
    const auto abortIfTooLarge = [reply, maxBytes](qint64 received, qint64 total) {
        if (received > maxBytes || total > maxBytes) {
            reply->setProperty("aviqtlSizeLimitExceeded", true);
            reply->abort();
        }
    };
    QObject::connect(reply, &QNetworkReply::metaDataChanged, reply, [reply, abortIfTooLarge]() {
        abortIfTooLarge(reply->bytesAvailable(), reply->header(QNetworkRequest::ContentLengthHeader).toLongLong());
    });
    QObject::connect(reply, &QNetworkReply::downloadProgress, reply, abortIfTooLarge);
}

const QString &appVersionString() {
    static const QString cached = QString::fromUtf8(AviQtl::VERSION_STRING);
    return cached;
}

QString getInstalledPackagesPath() {
    const QString path = QCoreApplication::applicationDirPath() + QStringLiteral("/repos");
    QDir().mkpath(path);
    return path + QStringLiteral("/installed.json");
}

QString getReposCachePath() {
    const QString path = QCoreApplication::applicationDirPath() + QStringLiteral("/repos");
    QDir().mkpath(path);
    return path;
}

bool copyDirectory(const QString &srcPath, const QString &destPath) {
    QDir srcDir(srcPath);
    if (!srcDir.exists())
        return false;
    QDir destDir(destPath);
    if (!destDir.exists())
        QDir().mkpath(destPath);
    const QStringList entries = srcDir.entryList(QDir::Files | QDir::Dirs | QDir::NoDotAndDotDot);
    for (const QString &entry : entries) {
        const QString srcFilePath = srcDir.absoluteFilePath(entry);
        const QString destFilePath = destPath + QStringLiteral("/") + entry;
        QFileInfo srcInfo(srcFilePath);
        if (srcInfo.isDir()) {
            if (!copyDirectory(srcFilePath, destFilePath))
                return false;
        } else {
            if (QFile::exists(destFilePath))
                QFile::remove(destFilePath);
            if (!QFile::copy(srcFilePath, destFilePath))
                return false;
        }
    }
    return true;
}

int compareVersions(const QString &v1, const QString &v2) {
    if (v1 == v2)
        return 0;
    auto sanitize = [](QString v) {
        if (v.startsWith('v'))
            v.remove(0, 1);
        return v;
    };
    QStringList parts1 = sanitize(v1).split('.');
    QStringList parts2 = sanitize(v2).split('.');
    int i = 0;
    while (i < parts1.size() && i < parts2.size()) {
        bool ok1, ok2;
        int num1 = parts1[i].toInt(&ok1);
        int num2 = parts2[i].toInt(&ok2);
        if (ok1 && ok2) {
            if (num1 < num2) return -1;
            if (num1 > num2) return 1;
        } else {
            if (parts1[i] < parts2[i]) return -1;
            if (parts1[i] > parts2[i]) return 1;
        }
        i++;
    }
    if (parts1.size() > parts2.size()) return 1;
    if (parts1.size() < parts2.size()) return -1;
    return 0;
}

QVariantMap loadInstalledPackagesFromFile() {
    const QString installedPath = getInstalledPackagesPath();
    QFile file(installedPath);
    QVariantMap installed;
    if (file.open(QIODevice::ReadOnly)) {
        installed = QJsonDocument::fromJson(file.readAll()).object().toVariantMap();
        file.close();
    }
    return installed;
}

QString resolveLanguageField(const QVariant &field, const QString &fallback) {
    if (field.metaType().id() == QMetaType::QVariantMap) {
        QVariantMap map = field.toMap();
        const QString lang = QLocale::system().name().left(2);
        if (map.contains(lang))
            return map[lang].toString();
        if (map.contains("en"))
            return map["en"].toString();
        for (auto it = map.begin(); it != map.end(); ++it)
            return it.value().toString();
        return fallback;
    }
    QString s = field.toString();
    return s.isEmpty() ? fallback : s;
}

QString sha256OfFile(const QString &path) {
    QFile f(path);
    if (!f.open(QIODevice::ReadOnly))
        return {};
    QCryptographicHash hash(QCryptographicHash::Sha256);
    if (hash.addData(&f))
        return hash.result().toHex();
    return {};
}

} // namespace

PackageManager &PackageManager::instance() {
    static PackageManager instance;
    return instance;
}

PackageManager::PackageManager(QObject *parent) : QObject(parent) {
    m_statusText = tr("Idle");
    m_networkManager = new QNetworkAccessManager(this);
    QTimer::singleShot(0, this, [this]() { loadCachedPackages(); });
}

void PackageManager::setBusy(bool busy) {
    if (m_isBusy == busy) return;
    m_isBusy = busy;
    emit isBusyChanged();
}

void PackageManager::setStatus(const QString &status) {
    if (m_statusText == status) return;
    m_statusText = status;
    emit statusTextChanged();
}

void PackageManager::setProgress(double p) {
    if (m_progress == p) return;
    m_progress = p;
    emit progressChanged();
}

void PackageManager::setHasUpdatesAvailable(bool available) {
    if (m_hasUpdatesAvailable == available) return;
    m_hasUpdatesAvailable = available;
    emit hasUpdatesAvailableChanged();
}

void PackageManager::loadCachedPackages() {
    const QString cacheDir = getReposCachePath();
    QVariantMap installed = loadInstalledPackagesFromFile();

    // Load cached catalog files
    QDir dir(cacheDir);
    const QStringList files = dir.entryList({QStringLiteral("catalog_*.json")}, QDir::Files);
    for (const QString &fileName : files) {
        QFile file(dir.absoluteFilePath(fileName));
        if (!file.open(QIODevice::ReadOnly))
            continue;
        QJsonDocument doc = QJsonDocument::fromJson(file.readAll());
        if (!doc.isObject())
            continue;
        QJsonArray packages = doc.object().value(QStringLiteral("packages")).toArray();
        QString repoUrl = doc.object().value(QStringLiteral("_repo_url")).toString();
        QVariantMap repoInfo;
        repoInfo[QStringLiteral("url")] = repoUrl;
        for (const auto &pVal : packages) {
            QVariantMap p = pVal.toObject().toVariantMap();
            p[QStringLiteral("_repo_url")] = repoUrl;
            mergeCatalogPackage(p, repoInfo, installed);
        }
    }
    emit packageListChanged();
    updateUpdateState();
    setStatus(tr("Packages loaded from cache (Press Sync to check for updates)"));
}

QVariantList PackageManager::repositories() const {
    return SettingsManager::instance().value(
        QStringLiteral("packageRepositories"),
        QVariantList{
            QVariantMap{
                {QStringLiteral("url"), QStringLiteral("https://raw.githubusercontent.com/GT-610/AviQtl-Plus/main/repos/repo.json")},
                {QStringLiteral("name"), QStringLiteral("AviQtl Official")},
                {QStringLiteral("enabled"), true},
                {QStringLiteral("priority"), 10}
            }
        }).toList();
}

void PackageManager::saveRepositories(const QVariantList &repos) {
    SettingsManager::instance().setValue(QStringLiteral("packageRepositories"), repos);
    emit repositoriesChanged();
}

void PackageManager::addRepository(const QString &url, bool enabled, int priority) {
    if (!Internal::isSecureNetworkUrl(QUrl(url)))
        return;
    QVariantList repos = repositories();
    for (const auto &r : repos) {
        if (r.toMap().value(QStringLiteral("url")).toString() == url)
            return;
    }
    QVariantMap entry;
    entry[QStringLiteral("url")] = url;
    entry[QStringLiteral("name")] = url; // will be updated on first sync
    entry[QStringLiteral("enabled")] = enabled;
    entry[QStringLiteral("priority")] = priority;
    repos.append(entry);
    saveRepositories(repos);
}

void PackageManager::removeRepository(const QString &url) {
    QVariantList repos = repositories();
    QVariantList filtered;
    for (const auto &r : repos) {
        if (r.toMap().value(QStringLiteral("url")).toString() != url)
            filtered.append(r);
    }
    if (filtered.size() != repos.size())
        saveRepositories(filtered);
}

void PackageManager::setRepositoryEnabled(const QString &url, bool enabled) {
    QVariantList repos = repositories();
    for (int i = 0; i < repos.size(); ++i) {
        QVariantMap r = repos[i].toMap();
        if (r.value(QStringLiteral("url")).toString() == url) {
            r[QStringLiteral("enabled")] = enabled;
            repos[i] = r;
            saveRepositories(repos);
            return;
        }
    }
}

void PackageManager::setRepositoryPriority(const QString &url, int priority) {
    QVariantList repos = repositories();
    for (int i = 0; i < repos.size(); ++i) {
        QVariantMap r = repos[i].toMap();
        if (r.value(QStringLiteral("url")).toString() == url) {
            r[QStringLiteral("priority")] = priority;
            repos[i] = r;
            saveRepositories(repos);
            return;
        }
    }
}

// --- Sync Pipeline ---

void PackageManager::sync() {
    refreshRepositories();
}

void PackageManager::refreshRepositories() {
    // Backward compatible alias using the old repo.json-at-packages-list format
    if (m_isBusy)
        return;
    setBusy(true);
    m_packageList.clear();
    emit packageListChanged();

    QVariantMap installed = loadInstalledPackagesFromFile();
    installed.insert(QStringLiteral("org.aviqtl.app"), QVariantMap{{QStringLiteral("version"), appVersionString()}});

    setStatus(tr("Syncing repository..."));
    setProgress(0.0);

    QVariantList repos = repositories();
    // Sort by ascending priority so higher-priority repos are synced first
    // and win tie-breaks in mergeCatalogPackage.
    std::sort(repos.begin(), repos.end(), [](const QVariant &a, const QVariant &b) {
        return a.toMap().value(QStringLiteral("priority"), 10).toInt()
             < b.toMap().value(QStringLiteral("priority"), 10).toInt();
    });
    int enabledCount = 0;
    for (const auto &r : repos) {
        if (r.toMap().value(QStringLiteral("enabled"), true).toBool())
            enabledCount++;
    }
    if (enabledCount == 0) {
        setBusy(false);
        return;
    }
    m_pendingRequests = enabledCount;

    for (const auto &r : repos) {
        QVariantMap repo = r.toMap();
        if (!repo.value(QStringLiteral("enabled"), true).toBool())
            continue;

        QString repoUrl = repo.value(QStringLiteral("url")).toString();
        struct SyncCtx {
            QVariantMap repoInfo;
            QByteArray catalogData;
        };
        auto ctx = std::make_shared<SyncCtx>();
        ctx->repoInfo = repo;

        // Fetch repo.json
        QUrl fetchUrl(repoUrl);
        if (!fetchUrl.path().endsWith(QStringLiteral("/repo.json")))
            fetchUrl.setPath(fetchUrl.path() + (fetchUrl.path().endsWith('/') ? QStringLiteral("repo.json") : QStringLiteral("/repo.json")));

        if (!Internal::isSecureNetworkUrl(fetchUrl)) {
            m_pendingRequests--;
            emit errorOccurred(tr("Repository URL must use HTTPS: %1").arg(repoUrl));
            tryFinishSyncLegacy(installed);
            continue;
        }
        QNetworkReply *reply = m_networkManager->get(packageNetworkRequest(fetchUrl));
        enforceReplySizeLimit(reply, kMaxRepositoryResponseBytes);
        connect(reply, &QNetworkReply::finished, this, [this, reply, fetchUrl, repoUrl, ctx, installed]() {
            reply->deleteLater();
            m_pendingRequests--;

            if (reply->error() == QNetworkReply::NoError) {
                QByteArray body = reply->readAll();
                QJsonDocument doc = QJsonDocument::fromJson(body);
                if (doc.isObject()) {
                    QJsonObject repoObj = doc.object();
                    ctx->repoInfo[QStringLiteral("name")] = repoObj.value(QStringLiteral("repo_name")).toString();
                    QString catalogUrl = repoObj.value(QStringLiteral("catalog_url")).toString();
                    if (!catalogUrl.isEmpty()) {
                        const QUrl absUrl = Internal::resolveRepositoryReference(fetchUrl, catalogUrl);

                        if (!Internal::isSecureNetworkUrl(absUrl)) {
                            emit errorOccurred(tr("Catalog URL must use HTTPS: %1").arg(absUrl.toString()));
                            onCatalogFetched(ctx->repoInfo, {}, installed);
                            tryFinishSyncLegacy(installed);
                            return;
                        }
                        QNetworkReply *catReply = m_networkManager->get(packageNetworkRequest(absUrl));
                        enforceReplySizeLimit(catReply, kMaxRepositoryResponseBytes);
                        m_pendingRequests++;
                        connect(catReply, &QNetworkReply::finished, this, [this, catReply, ctx, absUrl, installed]() {
                            catReply->deleteLater();
                            m_pendingRequests--;
                            if (catReply->error() == QNetworkReply::NoError) {
                                ctx->catalogData = catReply->readAll();
                                // Cache catalog with repo URL so loadCachedPackages can restore provenance
                                QJsonObject cacheObj;
                                cacheObj[QStringLiteral("_repo_url")] = ctx->repoInfo.value(QStringLiteral("url")).toString();
                                QJsonDocument catDoc = QJsonDocument::fromJson(ctx->catalogData);
                                if (catDoc.isObject())
                                    cacheObj[QStringLiteral("packages")] = catDoc.object().value(QStringLiteral("packages"));
                                else
                                    cacheObj[QStringLiteral("packages")] = QJsonArray();
                                QString cacheName = QStringLiteral("catalog_") +
                                    QString::fromLatin1(QCryptographicHash::hash(
                                        ctx->repoInfo.value(QStringLiteral("url")).toString().toUtf8(),
                                        QCryptographicHash::Sha256).toHex()) +
                                    QStringLiteral(".json");
                                writeJsonAtomically(getReposCachePath() + QStringLiteral("/") + cacheName,
                                                    QJsonDocument(cacheObj));
                            }
                            onCatalogFetched(ctx->repoInfo, ctx->catalogData, installed);
                            tryFinishSyncLegacy(installed);
                        });
                    } else {
                        // Old format: treat repo.json itself as a flat packages list
                        onCatalogFetched(ctx->repoInfo, body, installed);
                    }
                }
            }
            tryFinishSyncLegacy(installed);
        });
    }
}

void PackageManager::onCatalogFetched(const QVariantMap &repoInfo, const QByteArray &data, const QVariantMap &installed) {
    QJsonDocument doc = QJsonDocument::fromJson(data);
    if (!doc.isObject())
        return;
    QJsonArray packages = doc.object().value(QStringLiteral("packages")).toArray();
    for (const auto &pVal : packages) {
        QVariantMap p = pVal.toObject().toVariantMap();
        p[QStringLiteral("_repo_url")] = repoInfo.value(QStringLiteral("url")).toString();
        if (p.value(QStringLiteral("id")).toString() == QStringLiteral("org.aviqtl.app")) {
            p[QStringLiteral("installed_version")] = appVersionString();
        } else if (installed.contains(p.value(QStringLiteral("id")).toString())) {
            p[QStringLiteral("installed_version")] = installed.value(p.value(QStringLiteral("id")).toString()).toMap().value(QStringLiteral("version"));
        }
        mergeCatalogPackage(p, repoInfo, installed);
    }
}

void PackageManager::mergeCatalogPackage(const QVariantMap &pkg, const QVariantMap &repoInfo, const QVariantMap &installed) {
    const QString id = pkg.value(QStringLiteral("id")).toString();
    const QString repoUrl = repoInfo.value(QStringLiteral("url")).toString();
    const int repoPriority = repoInfo.value(QStringLiteral("priority"), 10).toInt();

    // Find existing entry for this ID
    int existingIdx = -1;
    for (int i = 0; i < m_packageList.size(); ++i) {
        if (m_packageList[i].toMap().value(QStringLiteral("id")).toString() == id) {
            existingIdx = i;
            break;
        }
    }

    // Resolve display fields
    QString displayName = resolveLanguageField(pkg.value(QStringLiteral("display_name")), id);
    QString description = resolveLanguageField(pkg.value(QStringLiteral("short_description")), {});

    QVariantMap sources = pkg.value(QStringLiteral("_sources")).toMap();
    sources[repoUrl] = pkg.value(QStringLiteral("version")).toString();

    QVariantMap entry;
    entry[QStringLiteral("id")] = id;
    entry[QStringLiteral("type")] = pkg.value(QStringLiteral("type"));
    entry[QStringLiteral("display_name")] = displayName;
    entry[QStringLiteral("description")] = description;
    entry[QStringLiteral("author")] = pkg.value(QStringLiteral("author"));
    entry[QStringLiteral("version")] = pkg.value(QStringLiteral("version"));
    entry[QStringLiteral("categories")] = pkg.value(QStringLiteral("categories"));
    entry[QStringLiteral("min_app_version")] = pkg.value(QStringLiteral("min_app_version"));
    entry[QStringLiteral("metadata_url")] = pkg.value(QStringLiteral("metadata_url"));
    entry[QStringLiteral("metadata_sha256")] = pkg.value(QStringLiteral("metadata_sha256"));
    entry[QStringLiteral("_sources")] = sources;
    entry[QStringLiteral("_primary_repo")] = repoUrl;

    // Hydrate installed version
    if (id == QStringLiteral("org.aviqtl.app")) {
        entry[QStringLiteral("installed_version")] = appVersionString();
    } else if (installed.contains(id)) {
        entry[QStringLiteral("installed_version")] = installed.value(id).toMap().value(QStringLiteral("version"));
    }

    // Latest version: prefer the highest seen
    entry[QStringLiteral("latest_version")] = pkg.value(QStringLiteral("version"));

    if (existingIdx >= 0) {
        QVariantMap existing = m_packageList[existingIdx].toMap();
        QVariantMap existingSources = existing.value(QStringLiteral("_sources")).toMap();
        for (auto it = sources.constBegin(); it != sources.constEnd(); ++it)
            existingSources.insert(it.key(), it.value());
        existing[QStringLiteral("_sources")] = existingSources;

        // Determine the priority of the currently-stored primary repo so we
        // can make precedence explicit instead of relying on arrival order.
        const QString existingPrimaryRepo = existing.value(QStringLiteral("_primary_repo")).toString();
        int existingPriority = std::numeric_limits<int>::max();
        for (const auto &r : repositories()) {
            QVariantMap rm = r.toMap();
            if (rm.value(QStringLiteral("url")).toString() == existingPrimaryRepo) {
                existingPriority = rm.value(QStringLiteral("priority"), 10).toInt();
                break;
            }
        }

        // Take highest version as latest; on ties prefer the higher-priority
        // repo (lower priority number wins).
        QString existingLatest = existing.value(QStringLiteral("latest_version")).toString();
        QString newVersion = pkg.value(QStringLiteral("version")).toString();
        int cmp = compareVersions(newVersion, existingLatest);
        if (cmp > 0) {
            // Newer version: adopt the winning package's metadata fields so the
            // merged record stays consistent with the chosen source.
            existing[QStringLiteral("latest_version")] = newVersion;
            existing[QStringLiteral("version")] = newVersion;
            existing[QStringLiteral("_primary_repo")] = repoUrl;
            existing[QStringLiteral("metadata_url")] = pkg.value(QStringLiteral("metadata_url"));
            existing[QStringLiteral("metadata_sha256")] = pkg.value(QStringLiteral("metadata_sha256"));
        } else if (cmp == 0) {
            // Same version: keep the higher-priority repo as _primary_repo.
            if (repoPriority < existingPriority) {
                existing[QStringLiteral("_primary_repo")] = repoUrl;
                existing[QStringLiteral("metadata_url")] = pkg.value(QStringLiteral("metadata_url"));
                existing[QStringLiteral("metadata_sha256")] = pkg.value(QStringLiteral("metadata_sha256"));
            } else if (existingPrimaryRepo.isEmpty()) {
                existing[QStringLiteral("_primary_repo")] = repoUrl;
            }
        }
        m_packageList[existingIdx] = existing;
    } else {
        m_packageList.append(entry);
    }
}

void PackageManager::tryFinishSyncLegacy(const QVariantMap &installed) {
    Q_UNUSED(installed)
    if (m_pendingRequests > 0)
        return;
    emit packageListChanged();
    updateUpdateState();
    setProgress(1.0);
    setStatus(tr("Sync complete"));
    setBusy(false);
    emit repositoryRefreshed();
}

void PackageManager::updateUpdateState() {
    bool anyUpdates = false;
    for (const auto &p : m_packageList) {
        const QVariantMap item = p.toMap();
        const QString instVer = item.value(QStringLiteral("installed_version")).toString();
        const QString latVer = item.value(QStringLiteral("latest_version")).toString();
        if (!instVer.isEmpty() && !latVer.isEmpty() && compareVersions(latVer, instVer) > 0) {
            anyUpdates = true;
            break;
        }
    }
    setHasUpdatesAvailable(anyUpdates);
}

// --- Metadata Fetching ---

QString PackageManager::detailCacheKey(const QString &packageId, const QString &sourceRepo) {
    // Key metadata by both sourceRepo and packageId so same-ID packages from
    // different repositories don't mix up their cached details.
    return sourceRepo.isEmpty() ? packageId : sourceRepo + QStringLiteral("|") + packageId;
}

void PackageManager::fetchPackageMetadata(const QString &packageId, const QString &sourceRepo) {
    const QString cacheKey = detailCacheKey(packageId, sourceRepo);
    if (m_packageDetails.contains(cacheKey)) {
        emit packageDetailReady(packageId, sourceRepo, m_packageDetails[cacheKey]);
        return;
    }

    // Find catalog entry to get metadata_url; prefer the entry whose
    // _primary_repo matches sourceRepo when disambiguating same-ID packages.
    QVariantMap catalogEntry;
    for (const auto &p : std::as_const(m_packageList)) {
        QVariantMap pm = p.toMap();
        if (pm.value(QStringLiteral("id")).toString() == packageId) {
            if (!sourceRepo.isEmpty()) {
                if (pm.value(QStringLiteral("_primary_repo")).toString() == sourceRepo) {
                    catalogEntry = pm;
                    break;
                }
                // Keep looking for a repo match, but fall back to first hit.
                if (catalogEntry.isEmpty())
                    catalogEntry = pm;
            } else {
                catalogEntry = pm;
                break;
            }
        }
    }
    if (catalogEntry.isEmpty()) {
        emit errorOccurred(tr("Package not found: %1").arg(packageId));
        return;
    }

    QString metadataUrl = catalogEntry.value(QStringLiteral("metadata_url")).toString();
    if (metadataUrl.isEmpty()) {
        emit errorOccurred(tr("No metadata URL for package: %1").arg(packageId));
        return;
    }
    const QString expectedMetadataSha256 = catalogEntry.value(QStringLiteral("metadata_sha256")).toString();

    setStatus(tr("Fetching package details: %1").arg(packageId));
    QUrl url(metadataUrl);
    if (!Internal::isSecureNetworkUrl(url)) {
        emit errorOccurred(tr("Invalid or insecure metadata URL for package: %1").arg(packageId));
        return;
    }
    QNetworkReply *reply = m_networkManager->get(packageNetworkRequest(url));
    enforceReplySizeLimit(reply, kMaxRepositoryResponseBytes);
    connect(reply, &QNetworkReply::finished, this, [this, reply, packageId, sourceRepo, metadataUrl, cacheKey, expectedMetadataSha256]() {
        reply->deleteLater();
        if (reply->error() != QNetworkReply::NoError) {
            emit errorOccurred(tr("Failed to fetch package metadata (%1): %2").arg(packageId, reply->errorString()));
            return;
        }

        QByteArray body = reply->readAll();

        // Verify metadata checksum if the catalog provided one, so a
        // tampered payload cannot be trusted or cached.
        if (!expectedMetadataSha256.isEmpty()) {
            const QString actualSha256 = QString::fromLatin1(
                QCryptographicHash::hash(body, QCryptographicHash::Sha256).toHex());
            if (actualSha256 != expectedMetadataSha256) {
                emit errorOccurred(tr("Metadata checksum mismatch for package %1: expected %2, got %3")
                                       .arg(packageId, expectedMetadataSha256, actualSha256));
                return;
            }
        }

        QJsonDocument doc = QJsonDocument::fromJson(body);
        if (!doc.isObject()) {
            emit errorOccurred(tr("Invalid metadata format for package: %1").arg(packageId));
            return;
        }

        QVariantMap detail = doc.object().toVariantMap();
        m_packageDetails[cacheKey] = detail;

        // Cache to repos directory
        QString cacheName = QStringLiteral("detail_") +
            QString::fromLatin1(QCryptographicHash::hash(metadataUrl.toUtf8(), QCryptographicHash::Sha256).toHex()) +
            QStringLiteral(".json");
        writeJsonAtomically(getReposCachePath() + QStringLiteral("/") + cacheName, doc);

        emit packageDetailReady(packageId, sourceRepo, detail);
    });
}

void PackageManager::fetchPackageMetadataForInstall(const QString &packageId, const QString &sourceRepo, const QString &version) {
    m_pendingInstall = {
        {QStringLiteral("id"), packageId},
        {QStringLiteral("sourceRepo"), sourceRepo},
        {QStringLiteral("version"), version}
    };

    const QString cacheKey = detailCacheKey(packageId, sourceRepo);
    if (m_packageDetails.contains(cacheKey)) {
        continueInstallWithMetadata(packageId, sourceRepo, version, m_packageDetails[cacheKey]);
        return;
    }

    QMetaObject::Connection *conn = new QMetaObject::Connection();
    QMetaObject::Connection *errConn = new QMetaObject::Connection();

    *conn = connect(this, &PackageManager::packageDetailReady, this,
        [this, conn, errConn, packageId, sourceRepo, version](const QString &readyId, const QString &readyRepo, const QVariantMap &detail) {
            if (readyId == packageId && readyRepo == sourceRepo) {
                if (*conn) { disconnect(*conn); *conn = {}; }
                if (*errConn) { disconnect(*errConn); *errConn = {}; }
                delete conn;
                delete errConn;
                if (m_pendingInstall.value(QStringLiteral("id")).toString() == packageId)
                    continueInstallWithMetadata(packageId, sourceRepo, version, detail);
            }
        });

    // Clean up and abort the install flow when metadata fetch fails so the
    // pending install/upgrade queue is not left silently waiting.
    *errConn = connect(this, &PackageManager::errorOccurred, this,
        [this, conn, errConn, packageId](const QString &message) {
            Q_UNUSED(message)
            if (m_pendingInstall.value(QStringLiteral("id")).toString() == packageId) {
                if (*conn) { disconnect(*conn); *conn = {}; }
                if (*errConn) { disconnect(*errConn); *errConn = {}; }
                delete conn;
                delete errConn;
                setBusy(false);
                // Advance the upgrade queue if we were in an upgrade flow
                if (!m_upgradeQueue.isEmpty())
                    processUpgradeQueue();
            }
        });

    fetchPackageMetadata(packageId, sourceRepo);
}

void PackageManager::continueInstallWithMetadata(const QString &packageId, const QString &sourceRepo, const QString &version, const QVariantMap &detail) {
    if (version.isEmpty())
        m_pendingInstall[QStringLiteral("version")] = QString(); // use latest from detail

    QString targetVersion = version;
    QString downloadUrl;
    QString sha256;
    QString minAppVersion;

    QVariantList versions = detail.value(QStringLiteral("versions")).toList();
    if (versions.isEmpty()) {
        // Legacy flat format: use top-level fields
        downloadUrl = detail.value(QStringLiteral("download_url")).toString();
        sha256 = detail.value(QStringLiteral("download_sha256")).toString();
        minAppVersion = detail.value(QStringLiteral("min_app_version")).toString();
        if (targetVersion.isEmpty())
            targetVersion = detail.value(QStringLiteral("version")).toString();
    } else {
        if (targetVersion.isEmpty()) {
            // Find highest version
            QString bestVer;
            QVariantMap bestEntry;
            for (const auto &v : versions) {
                QVariantMap ve = v.toMap();
                QString ver = ve.value(QStringLiteral("version")).toString();
                if (bestVer.isEmpty() || compareVersions(ver, bestVer) > 0) {
                    bestVer = ver;
                    bestEntry = ve;
                }
            }
            targetVersion = bestVer;
            downloadUrl = bestEntry.value(QStringLiteral("download_url")).toString();
            sha256 = bestEntry.value(QStringLiteral("download_sha256")).toString();
            minAppVersion = bestEntry.value(QStringLiteral("min_app_version")).toString();
        } else {
            for (const auto &v : versions) {
                QVariantMap ve = v.toMap();
                if (ve.value(QStringLiteral("version")).toString() == targetVersion) {
                    downloadUrl = ve.value(QStringLiteral("download_url")).toString();
                    sha256 = ve.value(QStringLiteral("download_sha256")).toString();
                    minAppVersion = ve.value(QStringLiteral("min_app_version")).toString();
                    break;
                }
            }
        }
    }

    if (downloadUrl.isEmpty()) {
        setBusy(false);
        emit errorOccurred(tr("No download URL found for package %1 version %2").arg(packageId, targetVersion));
        return;
    }

    // Check min app version
    if (!minAppVersion.isEmpty() && compareVersions(appVersionString(), minAppVersion) < 0) {
        setBusy(false);
        emit errorOccurred(tr("Package %1 requires AviQtl %2 or newer (current: %3)").arg(packageId, minAppVersion, appVersionString()));
        return;
    }

    QString packageType = detail.value(QStringLiteral("type")).toString();
    QString effectiveRepo = sourceRepo.isEmpty() ? m_pendingInstall.value(QStringLiteral("sourceRepo")).toString() : sourceRepo;

    downloadPackage(packageId, QUrl(downloadUrl), sha256, packageType, targetVersion, effectiveRepo);
}

// --- Package Installation ---

void PackageManager::installPackage(const QString &packageId, const QString &sourceRepo, const QString &version) {
    if (m_isBusy)
        return;

    // Self-update is handled separately
    if (packageId == QStringLiteral("org.aviqtl.app")) {
        QString ver = version.isEmpty() ? QStringLiteral("latest") : version;
        emit selfUpdateAvailable(ver, QString());
        setStatus(tr("AviQtl update available. Restart to apply."));
        return;
    }

    setBusy(true);
    setProgress(0.0);
    m_pendingInstall = {
        {QStringLiteral("id"), packageId},
        {QStringLiteral("sourceRepo"), sourceRepo},
        {QStringLiteral("version"), version}
    };

    fetchPackageMetadataForInstall(packageId, sourceRepo, version);
}

void PackageManager::downloadPackage(const QString &packageId, const QUrl &url, const QString &expectedSha256, const QString &packageType, const QString &version, const QString &sourceRepo) {
    if (!isValidPackageId(packageId) || !isValidPackageType(packageType)) {
        setBusy(false);
        emit errorOccurred(tr("Invalid package ID or type."));
        return;
    }
    if (!Internal::isSecureNetworkUrl(url)) {
        setBusy(false);
        emit errorOccurred(tr("Invalid or insecure package download URL."));
        return;
    }
    setStatus(tr("Downloading package: %1").arg(packageId));
    setProgress(0.0);

    QNetworkReply *reply = m_networkManager->get(packageNetworkRequest(url));
    enforceReplySizeLimit(reply, kMaxPackageDownloadBytes);
    connect(reply, &QNetworkReply::downloadProgress, this, [this, reply](qint64 received, qint64 total) {
        if (total > 0)
            setProgress(static_cast<double>(received) / total * 0.5);
    });
    connect(reply, &QNetworkReply::finished, this, [this, reply, packageId, url, expectedSha256, packageType, version, sourceRepo]() {
        reply->deleteLater();
        if (reply->error() != QNetworkReply::NoError) {
            setBusy(false);
            if (reply->property("aviqtlSizeLimitExceeded").toBool())
                emit errorOccurred(tr("Package archive exceeds the maximum allowed size."));
            else
                emit errorOccurred(tr("Download failed: %1").arg(reply->errorString()));
            return;
        }

        QTemporaryDir tempDir;
        if (!tempDir.isValid()) {
            setBusy(false);
            emit errorOccurred(tr("Failed to create temporary directory."));
            return;
        }

        QString fileName = url.fileName();
        if (fileName.isEmpty()) fileName = QStringLiteral("package.zip");
        const QString archivePath = tempDir.path() + QStringLiteral("/") + fileName;
        QFile file(archivePath);
        if (!file.open(QIODevice::WriteOnly)) {
            setBusy(false);
            emit errorOccurred(tr("Failed to save downloaded file."));
            return;
        }
        QByteArray data = reply->readAll();
        if (data.size() > kMaxPackageDownloadBytes) {
            setBusy(false);
            emit errorOccurred(tr("Package archive exceeds the maximum allowed size."));
            return;
        }
        if (file.write(data) != data.size()) {
            setBusy(false);
            emit errorOccurred(tr("Failed to write the complete downloaded package."));
            return;
        }
        file.close();

        // SHA256 verification
        if (!expectedSha256.isEmpty()) {
            QString actualSha256 = QString::fromLatin1(QCryptographicHash::hash(data, QCryptographicHash::Sha256).toHex());
            if (actualSha256 != expectedSha256) {
                setBusy(false);
                emit errorOccurred(tr("Checksum mismatch for %1: expected %2, got %3").arg(packageId, expectedSha256, actualSha256));
                return;
            }
        }

        setStatus(tr("Extracting package..."));
        setProgress(0.6);

        extractAndDeploy(packageId, archivePath, packageType, version, url.toString(), sourceRepo);
    });
}

void PackageManager::extractAndDeploy(const QString &packageId, const QString &archivePath, const QString &packageType, const QString &version, const QString &downloadUrl, const QString &sourceRepo) {
    QTemporaryDir extractDir;
    if (!extractDir.isValid()) {
        setBusy(false);
        emit errorOccurred(tr("Failed to create extraction directory."));
        return;
    }

    if (!extractPackageArchive(archivePath, extractDir.path())) {
        setBusy(false);
        emit errorOccurred(tr("Failed to extract package archive."));
        return;
    }

    // Prepare installation state before deployment. The deployment helper
    // commits it only after the staged files are in place and rolls the files
    // back if the atomic state write fails.
    QVariantMap installed = loadInstalledPackagesFromFile();
    QVariantMap info;
    info[QStringLiteral("version")] = version;
    info[QStringLiteral("type")] = packageType;
    info[QStringLiteral("installed_at")] = QDateTime::currentDateTimeUtc().toString(Qt::ISODate);
    info[QStringLiteral("installed_from_repo")] = sourceRepo;
    info[QStringLiteral("installed_from_url")] = downloadUrl;
    info[QStringLiteral("sha256")] = sha256OfFile(archivePath);
    installed[packageId] = info;

    setStatus(tr("Deploying package files..."));
    setProgress(0.8);
    const FileOperationResult deployResult = deployPackageFiles(packageId, extractDir.path(), packageType, [installed]() {
            return writeJsonAtomically(getInstalledPackagesPath(), QJsonDocument::fromVariant(installed));
        });
    if (deployResult != FileOperationResult::Success) {
        qWarning() << "[PackageManager] Failed to deploy package or atomically save installation state.";
        setBusy(false);
        if (deployResult == FileOperationResult::RollbackFailed)
            emit errorOccurred(tr("Package deployment failed and automatic rollback was incomplete; the backup was preserved."));
        else
            emit errorOccurred(tr("Failed to deploy package; the previous installation was restored."));
        if (!m_upgradeQueue.isEmpty())
            processUpgradeQueue();
        return;
    }

    // Compile shaders for effect/object/transition packages
    if (packageType == QStringLiteral("effect") || packageType == QStringLiteral("object") || packageType == QStringLiteral("transition")) {
        const QString deployDir = getPackageDeployDir(packageType);
        const QString packageDir = deployDir + QStringLiteral("/") + packageId;
        compileShadersInDirectory(packageDir);
    }

    // Reload registry for effect/object/transition
    if (packageType == QStringLiteral("effect") || packageType == QStringLiteral("object") || packageType == QStringLiteral("transition")) {
        const QString deployDir = getPackageDeployDir(packageType);
        EffectRegistry::instance().loadEffectsFromDirectory(deployDir);
    } else if (packageType == QStringLiteral("mod")) {
        // Mods will be loaded on next app restart
    }

    setProgress(1.0);
    setStatus(tr("Installation complete: %1").arg(packageId));
    setBusy(false);

    // Update list state
    for (int i = 0; i < m_packageList.size(); ++i) {
        QVariantMap item = m_packageList[i].toMap();
        if (item.value(QStringLiteral("id")).toString() == packageId) {
            item[QStringLiteral("installed_version")] = version;
            m_packageList[i] = item;
            emit packageListChanged();
        }
    }

    emit packageInstalled(packageId);
    updateUpdateState();

    // Continue upgrade queue
    if (!m_upgradeQueue.isEmpty())
        processUpgradeQueue();
}

bool PackageManager::extractPackageArchive(const QString &archivePath, const QString &destDir) {
    QZipReader reader(archivePath);
    if (!reader.exists() || !reader.isReadable() || reader.status() != QZipReader::NoError) {
        qWarning() << "[PackageManager] Package archive is not a readable ZIP file.";
        return false;
    }

    const QList<QZipReader::FileInfo> entries = reader.fileInfoList();
    if (entries.size() > kMaxPackageArchiveEntries)
        return false;

    qint64 extractedBytes = 0;
    for (const QZipReader::FileInfo &entry : entries) {
        if (!entry.isValid() || entry.isSymLink || !isSafeArchivePath(entry.filePath) || entry.size < 0 ||
            extractedBytes > kMaxPackageExtractedBytes - entry.size) {
            qWarning() << "[PackageManager] Unsafe package archive entry:" << entry.filePath;
            return false;
        }
        extractedBytes += entry.size;
    }

    if (!QDir().mkpath(destDir) || !reader.extractAll(destDir))
        return false;
    return true;
}

bool PackageManager::isSafeArchivePath(const QString &path) {
    if (path.isEmpty() || QDir::isAbsolutePath(path) || path.contains(QLatin1Char('\\')))
        return false;
    const QString normalized = QDir::cleanPath(path);
    return normalized != QStringLiteral("..") && !normalized.startsWith(QStringLiteral("../")) &&
           !normalized.contains(QStringLiteral("/../"));
}

PackageManager::FileOperationResult PackageManager::deployPackageFiles(
    const QString &packageId, const QString &extractDir, const QString &packageType,
    const std::function<bool()> &commitState) {
    if (!isValidPackageId(packageId) || !isValidPackageType(packageType)) {
        qWarning() << "[PackageManager] Invalid package ID or type:" << packageId << packageType;
        return FileOperationResult::Failed;
    }
    const QString deployBase = getPackageDeployDir(packageType);
    if (deployBase.isEmpty()) return FileOperationResult::Failed;
    const QString packageDir = deployBase + QStringLiteral("/") + packageId;

    // Resolve the source directory (handle single-subdir wrapper archives)
    QDir sourceDir(extractDir);
    QStringList entries = sourceDir.entryList(QDir::Files | QDir::Dirs | QDir::NoDotAndDotDot);
    if (entries.size() == 1) {
        QFileInfo fi(sourceDir.absoluteFilePath(entries.first()));
        if (fi.isDir()) { sourceDir.cd(entries.first()); entries = sourceDir.entryList(QDir::Files | QDir::Dirs | QDir::NoDotAndDotDot); }
    }

    // Ensure the base directory exists before creating the staging directory.
    QDir().mkpath(deployBase);

    // Stage new contents into a temporary sibling directory so we can roll
    // back if copying fails and avoid leaving stale files behind on upgrade.
    QTemporaryDir stagingDir(deployBase + QStringLiteral("/.staging_XXXXXX"));
    if (!stagingDir.isValid()) {
        qWarning() << "[PackageManager] Failed to create staging directory for deployment.";
        return FileOperationResult::Failed;
    }
    const QString stagingPath = stagingDir.path();
    for (const QString &entry : std::as_const(entries)) {
        const QString srcPath = sourceDir.absoluteFilePath(entry);
        const QString destPath = stagingPath + QStringLiteral("/") + entry;
        QFileInfo srcInfo(srcPath);
        if (srcInfo.isDir()) { if (!copyDirectory(srcPath, destPath)) return FileOperationResult::Failed; }
        else { if (QFile::exists(destPath)) QFile::remove(destPath); if (!QFile::copy(srcPath, destPath)) return FileOperationResult::Failed; }
    }

    const QString backupDir = deployBase + QStringLiteral("/.backup_") + packageId;
    QDir().mkpath(deployBase);
    const FileOperationResult result = runFileTransaction(
        packageDir, backupDir,
        [stagingPath, packageDir] { return QDir().rename(stagingPath, packageDir); },
        [packageDir] { return !QDir(packageDir).exists() || QDir(packageDir).removeRecursively(); },
        commitState, "deploy");
    if (result == FileOperationResult::Success)
        stagingDir.setAutoRemove(false);
    return result;
}

PackageManager::FileOperationResult PackageManager::removePackageFiles(
    const QString &packageId, const QString &packageType, const std::function<bool()> &commitState) {
    if (!isValidPackageId(packageId) || !isValidPackageType(packageType) || !commitState)
        return FileOperationResult::Failed;
    const QString deployDir = getPackageDeployDir(packageType);
    if (deployDir.isEmpty())
        return FileOperationResult::Failed;

    const QString packageDir = deployDir + QLatin1Char('/') + packageId;
    const QString backupDir = deployDir + QStringLiteral("/.remove_backup_") + packageId;
    return runFileTransaction(packageDir, backupDir, [] { return true; }, [] { return true; }, commitState, "remove");
}

PackageManager::FileOperationResult PackageManager::runFileTransaction(
    const QString &targetDir, const QString &backupDir,
    const std::function<bool()> &applyMutation, const std::function<bool()> &revertMutation,
    const std::function<bool()> &commitState, const char *operationName) {
    const bool hadExisting = QDir(targetDir).exists();
    if (hadExisting) {
        if (QDir(backupDir).exists() && !QDir(backupDir).removeRecursively()) {
            qWarning() << "[PackageManager] Could not remove stale backup before" << operationName << ':' << backupDir;
            return FileOperationResult::Failed;
        }
        if (!QDir().rename(targetDir, backupDir)) {
            qWarning() << "[PackageManager] Could not create backup before" << operationName << ':' << targetDir;
            return FileOperationResult::Failed;
        }
    }

    if (!applyMutation()) {
        qWarning() << "[PackageManager] File mutation failed during" << operationName;
        if (hadExisting && !QDir().rename(backupDir, targetDir)) {
            qCritical() << "[PackageManager] Could not restore backup after mutation failure:" << backupDir;
            return FileOperationResult::RollbackFailed;
        }
        return FileOperationResult::Failed;
    }

    if (commitState && !commitState()) {
        qWarning() << "[PackageManager] State commit failed during" << operationName << "; rolling back.";
        const bool reverted = revertMutation();
        const bool restored = !hadExisting || (reverted && QDir().rename(backupDir, targetDir));
        if (!reverted || !restored) {
            qCritical() << "[PackageManager] Could not restore backup after state commit failure:" << backupDir;
            return FileOperationResult::RollbackFailed;
        }
        return FileOperationResult::StateCommitFailed;
    }

    if (hadExisting && !QDir(backupDir).removeRecursively())
        qWarning() << "[PackageManager] Operation succeeded but backup cleanup failed:" << backupDir;
    return FileOperationResult::Success;
}

QString PackageManager::getPackageDeployDir(const QString &packageType) const {
    const QString appDir = QCoreApplication::applicationDirPath();
    if (packageType == QStringLiteral("mod")) return appDir + QStringLiteral("/plugins");
    if (packageType == QStringLiteral("effect")) return appDir + QStringLiteral("/effects");
    if (packageType == QStringLiteral("object")) return appDir + QStringLiteral("/objects");
    if (packageType == QStringLiteral("transition")) return appDir + QStringLiteral("/transitions");
    return {};
}

void PackageManager::removePackage(const QString &packageId) {
    if (m_isBusy || packageId == QStringLiteral("org.aviqtl.app")) return;
    if (!isValidPackageId(packageId)) {
        emit errorOccurred(tr("Invalid package ID.")); return;
    }
    const QVariantMap currentInstalled = loadInstalledPackagesFromFile();
    const QVariantMap installedPackage = currentInstalled.value(packageId).toMap();
    const QString packageType = installedPackage.value(QStringLiteral("type")).toString();
    if (!currentInstalled.contains(packageId) || !isValidPackageType(packageType)) {
        emit errorOccurred(tr("Cannot remove package because its installed type is missing or invalid."));
        return;
    }
    setBusy(true);
    setStatus(tr("Removing package: %1").arg(packageId));
    QVariantMap updatedInstalled = currentInstalled;
    updatedInstalled.remove(packageId);
    const FileOperationResult removalResult = removePackageFiles(packageId, packageType, [updatedInstalled]() {
        return writeJsonAtomically(getInstalledPackagesPath(), QJsonDocument::fromVariant(updatedInstalled));
    });
    if (removalResult != FileOperationResult::Success) {
        setBusy(false);
        if (removalResult == FileOperationResult::RollbackFailed)
            emit errorOccurred(tr("Package removal failed and automatic rollback was incomplete; the backup was preserved."));
        else
            emit errorOccurred(tr("Failed to remove package; the installed state and files were restored."));
        return;
    }
    for (int i = 0; i < m_packageList.size(); ++i) {
        QVariantMap item = m_packageList[i].toMap();
        if (item.value(QStringLiteral("id")).toString() == packageId) {
            item.remove(QStringLiteral("installed_version"));
            m_packageList[i] = item;
            emit packageListChanged();
            break;
        }
    }
    setBusy(false);
    setStatus(tr("Removal complete: %1").arg(packageId));
    emit packageRemoved(packageId);
}

QVariantList PackageManager::getPackagesByType(const QString &type) const {
    QVariantList result;
    for (const auto &p : m_packageList) {
        QVariantMap pkg = p.toMap();
        if (type == QStringLiteral("installed")) {
            if (!pkg.value(QStringLiteral("installed_version")).toString().isEmpty())
                result.append(pkg);
        } else {
            if (pkg.value(QStringLiteral("type")).toString() == type)
                result.append(pkg);
        }
    }
    return result;
}

void PackageManager::upgradeAllPackages() {
    if (m_isBusy) return;
    m_upgradeQueue.clear();
    for (const auto &p : m_packageList) {
        const QVariantMap item = p.toMap();
        const QString installedVer = item.value(QStringLiteral("installed_version")).toString();
        const QString latestVer = item.value(QStringLiteral("latest_version")).toString();
        if (!installedVer.isEmpty() && !latestVer.isEmpty() && compareVersions(latestVer, installedVer) > 0)
            m_upgradeQueue.enqueue(item.value(QStringLiteral("id")).toString());
    }
    if (m_upgradeQueue.isEmpty()) { setStatus(tr("No packages to upgrade.")); return; }
    setBusy(true);
    setStatus(tr("Upgrading all packages..."));
    processUpgradeQueue();
}

void PackageManager::processUpgradeQueue() {
    if (m_upgradeQueue.isEmpty()) {
        setBusy(false);
        setStatus(tr("All upgrades complete."));
        setHasUpdatesAvailable(false);
        return;
    }
    QString nextPackageId = m_upgradeQueue.dequeue();
    setStatus(tr("Upgrading package: %1").arg(nextPackageId));
    // Drive the install pipeline directly: upgradeAllPackages already set
    // m_isBusy, so we must bypass installPackage's busy guard to keep the
    // queue advancing through the existing m_pendingInstall flow.
    setProgress(0.0);
    m_pendingInstall = {
        {QStringLiteral("id"), nextPackageId},
        {QStringLiteral("sourceRepo"), QString()},
        {QStringLiteral("version"), QString()}
    };
    fetchPackageMetadataForInstall(nextPackageId, QString(), QString());
}

void PackageManager::compileShadersInDirectory(const QString &directory) {
    const QStringList shaderExtensions = {QStringLiteral("*.frag"), QStringLiteral("*.comp"), QStringLiteral("*.vert")};
    QDirIterator it(directory, shaderExtensions, QDir::Files, QDirIterator::Subdirectories);
    int compiled = 0, skipped = 0, failed = 0;
    while (it.hasNext()) {
        const QString sourcePath = it.next();
        const QString qsbPath = sourcePath + QStringLiteral(".qsb");
        if (!ShaderCompiler::needsRecompile(sourcePath, qsbPath)) { skipped++; continue; }
        QString error;
        if (ShaderCompiler::compileToFile(sourcePath, qsbPath, &error)) compiled++;
        else { qWarning().noquote() << "[PackageManager] Shader compilation failed:" << sourcePath << ":" << error; failed++; }
    }
    if (compiled > 0 || failed > 0)
        qDebug().noquote() << "[PackageManager] Shaders compiled:" << compiled << "skipped:" << skipped << "failed:" << failed;
}

} // namespace AviQtl::Core
