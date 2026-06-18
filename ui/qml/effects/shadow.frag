#version 440
layout(location=0) in vec2 qt_TexCoord0;
layout(location=0) out vec4 fragColor;
layout(std140, binding=0) uniform buf {
    mat4  qt_Matrix;
    float qt_Opacity;
    float offsetX;
    float offsetY;
    float shadowOpacity;
    float diffusion;
    float shadowColor_r;
    float shadowColor_g;
    float shadowColor_b;
    float texelW;
    float texelH;
};
layout(binding=1) uniform sampler2D source;

void main() {
    vec4 col = texture(source, qt_TexCoord0);

    vec2 shift = vec2(offsetX * texelW, offsetY * texelH);

    if (diffusion < 0.001) {
        vec4 s = texture(source, qt_TexCoord0 - shift);
        float sa = s.a * shadowOpacity;
        vec3 sc = vec3(shadowColor_r, shadowColor_g, shadowColor_b);
        vec3 result = mix(sc * sa, col.rgb, col.a);
        float outA = max(col.a, sa);
        fragColor = vec4(result, outA) * qt_Opacity;
        return;
    }

    float total = 0.0;
    vec3 sum = vec3(0.0);
    int radius = int(max(1.0, diffusion * 15.0));

    for (int x = -3; x <= 3; x++) {
        for (int y = -3; y <= 3; y++) {
            if (x * x + y * y > radius * radius) continue;
            vec2 off = vec2(float(x) * texelW * diffusion * 2.0,
                           float(y) * texelH * diffusion * 2.0);
            vec4 s = texture(source, qt_TexCoord0 - shift + off);
            sum += s.rgb * s.a;
            total += s.a;
        }
    }

    vec3 shadowBase = (total > 0.0) ? sum / total : vec3(0.0);
    float shadowAlpha = (total / float((2 * radius + 1) * (2 * radius + 1))) * shadowOpacity;
    vec3 sc = vec3(shadowColor_r, shadowColor_g, shadowColor_b) * shadowAlpha;
    vec3 result = mix(sc, col.rgb, col.a);
    float outA = max(col.a, shadowAlpha);
    fragColor = vec4(result, outA) * qt_Opacity;
}
