#include "script_params.hpp"
#include "rust_script_document.hpp"
#include <QStringList>
#include <utility>

namespace AviQtl::Scripting {

namespace {
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
} // namespace

ScriptMetadata ScriptParamParser::parse(const QString &scriptContent) {
    const auto metadata = AviQtl::RustCore::Script::parse(scriptContent);
    return metadata.has_value() ? metadataFromMap(*metadata) : ScriptMetadata{};
}

ScriptMetadata ScriptParamParser::parseHeader(const QStringList &lines) { return parse(lines.join(QLatin1Char('\n'))); }

} // namespace AviQtl::Scripting
