#include "package_manager.hpp"
#include "effect_registry.hpp"
#include "package_deployment.hpp"
#include "package_url_utils.hpp"
#include "rust_package_document.hpp"
#include "settings_manager.hpp"
#include "shader_compiler.hpp"
#include "version.hpp"
#include <QCoreApplication>
#include <QCryptographicHash>
#include <QDateTime>
#include <QDir>
#include <QDirIterator>
#include <QFile>
#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QLocale>
#include <QNetworkAccessManager>
#include <QNetworkReply>
#include <QSaveFile>
#include <QTemporaryDir>
#include <QTimer>
#include <QXmlStreamReader>
#include <algorithm>
#include <memory>

namespace AviQtl::Core {

namespace {
constexpr qint64 kMaxPackageDownloadBytes = 256LL * 1024LL * 1024LL;
constexpr qint64 kMaxRepositoryResponseBytes = 16LL * 1024LL * 1024LL;
constexpr int kNetworkTransferTimeoutMs = 30000;

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
    QObject::connect(reply, &QNetworkReply::metaDataChanged, reply, [reply, abortIfTooLarge]() { abortIfTooLarge(reply->bytesAvailable(), reply->header(QNetworkRequest::ContentLengthHeader).toLongLong()); });
    QObject::connect(reply, &QNetworkReply::downloadProgress, reply, abortIfTooLarge);
}

const QString &appVersionString() {
    static const QString cached = QString::fromUtf8(AviQtl::VERSION_STRING);
    return cached;
}

bool isPathWithinDirectory(const QString &path, const QString &directoryPath) {
    const QString canonicalPath = QFileInfo(path).canonicalFilePath();
    const QString canonicalDirectory = QFileInfo(directoryPath).canonicalFilePath();
    if (canonicalPath.isEmpty() || canonicalDirectory.isEmpty()) {
        return false;
    }
    const QString relativePath = QDir(canonicalDirectory).relativeFilePath(canonicalPath);
    return relativePath != QStringLiteral("..") && !relativePath.startsWith(QStringLiteral("../")) && !QDir::isAbsolutePath(relativePath);
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
    if (m_isBusy == busy)
        return;
    m_isBusy = busy;
    emit isBusyChanged();
}

void PackageManager::setStatus(const QString &status) {
    if (m_statusText == status)
        return;
    m_statusText = status;
    emit statusTextChanged();
}

void PackageManager::setProgress(double p) {
    if (m_progress == p)
        return;
    m_progress = p;
    emit progressChanged();
}

void PackageManager::setHasUpdatesAvailable(bool available) {
    if (m_hasUpdatesAvailable == available)
        return;
    m_hasUpdatesAvailable = available;
    emit hasUpdatesAvailableChanged();
}

void PackageManager::loadCachedPackages() {
    const QString cacheDir = getReposCachePath();
    QVariantMap installed = loadInstalledPackagesFromFile();

    // Load cached catalog files
    QDir dir(cacheDir);
    const QStringList files = dir.entryList({QStringLiteral("catalog_*.json")}, QDir::Files);
    const QVariantList configuredRepositories = repositories();
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
        QVariantList catalogPackages;
        catalogPackages.reserve(packages.size());
        for (const auto &pVal : packages) {
            QVariantMap p = pVal.toObject().toVariantMap();
            p[QStringLiteral("_repo_url")] = repoUrl;
            catalogPackages.append(p);
        }
        mergeCatalogPackages(catalogPackages, repoInfo, configuredRepositories, installed);
    }
    emit packageListChanged();
    updateUpdateState();
    setStatus(tr("Packages loaded from cache (Press Sync to check for updates)"));
}

QVariantList PackageManager::repositories() const { return SettingsManager::instance().value(QStringLiteral("packageRepositories"), QVariantList{}).toList(); }

void PackageManager::saveRepositories(const QVariantList &repos) {
    SettingsManager::instance().setValue(QStringLiteral("packageRepositories"), repos);
    emit repositoriesChanged();
}

void PackageManager::addRepository(const QString &url, bool enabled, int priority) {
    if (!Internal::isSecureNetworkUrl(QUrl(url))) {
        emit errorOccurred(tr("Repository URL must use HTTPS: %1").arg(url));
        return;
    }
    const auto mutation = RustCore::Package::mutateRepositories(repositories(), RustCore::Package::RepositoryOperation::Add, url, enabled, priority);
    if (mutation.changed)
        saveRepositories(mutation.repositories);
}

