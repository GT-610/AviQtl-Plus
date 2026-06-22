#pragma once
#include "constants.hpp"
#include <QElapsedTimer>
#include <QObject>
#include <QTimer>
#include <cmath>

namespace AviQtl::UI {

class TransportService : public QObject {
    Q_OBJECT
    Q_PROPERTY(int currentFrame READ currentFrame WRITE setCurrentFrame NOTIFY currentFrameChanged)
    Q_PROPERTY(bool isPlaying READ isPlaying NOTIFY isPlayingChanged)
    Q_PROPERTY(double playbackSpeed READ playbackSpeed WRITE setPlaybackSpeed NOTIFY playbackSpeedChanged)
    Q_PROPERTY(double fps READ fps WRITE setFps NOTIFY fpsChanged)
    Q_PROPERTY(int totalFrames READ totalFrames WRITE setTotalFrames NOTIFY totalFramesChanged)
    Q_PROPERTY(bool loopEnabled READ loopEnabled WRITE setLoopEnabled NOTIFY loopEnabledChanged)

  public:
    explicit TransportService(QObject *parent = nullptr);

    int currentFrame() const;
    bool isPlaying() const;
    double playbackSpeed() const { return m_playbackSpeed; }
    double fps() const { return m_fps; }
    int totalFrames() const { return m_totalFrames; }
    bool loopEnabled() const { return m_loopEnabled; }

    void setCurrentFrame(int f);
    Q_INVOKABLE void setCurrentFrame_seek(int f); // シーク専用（起点リセット付き）
    Q_INVOKABLE void togglePlay();
    Q_INVOKABLE void stepForward();
    Q_INVOKABLE void stepBackward();
    Q_INVOKABLE void play() {
        if (!m_isPlaying)
            togglePlay();
    }
    Q_INVOKABLE void pause() {
        if (m_isPlaying)
            togglePlay();
    }

    Q_INVOKABLE void beginScrub() {
        m_wasPlayingBeforeScrub = m_isPlaying;
        if (m_isPlaying)
            pause();
        m_lastScrubFrame = -1;
    }

    Q_INVOKABLE void scrubTo(int frame) {
        if (frame == m_lastScrubFrame)
            return;
        m_lastScrubFrame = frame;
        setCurrentFrame_seek(frame);
    }

    Q_INVOKABLE void endScrub() {
        m_lastScrubFrame = -1;
        if (m_wasPlayingBeforeScrub)
            play();
        m_wasPlayingBeforeScrub = false;
    }
    void updateTimerInterval(double fps); // 旧 API 互換

    void setPlaybackSpeed(double speed) {
        if (std::abs(m_playbackSpeed - speed) < 0.001)
            return;
        m_playbackSpeed = speed;
        emit playbackSpeedChanged();
    }

    void setFps(double fps) {
        if (std::abs(m_fps - fps) < 0.001)
            return;
        m_fps = fps;
        emit fpsChanged();
    }

    void setTotalFrames(int n) {
        if (m_totalFrames == n)
            return;
        m_totalFrames = n;
        emit totalFramesChanged();
    }

    void setLoopEnabled(bool enabled) {
        if (m_loopEnabled == enabled)
            return;
        m_loopEnabled = enabled;
        emit loopEnabledChanged();
    }

  signals:
    void currentFrameChanged();
    void isPlayingChanged();
    void playbackSpeedChanged();
    void fpsChanged();
    void totalFramesChanged();
    void loopEnabledChanged();

  private slots:
    void onTick();

  private:
    int m_currentFrame = 0;
    bool m_isPlaying = false;
    double m_playbackSpeed = 1.0;
    double m_fps = AviQtl::kDefaultFps;
    int m_totalFrames = 0;
    bool m_loopEnabled = false;

    // 再生クロック
    QElapsedTimer m_clock;      // 起動時からの単調増加クロック
    qint64 m_playStartTime = 0; // 再生開始時の m_clock.nsecsElapsed()
    int m_playStartFrame = 0;

    bool m_wasPlayingBeforeScrub = false;
    int m_lastScrubFrame = -1;

    QTimer *m_timer;
};
} // namespace AviQtl::UI