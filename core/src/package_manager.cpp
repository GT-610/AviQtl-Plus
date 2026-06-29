#include "package_manager.hpp"
#include "effect_registry.hpp"
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
#include <QNetworkAccessManager>
#include <QNetworkReply>
#include <QProcess>
#include <QTemporaryDir>
#include <QTimer>
#include <QXmlStreamReader>

namespace AviQtl::Core {

namespace {
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

// Future GPG signature verification placeholder
// When package signing is implemented, resolveSignature() will validate
// against a trusted keyring. Currently unused; reserved for future use.
// static bool verifySignature(const QByteArray & /* data */,
//                              const QByteArray & /* signature */,
//                              const QString & /* keyId */) { return true; }

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
    if (url.isEmpty())
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
        QUrl baseUrl(repoUrl);
        QString basePath = repoUrl;
        if (basePath.endsWith(QStringLiteral("/repo.json")))
            basePath.chop(9);
        else if (basePath.endsWith(QStringLiteral(".json"))) {
            int slash = basePath.lastIndexOf('/');
            if (slash != -1)
                basePath = basePath.left(slash);
        }

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

        QNetworkReply *reply = m_networkManager->get(QNetworkRequest(fetchUrl));
        connect(reply, &QNetworkReply::finished, this, [this, reply, fetchUrl, repoUrl, basePath, ctx, installed]() {
            reply->deleteLater();
            m_pendingRequests--;

            if (reply->error() == QNetworkReply::NoError) {
                QJsonDocument doc = QJsonDocument::fromJson(reply->readAll());
                if (doc.isObject()) {
                    QJsonObject repoObj = doc.object();
                    ctx->repoInfo[QStringLiteral("name")] = repoObj.value(QStringLiteral("repo_name")).toString();
                    QString catalogUrl = repoObj.value(QStringLiteral("catalog_url")).toString();
                    if (!catalogUrl.isEmpty()) {
                        QUrl absUrl;
                        if (catalogUrl.startsWith(QStringLiteral("http://")) || catalogUrl.startsWith(QStringLiteral("https://")))
                            absUrl = QUrl(catalogUrl);
                        else
                            absUrl = QUrl(basePath + QStringLiteral("/") + catalogUrl);

                        QNetworkReply *catReply = m_networkManager->get(QNetworkRequest(absUrl));
                        m_pendingRequests++;
                        connect(catReply, &QNetworkReply::finished, this, [this, catReply, ctx, absUrl, installed]() {
                            catReply->deleteLater();
                            m_pendingRequests--;
                            if (catReply->error() == QNetworkReply::NoError) {
                                ctx->catalogData = catReply->readAll();
                                // Cache catalog
                                QString cacheName = QStringLiteral("catalog_") +
                                    QString::fromLatin1(QCryptographicHash::hash(
                                        ctx->repoInfo.value(QStringLiteral("url")).toString().toUtf8(),
                                        QCryptographicHash::Sha256).toHex()) +
                                    QStringLiteral(".json");
                                QFile cf(getReposCachePath() + QStringLiteral("/") + cacheName);
                                if (cf.open(QIODevice::WriteOnly)) {
                                    cf.write(ctx->catalogData);
                                    cf.close();
                                }
                            }
                            onCatalogFetched(ctx->repoInfo, ctx->catalogData, installed);
                        });
                    } else {
                        // Old format: treat repo.json itself as a flat packages list
                        onCatalogFetched(ctx->repoInfo, reply->readAll(), installed);
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

        // Take highest version as latest
        QString existingLatest = existing.value(QStringLiteral("latest_version")).toString();
        QString newVersion = pkg.value(QStringLiteral("version")).toString();
        if (compareVersions(newVersion, existingLatest) > 0) {
            existing[QStringLiteral("latest_version")] = newVersion;
            existing[QStringLiteral("version")] = newVersion;
            existing[QStringLiteral("_primary_repo")] = repoUrl;
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

void PackageManager::fetchPackageMetadata(const QString &packageId, const QString &sourceRepo) {
    Q_UNUSED(sourceRepo)
    if (m_packageDetails.contains(packageId)) {
        emit packageDetailReady(packageId, m_packageDetails[packageId]);
        return;
    }

    // Find catalog entry to get metadata_url
    QVariantMap catalogEntry;
    for (const auto &p : std::as_const(m_packageList)) {
        if (p.toMap().value(QStringLiteral("id")).toString() == packageId) {
            catalogEntry = p.toMap();
            break;
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

    setStatus(tr("Fetching package details: %1").arg(packageId));
    QUrl url(metadataUrl);
    QNetworkReply *reply = m_networkManager->get(QNetworkRequest(url));
    connect(reply, &QNetworkReply::finished, this, [this, reply, packageId, metadataUrl]() {
        reply->deleteLater();
        if (reply->error() != QNetworkReply::NoError) {
            emit errorOccurred(tr("Failed to fetch package metadata (%1): %2").arg(packageId, reply->errorString()));
            return;
        }

        QJsonDocument doc = QJsonDocument::fromJson(reply->readAll());
        if (!doc.isObject()) {
            emit errorOccurred(tr("Invalid metadata format for package: %1").arg(packageId));
            return;
        }

        QVariantMap detail = doc.object().toVariantMap();
        m_packageDetails[packageId] = detail;

        // Cache to repos directory
        QString cacheName = QStringLiteral("detail_") +
            QString::fromLatin1(QCryptographicHash::hash(metadataUrl.toUtf8(), QCryptographicHash::Sha256).toHex()) +
            QStringLiteral(".json");
        QFile cf(getReposCachePath() + QStringLiteral("/") + cacheName);
        if (cf.open(QIODevice::WriteOnly)) {
            cf.write(reply->readAll());
            cf.close();
        }

        emit packageDetailReady(packageId, detail);
    });
}

void PackageManager::fetchPackageMetadataForInstall(const QString &packageId, const QString &sourceRepo, const QString &version) {
    m_pendingInstall = {
        {QStringLiteral("id"), packageId},
        {QStringLiteral("sourceRepo"), sourceRepo},
        {QStringLiteral("version"), version}
    };

    if (m_packageDetails.contains(packageId)) {
        continueInstallWithMetadata(packageId, sourceRepo, version, m_packageDetails[packageId]);
        return;
    }

    QMetaObject::Connection *conn = new QMetaObject::Connection();
    *conn = connect(this, &PackageManager::packageDetailReady, this,
        [this, conn, packageId, sourceRepo, version](const QString &readyId, const QVariantMap &detail) {
            if (readyId == packageId) {
                disconnect(*conn);
                delete conn;
                if (m_pendingInstall.value(QStringLiteral("id")).toString() == packageId)
                    continueInstallWithMetadata(packageId, sourceRepo, version, detail);
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

void PackageManager::fetchAssets(const QString &packageId) {
    // Legacy method: hit GitHub/Codeberg API to list release assets.
    // Kept for backward compatibility until QML is updated to use the new flow.
    QVariantMap pkg;
    for (const auto &p : std::as_const(m_packageList)) {
        if (p.toMap().value(QStringLiteral("id")).toString() == packageId) {
            pkg = p.toMap();
            break;
        }
    }
    if (pkg.isEmpty()) {
        emit errorOccurred(tr("Package not found: %1").arg(packageId));
        return;
    }

    QString repoUrl = pkg.value(QStringLiteral("repository_url")).toString();
    if (repoUrl.isEmpty()) {
        QString feed = pkg.value(QStringLiteral("release_feed")).toString();
        if (!feed.isEmpty()) {
            int idx = feed.indexOf(QStringLiteral("/releases"));
            if (idx != -1)
                repoUrl = feed.left(idx);
        }
    }
    if (repoUrl.isEmpty()) {
        emit errorOccurred(tr("Could not determine repository URL for the package."));
        return;
    }

    setBusy(true);
    setStatus(tr("Searching for available files..."));

    QUrl apiUrl;
    bool isGitHub = repoUrl.contains(QStringLiteral("github.com"));
    bool isCodeberg = repoUrl.contains(QStringLiteral("codeberg.org"));
    QString path = QUrl(repoUrl).path();
    if (path.startsWith('/')) path.remove(0, 1);
    QStringList parts = path.split('/');
    if (parts.size() < 2) { setBusy(false); emit errorOccurred(tr("Repository URL is not in a valid format.")); return; }
    QString owner = parts[0], repo = parts[1];
    if (repo.endsWith(".git")) repo.chop(4);

    if (isGitHub)
        apiUrl = QStringLiteral("https://api.github.com/repos/%1/%2/releases/latest").arg(owner, repo);
    else if (isCodeberg)
        apiUrl = QStringLiteral("https://codeberg.org/api/v1/repos/%1/%2/releases?limit=1").arg(owner, repo);
    else { setBusy(false); emit errorOccurred(tr("Unsupported repository host.")); return; }

    QNetworkReply *reply = m_networkManager->get(QNetworkRequest(apiUrl));
    connect(reply, &QNetworkReply::finished, this, [this, reply, packageId]() {
        reply->deleteLater();
        setBusy(false);
        if (reply->error() != QNetworkReply::NoError) {
            emit errorOccurred(tr("Failed to fetch release info (%1): %2").arg(packageId, reply->errorString()));
            return;
        }
        QJsonDocument doc = QJsonDocument::fromJson(reply->readAll());
        QVariantList assetsList;
        QJsonObject releaseObj;
        if (doc.isArray() && !doc.array().isEmpty())
            releaseObj = doc.array().at(0).toObject();
        else if (doc.isObject())
            releaseObj = doc.object();

        if (!releaseObj.isEmpty()) {
            QString body = releaseObj.value(QStringLiteral("body")).toString();
            QString author;
            QJsonObject authorObj = releaseObj.value(QStringLiteral("author")).toObject();
            author = authorObj.value(QStringLiteral("login")).toString();
            if (author.isEmpty()) author = authorObj.value(QStringLiteral("username")).toString();
            for (int i = 0; i < m_packageList.size(); ++i) {
                QVariantMap item = m_packageList[i].toMap();
                if (item.value(QStringLiteral("id")).toString() == packageId) {
                    if (!author.isEmpty()) item[QStringLiteral("author")] = author;
                    if (!body.isEmpty()) item[QStringLiteral("description")] = body.left(200).trimmed() + (body.size() > 200 ? QStringLiteral("...") : QString());
                    m_packageList[i] = item;
                    emit packageListChanged();
                    break;
                }
            }
            QJsonArray assetsArr = releaseObj.value(QStringLiteral("assets")).toArray();
            for (const auto &aVal : assetsArr) {
                QJsonObject aObj = aVal.toObject();
                QVariantMap asset;
                asset[QStringLiteral("name")] = aObj.value(QStringLiteral("name")).toString();
                asset[QStringLiteral("size")] = aObj.value(QStringLiteral("size")).toVariant();
                asset[QStringLiteral("url")] = aObj.value(QStringLiteral("browser_download_url")).toString();
                assetsList.append(asset);
            }
        }
        if (assetsList.isEmpty())
            emit errorOccurred(tr("No downloadable files found."));
        else
            emit assetsReady(packageId, assetsList);
    });
}

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
    setStatus(tr("Downloading package: %1").arg(packageId));
    setProgress(0.0);

    QNetworkReply *reply = m_networkManager->get(QNetworkRequest(url));
    connect(reply, &QNetworkReply::downloadProgress, this, [this](qint64 received, qint64 total) {
        if (total > 0)
            setProgress(static_cast<double>(received) / total * 0.5);
    });
    connect(reply, &QNetworkReply::finished, this, [this, reply, packageId, url, expectedSha256, packageType, version, sourceRepo]() {
        reply->deleteLater();
        if (reply->error() != QNetworkReply::NoError) {
            setBusy(false);
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
        file.write(data);
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

    if (!extractZip(archivePath, extractDir.path())) {
        setBusy(false);
        emit errorOccurred(tr("Failed to extract package archive."));
        return;
    }

    setStatus(tr("Deploying package files..."));
    setProgress(0.8);

    if (!deployPackageFiles(packageId, extractDir.path(), packageType)) {
        setBusy(false);
        emit errorOccurred(tr("Failed to deploy package files."));
        return;
    }

    // Compile shaders for effect/object/transition packages
    if (packageType == QStringLiteral("effect") || packageType == QStringLiteral("object") || packageType == QStringLiteral("transition")) {
        const QString deployDir = getPackageDeployDir(packageType);
        const QString packageDir = deployDir + QStringLiteral("/") + packageId;
        compileShadersInDirectory(packageDir);
    }

    // Save installed info
    QVariantMap installed = loadInstalledPackagesFromFile();
    QVariantMap info;
    info[QStringLiteral("version")] = version;
    info[QStringLiteral("type")] = packageType;
    info[QStringLiteral("installed_at")] = QDateTime::currentDateTimeUtc().toString(Qt::ISODate);
    info[QStringLiteral("installed_from_repo")] = sourceRepo;
    info[QStringLiteral("installed_from_url")] = downloadUrl;
    info[QStringLiteral("sha256")] = QString::fromLatin1(QCryptographicHash::hash(
        QFile(archivePath).exists() ? QFile(archivePath).readAll() : QByteArray(), QCryptographicHash::Sha256).toHex());
    installed[packageId] = info;

    QFile installedFile(getInstalledPackagesPath());
    if (installedFile.open(QIODevice::WriteOnly)) {
        installedFile.write(QJsonDocument::fromVariant(installed).toJson(QJsonDocument::Indented));
        installedFile.close();
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

bool PackageManager::extractZip(const QString &archivePath, const QString &destDir) {
    QProcess process;
#ifdef Q_OS_WIN
    QString escapedPath = archivePath;
    escapedPath.replace(QLatin1Char('\''), QStringLiteral("''"));
    QString escapedDest = destDir;
    escapedDest.replace(QLatin1Char('\''), QStringLiteral("''"));
    process.start(QStringLiteral("powershell"), {
        QStringLiteral("-NoProfile"), QStringLiteral("-NonInteractive"),
        QStringLiteral("-Command"),
        QStringLiteral("Expand-Archive -Path '%1' -DestinationPath '%2' -Force").arg(escapedPath, escapedDest)
    });
#else
    process.start(QStringLiteral("unzip"), {QStringLiteral("-o"), archivePath, QStringLiteral("-d"), destDir});
#endif
    process.waitForFinished(30000);
    if (process.exitStatus() != QProcess::NormalExit || process.exitCode() != 0) {
        qWarning() << "[PackageManager] Extraction failed:" << process.errorString();
        return false;
    }
    // Zip Slip detection
    const QDir destDirObj(destDir);
    const QString canonicalDest = destDirObj.canonicalPath();
    QDirIterator it(destDirObj.path(), QDir::Files | QDir::Dirs | QDir::NoDotAndDotDot, QDirIterator::Subdirectories);
    while (it.hasNext()) {
        it.next();
        const QString canonicalPath = QFileInfo(it.filePath()).canonicalFilePath();
        if (canonicalPath.isEmpty()) { qWarning() << "[PackageManager] Path disappeared:" << it.filePath(); return false; }
        if (!canonicalPath.startsWith(canonicalDest + QLatin1Char('/'))) {
            qWarning() << "[PackageManager] Zip Slip detected:" << it.filePath();
            if (it.fileInfo().isDir()) QDir(it.filePath()).removeRecursively();
            else QFile::remove(it.filePath());
            return false;
        }
    }
    return true;
}

bool PackageManager::deployPackageFiles(const QString &packageId, const QString &extractDir, const QString &packageType) {
    Q_UNUSED(packageType)
    if (packageId.contains(QStringLiteral("..")) || packageId.contains('/') || packageId.contains('\\')) {
        qWarning() << "[PackageManager] Invalid package ID (path traversal):" << packageId;
        return false;
    }
    const QString deployBase = getPackageDeployDir(packageType);
    if (deployBase.isEmpty()) return false;
    const QString packageDir = deployBase + QStringLiteral("/") + packageId;
    QDir().mkpath(packageDir);

    QDir sourceDir(extractDir);
    QStringList entries = sourceDir.entryList(QDir::Files | QDir::Dirs | QDir::NoDotAndDotDot);
    if (entries.size() == 1) {
        QFileInfo fi(sourceDir.absoluteFilePath(entries.first()));
        if (fi.isDir()) { sourceDir.cd(entries.first()); entries = sourceDir.entryList(QDir::Files | QDir::Dirs | QDir::NoDotAndDotDot); }
    }
    for (const QString &entry : std::as_const(entries)) {
        const QString srcPath = sourceDir.absoluteFilePath(entry);
        const QString destPath = packageDir + QStringLiteral("/") + entry;
        QFileInfo srcInfo(srcPath);
        if (srcInfo.isDir()) { if (!copyDirectory(srcPath, destPath)) return false; }
        else { if (QFile::exists(destPath)) QFile::remove(destPath); if (!QFile::copy(srcPath, destPath)) return false; }
    }
    return true;
}

QString PackageManager::getPackageDeployDir(const QString &packageType) const {
    const QString appDir = QCoreApplication::applicationDirPath();
    if (packageType == QStringLiteral("mod")) return appDir + QStringLiteral("/plugins");
    if (packageType == QStringLiteral("effect")) return appDir + QStringLiteral("/effects");
    if (packageType == QStringLiteral("object")) return appDir + QStringLiteral("/objects");
    if (packageType == QStringLiteral("transition")) return appDir + QStringLiteral("/transitions");
    return appDir + QStringLiteral("/plugins");
}

void PackageManager::removePackage(const QString &packageId) {
    if (m_isBusy || packageId == QStringLiteral("org.aviqtl.app")) return;
    QVariantMap pkg;
    for (const auto &p : std::as_const(m_packageList)) {
        if (p.toMap().value(QStringLiteral("id")).toString() == packageId) { pkg = p.toMap(); break; }
    }
    if (packageId.contains(QStringLiteral("..")) || packageId.contains('/') || packageId.contains('\\')) {
        emit errorOccurred(tr("Invalid package ID.")); return;
    }
    setBusy(true);
    setStatus(tr("Removing package: %1").arg(packageId));
    const QString packageType = pkg.value(QStringLiteral("type")).toString();
    const QString deployDir = getPackageDeployDir(packageType);
    const QString packageDir = deployDir + QStringLiteral("/") + packageId;
    if (QDir(packageDir).exists()) QDir(packageDir).removeRecursively();

    QVariantMap installed = loadInstalledPackagesFromFile();
    if (installed.remove(packageId)) {
        QFile file(getInstalledPackagesPath());
        if (file.open(QIODevice::WriteOnly)) {
            file.write(QJsonDocument::fromVariant(installed).toJson());
            file.close();
        }
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

QVariantList PackageManager::searchPackages(const QString &query) const {
    if (query.isEmpty()) return m_packageList;
    QVariantList filtered;
    for (const auto &p : m_packageList) {
        QVariantMap map = p.toMap();
        if (map.value(QStringLiteral("display_name")).toString().contains(query, Qt::CaseInsensitive) ||
            map.value(QStringLiteral("id")).toString().contains(query, Qt::CaseInsensitive))
            filtered.append(p);
    }
    return filtered;
}

QVariantList PackageManager::getInstalledPackages() const {
    QVariantList list;
    QVariantMap installed = loadInstalledPackagesFromFile();
    installed.insert(QStringLiteral("org.aviqtl.app"), QVariantMap{{QStringLiteral("version"), appVersionString()}});
    for (auto it = installed.begin(); it != installed.end(); ++it) {
        QVariantMap pkg;
        pkg.insert(QStringLiteral("id"), it.key());
        pkg.insert(QStringLiteral("version"), it.value().toMap().value(QStringLiteral("version")));
        list.append(pkg);
    }
    return list;
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
    installPackage(nextPackageId);
}

void PackageManager::updatePackageLatestVersion(const QString &id, const QString &version) {
    // Legacy helper for old-style RSS feed-based version detection
    for (int i = 0; i < m_packageList.size(); ++i) {
        QVariantMap item = m_packageList[i].toMap();
        if (item.value(QStringLiteral("id")).toString() == id) {
            QString latest = version;
            if (latest.startsWith('v')) latest.remove(0, 1);
            if (id == QStringLiteral("org.aviqtl.app") && compareVersions(latest, appVersionString()) <= 0)
                latest = appVersionString();
            if (item.value(QStringLiteral("latest_version")).toString() != latest) {
                item[QStringLiteral("latest_version")] = latest;
                m_packageList[i] = item;
                emit packageListChanged();
            }
            break;
        }
    }
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
