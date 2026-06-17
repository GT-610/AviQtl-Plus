#version 440
layout(location=0) in vec2 qt_TexCoord0;
layout(location=0) out vec4 fragColor;
layout(std140, binding=0) uniform buf {
    mat4  qt_Matrix;
    float qt_Opacity;
    float transparency;
    float decay;
    int   direction;
    float centerOffset;
};
layout(binding=1) uniform sampler2D source;

void main() {
    vec2 uv = qt_TexCoord0;
    vec2 mirrored = uv;
    float dist = 0.0;

    if (direction == 0) {
        float center = 0.5 + centerOffset * 0.5;
        mirrored.x = 2.0 * center - uv.x;
        dist = abs(uv.x - center);
    } else if (direction == 1) {
        float center = 0.5 + centerOffset * 0.5;
        mirrored.y = 2.0 * center - uv.y;
        dist = abs(uv.y - center);
    } else if (direction == 2) {
        float center = 0.5 + centerOffset * 0.5;
        mirrored.x = 2.0 * center - uv.x;
        dist = abs(uv.x - center);
    } else {
        float center = 0.5 + centerOffset * 0.5;
        mirrored.y = 2.0 * center - uv.y;
        dist = abs(uv.y - center);
    }

    vec4 col = texture(source, uv);
    vec4 mirroredCol = texture(source, mirrored);

    float mirrorAlpha = transparency * (1.0 - decay * dist * 2.0);
    mirrorAlpha = max(0.0, mirrorAlpha);

    bool inMirrorZone = false;
    if (direction == 0) inMirrorZone = uv.x > 0.5 + centerOffset * 0.5;
    else if (direction == 1) inMirrorZone = uv.y > 0.5 + centerOffset * 0.5;
    else if (direction == 2) inMirrorZone = uv.x < 0.5 + centerOffset * 0.5;
    else inMirrorZone = uv.y < 0.5 + centerOffset * 0.5;

    if (inMirrorZone) {
        vec4 m = mirroredCol;
        m.a *= mirrorAlpha;
        vec3 result = mix(col.rgb, m.rgb, m.a);
        float outA = max(col.a, m.a);
        fragColor = vec4(result, outA) * qt_Opacity;
    } else {
        fragColor = col * qt_Opacity;
    }
}
