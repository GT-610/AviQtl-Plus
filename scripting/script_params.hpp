#pragma once
#include <QList>
#include <QString>
#include <QVariant>

namespace AviQtl::Scripting {

enum class ScriptParamType {
    Track,      // --track@var:label,min,max,default[,step]
    Check,      // --check@var:label,default
    Color,      // --color@var:label,default
    Select,     // --select@var:label=default,opt1=val1,...
    Text,       // --text@var:label,default
    String,     // --string@var:label,default (single line)
    File,       // --file@var:label
    Folder,     // --folder@var:label
    Value,      // --value@var:label,default
};

struct ScriptParamOption {
    QString label;
    QVariant value;
};

struct ScriptParam {
    ScriptParamType type;
    QString varName;     // Lua variable name
    QString label;       // Display label
    QVariant defaultValue;
    QVariant minValue;   // For Track
    QVariant maxValue;   // For Track
    QVariant step;       // For Track (0.01, 0.1, 1)
    QList<ScriptParamOption> options; // For Select
    QString groupName;   // For grouping
    bool isSectionCheck = false; // For checksection
};

struct ScriptGroup {
    QString name;
    bool defaultExpanded = true;
    QList<ScriptParam> params;
};

struct ScriptMetadata {
    QString information;  // --information:label
    QString scriptType;   // --script:lua or --script:luajit
    int requireVersion = 0; // --require:version
    bool isFilter = false;  // --filter
    QString label;        // --label:label
    QList<ScriptParam> params;
    QList<ScriptGroup> groups;
};

class ScriptParamParser {
  public:
    static ScriptMetadata parse(const QString &scriptContent);
    static ScriptMetadata parseHeader(const QStringList &lines);

  private:
    static ScriptParam parseTrack(const QString &def);
    static ScriptParam parseCheck(const QString &def);
    static ScriptParam parseColor(const QString &def);
    static ScriptParam parseSelect(const QString &def);
    static ScriptParam parseText(const QString &def);
    static ScriptParam parseFile(const QString &def);
    static ScriptParam parseFolder(const QString &def);
    static ScriptParam parseValue(const QString &def);
};

} // namespace AviQtl::Scripting
