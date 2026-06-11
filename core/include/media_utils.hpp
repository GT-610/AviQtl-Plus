#pragma once
#include <QString>
#include <QStringView>

namespace AviQtl::Core::MediaUtils {

inline bool isDirectAudioMode(QStringView playMode) {
    return playMode.contains(QStringLiteral("直接")) || playMode.contains(QStringLiteral("鐩存帴"));
}

inline bool isVideoFile(QStringView path) {
    const QString lower = path.toString().toLower();
    return lower.endsWith(QStringLiteral(".mp4")) || lower.endsWith(QStringLiteral(".mov")) ||
           lower.endsWith(QStringLiteral(".avi")) || lower.endsWith(QStringLiteral(".mkv")) ||
           lower.endsWith(QStringLiteral(".webm")) || lower.endsWith(QStringLiteral(".wmv"));
}

} // namespace AviQtl::Core::MediaUtils
