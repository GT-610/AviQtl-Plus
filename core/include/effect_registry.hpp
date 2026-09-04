#pragma once
#include "rust_effect_document.hpp"
#include <QList>
#include <QLoggingCategory>
#include <QString>
#include <QVariantMap>

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

    void registerEffect(const EffectMetadata &meta);
    [[nodiscard]] EffectMetadata getEffect(const QString &id) const;
    [[nodiscard]] QList<EffectMetadata> getAllEffects() const;

    void loadEffectsFromDirectory(const QString &path, const QString &source = {});
    void removeEffectsFromDirectory(const QString &path);

  private:
    EffectRegistry() = default;
    RustCore::Effect::CatalogState m_catalogState;
};

} // namespace AviQtl::Core
