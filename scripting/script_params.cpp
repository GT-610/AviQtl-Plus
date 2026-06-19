#include "script_params.hpp"
#include <QRegularExpression>
#include <QStringList>

namespace AviQtl::Scripting {

ScriptMetadata ScriptParamParser::parse(const QString &scriptContent) {
    QStringList lines = scriptContent.split('\n');
    return parseHeader(lines);
}

ScriptMetadata ScriptParamParser::parseHeader(const QStringList &lines) {
    ScriptMetadata meta;
    int groupStart = -1;
    QString currentGroup;

    for (int i = 0; i < lines.size(); ++i) {
        const QString &line = lines[i].trimmed();

        if (line.isEmpty()) {
            continue;
        }

        // Stop at first non-comment line (except for block comments)
        if (!line.startsWith(QStringLiteral("--"))) {
            // Allow block comments --[[ ... ]]
            if (!line.startsWith(QStringLiteral("--[["))) {
                break;
            }
        }

        // --information:label
        if (line.startsWith(QStringLiteral("--information:"))) {
            meta.information = line.mid(14).trimmed();
            continue;
        }

        // --script:type
        if (line.startsWith(QStringLiteral("--script:"))) {
            meta.scriptType = line.mid(10).trimmed().toLower();
            continue;
        }

        // --require:version
        if (line.startsWith(QStringLiteral("--require:"))) {
            bool ok;
            meta.requireVersion = line.mid(11).trimmed().toInt(&ok);
            continue;
        }

        // --filter
        if (line == QStringLiteral("--filter")) {
            meta.isFilter = true;
            continue;
        }

        // --label:label
        if (line.startsWith(QStringLiteral("--label:"))) {
            meta.label = line.mid(9).trimmed();
            continue;
        }

        // --group:groupname,default_expanded
        if (line.startsWith(QStringLiteral("--group:"))) {
            QString def = line.mid(8).trimmed();
            if (def.isEmpty()) {
                // End of group
                currentGroup.clear();
            } else {
                QStringList parts = def.split(',');
                currentGroup = parts[0].trimmed();
                ScriptGroup group;
                group.name = currentGroup;
                group.defaultExpanded = (parts.size() > 1) ? (parts[1].trimmed().toLower() == "true") : true;
                meta.groups.append(group);
            }
            continue;
        }

        // --separator:label
        if (line.startsWith(QStringLiteral("--separator:"))) {
            // Separators are visual-only, skip for now
            continue;
        }

        // Parse parameter definitions
        ScriptParam param;
        bool parsed = false;

        if (line.startsWith(QStringLiteral("--track@"))) {
            param = parseTrack(line.mid(8));
            parsed = true;
        } else if (line.startsWith(QStringLiteral("--check@"))) {
            param = parseCheck(line.mid(8));
            parsed = true;
        } else if (line.startsWith(QStringLiteral("--color@"))) {
            param = parseColor(line.mid(8));
            parsed = true;
        } else if (line.startsWith(QStringLiteral("--select@"))) {
            param = parseSelect(line.mid(9));
            parsed = true;
        } else if (line.startsWith(QStringLiteral("--text@"))) {
            param = parseText(line.mid(7));
            parsed = true;
        } else if (line.startsWith(QStringLiteral("--string@"))) {
            param = parseText(line.mid(9));
            param.type = ScriptParamType::String;
            parsed = true;
        } else if (line.startsWith(QStringLiteral("--file@"))) {
            param = parseFile(line.mid(7));
            parsed = true;
        } else if (line.startsWith(QStringLiteral("--folder@"))) {
            param = parseFolder(line.mid(9));
            parsed = true;
        } else if (line.startsWith(QStringLiteral("--value@"))) {
            param = parseValue(line.mid(8));
            parsed = true;
        }

        if (parsed && !param.varName.isEmpty()) {
            param.groupName = currentGroup;
            meta.params.append(param);

            // Also add to group if active
            if (!currentGroup.isEmpty()) {
                for (auto &group : meta.groups) {
                    if (group.name == currentGroup) {
                        group.params.append(param);
                        break;
                    }
                }
            }
        }
    }

    return meta;
}

ScriptParam ScriptParamParser::parseTrack(const QString &def) {
    // Format: varname:label,min,max,default[,step,unit]
    ScriptParam param;
    param.type = ScriptParamType::Track;

    QStringList parts = def.split(',');
    if (parts.size() < 4) return param;

    // First part contains varname:label
    QString varLabel = parts[0];
    int colonIdx = varLabel.indexOf(':');
    if (colonIdx > 0) {
        param.varName = varLabel.left(colonIdx).trimmed();
        param.label = varLabel.mid(colonIdx + 1).trimmed();
    } else {
        param.varName = varLabel.trimmed();
        param.label = param.varName;
    }

    bool ok;
    param.minValue = parts[1].trimmed().toDouble(&ok);
    param.maxValue = parts[2].trimmed().toDouble(&ok);
    param.defaultValue = parts[3].trimmed().toDouble(&ok);

    if (parts.size() > 4) {
        param.step = parts[4].trimmed().toDouble(&ok);
    } else {
        // Auto-detect step based on range
        double range = param.maxValue.toDouble() - param.minValue.toDouble();
        if (range <= 10) param.step = 0.01;
        else if (range <= 100) param.step = 0.1;
        else param.step = 1.0;
    }

    return param;
}

ScriptParam ScriptParamParser::parseCheck(const QString &def) {
    // Format: varname:label,default
    ScriptParam param;
    param.type = ScriptParamType::Check;

    int colonIdx = def.indexOf(':');
    if (colonIdx < 0) return param;

    param.varName = def.left(colonIdx).trimmed();
    QString rest = def.mid(colonIdx + 1).trimmed();

    int commaIdx = rest.lastIndexOf(',');
    if (commaIdx > 0) {
        param.label = rest.left(commaIdx).trimmed();
        QString defaultVal = rest.mid(commaIdx + 1).trimmed().toLower();
        param.defaultValue = (defaultVal == "true" || defaultVal == "1");
    } else {
        param.label = rest;
        param.defaultValue = false;
    }

    return param;
}

ScriptParam ScriptParamParser::parseColor(const QString &def) {
    // Format: varname:label,default
    ScriptParam param;
    param.type = ScriptParamType::Color;

    int colonIdx = def.indexOf(':');
    if (colonIdx < 0) return param;

    param.varName = def.left(colonIdx).trimmed();
    QString rest = def.mid(colonIdx + 1).trimmed();

    int commaIdx = rest.lastIndexOf(',');
    if (commaIdx > 0) {
        param.label = rest.left(commaIdx).trimmed();
        QString colorStr = rest.mid(commaIdx + 1).trimmed();
        bool ok;
        param.defaultValue = colorStr.toUInt(&ok, 16);
    } else {
        param.label = rest;
        param.defaultValue = 0xffffff;
    }

    return param;
}

ScriptParam ScriptParamParser::parseSelect(const QString &def) {
    // Format: varname:label=default,option1=value1,option2=value2
    ScriptParam param;
    param.type = ScriptParamType::Select;

    int colonIdx = def.indexOf(':');
    if (colonIdx < 0) return param;

    param.varName = def.left(colonIdx).trimmed();
    QString rest = def.mid(colonIdx + 1).trimmed();

    QString defaultToken;

    // Parse label=default
    int eqIdx = rest.indexOf('=');
    int firstComma = rest.indexOf(',');
    if (eqIdx > 0 && (firstComma < 0 || eqIdx < firstComma)) {
        param.label = rest.left(eqIdx).trimmed();
        defaultToken = rest.mid(eqIdx + 1, firstComma > 0 ? firstComma - eqIdx - 1 : -1).trimmed();
        rest = firstComma > 0 ? rest.mid(firstComma + 1).trimmed() : QString();
    }

    auto parseOptionValue = [](const QString &text) -> QVariant {
        bool ok = false;
        int intValue = text.toInt(&ok);
        if (ok) return intValue;
        double doubleValue = text.toDouble(&ok);
        if (ok) return doubleValue;
        if (text.compare(QStringLiteral("true"), Qt::CaseInsensitive) == 0) return true;
        if (text.compare(QStringLiteral("false"), Qt::CaseInsensitive) == 0) return false;
        return text;
    };

    // Parse options
    QStringList optionParts = rest.split(',');
    bool isFirst = true;
    for (const QString &opt : optionParts) {
        int optEq = opt.indexOf('=');
        if (optEq > 0) {
            ScriptParamOption option;
            option.label = opt.left(optEq).trimmed();
            const QString valueText = opt.mid(optEq + 1).trimmed();
            option.value = parseOptionValue(valueText);
            param.options.append(option);

            if (!defaultToken.isEmpty() && (defaultToken == option.label || defaultToken == valueText)) {
                param.defaultValue = option.value;
            } else if (isFirst && defaultToken.isEmpty()) {
                param.defaultValue = option.value;
            }
            isFirst = false;
        }
    }

    return param;
}

ScriptParam ScriptParamParser::parseText(const QString &def) {
    // Format: varname:label,default
    ScriptParam param;
    param.type = ScriptParamType::Text;

    int colonIdx = def.indexOf(':');
    if (colonIdx < 0) return param;

    param.varName = def.left(colonIdx).trimmed();
    QString rest = def.mid(colonIdx + 1).trimmed();

    int commaIdx = rest.lastIndexOf(',');
    if (commaIdx > 0) {
        param.label = rest.left(commaIdx).trimmed();
        param.defaultValue = rest.mid(commaIdx + 1).trimmed();
    } else {
        param.label = rest;
        param.defaultValue = QString();
    }

    return param;
}

ScriptParam ScriptParamParser::parseFile(const QString &def) {
    // Format: varname:label
    ScriptParam param;
    param.type = ScriptParamType::File;

    int colonIdx = def.indexOf(':');
    if (colonIdx > 0) {
        param.varName = def.left(colonIdx).trimmed();
        param.label = def.mid(colonIdx + 1).trimmed();
    } else {
        param.varName = def.trimmed();
        param.label = param.varName;
    }

    return param;
}

ScriptParam ScriptParamParser::parseFolder(const QString &def) {
    // Format: varname:label
    ScriptParam param;
    param.type = ScriptParamType::Folder;

    int colonIdx = def.indexOf(':');
    if (colonIdx > 0) {
        param.varName = def.left(colonIdx).trimmed();
        param.label = def.mid(colonIdx + 1).trimmed();
    } else {
        param.varName = def.trimmed();
        param.label = param.varName;
    }

    return param;
}

ScriptParam ScriptParamParser::parseValue(const QString &def) {
    // Format: varname:label,default
    ScriptParam param;
    param.type = ScriptParamType::Value;

    int colonIdx = def.indexOf(':');
    if (colonIdx < 0) return param;

    param.varName = def.left(colonIdx).trimmed();
    QString rest = def.mid(colonIdx + 1).trimmed();

    int commaIdx = rest.lastIndexOf(',');
    if (commaIdx > 0) {
        param.label = rest.left(commaIdx).trimmed();
        param.defaultValue = rest.mid(commaIdx + 1).trimmed();
    } else {
        param.label = rest;
        param.defaultValue = QVariant();
    }

    return param;
}

} // namespace AviQtl::Scripting
