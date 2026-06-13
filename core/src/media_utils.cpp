#include "media_utils.hpp"

extern "C" {
#include <libavformat/avformat.h>
#include <libavutil/avutil.h>
}

#include <algorithm>

namespace AviQtl::Core::MediaUtils {

static double streamDurationSeconds(const AVFormatContext *formatContext, const AVStream *stream) {
    if (stream != nullptr && stream->duration != AV_NOPTS_VALUE) {
        const double streamSeconds = static_cast<double>(stream->duration) * av_q2d(stream->time_base);
        if (streamSeconds > 0.0) {
            return streamSeconds;
        }
    }

    if (formatContext != nullptr && formatContext->duration != AV_NOPTS_VALUE) {
        const double formatSeconds = static_cast<double>(formatContext->duration) / static_cast<double>(AV_TIME_BASE);
        if (formatSeconds > 0.0) {
            return formatSeconds;
        }
    }

    return 0.0;
}

double mediaDurationSeconds(const QString &path, int mediaType) {
    if (path.isEmpty()) {
        return 0.0;
    }

    AVFormatContext *formatContext = nullptr;
    if (avformat_open_input(&formatContext, path.toUtf8().constData(), nullptr, nullptr) != 0) {
        return 0.0;
    }

    double seconds = 0.0;
    if (avformat_find_stream_info(formatContext, nullptr) >= 0) {
        const int streamIndex = av_find_best_stream(formatContext, static_cast<AVMediaType>(mediaType), -1, -1, nullptr, 0);
        const AVStream *stream = streamIndex >= 0 ? formatContext->streams[streamIndex] : nullptr;
        seconds = streamDurationSeconds(formatContext, stream);
    }

    avformat_close_input(&formatContext);
    return std::max(0.0, seconds);
}

} // namespace AviQtl::Core::MediaUtils
