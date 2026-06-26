#version 440
layout(location=0) in vec2 qt_TexCoord0;
layout(location=0) out vec4 fragColor;
layout(std140, binding=0) uniform buf {
    mat4  qt_Matrix;
    float qt_Opacity;
    float embossW;
    float embossH;
    float angle;
    float strength;
    float texelW;
    float texelH;
};
layout(binding=1) uniform sampler2D source;

void main() {
    float rad = radians(angle);
    vec2 dir = vec2(cos(rad), sin(rad));
    vec2 offset = vec2(texelW * embossW, texelH * embossH) * dir;

    vec4 light = texture(source, qt_TexCoord0 + offset);
    vec4 dark  = texture(source, qt_TexCoord0 - offset);

    vec3 diff = light.rgb - dark.rgb;
    float val = 0.5 + (diff.r + diff.g + diff.b) * 0.333 * strength;

    vec4 col = texture(source, qt_TexCoord0);
    vec3 result = mix(col.rgb, vec3(val), strength);

    fragColor = vec4(result, col.a) * qt_Opacity;
}
