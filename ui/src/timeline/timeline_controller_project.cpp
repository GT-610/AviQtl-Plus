#include "effect_registry.hpp"
#include "engine/plugin/audio_plugin_manager.hpp"
#include "project_serializer.hpp"
#include "project_service.hpp"
#include "media_utils.hpp"
#include "settings_manager.hpp"
#include "timeline_controller.hpp"
#include "timeline_service.hpp"
#include "../../scripting/mod_engine.hpp"
#include <QCoreApplication>
#include <QFile>
#include <QFileInfo>
#include <QSet>
#include <QUrl>
#include <algorithm>
#include <optional>

namespace AviQtl::UI {

namespace {
void addRecentProject(const QString &fileUrl, ProjectService *project) {
    if (fileUrl.isEmpty() || !project)
        return;

    QString path = QUrl(fileUrl).toLocalFile();
    if (path.isEmpty()) {
        path = fileUrl;
    }
    QString name = QFileInfo(path).fileName();

    auto &settingsManager = AviQtl::Core::SettingsManager::instance();
    QVariantList recentList = settingsManager.value(QStringLiteral("recentProjects"), QVariantList()).toList();

    QVariantList newList;
    QVariantMap newEntry;
    newEntry[QStringLiteral("name")] = name;
    newEntry[QStringLiteral("path")] = path;
    newEntry[QStringLiteral("width")] = project->width();
    newEntry[QStringLiteral("height")] = project->height();
    newEntry[QStringLiteral("fps")] = project->fps();

    newList.append(newEntry);

    for (const auto &val : recentList) {
        QVariantMap entry = val.toMap();
        if (entry.value(QStringLiteral("path")).toString() != path) {
            newList.append(entry);
        }
    }

    // 最大件数でトリミング
    int maxRecent = settingsManager.value(QStringLiteral("recentProjectMaxCount"), 10).toInt();
    while (newList.size() > maxRecent) {
        newList.removeLast();
    }

    settingsManager.setValue(QStringLiteral("recentProjects"), newList);
    settingsManager.save();
}

struct MediaEffectTarget {
    int index;
    QString parameter;
};

auto findMediaEffectTarget(const ClipData &clip) -> std::optional<MediaEffectTarget> {
    QString parameter;
    if (clip.type == QStringLiteral("audio"))
        parameter = QStringLiteral("source");
    else if (clip.type == QStringLiteral("image") || clip.type == QStringLiteral("video"))
        parameter = QStringLiteral("path");
    else
        return std::nullopt;

    for (int index = 0; index < clip.effects.size(); ++index) {
        const auto *effect = clip.effects.at(index);
        if (effect != nullptr && effect->id() == clip.type)
            return MediaEffectTarget{index, parameter};
    }
    return std::nullopt;
}
} // namespace

auto TimelineController::saveProject(const QString &fileUrl) -> bool {
    // 渡されたパスが空の場合は内部で保持しているパスを割り当てる
    QString targetUrl = fileUrl.isEmpty() ? m_currentProjectUrl : fileUrl;

    // パスが空の場合は新規作成直後なのでエラーで返す
    if (targetUrl.isEmpty()) {
        emit errorOccurred(tr("保存先のファイルパスが不明です"));
        return false;
    }

    QString error;
    bool result = AviQtl::Core::ProjectSerializer::save(targetUrl, m_timeline, m_project, &error);

    if (result) {
        // 保存に成功したパスを現在のプロジェクトパスとして記憶する
        m_currentProjectUrl = targetUrl;
        m_timeline->undoStack()->setClean();
        emit currentProjectUrlChanged();
        emit hasUnsavedChangesChanged();
        addRecentProject(targetUrl, m_project);

        // Call onProjectSave hook
        AviQtl::Scripting::ModEngine::instance().onProjectSave(targetUrl);
    } else {
        emit errorOccurred(error);
    }
    return result;
}

auto TimelineController::loadProject(const QString &fileUrl) -> bool {
    QString error;
    bool result = AviQtl::Core::ProjectSerializer::load(fileUrl, m_timeline, m_project, &error);

    if (result) {
        // 読み込みに成功したパスを現在のプロジェクトパスとして記憶する
        m_currentProjectUrl = fileUrl;
        m_timeline->undoStack()->setClean();
        emit currentProjectUrlChanged();
        emit hasUnsavedChangesChanged();
        addRecentProject(fileUrl, m_project);

        // Call onProjectOpen hook
        AviQtl::Scripting::ModEngine::instance().onProjectOpen(fileUrl);
        refreshMissingMedia();
    } else {
        emit errorOccurred(error);
    }
    return result;
}

void TimelineController::refreshMissingMedia() {
    QVariantList missing;
    for (const auto &scene : m_timeline->getAllScenes()) {
        for (const auto &clip : scene.clips) {
            const auto target = findMediaEffectTarget(clip);
            if (!target)
                continue;
            const QString path = clip.effects.at(target->index)->params().value(target->parameter).toString();
            if (!path.isEmpty() && !QFileInfo::exists(path)) {
                QVariantMap item;
                item.insert(QStringLiteral("clipId"), clip.id);
                item.insert(QStringLiteral("sceneId"), scene.id);
                item.insert(QStringLiteral("layer"), clip.layer);
                item.insert(QStringLiteral("type"), clip.type);
                item.insert(QStringLiteral("path"), path);
                item.insert(QStringLiteral("name"), QFileInfo(path).fileName());
                missing.append(item);
            }
        }
    }
    if (m_missingMedia != missing) {
        m_missingMedia = missing;
        emit missingMediaChanged();
    }
}

bool TimelineController::relinkMedia(int clipId, const QString &fileUrl) {
    const auto *clip = m_timeline->findClipById(clipId);
    if (clip == nullptr) {
        emit errorOccurred(tr("クリップが見つかりません: %1").arg(clipId));
        return false;
    }
    if (clip->type != QStringLiteral("audio") && clip->type != QStringLiteral("image") && clip->type != QStringLiteral("video")) {
        emit errorOccurred(tr("再リンクできないメディアタイプです: %1").arg(clip->type));
        return false;
    }
    QString path = QUrl(fileUrl).toLocalFile();
    if (path.isEmpty()) path = fileUrl;
    const QFileInfo info(path);
    if (!info.exists()) {
        emit errorOccurred(tr("ファイルが見つかりません: %1").arg(path));
        return false;
    }
    if (!info.isFile()) {
        emit errorOccurred(tr("有効なファイルではありません: %1").arg(path));
        return false;
    }
    const QString suffix = info.suffix().toLower();
    const QSet<QString> audioExts = {QStringLiteral("wav"), QStringLiteral("mp3"), QStringLiteral("aac"), QStringLiteral("m4a"), QStringLiteral("flac"), QStringLiteral("ogg")};
    const QSet<QString> imageExts = {QStringLiteral("png"), QStringLiteral("jpg"), QStringLiteral("jpeg"), QStringLiteral("bmp"), QStringLiteral("gif"), QStringLiteral("webp"), QStringLiteral("svg")};
    if ((clip->type == QStringLiteral("audio") && !audioExts.contains(suffix)) || (clip->type == QStringLiteral("image") && !imageExts.contains(suffix)) || (clip->type == QStringLiteral("video") && !AviQtl::Core::MediaUtils::isVideoFile(path))) {
        emit errorOccurred(tr("サポートされていないファイル形式です: %1").arg(suffix));
        return false;
    }
    const auto target = findMediaEffectTarget(*clip);
    if (!target) {
        emit errorOccurred(tr("メディアエフェクトが見つかりません: %1").arg(clipId));
        return false;
    }
    updateClipEffectParam(clipId, target->index, target->parameter, path);
    refreshMissingMedia();
    return true;
}

namespace {
auto catalogItemFromMetadata(const AviQtl::Core::EffectMetadata &meta) -> QVariantMap {
    QVariantMap item;
    item.insert(QStringLiteral("id"), meta.id);
    item.insert(QStringLiteral("name"), meta.name);
    item.insert(QStringLiteral("kind"), meta.kind);
    item.insert(QStringLiteral("version"), meta.version);
    item.insert(QStringLiteral("categories"), meta.categories);
    item.insert(QStringLiteral("source"), meta.source.isEmpty() ? QStringLiteral("built-in") : meta.source);
    item.insert(QStringLiteral("packageId"), meta.packageId);
    item.insert(QStringLiteral("sourcePath"), meta.sourcePath);
    return item;
}

auto categoryMatches(const QStringList &categories, const QString &category) -> bool {
    if (category.isEmpty()) {
        return true;
    }
    const QString categoryPrefix = category + QLatin1Char('/');
    return std::ranges::any_of(categories, [&category, &categoryPrefix](const QString &candidate) {
        return candidate.compare(category, Qt::CaseInsensitive) == 0 || candidate.startsWith(categoryPrefix, Qt::CaseInsensitive);
    });
}

auto catalogMetadataMatches(const AviQtl::Core::EffectMetadata &meta, const QString &query) -> bool {
    if (query.isEmpty()) {
        return true;
    }
    const QStringList fields = {meta.name, meta.id, meta.version, meta.source.isEmpty() ? QStringLiteral("built-in") : meta.source, meta.packageId, meta.sourcePath};
    if (std::ranges::any_of(fields, [&query](const QString &field) { return field.contains(query, Qt::CaseInsensitive); })) {
        return true;
    }
    return std::ranges::any_of(meta.categories, [&query](const QString &category) { return category.contains(query, Qt::CaseInsensitive); });
}

void insertIntoCategoryTree(QVariantList &list, const QStringList &path, const QVariantMap &item) {
    if (path.isEmpty()) {
        list.append(item);
        return;
    }

    QString currentCategory = path.first();
    int foundIdx = -1;
    for (int i = 0; i < list.size(); ++i) {
        QVariantMap node = list[i].toMap();
        if (node.value(QStringLiteral("isCategory")).toBool() && node.value(QStringLiteral("title")).toString() == currentCategory) {
            foundIdx = i;
            break;
        }
    }

    QVariantMap categoryNode;
    QVariantList children;
    if (foundIdx >= 0) {
        categoryNode = list[foundIdx].toMap();
        children = categoryNode.value(QStringLiteral("children")).toList();
    } else {
        categoryNode.insert(QStringLiteral("isCategory"), true);
        categoryNode.insert(QStringLiteral("title"), currentCategory);
    }

    insertIntoCategoryTree(children, path.mid(1), item);
    categoryNode.insert(QStringLiteral("children"), children);

    if (foundIdx >= 0) {
        list[foundIdx] = categoryNode;
    } else {
        list.append(categoryNode);
    }
}
} // namespace

auto TimelineController::queryCatalog(const QString &kind, const QString &query, const QString &category) -> QVariantList {
    QVariantList result;
    const auto effects = AviQtl::Core::EffectRegistry::instance().getAllEffects();
    for (const auto &meta : effects) {
        if (meta.kind != kind || !categoryMatches(meta.categories, category) || !catalogMetadataMatches(meta, query)) {
            continue;
        }
        result.append(catalogItemFromMetadata(meta));
    }
    return result;
}

auto TimelineController::getCatalogCategories(const QString &kind) -> QStringList {
    QSet<QString> uniqueCategories;
    const auto effects = AviQtl::Core::EffectRegistry::instance().getAllEffects();
    for (const auto &meta : effects) {
        if (meta.kind != kind) {
            continue;
        }
        for (const QString &category : meta.categories) {
            if (!category.isEmpty()) {
                uniqueCategories.insert(category);
            }
        }
    }
    QStringList categories(uniqueCategories.cbegin(), uniqueCategories.cend());
    categories.sort(Qt::CaseInsensitive);
    return categories;
}

auto TimelineController::getAvailableEffects() -> QVariantList {
    QVariantList list;
    const auto effects = AviQtl::Core::EffectRegistry::instance().getAllEffects();
    for (const auto &meta : effects) {
        if (meta.kind != "effect") {
            continue;
        }
        QVariantMap m = catalogItemFromMetadata(meta);

        for (const QString &categoryPath : meta.categories) {
            QStringList pathTokens = categoryPath.split(QStringLiteral("/"), Qt::SkipEmptyParts);
            insertIntoCategoryTree(list, pathTokens, m);
        }
    }
    return list;
}

auto TimelineController::getAvailableObjects() -> QVariantList {
    QVariantList list;
    const auto effects = AviQtl::Core::EffectRegistry::instance().getAllEffects();
    QHash<QString, AviQtl::Core::EffectMetadata> objectsById;
    for (const auto &meta : effects) {
        if (meta.kind != "object") {
            continue;
        }
        objectsById.insert(meta.id, meta);
    }

    auto translatedCategory = [](const char *source) { return QCoreApplication::translate("AviQtl::Core::EffectRegistry", source); };

    auto makeItem = [&objectsById](const QString &id) -> QVariantMap {
        QVariantMap item;
        auto it = objectsById.constFind(id);
        if (it == objectsById.cend()) {
            return item;
        }
        item = catalogItemFromMetadata(*it);
        return item;
    };

    auto appendItem = [&makeItem](QVariantList &target, const QString &id, QSet<QString> &handledIds) {
        QVariantMap item = makeItem(id);
        if (item.isEmpty()) {
            return;
        }
        target.append(item);
        handledIds.insert(id);
    };

    auto appendCategory = [&list, &appendItem](const QString &title, const QStringList &ids, QSet<QString> &handledIds) {
        QVariantList children;
        for (const QString &id : ids) {
            appendItem(children, id, handledIds);
        }
        if (children.isEmpty()) {
            return;
        }
        QVariantMap categoryNode;
        categoryNode.insert(QStringLiteral("isCategory"), true);
        categoryNode.insert(QStringLiteral("title"), title);
        categoryNode.insert(QStringLiteral("children"), children);
        list.append(categoryNode);
    };

    QSet<QString> handledIds;

    appendCategory(translatedCategory("メディア"), {QStringLiteral("video"), QStringLiteral("image"), QStringLiteral("audio")}, handledIds);
    appendItem(list, QStringLiteral("text"), handledIds);
    appendItem(list, QStringLiteral("rect"), handledIds);
    appendItem(list, QStringLiteral("frame_buffer"), handledIds);
    appendItem(list, QStringLiteral("scene"), handledIds);
    appendCategory(translatedCategory("制御"), {QStringLiteral("GroupControl"), QStringLiteral("camera_control")}, handledIds);
    appendCategory(translatedCategory("カスタムオブジェクト"),
                   {QStringLiteral("radial_lines"), QStringLiteral("counter"), QStringLiteral("lens_flare_object"), QStringLiteral("star"), QStringLiteral("snow"), QStringLiteral("rain"), QStringLiteral("track_line"), QStringLiteral("pie_shape"),
                    QStringLiteral("polygon_shape"), QStringLiteral("flare")},
                   handledIds);

    for (const auto &meta : effects) {
        if (meta.kind != "object" || handledIds.contains(meta.id)) {
            continue;
        }
        QVariantMap m = catalogItemFromMetadata(meta);

        for (const QString &categoryPath : meta.categories) {
            QStringList pathTokens = categoryPath.split(QStringLiteral("/"), Qt::SkipEmptyParts);
            insertIntoCategoryTree(list, pathTokens, m);
        }
    }
    return list;
}

auto TimelineController::getClipTypeColor(const QString &type) -> QString { return AviQtl::Core::EffectRegistry::instance().getEffect(type).color; }

auto TimelineController::getAvailableTransitions() -> QVariantList {
    QVariantList list;
    const auto effects = AviQtl::Core::EffectRegistry::instance().getAllEffects();

    for (const auto &meta : effects) {
        if (meta.kind != "transition") {
            continue;
        }
        QVariantMap m = catalogItemFromMetadata(meta);

        for (const QString &categoryPath : meta.categories) {
            QStringList pathTokens = categoryPath.split(QStringLiteral("/"), Qt::SkipEmptyParts);
            insertIntoCategoryTree(list, pathTokens, m);
        }
    }
    return list;
}

auto TimelineController::getAvailableAudioPlugins() -> QVariantList { return AviQtl::Engine::Plugin::AudioPluginManager::instance().getPluginList(); }

auto TimelineController::getPluginCategories() -> QVariantList {
    // AudioPluginManagerから重複のないカテゴリ名リストを抽出
    return AviQtl::Engine::Plugin::AudioPluginManager::instance().getCategories();
}

auto TimelineController::getPluginsByCategory(const QString &category) -> QVariantList {
    // 特定カテゴリに属するプラグインのみを返す
    return AviQtl::Engine::Plugin::AudioPluginManager::instance().getPluginsInCategory(category);
}

} // namespace AviQtl::UI
