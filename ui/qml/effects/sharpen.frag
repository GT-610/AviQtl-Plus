#version 440
layout(location=0) in vec2 qt_TexCoord0;
layout(location=0) out vec4 fragColor;
layout(std140, binding=0) uniform buf {
    mat4  qt_Matrix;
    float qt_Opacity;
    float strength;
    float range;
    float texelW;
    float texelH;
};
layout(binding=1) uniform sampler2D source;

void main() {
    vec2 dx = vec2(texelW * range, 0.0);
    vec2 dy = vec2(0.0, texelH * range);

    vec4 center = texture(source, qt_TexCoord0);
    vec4 top    = texture(source, qt_TexCoord0 - dy);
    vec4 bottom = texture(source, qt_TexCoord0 + dy);
    vec4 left   = texture(source, qt_TexCoord0 - dx);
    vec4 right  = texture(source, qt_TexCoord0 + dx);

    vec4 sharpened = center * (1.0 + 4.0 * strength) - (top + bottom + left + right) * strength;

    fragColor = clamp(sharpened, 0.0, 1.0) * qt_Opacity;
}
