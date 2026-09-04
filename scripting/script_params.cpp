#include "script_params.hpp"
#include "rust_script_document.hpp"
#include <QStringList>
#include <utility>

namespace AviQtl::Scripting {

namespace {
auto parameterTypeName(ScriptParamType type) -> QString {
    switch (type) {
    case ScriptParamType::Track:
        return QStringLiteral("track");
    case ScriptParamType::Check:
        return QStringLiteral("check");
    case ScriptParamType::Color:
        return QStringLiteral("color");
    case ScriptParamType::Select:
        return QStringLiteral("select");
    case ScriptParamType::Text:
        return QStringLiteral("text");
    case ScriptParamType::String:
        return QStringLiteral("string");
    case ScriptParamType::File:
        return QStringLiteral("file");
    case ScriptParamType::Folder:
        return QStringLiteral("folder");
    case ScriptParamType::Value:
        return QStringLiteral("value");
    }
    return QStringLiteral("track");
}

auto parameterType(const QString &value) -> ScriptParamType {
    if (value == QStringLiteral("check"))
        return ScriptParamType::Check;
    if (value == QStringLiteral("color"))
        return ScriptParamType::Color;
    if (value == QStringLiteral("select"))
        return ScriptParamType::Select;
    if (value == QStringLiteral("text"))
        return ScriptParamType::Text;
    if (value == QStringLiteral("string"))
        return ScriptParamType::String;
    if (value == QStringLiteral("file"))
        return ScriptParamType::File;
    if (value == QStringLiteral("folder"))
        return ScriptParamType::Folder;
    if (value == QStringLiteral("value"))
        return ScriptParamType::Value;
    return ScriptParamType::Track;
}

auto optionalValue(const QVariantMap &value, const QString &key) -> QVariant {
    const QVariant result = value.value(key);
    return result.isNull() ? QVariant{} : result;
}

auto parameterFromMap(const QVariantMap &value) -> ScriptParam {
    ScriptParam parameter;
    parameter.type = parameterType(value.value(QStringLiteral("type")).toString());
    parameter.varName = value.value(QStringLiteral("varName")).toString();
    parameter.label = value.value(QStringLiteral("label")).toString();
    parameter.defaultValue = optionalValue(value, QStringLiteral("defaultValue"));
    parameter.minValue = optionalValue(value, QStringLiteral("minValue"));
    parameter.maxValue = optionalValue(value, QStringLiteral("maxValue"));
    parameter.step = optionalValue(value, QStringLiteral("step"));
    parameter.groupName = value.value(QStringLiteral("groupName")).toString();
    parameter.isSectionCheck = value.value(QStringLiteral("isSectionCheck")).toBool();
    for (const QVariant &entry : value.value(QStringLiteral("options")).toList()) {
        const QVariantMap option = entry.toMap();
        parameter.options.append({option.value(QStringLiteral("label")).toString(), optionalValue(option, QStringLiteral("value"))});
    }
    return parameter;
}

auto parameterToMap(const ScriptParam &parameter) -> QVariantMap {
    QVariantList options;
    options.reserve(parameter.options.size());
    for (const ScriptParamOption &option : parameter.options) {
        options.append(QVariantMap{
            {QStringLiteral("label"), option.label},
            {QStringLiteral("value"), option.value},
        });
    }
    return {
        {QStringLiteral("type"), parameterTypeName(parameter.type)},
        {QStringLiteral("varName"), parameter.varName},
        {QStringLiteral("label"), parameter.label},
        {QStringLiteral("defaultValue"), parameter.defaultValue},
        {QStringLiteral("minValue"), parameter.minValue},
        {QStringLiteral("maxValue"), parameter.maxValue},
        {QStringLiteral("step"), parameter.step},
        {QStringLiteral("options"), options},
        {QStringLiteral("groupName"), parameter.groupName},
        {QStringLiteral("isSectionCheck"), parameter.isSectionCheck},
    };
}

auto metadataFromMap(const QVariantMap &value) -> ScriptMetadata {
    ScriptMetadata metadata;
    metadata.information = value.value(QStringLiteral("information")).toString();
    metadata.scriptType = value.value(QStringLiteral("scriptType")).toString();
    metadata.requireVersion = value.value(QStringLiteral("requireVersion")).toInt();
    metadata.isFilter = value.value(QStringLiteral("isFilter")).toBool();
    metadata.label = value.value(QStringLiteral("label")).toString();
    for (const QVariant &entry : value.value(QStringLiteral("params")).toList())
        metadata.params.append(parameterFromMap(entry.toMap()));
    for (const QVariant &entry : value.value(QStringLiteral("groups")).toList()) {
        const QVariantMap groupValue = entry.toMap();
        ScriptGroup group;
        group.name = groupValue.value(QStringLiteral("name")).toString();
        group.defaultExpanded = groupValue.value(QStringLiteral("defaultExpanded"), true).toBool();
        for (const QVariant &parameter : groupValue.value(QStringLiteral("params")).toList())
            group.params.append(parameterFromMap(parameter.toMap()));
        metadata.groups.append(std::move(group));
    }
    return metadata;
}

auto metadataToMap(const ScriptMetadata &metadata) -> QVariantMap {
    QVariantList parameters;
    parameters.reserve(metadata.params.size());
    for (const ScriptParam &parameter : metadata.params)
        parameters.append(parameterToMap(parameter));

    QVariantList groups;
    groups.reserve(metadata.groups.size());
    for (const ScriptGroup &group : metadata.groups) {
        QVariantList groupParameters;
        groupParameters.reserve(group.params.size());
        for (const ScriptParam &parameter : group.params)
            groupParameters.append(parameterToMap(parameter));
        groups.append(QVariantMap{
            {QStringLiteral("name"), group.name},
            {QStringLiteral("defaultExpanded"), group.defaultExpanded},
            {QStringLiteral("params"), groupParameters},
        });
    }

    return {
        {QStringLiteral("information"), metadata.information},
        {QStringLiteral("scriptType"), metadata.scriptType},
        {QStringLiteral("requireVersion"), metadata.requireVersion},
        {QStringLiteral("isFilter"), metadata.isFilter},
        {QStringLiteral("label"), metadata.label},
        {QStringLiteral("params"), parameters},
        {QStringLiteral("groups"), groups},
    };
}
} // namespace

ScriptMetadata ScriptParamParser::parse(const QString &scriptContent) {
    const auto metadata = AviQtl::RustCore::Script::parse(scriptContent);
    return metadata.has_value() ? fromVariantMap(*metadata) : ScriptMetadata{};
}

ScriptMetadata ScriptParamParser::parseHeader(const QStringList &lines) { return parse(lines.join(QLatin1Char('\n'))); }

QVariantMap ScriptParamParser::toVariantMap(const ScriptMetadata &metadata) {
    return metadataToMap(metadata);
}

ScriptMetadata ScriptParamParser::fromVariantMap(const QVariantMap &metadata) {
    return metadataFromMap(metadata);
}

} // namespace AviQtl::Scripting
