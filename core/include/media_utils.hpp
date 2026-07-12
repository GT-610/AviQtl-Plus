#pragma once
#include "constants.hpp"
#include <QString>
#include <QStringView>

namespace AviQtl::Core::MediaUtils {

inline bool isDirectAudioMode(QStringView playMode) {
    return playMode.contains(QStringLiteral("直接"));
}

inline bool isVideoFile(QStringView path) {
    const QString lower = path.toString().toLower();
    return lower.endsWith(QStringLiteral(".mp4")) || lower.endsWith(QStringLiteral(".mov")) ||
           lower.endsWith(QStringLiteral(".avi")) || lower.endsWith(QStringLiteral(".mkv")) ||
           lower.endsWith(QStringLiteral(".webm")) || lower.endsWith(QStringLiteral(".wmv"));
}

// Audio time mapping: resolves the source file position (in seconds) from
// the playMode, startTime/speed or directTime. Used identically by
// TimelineMediaManager::onCurrentFrameChanged and BakeController::bakeAudioState.
inline double resolveAudioTime(double relTime, bool isDirectMode, double directTime,
                               double startTime, double speed) {
    if (isDirectMode) {
        return directTime;
    }
    return (relTime * (speed / AviQtl::kDefaultSpeed)) + startTime;
}

// Video time mapping: resolves the source file position (in seconds) from
// the playMode, startFrame/speed or directFrame. Used identically by
// TimelineMediaManager::updateVideoClipFrame and BakeController.
inline double resolveVideoTime(int relFrame, double sourceFps, bool isDirectMode,
                               double directFrame, double startFrame, double speed) {
    if (sourceFps <= 0.0) {
        return 0.0;
    }
    if (isDirectMode) {
        return directFrame / sourceFps;
    }
    const double startSec = startFrame / sourceFps;
    const double relTime = static_cast<double>(relFrame) / sourceFps;
    return startSec + (relTime * (speed / AviQtl::kDefaultSpeed));
}

// Compute how many project frames the source video can sustain given its
// startFrame and speed. Used by videoMetaReady handler and similar auto-trim.
inline int maxVideoDurationFrames(int totalFrameCount, double sourceFps,
                                  double speed, double startFrame,
                                  int projectFps) {
    if (speed <= 0.0 || sourceFps <= 0.0 || projectFps <= 0) {
        return 0;
    }
    const double startSec = startFrame / sourceFps;
    const double remainingSec = (static_cast<double>(totalFrameCount) / sourceFps) - startSec;
    if (remainingSec <= 0.0) {
        return 0;
    }
    return static_cast<int>(remainingSec / (speed / AviQtl::kDefaultSpeed) * projectFps);
}

double mediaDurationSeconds(const QString &path, int mediaType);

} // namespace AviQtl::Core::MediaUtils
