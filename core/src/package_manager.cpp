#include "package_manager.hpp"
#include "effect_registry.hpp"
#include "settings_manager.hpp"
#include "version.hpp" // AviQtl本体のバージョン情報にアクセスするため
#include <QCoreApplication>
#include <QCryptographicHash>
#include <QDir>
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

bool copyDirectory(const QString &srcPath, const QString &destPath) {
    QDir srcDir(srcPath);
    if (!srcDir.exists()) {
        return false;
    }

    QDir destDir(destPath);
    if (!destDir.exists()) {
        QDir().mkpath(destPath);
    }

    const QStringList entries = srcDir.entryList(QDir::Files | QDir::Dirs | QDir::NoDotAndDotDot);
    for (const QString &entry : entries) {
        const QString srcFilePath = srcDir.absoluteFilePath(entry);
        const QString destFilePath = destPath + QStringLiteral("/") + entry;
        QFileInfo srcInfo(srcFilePath);

        if (srcInfo.isDir()) {
            if (!copyDirectory(srcFilePath, destFilePath)) {
                return false;
            }
        } else {
            if (QFile::exists(destFilePath)) {
                QFile::remove(destFilePath);
            }
            if (!QFile::copy(srcFilePath, destFilePath)) {
                return false;
            }
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

        if (ok1 && ok2) { // 両方とも数値の場合
            if (num1 < num2)
                return -1;
            if (num1 > num2)
                return 1;
        } else { // 少なくとも一方が純粋な数値ではない場合 (例: "84a")
            // 文字列として比較。プレリリース版タグの場合、辞書順比較が望ましい挙動になることが多い
            if (parts1[i] < parts2[i])
                return -1;
            if (parts1[i] > parts2[i])
                return 1;
        }
        i++;
    }

    // 片方のバージョンがより多くの部分を持つ場合、通常は新しいと見なす (例: 1.0.0 vs 1.0)
    if (parts1.size() > parts2.size())
        return 1;
    if (parts1.size() < parts2.size())
        return -1;

    return 0; // v1 != v2 の場合はここに到達しないはず
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

QString parseLatestVersionFromXml(const QByteArray &data) {
    QString latest;
    QXmlStreamReader xml(data);
    while (!xml.atEnd()) {
        if (xml.isStartElement() && (xml.name() == QStringLiteral("item") || xml.name() == QStringLiteral("entry"))) {
            while (xml.readNextStartElement()) {
                if (xml.name() == QStringLiteral("title")) {
                    latest = xml.readElementText().trimmed();
                } else {
                    xml.skipCurrentElement();
                }
                if (!latest.isEmpty())
                    break;
            }
            break;
        }
        xml.readNext();
    }
    return latest;
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
    const QString repoPath = QCoreApplication::applicationDirPath() + QStringLiteral("/repos");
    QDir dir(repoPath);
    if (!dir.exists())
        return;

    m_packageList.clear();
    QVariantMap installed = loadInstalledPackagesFromFile();

    // 保存されているすべてのリポジトリJSONを読み込む
    const QStringList files = dir.entryList({QStringLiteral("*.json")}, QDir::Files);
    for (const QString &fileName : files) {
        if (fileName == QStringLiteral("installed.json"))
            continue;

        QFile file(dir.absoluteFilePath(fileName));
        if (!file.open(QIODevice::ReadOnly))
            continue;

        QJsonDocument doc = QJsonDocument::fromJson(file.readAll());
        if (doc.isObject()) {
            QJsonArray packages = doc.object().value(QStringLiteral("packages")).toArray();
            for (const auto &pVal : packages) {
                QVariantMap p = pVal.toObject().toVariantMap();
                const QString id = p.value(QStringLiteral("id")).toString();

                // インストール済み情報の付加
                if (id == QStringLiteral("org.aviqtl.app")) {
                    p[QStringLiteral("installed_version")] = appVersionString();
                    p[QStringLiteral("latest_version")] = appVersionString();
                } else if (installed.contains(id)) {
                    p[QStringLiteral("installed_version")] = installed.value(id).toMap().value(QStringLiteral("version"));
                }

                // 起動直後は最新バージョンは不明（同期ボタンで初めて取得される）
                // ただし、キャッシュされたフィードがあれば読み込む
                if (id != QStringLiteral("org.aviqtl.app")) {
                    const QString feedUrl = p.value(QStringLiteral("release_feed")).toString();
                    if (!feedUrl.isEmpty()) {
                    const QString feedFileName = QStringLiteral("feed_") + QString::fromLatin1(QCryptographicHash::hash(feedUrl.toUtf8(), QCryptographicHash::Sha256).toHex()) + QStringLiteral(".xml");
                        QFile feedFile(repoPath + QStringLiteral("/") + feedFileName);
                        if (feedFile.open(QIODevice::ReadOnly)) {
                            p[QStringLiteral("latest_version")] = parseLatestVersionFromXml(feedFile.readAll());
                            feedFile.close();
                        }
                    }
                }

                m_packageList.append(p);
            }
        }
    }
    emit packageListChanged();

    // キャッシュ情報に基づいてアップデートの有無を初期評価
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

    setStatus(tr("Packages loaded from cache (Press Sync to check for updates)"));
}

QStringList PackageManager::repositories() const { return SettingsManager::instance().value(QStringLiteral("packageRepositoryUrls")).toStringList(); }

void PackageManager::addRepository(const QString &url) {
    QStringList repos = repositories();
    if (!url.isEmpty() && !repos.contains(url)) {
        repos.append(url);
        SettingsManager::instance().setValue(QStringLiteral("packageRepositoryUrls"), repos);
        emit repositoriesChanged();
    }
}

void PackageManager::removeRepository(const QString &url) {
    QStringList repos = repositories();
    if (repos.removeOne(url)) {
        SettingsManager::instance().setValue(QStringLiteral("packageRepositoryUrls"), repos);
        emit repositoriesChanged();
    }
}

void PackageManager::refreshRepositories() {
    if (m_isBusy)
        return;
    setBusy(true);
    m_packageList.clear();
    emit packageListChanged();

    QVariantMap installed = loadInstalledPackagesFromFile();
    installed.insert(QStringLiteral("org.aviqtl.app"), QVariantMap{{QStringLiteral("version"), appVersionString()}});

    setStatus(tr("Syncing repository..."));
    setProgress(0.0);

    QStringList urls = repositories();
    if (urls.isEmpty()) {
        setBusy(false);
        return;
    }

    m_pendingRequests = urls.size();
    for (const QString &url : urls) {
        QNetworkReply *reply = m_networkManager->get(QNetworkRequest(QUrl(url)));
        connect(reply, &QNetworkReply::finished, this, [this, reply, url, installed]() {
            reply->deleteLater();
            m_pendingRequests--;

            if (reply->error() == QNetworkReply::NoError) {
                const QByteArray data = reply->readAll();

                // JSONをreposディレクトリにキャッシュ保存
                const QString repoPath = QCoreApplication::applicationDirPath() + QStringLiteral("/repos");
                QDir().mkpath(repoPath);

                const QString fileName = QString::fromLatin1(QCryptographicHash::hash(url.toUtf8(), QCryptographicHash::Sha256).toHex()) + QStringLiteral(".json");
                QFile file(repoPath + QStringLiteral("/") + fileName);
                if (file.open(QIODevice::WriteOnly)) {
                    file.write(data);
                    file.close();
                }

                QJsonDocument doc = QJsonDocument::fromJson(data);
                if (doc.isObject()) {
                    QJsonArray packages = doc.object().value("packages").toArray();
                    for (const auto &pVal : packages) {
                        QVariantMap p = pVal.toObject().toVariantMap();
                        const QString id = p.value("id").toString();

                        // ローカルのインストール済み情報をチェック
                        if (id == QStringLiteral("org.aviqtl.app")) {
                            // AviQtl本体の場合、常に現在の実行バージョンをインストール済みとする
                            p["installed_version"] = appVersionString();
                        } else if (installed.contains(id)) {
                            // その他のパッケージの場合
                            p["installed_version"] = installed.value(id).toMap().value("version");
                        }

                        // JSON側のバージョンを初期の最新バージョンとしてセット
                        if (p.contains(QStringLiteral("version"))) {
                            QString jsonVer = p.value(QStringLiteral("version")).toString();
                            if (id == QStringLiteral("org.aviqtl.app") && compareVersions(jsonVer, appVersionString()) <= 0) {
                                p[QStringLiteral("latest_version")] = appVersionString();
                            } else {
                                p[QStringLiteral("latest_version")] = jsonVer;
                            }
                        }

                        // バージョン等の変動情報はクライアント側で解決する
                        // JSON側の "version" フィールドは無視し、release_feed を優先する
                        const QString feedUrl = p.value("release_feed").toString();

                        if (!feedUrl.isEmpty()) {
                            QNetworkReply *rssReply = m_networkManager->get(QNetworkRequest(QUrl(feedUrl)));
                            m_pendingRequests++;
                            connect(rssReply, &QNetworkReply::finished, this, [this, rssReply, id, feedUrl]() {
                                rssReply->deleteLater();
                                m_pendingRequests--;

                                if (rssReply->error() == QNetworkReply::NoError) {
                                    const QByteArray rssData = rssReply->readAll();
                                    // フィードをキャッシュ保存
                                    const QString repoPath = QCoreApplication::applicationDirPath() + QStringLiteral("/repos");
                        const QString feedFileName = QStringLiteral("feed_") + QString::fromLatin1(QCryptographicHash::hash(feedUrl.toUtf8(), QCryptographicHash::Sha256).toHex()) + QStringLiteral(".xml");
                                    QFile feedFile(repoPath + QStringLiteral("/") + feedFileName);
                                    if (feedFile.open(QIODevice::WriteOnly)) {
                                        feedFile.write(rssData);
                                        feedFile.close();
                                    }

                                    QString latest = parseLatestVersionFromXml(rssData);
                                    if (!latest.isEmpty()) {
                                        updatePackageLatestVersion(id, latest);
                                    }
                                }

                                if (m_pendingRequests <= 0) {
                                    // すべてのRSSリクエストが完了したら、最終的な状態を更新
                                    setBusy(false);
                                    setProgress(1.0);
                                    setStatus(tr("Sync complete"));
                                    emit repositoryRefreshed();
                                    emit packageListChanged();

                                    // アップデートの有無を再評価
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
                            });
                        }
                        m_packageList.append(p);
                    }
                    // メタデータの解析が終わった時点で一度リストを更新（ユーザーに即座に表示）
                    emit packageListChanged();
                }
            }

            if (m_pendingRequests <= 0) {
                setProgress(1.0);
                setStatus(tr("Sync complete"));
                setBusy(false);
                emit repositoryRefreshed();
                emit packageListChanged();
            } else {
                QStringList urls = repositories();
                if (urls.size() > 0)
                    setProgress(1.0 - (double)m_pendingRequests / urls.size());
            }
        });
    }
}

void PackageManager::updatePackageLatestVersion(const QString &id, const QString &version) {
    for (int i = 0; i < m_packageList.size(); ++i) {
        QVariantMap item = m_packageList[i].toMap();
        if (item.value(QStringLiteral("id")).toString() == id) {
            QString latest = version;

            // 先頭の 'v' を取り除く (v0.0.86 -> 0.0.86)
            if (latest.startsWith('v'))
                latest.remove(0, 1);

            // AviQtl本体の場合、自分自身のバージョン情報と比較して、新しい場合のみ更新する
            if (id == QStringLiteral("org.aviqtl.app") && compareVersions(latest, appVersionString()) <= 0) {
                latest = appVersionString();
            }

            if (item.value(QStringLiteral("latest_version")).toString() != latest || latest.isEmpty()) {
                item[QStringLiteral("latest_version")] = latest;
                m_packageList[i] = item;
                // hasUpdatesAvailable の状態は refreshRepositories の最後でまとめて更新される
                emit packageListChanged();
            }
            break;
        }
    }
}

void PackageManager::fetchAssets(const QString &packageId) {
    if (m_isBusy)
        return;

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

    // フォールバック: release_feed からリポジトリURLを推測 (https://host/owner/repo/...)
    if (repoUrl.isEmpty()) {
        QString feed = pkg.value(QStringLiteral("release_feed")).toString();
        if (!feed.isEmpty()) {
            int idx = feed.indexOf(QStringLiteral("/releases"));
            if (idx != -1) {
                repoUrl = feed.left(idx);
            }
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
    if (path.startsWith('/'))
        path.remove(0, 1);
    QStringList parts = path.split('/');
    if (parts.size() < 2) {
        setBusy(false);
        emit errorOccurred(tr("Repository URL is not in a valid format."));
        return;
    }
    QString owner = parts[0];
    QString repo = parts[1];
    if (repo.endsWith(".git"))
        repo.remove(repo.size() - 4, 4);

    if (isGitHub) {
        apiUrl = QStringLiteral("https://api.github.com/repos/%1/%2/releases/latest").arg(owner, repo);
    } else if (isCodeberg) {
        // Codeberg: リリース一覧の最新1件を取得
        apiUrl = QStringLiteral("https://codeberg.org/api/v1/repos/%1/%2/releases?limit=1").arg(owner, repo);
    } else {
        setBusy(false);
        emit errorOccurred(tr("Unsupported repository host."));
        return;
    }

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
        if (doc.isArray() && !doc.array().isEmpty()) {
            releaseObj = doc.array().at(0).toObject();
        } else if (doc.isObject()) {
            releaseObj = doc.object();
        }

        if (!releaseObj.isEmpty()) {
            // APIから取得した作者情報と説明文でリストを更新
            const QString body = releaseObj.value(QStringLiteral("body")).toString();
            QString author;
            QJsonObject authorObj = releaseObj.value(QStringLiteral("author")).toObject();
            author = authorObj.value(QStringLiteral("login")).toString(); // GitHub
            if (author.isEmpty())
                author = authorObj.value(QStringLiteral("username")).toString(); // Codeberg

            for (int i = 0; i < m_packageList.size(); ++i) {
                QVariantMap item = m_packageList[i].toMap();
                if (item.value(QStringLiteral("id")).toString() == packageId) {
                    if (!author.isEmpty())
                        item[QStringLiteral("author")] = author;
                    if (!body.isEmpty())
                        item[QStringLiteral("description")] = body.left(200).trimmed() + (body.size() > 200 ? QStringLiteral("...") : QStringLiteral(""));

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

        if (assetsList.isEmpty()) {
            emit errorOccurred(tr("No downloadable files found."));
        } else {
            emit assetsReady(packageId, assetsList);
        }
    });
}

void PackageManager::installPackage(const QString &packageId, const QString &assetUrl) {
    if (m_isBusy)
        return;

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

    if (assetUrl.isEmpty()) {
        emit errorOccurred(tr("Download URL not specified. Please fetch asset information."));
        return;
    }

    // Self-update is handled separately
    if (packageId == QStringLiteral("org.aviqtl.app")) {
        const QString versionToInstall = pkg.value(QStringLiteral("latest_version")).toString();
        emit selfUpdateAvailable(versionToInstall, assetUrl);
        setStatus(tr("AviQtl update available. Restart to apply."));
        return;
    }

    downloadPackage(packageId, QUrl(assetUrl));
}

void PackageManager::downloadPackage(const QString &packageId, const QUrl &url) {
    setBusy(true);
    setStatus(tr("Downloading package: %1").arg(packageId));
    setProgress(0.0);

    QNetworkReply *reply = m_networkManager->get(QNetworkRequest(url));
    connect(reply, &QNetworkReply::downloadProgress, this, [this](qint64 received, qint64 total) {
        if (total > 0) {
            setProgress(static_cast<double>(received) / total * 0.5); // 0-50% for download
        }
    });
    connect(reply, &QNetworkReply::finished, this, [this, reply, packageId, url]() {
        reply->deleteLater();

        if (reply->error() != QNetworkReply::NoError) {
            setBusy(false);
            emit errorOccurred(tr("Download failed: %1").arg(reply->errorString()));
            return;
        }

        // Get package type
        QVariantMap pkg;
        for (const auto &p : std::as_const(m_packageList)) {
            if (p.toMap().value(QStringLiteral("id")).toString() == packageId) {
                pkg = p.toMap();
                break;
            }
        }
        const QString packageType = pkg.value(QStringLiteral("type")).toString();

        // Save to temp file
        QTemporaryDir tempDir;
        if (!tempDir.isValid()) {
            setBusy(false);
            emit errorOccurred(tr("Failed to create temporary directory."));
            return;
        }

        const QString fileName = url.fileName();
        if (fileName.isEmpty()) {
            setBusy(false);
            emit errorOccurred(tr("Invalid download URL."));
            return;
        }

        const QString archivePath = tempDir.path() + QStringLiteral("/") + fileName;
        QFile file(archivePath);
        if (!file.open(QIODevice::WriteOnly)) {
            setBusy(false);
            emit errorOccurred(tr("Failed to save downloaded file."));
            return;
        }
        file.write(reply->readAll());
        file.close();

        setStatus(tr("Extracting package..."));
        setProgress(0.6);

        // Extract and deploy
        extractAndDeploy(packageId, archivePath, packageType);
    });
}

void PackageManager::extractAndDeploy(const QString &packageId, const QString &archivePath, const QString &packageType) {
    QTemporaryDir extractDir;
    if (!extractDir.isValid()) {
        setBusy(false);
        emit errorOccurred(tr("Failed to create extraction directory."));
        return;
    }

    // Extract archive
    if (!extractZip(archivePath, extractDir.path())) {
        setBusy(false);
        emit errorOccurred(tr("Failed to extract package archive."));
        return;
    }

    setStatus(tr("Deploying package files..."));
    setProgress(0.8);

    // Deploy files to correct location
    if (!deployPackageFiles(packageId, extractDir.path(), packageType)) {
        setBusy(false);
        emit errorOccurred(tr("Failed to deploy package files."));
        return;
    }

    // Get version info
    const QString versionToInstall = [this, packageId]() {
        for (const auto &p : std::as_const(m_packageList)) {
            if (p.toMap().value(QStringLiteral("id")).toString() == packageId) {
                return p.toMap().value(QStringLiteral("latest_version")).toString();
            }
        }
        return QString();
    }();

    // Save installed info
    QVariantMap installed = loadInstalledPackagesFromFile();
    QVariantMap info;
    info[QStringLiteral("version")] = versionToInstall;
    installed[packageId] = info;

    QFile installedFile(getInstalledPackagesPath());
    if (installedFile.open(QIODevice::WriteOnly)) {
        installedFile.write(QJsonDocument::fromVariant(installed).toJson());
        installedFile.close();
    }

    // Reload effects/objects if needed
    if (packageType == QStringLiteral("effect") || packageType == QStringLiteral("object")) {
        const QString deployDir = getPackageDeployDir(packageType);
        EffectRegistry::instance().loadEffectsFromDirectory(deployDir);
    } else if (packageType == QStringLiteral("mod")) {
        // Mods will be loaded on next app restart
    }

    setProgress(1.0);
    setStatus(tr("Installation complete: %1").arg(packageId));
    setBusy(false);

    emit packageInstalled(packageId);

    // Update list state
    bool anyUpdates = false;
    for (int i = 0; i < m_packageList.size(); ++i) {
        QVariantMap item = m_packageList[i].toMap();
        if (item.value(QStringLiteral("id")).toString() == packageId) {
            item[QStringLiteral("installed_version")] = versionToInstall;
            m_packageList[i] = item;
            emit packageListChanged();
        }
        const QString installedVer = item.value(QStringLiteral("installed_version")).toString();
        const QString latestVer = item.value(QStringLiteral("latest_version")).toString();
        if (!installedVer.isEmpty() && !latestVer.isEmpty() && compareVersions(latestVer, installedVer) > 0) {
            anyUpdates = true;
        }
    }
    setHasUpdatesAvailable(anyUpdates);
}

bool PackageManager::extractZip(const QString &archivePath, const QString &destDir) {
    QProcess process;

#ifdef Q_OS_WIN
    // Use PowerShell on Windows
    const QString cmd = QStringLiteral("Expand-Archive");
    const QStringList args = {
        QStringLiteral("-Path"), archivePath,
        QStringLiteral("-DestinationPath"), destDir,
        QStringLiteral("-Force")
    };
    process.start(QStringLiteral("powershell"), {QStringLiteral("-Command"), cmd + " " + args.join(" ")});
#else
    // Use unzip on Linux/macOS
    process.start(QStringLiteral("unzip"), {QStringLiteral("-o"), archivePath, QStringLiteral("-d"), destDir});
#endif

    process.waitForFinished(30000);
    return process.exitCode() == 0;
}

bool PackageManager::deployPackageFiles(const QString &packageId, const QString &extractDir, const QString &packageType) {
    // Validate packageId to prevent path traversal
    if (packageId.contains(QStringLiteral("..")) || packageId.contains('/') || packageId.contains('\\')) {
        qWarning() << "[PackageManager] Invalid package ID (path traversal):" << packageId;
        return false;
    }

    const QString deployBase = getPackageDeployDir(packageType);
    if (deployBase.isEmpty()) {
        return false;
    }

    // Create package-specific subdirectory
    const QString packageDir = deployBase + QStringLiteral("/") + packageId;
    QDir().mkpath(packageDir);

    // Find the content to deploy
    QDir sourceDir(extractDir);
    QStringList entries = sourceDir.entryList(QDir::Files | QDir::Dirs | QDir::NoDotAndDotDot);

    // If archive contains a single directory, use its contents
    if (entries.size() == 1) {
        QFileInfo fi(sourceDir.absoluteFilePath(entries.first()));
        if (fi.isDir()) {
            sourceDir.cd(entries.first());
            entries = sourceDir.entryList(QDir::Files | QDir::Dirs | QDir::NoDotAndDotDot);
        }
    }

    // Copy all files
    for (const QString &entry : std::as_const(entries)) {
        const QString srcPath = sourceDir.absoluteFilePath(entry);
        const QString destPath = packageDir + QStringLiteral("/") + entry;
        QFileInfo srcInfo(srcPath);

        if (srcInfo.isDir()) {
            if (!copyDirectory(srcPath, destPath)) {
                return false;
            }
        } else {
            if (QFile::exists(destPath)) {
                QFile::remove(destPath);
            }
            if (!QFile::copy(srcPath, destPath)) {
                return false;
            }
        }
    }

    return true;
}

QString PackageManager::getPackageDeployDir(const QString &packageType) const {
    const QString appDir = QCoreApplication::applicationDirPath();

    if (packageType == QStringLiteral("mod")) {
        return appDir + QStringLiteral("/plugins");
    } else if (packageType == QStringLiteral("effect")) {
        return appDir + QStringLiteral("/effects");
    } else if (packageType == QStringLiteral("object")) {
        return appDir + QStringLiteral("/objects");
    }

    // Default to plugins directory
    return appDir + QStringLiteral("/plugins");
}

void PackageManager::removePackage(const QString &packageId) {
    if (m_isBusy || packageId == QStringLiteral("org.aviqtl.app"))
        return;

    // Get package type to determine file location
    QVariantMap pkg;
    for (const auto &p : std::as_const(m_packageList)) {
        if (p.toMap().value(QStringLiteral("id")).toString() == packageId) {
            pkg = p.toMap();
            break;
        }
    }

    // Validate packageId to prevent path traversal
    if (packageId.contains(QStringLiteral("..")) || packageId.contains('/') || packageId.contains('\\')) {
        emit errorOccurred(tr("Invalid package ID."));
        return;
    }

    setBusy(true);
    setStatus(tr("Removing package: %1").arg(packageId));

    const QString packageType = pkg.value(QStringLiteral("type")).toString();
    const QString deployDir = getPackageDeployDir(packageType);
    const QString packageDir = deployDir + QStringLiteral("/") + packageId;

    // Remove package files
    if (QDir(packageDir).exists()) {
        QDir(packageDir).removeRecursively();
    }

    // Remove from installed list
    QVariantMap installed = loadInstalledPackagesFromFile();
    if (installed.remove(packageId)) {
        QFile file(getInstalledPackagesPath());
        if (file.open(QIODevice::WriteOnly)) {
            file.write(QJsonDocument::fromVariant(installed).toJson());
            file.close();
        }
    }

    // Update list state
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
    if (query.isEmpty())
        return m_packageList;
    QVariantList filtered;
    for (const auto &p : m_packageList) {
        QVariantMap map = p.toMap();
        if (map.value("display_name").toString().contains(query, Qt::CaseInsensitive) || map.value("id").toString().contains(query, Qt::CaseInsensitive)) {
            filtered.append(p);
        }
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
    if (m_isBusy)
        return;

    m_upgradeQueue.clear();
    for (const auto &p : m_packageList) {
        const QVariantMap item = p.toMap();
        const QString installedVer = item.value(QStringLiteral("installed_version")).toString();
        const QString latestVer = item.value(QStringLiteral("latest_version")).toString();
        if (!installedVer.isEmpty() && !latestVer.isEmpty() && compareVersions(latestVer, installedVer) > 0) {
            m_upgradeQueue.enqueue(item.value(QStringLiteral("id")).toString());
        }
    }

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

    // Find asset URL from package list
    QVariantMap pkg;
    for (const auto &p : std::as_const(m_packageList)) {
        if (p.toMap().value(QStringLiteral("id")).toString() == nextPackageId) {
            pkg = p.toMap();
            break;
        }
    }

    // Fetch assets, then download on success
    QString repoUrl = pkg.value(QStringLiteral("repository_url")).toString();
    if (repoUrl.isEmpty()) {
        QString feed = pkg.value(QStringLiteral("release_feed")).toString();
        if (!feed.isEmpty()) {
            int idx = feed.indexOf(QStringLiteral("/releases"));
            if (idx != -1) repoUrl = feed.left(idx);
        }
    }

    if (repoUrl.isEmpty()) {
        qWarning() << "[PackageManager] No repository URL for" << nextPackageId;
        processUpgradeQueue(); // skip, try next
        return;
    }

    QUrl apiUrl;
    bool isGitHub = repoUrl.contains(QStringLiteral("github.com"));
    bool isCodeberg = repoUrl.contains(QStringLiteral("codeberg.org"));
    QString path = QUrl(repoUrl).path();
    if (path.startsWith('/')) path.remove(0, 1);
    QStringList parts = path.split('/');
    if (parts.size() < 2) {
        processUpgradeQueue();
        return;
    }
    QString owner = parts[0];
    QString repo = parts[1];
    if (repo.endsWith(".git")) repo.remove(repo.size() - 4, 4);

    if (isGitHub)
        apiUrl = QStringLiteral("https://api.github.com/repos/%1/%2/releases/latest").arg(owner, repo);
    else if (isCodeberg)
        apiUrl = QStringLiteral("https://codeberg.org/api/v1/repos/%1/%2/releases?limit=1").arg(owner, repo);
    else {
        processUpgradeQueue();
        return;
    }

    QNetworkReply *reply = m_networkManager->get(QNetworkRequest(apiUrl));
    connect(reply, &QNetworkReply::finished, this, [this, reply, nextPackageId]() {
        reply->deleteLater();
        if (reply->error() != QNetworkReply::NoError) {
            processUpgradeQueue();
            return;
        }
        QJsonDocument doc = QJsonDocument::fromJson(reply->readAll());
        QJsonObject releaseObj;
        if (doc.isArray() && !doc.array().isEmpty())
            releaseObj = doc.array().at(0).toObject();
        else if (doc.isObject())
            releaseObj = doc.object();

        QJsonArray assetsArr = releaseObj.value(QStringLiteral("assets")).toArray();
        if (!assetsArr.isEmpty()) {
            QString assetUrl = assetsArr.first().toObject().value(QStringLiteral("browser_download_url")).toString();
            if (!assetUrl.isEmpty()) {
                downloadPackage(nextPackageId, QUrl(assetUrl));
                return;
            }
        }
        processUpgradeQueue();
    });
}

} // namespace AviQtl::Core
