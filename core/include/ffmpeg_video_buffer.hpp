#pragma once
#include <QAbstractVideoBuffer>
#include <QVideoFrame>
#include <QVideoFrameFormat>
#include <cstdlib>

extern "C" {
#include <libavutil/frame.h>
}

namespace AviQtl::Core {

inline int videoPlaneHeight(QVideoFrameFormat::PixelFormat format, int plane, int frameHeight) {
    if (plane == 0)
        return frameHeight;
    switch (format) {
    case QVideoFrameFormat::Format_YUV420P:
    case QVideoFrameFormat::Format_YV12:
    case QVideoFrameFormat::Format_NV12:
    case QVideoFrameFormat::Format_NV21:
    case QVideoFrameFormat::Format_P010:
    case QVideoFrameFormat::Format_YUV420P10:
        return (frameHeight + 1) / 2;
    default:
        return frameHeight;
    }
}

class FFmpegVideoBuffer final : public QAbstractVideoBuffer {
  public:
    FFmpegVideoBuffer(const FFmpegVideoBuffer &) = delete;
    FFmpegVideoBuffer &operator=(const FFmpegVideoBuffer &) = delete;
    FFmpegVideoBuffer(AVFrame *frame, const QVideoFrameFormat &format) : m_frame(av_frame_alloc()), m_format(format) { av_frame_ref(m_frame, frame); }

    ~FFmpegVideoBuffer() override { av_frame_free(&m_frame); }

    QVideoFrameFormat format() const override { return m_format; }

    MapData map(QVideoFrame::MapMode) override {
        MapData d;
        d.planeCount = 0;
        for (int i = 0; i < AV_NUM_DATA_POINTERS && m_frame->data[i]; ++i) {
            d.data[i] = m_frame->data[i];
            d.bytesPerLine[i] = m_frame->linesize[i];
            d.dataSize[i] = std::abs(m_frame->linesize[i]) * videoPlaneHeight(m_format.pixelFormat(), i, m_frame->height);
            ++d.planeCount;
        }
        return d;
    }

    void unmap() override {}

  private:
    AVFrame *m_frame = nullptr;
    QVideoFrameFormat m_format;
};

} // namespace AviQtl::Core
