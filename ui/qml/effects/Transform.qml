import QtQuick
import QtQuick3D
import "qrc:/qt/qml/AviQtl/ui/qml/common" as Common

Common.BaseEffect {
    id: root

    readonly property vector3d outputPosition: {
        const x = evalNumber("x", 0);
        const y = evalNumber("y", 0);
        const z = evalNumber("z", 0);
        return Qt.vector3d(x, y, z);
    }
    readonly property vector3d outputRotation: {
        const rx = evalNumber("rotationX", 0);
        const ry = evalNumber("rotationY", 0);
        const rz = evalNumber("rotationZ", 0);
        return Qt.vector3d(rx, ry, -rz);
    }
    readonly property vector3d outputPivot: {
        const cx = evalNumber("cx", 0);
        const cy = evalNumber("cy", 0);
        const cz = evalNumber("cz", 0);
        return Qt.vector3d(cx, cy, cz);
    }
    readonly property int outputCullMode: {
        return evalParam("backfaceVisible", true) ? DefaultMaterial.NoCulling : DefaultMaterial.BackFaceCulling;
    }
}