void PackageManager::removeRepository(const QString &url) {
    const auto mutation = RustCore::Package::mutateRepositories(repositories(), RustCore::Package::RepositoryOperation::Remove, url);
    if (mutation.changed)
        saveRepositories(mutation.repositories);
}

void PackageManager::setRepositoryEnabled(const QString &url, bool enabled) {
    const auto mutation = RustCore::Package::mutateRepositories(repositories(), RustCore::Package::RepositoryOperation::Enabled, url, enabled);
    if (mutation.changed)
        saveRepositories(mutation.repositories);
}

void PackageManager::setRepositoryPriority(const QString &url, int priority) {
    const auto mutation = RustCore::Package::mutateRepositories(repositories(), RustCore::Package::RepositoryOperation::Priority, url, true, priority);
    if (mutation.changed)
        saveRepositories(mutation.repositories);
}

// --- Sync Pipeline ---

void PackageManager::sync() { refreshRepositories(); }

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

    const QVariantList repos = RustCore::Package::enabledRepositories(repositories());
    if (repos.isEmpty()) {
        m_pendingRequests = 0;
        updateUpdateState();
        setProgress(1.0);
        setStatus(tr("Idle"));
        setBusy(false);
        emit repositoryRefreshed();
        return;
    }
    m_pendingRequests = repos.size();

    for (const auto &r : repos) {
        QVariantMap repo = r.toMap();
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
                                QString cacheName = QStringLiteral("catalog_") + QString::fromLatin1(QCryptographicHash::hash(ctx->repoInfo.value(QStringLiteral("url")).toString().toUtf8(), QCryptographicHash::Sha256).toHex()) + QStringLiteral(".json");
                                writeJsonAtomically(getReposCachePath() + QStringLiteral("/") + cacheName, QJsonDocument(cacheObj));
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
    QVariantList catalogPackages;
    catalogPackages.reserve(packages.size());
    for (const auto &pVal : packages) {
        QVariantMap p = pVal.toObject().toVariantMap();
        p[QStringLiteral("_repo_url")] = repoInfo.value(QStringLiteral("url")).toString();
        catalogPackages.append(p);
    }
    mergeCatalogPackages(catalogPackages, repoInfo, repositories(), installed);
}

void PackageManager::mergeCatalogPackages(const QVariantList &packages, const QVariantMap &repoInfo,
                                          const QVariantList &repositories,
                                          const QVariantMap &installed) {
    const auto merged = RustCore::Package::mergeCatalogBatch(
        m_packageList, packages, repoInfo, repositories, installed,
        QLocale::system().name().left(2), appVersionString());
    if (merged.has_value())
        m_packageList = *merged;
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

void PackageManager::updateUpdateState() { setHasUpdatesAvailable(RustCore::Package::hasUpdates(m_packageList)); }

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

    const QVariantMap catalogEntry = RustCore::Package::find(m_packageList, packageId, sourceRepo);
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
            const QString actualSha256 = QString::fromLatin1(QCryptographicHash::hash(body, QCryptographicHash::Sha256).toHex());
            if (actualSha256 != expectedMetadataSha256) {
                emit errorOccurred(tr("Metadata checksum mismatch for package %1: expected %2, got %3").arg(packageId, expectedMetadataSha256, actualSha256));
                return;
            }
        }

        const QJsonDocument document = QJsonDocument::fromJson(body);
        if (!document.isObject()) {
            emit errorOccurred(tr("Invalid metadata format for package: %1").arg(packageId));
            return;
        }

        const auto normalized = RustCore::Package::normalizeMetadata(document.object().toVariantMap());
        if (!normalized.has_value()) {
            emit errorOccurred(tr("Invalid metadata format for package: %1").arg(packageId));
            return;
        }
        const QVariantMap detail = *normalized;
        m_packageDetails[cacheKey] = detail;

        // Cache to repos directory
        QString cacheName = QStringLiteral("detail_") + QString::fromLatin1(QCryptographicHash::hash(metadataUrl.toUtf8(), QCryptographicHash::Sha256).toHex()) + QStringLiteral(".json");
        writeJsonAtomically(getReposCachePath() + QStringLiteral("/") + cacheName, QJsonDocument::fromVariant(detail));

        emit packageDetailReady(packageId, sourceRepo, detail);
    });
}

