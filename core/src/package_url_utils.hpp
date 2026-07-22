#pragma once

#include <QString>
#include <QUrl>

namespace AviQtl::Core::Internal {

inline bool isSecureNetworkUrl(const QUrl &url) {
    return url.isValid() && url.scheme() == QStringLiteral("https") && !url.host().isEmpty();
}

inline QUrl resolveRepositoryReference(const QUrl &repositoryIndexUrl, const QString &reference) {
    if (!repositoryIndexUrl.isValid() || reference.isEmpty()) {
        return {};
    }
    return repositoryIndexUrl.resolved(QUrl(reference));
}

} // namespace AviQtl::Core::Internal
