#version 440
layout(location=0) in vec2 qt_TexCoord0;
layout(location=0) out vec4 fragColor;
layout(std140, binding=0) uniform buf {
    mat4  qt_Matrix;
    float qt_Opacity;
    float strength;
    vec3 monoColor;
    float preserveLuma;
};
layout(binding=1) uniform sampler2D source;

void main() {
    vec4 col = texture(source, qt_TexCoord0);
    float luma = dot(col.rgb, vec3(0.299, 0.587, 0.114));

    vec3 gray;
    if (preserveLuma > 0.5) {
        gray = vec3(luma);
    } else {
        float baseLuma = dot(monoColor, vec3(0.299, 0.587, 0.114));
        gray = (baseLuma > 0.001)
            ? monoColor * (luma / baseLuma)
            : vec3(luma);
    }

    vec3 result = mix(col.rgb, gray, strength);
    fragColor = vec4(result, col.a) * qt_Opacity;
}
