// ui/qml/common/BlendModeUtils.js
// Shared blend mode string→int mapping used by Transform.qml and BaseObject.qml.
// The int IDs must match the GLSL dispatch in blend_layer.frag / blend.glsl.
//
//  0 = 通常 (Normal)
//  1 = スクリーン (Screen)
//  2 = 乗算 (Multiply)
//  3 = オーバーレイ (Overlay)
//  4 = 加算 (Add)
//  5 = 減算 (Subtract)
//  6 = 比較（明）/ 比較明 (Lighten)
//  7 = 比較（暗）/ 比較暗 (Darken)
//  8 = 色反転 (Invert)
//  9 = ソフトライト (Soft Light)
// 10 = ハードライト (Hard Light)
// 11 = 差の絶対値 (Difference)
// 12 = 色相 (Hue)
// 13 = 彩度 (Saturation)
// 14 = カラー (Color)
// 15 = 輝度 (Luminosity)

function blendModeToInt(name) {
    switch (name) {
    case "スクリーン":    return 1;
    case "乗算":          return 2;
    case "オーバーレイ":   return 3;
    case "加算":          return 4;
    case "減算":          return 5;
    case "比較（明）":
    case "比較明":        return 6;
    case "比較（暗）":
    case "比較暗":        return 7;
    case "色反転":        return 8;
    case "ソフトライト":   return 9;
    case "ハードライト":   return 10;
    case "差の絶対値":     return 11;
    case "色相":          return 12;
    case "彩度":          return 13;
    case "カラー":        return 14;
    case "輝度":          return 15;
    default:              return 0; // 通常 (Normal)
    }
}
