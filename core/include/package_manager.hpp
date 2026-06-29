#pragma once
#include <QObject>
#include <QQueue>
#include <QHash>
#include <QStringList>
#include <QVariantList>
#include <QVariantMap>

class QNetworkAccessManager;

namespace AviQtl::Core {

class PackageManager : public QObject {
    Q_OBJECT
    Q_PROPERTY(bool isBusy READ isBusy NOTIFY isBusyChanged)
    Q_PROPERTY(QVariantList packageList READ packageList NOTIFY packageListChanged)
    Q_PROPERTY(QString statusText READ statusText NOTIFY statusTextChanged)
    Q_PROPERTY(double progress READ progress NOTIFY progressChanged)
    Q_PROPERTY(bool hasUpdatesAvailable READ hasUpdatesAvailable NOTIFY hasUpdatesAvailableChanged)
    Q_PROPERTY(QVariantList repositories READ repositories NOTIFY repositoriesChanged)

  public:
    static PackageManager &instance();

    bool isBusy() const { return m_isBusy; }
    QString statusText() const { return m_statusText; }
    bool hasUpdatesAvailable() const { return m_hasUpdatesAvailable; }
    double progress() const { return m_progress; }
    QVariantList packageList() const { return m_packageList; }
    QVariantList repositories() const;

    Q_INVOKABLE void sync();
    Q_INVOKABLE void refreshRepositories();
    Q_INVOKABLE void addRepository(const QString &url, bool enabled = true, int priority = 10);
    Q_INVOKABLE void removeRepository(const QString &url);
    Q_INVOKABLE void setRepositoryEnabled(const QString &url, bool enabled);
    Q_INVOKABLE void setRepositoryPriority(const QString &url, int priority);
    Q_INVOKABLE void fetchAssets(const QString &packageId);
    Q_INVOKABLE void fetchPackageMetadata(const QString &packageId, const QString &sourceRepo = QString());
    Q_INVOKABLE void installPackage(const QString &packageId, const QString &sourceRepo = QString(), const QString &version = QString());
    Q_INVOKABLE void upgradeAllPackages();
    Q_INVOKABLE void removePackage(const QString &packageId);
    Q_INVOKABLE QVariantList searchPackages(const QString &query) const;
    Q_INVOKABLE QVariantList getInstalledPackages() const;

  signals:
    void isBusyChanged();
    void statusTextChanged();
    void packageListChanged();
    void progressChanged();
    void repositoryRefreshed();
    void hasUpdatesAvailableChanged();
    void repositoriesChanged();
    void assetsReady(const QString &packageId, const QVariantList &assets);
    void packageDetailReady(const QString &packageId, const QVariantMap &detail);
    void packageInstalled(const QString &packageId);
    void packageRemoved(const QString &packageId);
    void errorOccurred(const QString &message);
    void selfUpdateAvailable(const QString &newVersion, const QString &downloadUrl);

  private:
    explicit PackageManager(QObject *parent = nullptr);
    ~PackageManager() override = default;

    void setBusy(bool busy);
    void setStatus(const QString &status);
    void setProgress(double p);
    void loadCachedPackages();
    void updatePackageLatestVersion(const QString &id, const QString &version);
    void setHasUpdatesAvailable(bool available);
    void processUpgradeQueue();
    void saveRepositories(const QVariantList &repos);
    void mergeCatalogPackage(const QVariantMap &pkg, const QVariantMap &repo, const QVariantMap &installed);
    void onCatalogFetched(const QVariantMap &repoInfo, const QByteArray &data, const QVariantMap &installed);
    void tryFinishSyncLegacy(const QVariantMap &installed);
    void updateUpdateState();
    void fetchPackageMetadataForInstall(const QString &packageId, const QString &sourceRepo, const QString &version);
    void continueInstallWithMetadata(const QString &packageId, const QString &sourceRepo, const QString &version, const QVariantMap &detail);

    // Package installation pipeline
    void downloadPackage(const QString &packageId, const QUrl &url, const QString &expectedSha256, const QString &packageType, const QString &version, const QString &sourceRepo);
    void extractAndDeploy(const QString &packageId, const QString &archivePath, const QString &packageType, const QString &version = {}, const QString &downloadUrl = {}, const QString &sourceRepo = {});
    bool extractZip(const QString &archivePath, const QString &destDir);
    bool deployPackageFiles(const QString &packageId, const QString &extractDir, const QString &packageType);
    QString getPackageDeployDir(const QString &packageType) const;
    void compileShadersInDirectory(const QString &directory);

    bool m_isBusy = false;
    QVariantList m_packageList;
    QNetworkAccessManager *m_networkManager;
    int m_pendingRequests = 0;

    QString m_statusText;
    double m_progress = 0.0;
    bool m_hasUpdatesAvailable = false;
    QQueue<QString> m_upgradeQueue;
    QHash<QString, QVariantMap> m_packageDetails;
    QVariantMap m_pendingInstall;
};

} // namespace AviQtl::Core
