#include "video_encoder.hpp"
#include <QDir>
#include <QFile>
#include <QImage>
#include <QTemporaryDir>
#include <QTest>
extern "C" {
#include <libavcodec/avcodec.h>
#include <libavformat/avformat.h>
#include <libavutil/avutil.h>
}

using namespace AviQtl::Core;

class TestVideoEncoder : public QObject {
    Q_OBJECT

  private slots:
    void defaultConfigUsesSoftwareCodec() {
        VideoEncoder::Config c;
        QCOMPARE(c.codecName, QStringLiteral("libx264"));
    }

    void openFailsWithInvalidPath() {
        VideoEncoder encoder;
        VideoEncoder::Config c;
        c.width = 320;
        c.height = 240;
        c.fps_num = 30;
        c.fps_den = 1;
        c.outputUrl = QStringLiteral("");
        QVERIFY(!encoder.open(c));
    }

    void openAndCloseProducesFile() {
        QTemporaryDir dir;
        QVERIFY(dir.isValid());

        const QString outputPath = dir.path() + QStringLiteral("/test_output.mp4");

        VideoEncoder encoder;
        VideoEncoder::Config c;
        c.width = 320;
        c.height = 240;
        c.fps_num = 30;
        c.fps_den = 1;
        c.codecName = QStringLiteral("libx264");
        c.audioCodecName = QStringLiteral("aac");
        c.outputUrl = outputPath;

        QVERIFY(encoder.open(c));
        QVERIFY(encoder.addAudioStream(48000, 2));

        for (int i = 0; i < 5; ++i) {
            QImage img(320, 240, QImage::Format_RGBA8888);
            img.fill(Qt::black);
            QVERIFY(encoder.pushFrame(img, i));

            const int samples = 48000 / 30;
            std::vector<float> audio(samples * 2, 0.0F);
            QVERIFY(encoder.pushAudio(audio.data(), static_cast<int>(audio.size())));
        }

        encoder.close();
        QVERIFY(QFile::exists(outputPath));
        QVERIFY(QFile(outputPath).size() > 0);

        // Verify the container contains the expected number of video frames.
        AVFormatContext *fmtCtx = nullptr;
        const int openRet = avformat_open_input(&fmtCtx, outputPath.toUtf8().constData(), nullptr, nullptr);
        QVERIFY(openRet >= 0);
        if (openRet >= 0) {
            QVERIFY(avformat_find_stream_info(fmtCtx, nullptr) >= 0);
            int videoStreamIndex = -1;
            for (unsigned int i = 0; i < fmtCtx->nb_streams; ++i) {
                if (fmtCtx->streams[i]->codecpar->codec_type == AVMEDIA_TYPE_VIDEO) {
                    videoStreamIndex = static_cast<int>(i);
                    break;
                }
            }
            QVERIFY(videoStreamIndex >= 0);

            int frameCount = 0;
            AVPacket *pkt = av_packet_alloc();
            while (av_read_frame(fmtCtx, pkt) >= 0) {
                if (pkt->stream_index == videoStreamIndex) {
                    ++frameCount;
                }
                av_packet_unref(pkt);
            }
            av_packet_free(&pkt);
            avformat_close_input(&fmtCtx);
            QCOMPARE(frameCount, 5);
        }
    }

    void pushFrameReturnsFalseAfterError() {
        VideoEncoder encoder;
        // After a failed open(), subsequent pushFrame calls should be rejected.
        VideoEncoder::Config c;
        c.width = 320;
        c.height = 240;
        c.fps_num = 30;
        c.fps_den = 1;
        c.outputUrl = QStringLiteral("");
        QVERIFY(!encoder.open(c));

        QImage img(320, 240, QImage::Format_RGBA8888);
        img.fill(Qt::red);
        QVERIFY(!encoder.pushFrame(img, 0));
    }
};

QTEST_MAIN(TestVideoEncoder)
#include "test_video_encoder.moc"
