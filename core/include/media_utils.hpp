#pragma once
#include "rust_core_policy.hpp"
#include <QString>
#include <QStringView>

namespace AviQtl::Core::MediaUtils {

using PlaybackMode = RustCore::Policy::PlaybackMode;

inline PlaybackMode playbackMode(QStringView value) {
    return RustCore::Policy::playbackMode(value);
}

inline bool isVideoFile(QStringView path) {
    return RustCore::Policy::isVideoFile(path);
}

// Audio time mapping: resolves the source file position (in seconds) from
// the playMode, startTime/speed or directTime. Used identically by
// TimelineMediaManager::onCurrentFrameChanged and BakeController::bakeAudioState.
inline double resolveAudioTime(double relTime, bool isDirectMode, double directTime,
                               double startTime, double speed) {
    return RustCore::Policy::resolveAudioTime(relTime, isDirectMode, directTime, startTime, speed);
}

// Video time mapping: resolves the source file position (in seconds) from
// the playMode, startFrame/speed or directFrame. Used identically by
// TimelineMediaManager::updateVideoClipFrame and BakeController.
inline double resolveVideoTime(int relFrame, double sourceFps, bool isDirectMode,
                               double directFrame, double startFrame, double speed) {
    return RustCore::Policy::resolveVideoTime(relFrame, sourceFps, isDirectMode, directFrame,
                                              startFrame, speed);
}

// Compute how many project frames the source video can sustain given its
// startFrame and speed. Used by videoMetaReady handler and similar auto-trim.
inline int maxVideoDurationFrames(int totalFrameCount, double sourceFps,
                                  double speed, double startFrame,
                                  int projectFps) {
    return RustCore::Policy::maxVideoDurationFrames(totalFrameCount, sourceFps, speed, startFrame,
                                                    projectFps);
}

double mediaDurationSeconds(const QString &path, int mediaType);

} // namespace AviQtl::Core::MediaUtils
