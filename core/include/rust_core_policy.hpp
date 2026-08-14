#pragma once

#include "rust_core_abi.hpp"
#include <QByteArray>
#include <QString>
#include <QStringList>
#include <QStringView>
#include <cstdint>
#include <cstring>

namespace AviQtl::RustCore::Policy {

enum class PackageType : std::int32_t {
    Invalid = -1,
    Mod = 0,
    Effect = 1,
    Object = 2,
};

inline QByteArray utf8(QStringView value) { return value.toString().toUtf8(); }

inline bool isDirectAudioMode(QStringView value) {
    const QByteArray encoded = utf8(value);
    return aviqtl_media_is_direct_audio_mode(
               reinterpret_cast<const std::uint8_t *>(encoded.constData()),
               static_cast<std::size_t>(encoded.size())) != 0;
}

inline bool isVideoFile(QStringView value) {
    const QByteArray encoded = utf8(value);
    return aviqtl_media_is_video_file(
               reinterpret_cast<const std::uint8_t *>(encoded.constData()),
               static_cast<std::size_t>(encoded.size())) != 0;
}

inline double resolveAudioTime(double relativeTime, bool directMode, double directTime,
                               double startTime, double speed) {
    return aviqtl_media_resolve_audio_time(relativeTime, directMode ? 1U : 0U, directTime,
                                           startTime, speed);
}

inline double resolveVideoTime(int relativeFrame, double sourceFps, bool directMode,
                               double directFrame, double startFrame, double speed) {
    return aviqtl_media_resolve_video_time(relativeFrame, sourceFps, directMode ? 1U : 0U,
                                           directFrame, startFrame, speed);
}

inline int maxVideoDurationFrames(int totalFrameCount, double sourceFps, double speed,
                                  double startFrame, int projectFps) {
    return aviqtl_media_max_video_duration_frames(totalFrameCount, sourceFps, speed, startFrame,
                                                  projectFps);
}

inline int permissionFromName(QStringView value) {
    const QByteArray encoded = utf8(value);
    return aviqtl_permission_from_name(
        reinterpret_cast<const std::uint8_t *>(encoded.constData()),
        static_cast<std::size_t>(encoded.size()));
}

inline int permissionForApi(const char *value) {
    if (value == nullptr)
        return -1;
    return aviqtl_permission_for_api(reinterpret_cast<const std::uint8_t *>(value),
                                     std::strlen(value));
}

inline int permissionCount() { return aviqtl_permission_count(); }

inline QString permissionName(int permission) {
    std::size_t length = 0;
    const std::uint8_t *name = aviqtl_permission_name(permission, &length);
    return name == nullptr ? QString()
                           : QString::fromUtf8(reinterpret_cast<const char *>(name),
                                               static_cast<qsizetype>(length));
}

inline QStringList allPermissionNames() {
    QStringList names;
    names.reserve(permissionCount());
    for (int permission = 0; permission < permissionCount(); ++permission)
        names.append(permissionName(permission));
    return names;
}

inline bool isValidPackageId(QStringView value) {
    const QByteArray encoded = utf8(value);
    return aviqtl_package_id_is_valid(
               reinterpret_cast<const std::uint8_t *>(encoded.constData()),
               static_cast<std::size_t>(encoded.size())) != 0;
}

inline PackageType packageType(QStringView value) {
    const QByteArray encoded = utf8(value);
    return static_cast<PackageType>(aviqtl_package_type(
        reinterpret_cast<const std::uint8_t *>(encoded.constData()),
        static_cast<std::size_t>(encoded.size())));
}

inline bool isSafeArchivePath(QStringView value) {
    const QByteArray encoded = utf8(value);
    return aviqtl_package_archive_path_is_safe(
               reinterpret_cast<const std::uint8_t *>(encoded.constData()),
               static_cast<std::size_t>(encoded.size())) != 0;
}

} // namespace AviQtl::RustCore::Policy
