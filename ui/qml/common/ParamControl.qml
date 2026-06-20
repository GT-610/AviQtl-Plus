import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

// AviUtl風の中間点対応パラメータコントロール
RowLayout {
    id: root

    property string paramName: qsTr("パラメータ")
    property real minValue: 0
    property real maxValue: 100
    property real startValue: 0
    property real endValue: 0
    property int decimals: 0
    property string interpolationType: "constant"
    property bool isRangeMode: Math.abs(startValue - endValue) > 0.001
    property bool interpolationSelected: interpolationType !== "" && interpolationType !== "constant"
    property bool rightLinked: !interpolationSelected
    property bool rightInteractive: root.enabled && root.isRangeMode && root.interpolationSelected

    signal startValueModified(real value)
    signal endValueModified(real value)
    signal paramButtonClicked()

    property var _lastLeftSliderValue: undefined
    property var _lastRightSliderValue: undefined

    function formatValue(val) {
        var num = Number(val);
        if (isNaN(num))
            num = 0;

        return root.decimals === 0 ? num.toFixed(0) : num.toFixed(root.decimals);
    }

    function syncRightDisplay(val) {
        var text = formatValue(val);
        if (rightValueField.text !== text)
            rightValueField.text = text;

    }

    function clearFieldFocus(field) {
        if (field && field.activeFocus) {
            field.ignoreNextEditingFinished = true;
            field.focus = false;
        }
    }

    function ignoreEditingFinished(field) {
        if (!field || !field.ignoreNextEditingFinished)
            return false;

        field.ignoreNextEditingFinished = false;
        return true;
    }

    function normalizeSliderValue(val) {
        return root.decimals === 0 ? Math.round(val) : Number(val);
    }

    function sameValue(a, b) {
        if (a === undefined || b === undefined)
            return false;

        var lhs = Number(a);
        var rhs = Number(b);
        if (isNaN(lhs) || isNaN(rhs))
            return false;

        var tolerance = root.decimals === 0 ? 0.5 : Math.pow(10, -root.decimals) / 2;
        return Math.abs(lhs - rhs) < tolerance;
    }

    function pushLeftValue(val) {
        var text = formatValue(val);
        if (leftValueField.text !== text)
            leftValueField.text = text;

        root.startValueModified(val);
        if (root.rightLinked) {
            syncRightDisplay(val);
            root.endValueModified(val);
        }
    }

    function commitLeftSliderValue(val) {
        var normalized = normalizeSliderValue(val);
        root._lastLeftSliderValue = normalized;
        root.pushLeftValue(normalized);
    }

    function commitRightSliderValue(val) {
        var normalized = normalizeSliderValue(val);
        root._lastRightSliderValue = normalized;
        rightValueField.text = root.formatValue(normalized);
        root.endValueModified(normalized);
    }

    spacing: 8
    Component.onCompleted: {
        if (root.rightLinked)
            syncRightDisplay(root.startValue);

    }
    onStartValueChanged: {
        var text = formatValue(root.startValue);
        if (leftValueField.text !== text)
            leftValueField.text = text;

        if (root.rightLinked)
            syncRightDisplay(root.startValue);

    }
    onEndValueChanged: {
        if (root.rightLinked)
            return ;

        var text = formatValue(root.endValue);
        if (rightValueField.text !== text)
            rightValueField.text = text;

    }
    onInterpolationSelectedChanged: {
        if (root.rightLinked) {
            syncRightDisplay(root.startValue);
        } else {
            var text = formatValue(root.endValue);
            if (rightValueField.text !== text)
                rightValueField.text = text;

        }
    }

    // 左側スライダー（ボックスに追従）
    Slider {
        id: leftSlider

        Layout.fillWidth: true
        Layout.preferredWidth: 120
        from: root.minValue
        to: root.maxValue
        enabled: root.enabled
        Accessible.name: root.paramName + qsTr(" スライダー")
        Accessible.description: qsTr("現在値: ") + root.formatValue(value) + qsTr("、範囲: ") + root.formatValue(root.minValue) + "～" + root.formatValue(root.maxValue)
        value: {
            var val = parseFloat(leftValueField.text);
            return isNaN(val) ? root.startValue : val;
        }
        onMoved: {
            root.commitLeftSliderValue(value);
        }
        onPressedChanged: {
            if (!pressed) {
                var val = parseFloat(leftValueField.text);
                if (!isNaN(val) && !root.sameValue(val, root._lastLeftSliderValue))
                    root.pushLeftValue(val);

                root._lastLeftSliderValue = undefined;
            }
        }
    }

    // 左側数値ボックス（親）
    TextField {
        id: leftValueField

        property bool ignoreNextEditingFinished: false

        Layout.preferredWidth: 70
        text: root.formatValue(root.startValue)
        horizontalAlignment: TextInput.AlignHCenter
        selectByMouse: true
        enabled: root.enabled
        Accessible.name: root.paramName + qsTr(" 数値入力")
        Accessible.description: qsTr("パラメータの数値を直接入力します")
        onEditingFinished: {
            if (root.ignoreEditingFinished(leftValueField))
                return ;

            var newVal = parseFloat(text);
            if (!isNaN(newVal))
                root.pushLeftValue(newVal);

            root.clearFieldFocus(leftValueField);
        }

        validator: DoubleValidator {
            decimals: root.decimals
            notation: DoubleValidator.StandardNotation
        }

    }

    // 中央ボタン
    Button {
        id: paramButton

        Layout.preferredWidth: 100
        text: root.paramName
        enabled: root.enabled
        Accessible.name: root.paramName
        Accessible.description: qsTr("イージング設定を開きます")
        onClicked: root.paramButtonClicked()
    }

    // 右側数値ボックス
    TextField {
        id: rightValueField

        property bool ignoreNextEditingFinished: false

        Layout.preferredWidth: 70
        text: root.formatValue(root.rightLinked ? root.startValue : root.endValue)
        horizontalAlignment: TextInput.AlignHCenter
        selectByMouse: true
        enabled: root.rightInteractive
        opacity: root.rightInteractive ? 1 : 0.45
        Accessible.name: root.paramName + qsTr(" 終了値入力")
        Accessible.description: qsTr("終了点の数値を入力します (キーフレーム使用時)")
        onEditingFinished: {
            if (root.ignoreEditingFinished(rightValueField))
                return ;

            var newVal = parseFloat(text);
            if (!isNaN(newVal))
                root.endValueModified(newVal);

            root.clearFieldFocus(rightValueField);
        }

        validator: DoubleValidator {
            decimals: root.decimals
            notation: DoubleValidator.StandardNotation
        }

    }

    // 右側スライダー（ボックスに追従）
    Slider {
        id: rightSlider

        Layout.fillWidth: true
        Layout.preferredWidth: 120
        from: root.minValue
        to: root.maxValue
        enabled: root.rightInteractive
        opacity: root.rightInteractive ? 1 : 0.45
        // ボックスの値をバインディング
        value: {
            var val = parseFloat(rightValueField.text);
            return isNaN(val) ? (root.rightLinked ? root.startValue : root.endValue) : val;
        }
        onMoved: {
            root.commitRightSliderValue(value);
        }
        onPressedChanged: {
            if (!pressed) {
                var val = parseFloat(rightValueField.text);
                if (!isNaN(val) && !root.sameValue(val, root._lastRightSliderValue))
                    root.endValueModified(val);

                root._lastRightSliderValue = undefined;
            }
        }
    }

}