void PackageManager::fetchPackageMetadataForInstall(const QString &packageId, const QString &sourceRepo, const QString &version) {
    m_pendingInstall = {{QStringLiteral("id"), packageId}, {QStringLiteral("sourceRepo"), sourceRepo}, {QStringLiteral("version"), version}};

    const QString cacheKey = detailCacheKey(packageId, sourceRepo);
    if (m_packageDetails.contains(cacheKey)) {
        continueInstallWithMetadata(packageId, sourceRepo, version, m_packageDetails[cacheKey]);
        return;
    }

    auto conn = std::make_shared<QMetaObject::Connection>();
    auto errConn = std::make_shared<QMetaObject::Connection>();
    const auto disconnectHandlers = [conn, errConn]() {
        if (*conn) {
            QObject::disconnect(*conn);
            *conn = {};
        }
        if (*errConn) {
            QObject::disconnect(*errConn);
            *errConn = {};
        }
    };

    *conn = connect(this, &PackageManager::packageDetailReady, this, [this, disconnectHandlers, packageId, sourceRepo, version](const QString &readyId, const QString &readyRepo, const QVariantMap &detail) {
        if (readyId == packageId && readyRepo == sourceRepo) {
            disconnectHandlers();
            if (m_pendingInstall.value(QStringLiteral("id")).toString() == packageId)
                continueInstallWithMetadata(packageId, sourceRepo, version, detail);
        }
    });

    // Clean up and abort the install flow when metadata fetch fails so the
    // pending install/upgrade queue is not left silently waiting.
    *errConn = connect(this, &PackageManager::errorOccurred, this, [this, disconnectHandlers, packageId](const QString &message) {
        Q_UNUSED(message)
        if (m_pendingInstall.value(QStringLiteral("id")).toString() == packageId) {
            disconnectHandlers();
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

    const auto selection = RustCore::Package::selectInstall(detail, version, appVersionString());
    if (!selection.has_value() || selection->status == QStringLiteral("invalid_type")) {
        setBusy(false);
        emit errorOccurred(tr("Invalid package ID or type."));
        return;
    }
    if (selection->status == QStringLiteral("no_download")) {
        setBusy(false);
        emit errorOccurred(tr("No download URL found for package %1 version %2").arg(packageId, selection->version));
        return;
    }
    if (selection->status == QStringLiteral("requires_newer_app")) {
        setBusy(false);
        emit errorOccurred(tr("Package %1 requires AviQtl %2 or newer (current: %3)").arg(packageId, selection->minAppVersion, appVersionString()));
        return;
    }
    if (selection->status != QStringLiteral("ok")) {
        setBusy(false);
        emit errorOccurred(tr("Package installation could not be validated."));
        return;
    }

    const QString effectiveRepo = sourceRepo.isEmpty() ? m_pendingInstall.value(QStringLiteral("sourceRepo")).toString() : sourceRepo;
    downloadPackage(packageId, QUrl(selection->downloadUrl), selection->sha256, selection->type, selection->version, effectiveRepo);
}

// --- Package Installation ---

void PackageManager::installPackage(const QString &packageId, const QString &sourceRepo, const QString &version) {
    if (m_isBusy)
        return;

    if (!Internal::PackageDeployment::isValidPackageId(packageId)) {
        m_pendingInstall.clear();
        emit errorOccurred(tr("Invalid package ID or type."));
        return;
    }

    // Self-update is handled separately
    if (packageId == QStringLiteral("org.aviqtl.app")) {
        QString ver = version.isEmpty() ? QStringLiteral("latest") : version;
        emit selfUpdateAvailable(ver, QString());
        setStatus(tr("AviQtl update available. Restart to apply."));
        return;
    }

    const QVariantMap package = RustCore::Package::find(m_packageList, packageId, sourceRepo);
    if (package.isEmpty()) {
        m_pendingInstall.clear();
        emit errorOccurred(tr("Package not found: %1").arg(packageId));
        return;
    }
    const QString packageType = package.value(QStringLiteral("type")).toString();
    if (!Internal::PackageDeployment::isValidPackageType(packageType)) {
        m_pendingInstall.clear();
        emit errorOccurred(tr("Invalid package ID or type."));
        return;
    }

    setBusy(true);
    setProgress(0.0);
    m_pendingInstall = {{QStringLiteral("id"), packageId}, {QStringLiteral("sourceRepo"), sourceRepo}, {QStringLiteral("version"), version}};

    fetchPackageMetadataForInstall(packageId, sourceRepo, version);
}

void PackageManager::downloadPackage(const QString &packageId, const QUrl &url, const QString &expectedSha256, const QString &packageType, const QString &version, const QString &sourceRepo) {
    if (!Internal::PackageDeployment::isValidPackageId(packageId) || !Internal::PackageDeployment::isValidPackageType(packageType)) {
        m_pendingInstall.clear();
        setBusy(false);
        emit errorOccurred(tr("Invalid package ID or type."));
        if (!m_upgradeQueue.isEmpty())
            processUpgradeQueue();
        return;
    }
    if (!Internal::isSecureNetworkUrl(url)) {
        setBusy(false);
        emit errorOccurred(tr("Invalid or insecure package download URL."));
        if (!m_upgradeQueue.isEmpty())
            processUpgradeQueue();
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
        if (fileName.isEmpty())
            fileName = QStringLiteral("package.zip");
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
        if (!m_upgradeQueue.isEmpty())
            processUpgradeQueue();
        return;
    }

    if (!Internal::PackageDeployment::extractArchive(archivePath, extractDir.path())) {
        setBusy(false);
        emit errorOccurred(tr("Failed to extract package archive."));
        if (!m_upgradeQueue.isEmpty())
            processUpgradeQueue();
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
    const Internal::PackageDeployment::FileOperationResult deployResult =
        Internal::PackageDeployment::deployFiles(packageId, extractDir.path(), packageType, [installed]() { return writeJsonAtomically(getInstalledPackagesPath(), QJsonDocument::fromVariant(installed)); });
    if (deployResult != Internal::PackageDeployment::FileOperationResult::Success) {
        qWarning() << "[PackageManager] Failed to deploy package or atomically save installation state.";
        setBusy(false);
        if (deployResult == Internal::PackageDeployment::FileOperationResult::RollbackFailed)
            emit errorOccurred(tr("Package deployment failed and automatic rollback was incomplete; the backup was preserved."));
        else
            emit errorOccurred(tr("Failed to deploy package; the previous installation was restored."));
        if (!m_upgradeQueue.isEmpty())
            processUpgradeQueue();
        return;
    }

    // Compile shaders and reload the registry for effect/object packages.
    if (packageType == QStringLiteral("effect") || packageType == QStringLiteral("object")) {
        const QString deployDir = Internal::PackageDeployment::deployDirectory(packageType);
        const QString packageDir = deployDir + QStringLiteral("/") + packageId;
        compileShadersInDirectory(packageDir);
        EffectRegistry::instance().loadEffectsFromDirectory(deployDir);
    } else if (packageType == QStringLiteral("mod")) {
        // Mods will be loaded on next app restart
    }

    setProgress(1.0);
    setStatus(tr("Installation complete: %1").arg(packageId));
    setBusy(false);

    m_packageList = RustCore::Package::setInstalled(m_packageList, packageId, version);
    emit packageListChanged();

    emit packageInstalled(packageId);
    updateUpdateState();

    // Continue upgrade queue
    if (!m_upgradeQueue.isEmpty())
        processUpgradeQueue();
}

void PackageManager::removePackage(const QString &packageId) {
    if (m_isBusy || packageId == QStringLiteral("org.aviqtl.app"))
        return;
    if (!Internal::PackageDeployment::isValidPackageId(packageId)) {
        emit errorOccurred(tr("Invalid package ID."));
        return;
    }
    const QVariantMap currentInstalled = loadInstalledPackagesFromFile();
    const QVariantMap installedPackage = currentInstalled.value(packageId).toMap();
    const QString packageType = installedPackage.value(QStringLiteral("type")).toString();
    if (!currentInstalled.contains(packageId) || !Internal::PackageDeployment::isValidPackageType(packageType)) {
        emit errorOccurred(tr("Cannot remove package because its installed type is missing or invalid."));
        return;
    }
    setBusy(true);
    setStatus(tr("Removing package: %1").arg(packageId));
    QVariantMap updatedInstalled = currentInstalled;
    updatedInstalled.remove(packageId);
    const Internal::PackageDeployment::FileOperationResult removalResult =
        Internal::PackageDeployment::removeFiles(packageId, packageType, [updatedInstalled]() { return writeJsonAtomically(getInstalledPackagesPath(), QJsonDocument::fromVariant(updatedInstalled)); });
    if (removalResult != Internal::PackageDeployment::FileOperationResult::Success) {
        setBusy(false);
        if (removalResult == Internal::PackageDeployment::FileOperationResult::RollbackFailed)
            emit errorOccurred(tr("Package removal failed and automatic rollback was incomplete; the backup was preserved."));
        else
            emit errorOccurred(tr("Failed to remove package; the installed state and files were restored."));
        return;
    }
    m_packageList = RustCore::Package::setInstalled(m_packageList, packageId, std::nullopt);
    emit packageListChanged();
    updateUpdateState();
    if (packageType == QStringLiteral("effect") || packageType == QStringLiteral("object")) {
        const QString deployDir = Internal::PackageDeployment::deployDirectory(packageType);
        EffectRegistry &registry = EffectRegistry::instance();
        registry.removeEffectsFromDirectory(QDir(deployDir).filePath(packageId));
        registry.loadEffectsFromDirectory(deployDir);
    }
    setBusy(false);
    setStatus(tr("Removal complete: %1").arg(packageId));
    emit packageRemoved(packageId);
}

QVariantList PackageManager::getPackagesByType(const QString &type) const {
    QVariantList result = RustCore::Package::filter(m_packageList, type);
    if (type == QStringLiteral("installed") || type == QStringLiteral("mod")) {
        const QDir pluginsDir(QDir(QCoreApplication::applicationDirPath()).filePath(QStringLiteral("plugins")));
        QStringList installedModDirectories;
        const QVariantMap installedPackages = loadInstalledPackagesFromFile();
        for (auto it = installedPackages.cbegin(); it != installedPackages.cend(); ++it) {
            if (it.value().toMap().value(QStringLiteral("type")).toString() != QStringLiteral("mod") || !Internal::PackageDeployment::isValidPackageId(it.key())) {
                continue;
            }
            const QString packageDirectory = pluginsDir.filePath(it.key());
            if (QFileInfo(packageDirectory).isDir()) {
                installedModDirectories.append(packageDirectory);
            }
        }
        const QFileInfoList filePlugins = pluginsDir.entryInfoList({QStringLiteral("*.lua")}, QDir::Files, QDir::Name);
        for (const QFileInfo &fileInfo : filePlugins) {
            const bool providedByInstalledPackage =
                std::any_of(installedModDirectories.cbegin(), installedModDirectories.cend(), [&fileInfo](const QString &packageDirectory) { return isPathWithinDirectory(fileInfo.absoluteFilePath(), packageDirectory); });
            if (providedByInstalledPackage)
                continue;
            const QString pluginId = QStringLiteral("file:%1").arg(fileInfo.fileName());
            const bool alreadyPresent = std::any_of(result.cbegin(), result.cend(), [&pluginId](const QVariant &entry) { return entry.toMap().value(QStringLiteral("id")).toString() == pluginId; });
            if (alreadyPresent)
                continue;
            result.append(QVariantMap{
                {QStringLiteral("id"), pluginId},
                {QStringLiteral("type"), QStringLiteral("mod")},
                {QStringLiteral("display_name"), fileInfo.completeBaseName()},
                {QStringLiteral("version"), QStringLiteral("file")},
                {QStringLiteral("installed_version"), QStringLiteral("file")},
                {QStringLiteral("local_file_plugin"), true},
            });
        }
    }
    return result;
}

void PackageManager::upgradeAllPackages() {
    if (m_isBusy)
        return;
    m_upgradeQueue.clear();
    for (const QString &packageId : RustCore::Package::upgradeIds(m_packageList))
        m_upgradeQueue.enqueue(packageId);
    if (m_upgradeQueue.isEmpty()) {
        setStatus(tr("No packages to upgrade."));
        return;
    }
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
    m_pendingInstall = {{QStringLiteral("id"), nextPackageId}, {QStringLiteral("sourceRepo"), QString()}, {QStringLiteral("version"), QString()}};
    fetchPackageMetadataForInstall(nextPackageId, QString(), QString());
}

void PackageManager::compileShadersInDirectory(const QString &directory) {
    const QStringList shaderExtensions = {QStringLiteral("*.frag"), QStringLiteral("*.comp"), QStringLiteral("*.vert")};
    QDirIterator it(directory, shaderExtensions, QDir::Files, QDirIterator::Subdirectories);
    int compiled = 0, skipped = 0, failed = 0;
    while (it.hasNext()) {
        const QString sourcePath = it.next();
        const QString qsbPath = sourcePath + QStringLiteral(".qsb");
        if (!ShaderCompiler::needsRecompile(sourcePath, qsbPath)) {
            skipped++;
            continue;
        }
        QString error;
        if (ShaderCompiler::compileToFile(sourcePath, qsbPath, &error))
            compiled++;
        else {
            qWarning().noquote() << "[PackageManager] Shader compilation failed:" << sourcePath << ":" << error;
            failed++;
        }
    }
    if (compiled > 0 || failed > 0)
        qDebug().noquote() << "[PackageManager] Shaders compiled:" << compiled << "skipped:" << skipped << "failed:" << failed;
}

} // namespace AviQtl::Core
