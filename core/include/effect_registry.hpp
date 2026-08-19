#pragma once
#include <QHash>
#include <QList>
#include <QLoggingCategory>
#include <QString>
#include <QVariantMap>
#include <vector>

Q_DECLARE_LOGGING_CATEGORY(lcEffectRegistry)

namespace AviQtl::Core {

// Resolves symlinks when possible; otherwise returns a clean absolute path.
[[nodiscard]] QString filesystemPathIdentity(const QString &path);
[[nodiscard]] bool filesystemPathsEqual(const QString &first, const QString &second);

struct EffectMetadata {
    QString id;
    QString name;
    QString version;
    QString kind;
    QStringList categories;
    QString qmlSource; // QML実装へのパス
    QString color;     // ← 追加: JSON の "color" フィールド（省略可）
    // Display metadata for catalog labels and search only. It never controls loading,
    // permissions, or trust decisions; JSON may override the caller's display value.
    QString source;
    QString packageId;
    QString sourcePath;
    QVariantMap defaultParams;
    QVariantMap uiDefinition; // UI定義（隠しパラメータやウィジェットタイプなど）
};

class EffectRegistry {
  public:
    static EffectRegistry &instance() {
        static EffectRegistry inst;
        return inst;
    }

    void registerEffect(const EffectMetadata &meta) {
        if (!m_effects.contains(meta.id)) {
            m_orderedIds.push_back(meta.id);
        }
        m_effects.insert(meta.id, meta);
    }

    /// Returns a reference valid only until loadEffectsFromDirectory() registers
    /// or reloads effects, or removeEffectsFromDirectory() removes entries.
    /// Callers must copy if they need a stable value.
    const EffectMetadata &getEffect(const QString &id) const {
        static const EffectMetadata s_empty;
        auto it = m_effects.constFind(id);
        return it != m_effects.cend() ? it.value() : s_empty;
    }

    QList<EffectMetadata> getAllEffects() const {
        QList<EffectMetadata> list;
        for (const auto &id : m_orderedIds) {
            list.append(m_effects[id]);
        }
        return list;
    }

    void loadEffectsFromDirectory(const QString &path, const QString &source = {});
    void removeEffectsFromDirectory(const QString &path);

  private:
    EffectRegistry() = default;
    QHash<QString, EffectMetadata> m_effects;
    std::vector<QString> m_orderedIds;
};

} // namespace AviQtl::Core
