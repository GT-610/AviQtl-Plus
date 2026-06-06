#include "transport_service.hpp"
#include <QElapsedTimer>

namespace AviQtl::UI {

TransportService::TransportService(QObject *parent) : QObject(parent), m_timer(new QTimer(this)) {
    m_clock.start(); // ← 追加：プロセス起動直後から単調増加

    m_timer->setTimerType(Qt::PreciseTimer);
    // tick間隔は短めに固定し、フレーム計算は経過時間から行う
    // 4ms ≒ 240fps 相当のポーリング。過剰だが CPU 使用量は僅少
    m_timer->setInterval(4);

    connect(m_timer, &QTimer::timeout, this, &TransportService::onTick);
}

auto TransportService::currentFrame() const -> int { return m_currentFrame; }
auto TransportService::isPlaying() const -> bool { return m_isPlaying; }

void TransportService::setCurrentFrame(int f) {
    if (m_currentFrame == f) {
        return;
    }
    m_currentFrame = f;
    emit currentFrameChanged();
}

void TransportService::togglePlay() {
    if (m_isPlaying) {
        m_timer->stop();
        m_isPlaying = false;
    } else {
        // 再生開始: 現在フレームを起点に時刻を記録
        m_prePlayFrame = m_currentFrame;
        m_playStartFrame = m_currentFrame;
        m_playStartTime = m_clock.nsecsElapsed();
        m_isPlaying = true;
        m_timer->start();
    }
    emit isPlayingChanged();
}

void TransportService::setCurrentFrame_seek(int f) {
    // シーク時は起点を再設定
    m_playStartFrame = f;
    m_playStartTime = m_clock.nsecsElapsed();
    setCurrentFrame(f);
}

void TransportService::onTick() {
    if (!m_isPlaying || m_fps <= 0) {
        return;
    }

    qint64 elapsedNs = m_clock.nsecsElapsed() - m_playStartTime;
    double elapsedSec = static_cast<double>(elapsedNs) / 1'000'000'000.0;

    int targetFrame = m_playStartFrame + static_cast<int>(elapsedSec * m_fps * m_playbackSpeed);

    if (m_totalFrames > 0 && targetFrame >= m_totalFrames) {
        if (m_loopEnabled) {
            targetFrame = targetFrame % m_totalFrames;
            m_playStartFrame = targetFrame;
            m_playStartTime = m_clock.nsecsElapsed();
        } else {
            targetFrame = m_totalFrames;
            setCurrentFrame(targetFrame);
            pause();
            return;
        }
    }

    if (targetFrame != m_currentFrame) {
        setCurrentFrame(targetFrame);
    }
}

void TransportService::stepForward() {
    if (m_isPlaying)
        return;
    if (m_currentFrame < m_totalFrames)
        setCurrentFrame_seek(m_currentFrame + 1);
}

void TransportService::stepBackward() {
    if (m_isPlaying)
        return;
    if (m_currentFrame > 0)
        setCurrentFrame_seek(m_currentFrame - 1);
}

void TransportService::updateTimerInterval(double fps) { setFps(fps); }

} // namespace AviQtl::UI