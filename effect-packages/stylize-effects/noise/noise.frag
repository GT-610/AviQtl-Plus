#version 440
layout(location=0) in vec2 qt_TexCoord0;
layout(location=0) out vec4 fragColor;
layout(std140, binding=0) uniform buf {
    mat4  qt_Matrix;
    float qt_Opacity;
    float strength;
    float seed;
    float time;
};
layout(binding=1) uniform sampler2D source;

float hash(vec2 p) {
    vec3 p3 = fract(vec3(p.xyx) * 0.1031 + seed * 0.01);
    p3 += dot(p3, p3.yzx + 33.33);
    return fract((p3.x + p3.y) * p3.z);
}

void main() {
    vec4 col = texture(source, qt_TexCoord0);
    vec2 uv = qt_TexCoord0;

    float n = hash(floor(uv * 500.0 + time * 50.0)) * 2.0 - 1.0;
    vec3 result = col.rgb + n * strength;

    fragColor = vec4(clamp(result, 0.0, 1.0), col.a) * qt_Opacity;
}
