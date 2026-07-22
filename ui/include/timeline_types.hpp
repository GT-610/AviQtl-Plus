#pragma once
#include "constants.hpp"
#include "effect_model.hpp"
#include <QList>
#include <QHash>
#include <QSet>
#include <QString>
#include <QVariant>

namespace AviQtl::UI {

struct AudioPluginState {
    QString id;
    bool enabled = true;
    QVariantMap params;
    QVariantMap keyframeTracks; // paramIndex -> structured keyframe track

    void invalidateKeyframeCache() {
        ++keyframeRevision;
        resolvedKeyframeTracks.clear();
        resolvedKeyframeRevision = keyframeRevision;
    }

    quint64 keyframeRevision = 0;
    mutable quint64 resolvedKeyframeRevision = 0;
    mutable QHash<QString, QVariantList> resolvedKeyframeTracks;
};

struct ClipData {
    int id;
    int sceneId = 0;
    QString type;
    int startFrame;
    int durationFrames;
    int layer;
    bool clipByUpperObject = false;

    // ハイブリッド設計: EffectModelは振る舞いを持つためポインタで保持する
    QList<EffectModel *> effects;
    QList<AudioPluginState> audioPlugins;
    QVariantMap params; // 各クリップ固有のパラメータ（ファイルパス等）
};

struct SceneData {
    int id;
    QString name;
    QList<ClipData> clips;

    // レイヤー状態
    QSet<int> lockedLayers;
    QSet<int> hiddenLayers;

    int width = AviQtl::kDefaultWidth;
    int height = AviQtl::kDefaultHeight;
    double fps = AviQtl::kDefaultFps;
    int totalFrames = AviQtl::kDefaultTotalFrames;

    // ネスト利用のためのメタデータ
    int startFrame = 0;
    int durationFrames = 0;

    // Grid & Snap Settings (Moved from UI/System state to Scene state)
    QString gridMode = QStringLiteral("Auto"); // QStringLiteral("Auto"), QStringLiteral("BPM"), QStringLiteral("Frame")
    double gridBpm = 120.0;
    double gridOffset = 0.0;
    int gridInterval = 10;
    int gridSubdivision = 4;
    bool enableSnap = true;
    int magneticSnapRange = 10;
};
} // namespace AviQtl::UI
