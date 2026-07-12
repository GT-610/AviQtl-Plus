#include "video_encoder.hpp"
#include <QDir>
#include <QFile>
#include <QImage>
#include <QTemporaryDir>
#include <QTest>
#include <cmath>
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

        constexpr int frameCount = 30;
        constexpr int sampleRate = 48'000;
        constexpr int channelCount = 2;
        for (int i = 0; i < frameCount; ++i) {
            QImage img(320, 240, QImage::Format_RGBA8888);
            img.fill(QColor::fromRgb(i * 8, 0, 255 - (i * 8)));
            QVERIFY(encoder.pushFrame(img, i));

            const int samples = sampleRate / 30;
            std::vector<float> audio(samples * channelCount, 0.0F);
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
            int audioStreamIndex = -1;
            for (unsigned int i = 0; i < fmtCtx->nb_streams; ++i) {
                if (fmtCtx->streams[i]->codecpar->codec_type == AVMEDIA_TYPE_VIDEO) {
                    videoStreamIndex = static_cast<int>(i);
                } else if (fmtCtx->streams[i]->codecpar->codec_type == AVMEDIA_TYPE_AUDIO) {
                    audioStreamIndex = static_cast<int>(i);
                }
            }
            QVERIFY(videoStreamIndex >= 0);
            QVERIFY(audioStreamIndex >= 0);

            const AVStream *videoStream = fmtCtx->streams[videoStreamIndex];
            QCOMPARE(videoStream->codecpar->width, 320);
            QCOMPARE(videoStream->codecpar->height, 240);
            QVERIFY(std::abs(av_q2d(videoStream->avg_frame_rate) - 30.0) < 0.01);

            const AVStream *audioStream = fmtCtx->streams[audioStreamIndex];
            QCOMPARE(audioStream->codecpar->sample_rate, sampleRate);
            QCOMPARE(audioStream->codecpar->ch_layout.nb_channels, channelCount);

            const AVCodec *audioCodec = avcodec_find_decoder(audioStream->codecpar->codec_id);
            QVERIFY(audioCodec != nullptr);
            AVCodecContext *audioCodecContext = avcodec_alloc_context3(audioCodec);
            QVERIFY(audioCodecContext != nullptr);
            QVERIFY(avcodec_parameters_to_context(audioCodecContext, audioStream->codecpar) >= 0);
            QVERIFY(avcodec_open2(audioCodecContext, audioCodec, nullptr) >= 0);

            int videoPackets = 0;
            int decodedAudioSamples = 0;
            AVPacket *pkt = av_packet_alloc();
            AVFrame *audioFrame = av_frame_alloc();
            QVERIFY(pkt != nullptr);
            QVERIFY(audioFrame != nullptr);
            while (av_read_frame(fmtCtx, pkt) >= 0) {
                if (pkt->stream_index == videoStreamIndex) {
                    ++videoPackets;
                } else if (pkt->stream_index == audioStreamIndex) {
                    QVERIFY(avcodec_send_packet(audioCodecContext, pkt) >= 0);
                    while (avcodec_receive_frame(audioCodecContext, audioFrame) >= 0) {
                        decodedAudioSamples += audioFrame->nb_samples;
                        av_frame_unref(audioFrame);
                    }
                }
                av_packet_unref(pkt);
            }
            QVERIFY(avcodec_send_packet(audioCodecContext, nullptr) >= 0);
            while (avcodec_receive_frame(audioCodecContext, audioFrame) >= 0) {
                decodedAudioSamples += audioFrame->nb_samples;
                av_frame_unref(audioFrame);
            }

            av_frame_free(&audioFrame);
            av_packet_free(&pkt);
            avcodec_free_context(&audioCodecContext);
            avformat_close_input(&fmtCtx);
            QCOMPARE(videoPackets, frameCount);
            QVERIFY(decodedAudioSamples >= 47'000);
            QVERIFY(decodedAudioSamples <= 50'500);
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
