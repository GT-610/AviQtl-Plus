#version 440
layout(location=0) in vec2 qt_TexCoord0;
layout(location=0) out vec4 fragColor;
layout(std140, binding=0) uniform buf {
    mat4  qt_Matrix;
    float qt_Opacity;
    float strength;
    float diffusion;
    float threshold;
    float glowColor_r;
    float glowColor_g;
    float glowColor_b;
    float useCustomColor;
    float texelW;
    float texelH;
};
layout(binding=1) uniform sampler2D source;

void main() {
    vec4 col = texture(source, qt_TexCoord0);
    float luma = dot(col.rgb, vec3(0.299, 0.587, 0.114));
    float glowMask = smoothstep(threshold - 0.05, threshold + 0.05, luma);

    vec3 sum = vec3(0.0);
    float total = 0.0;
    int radius = int(max(1.0, diffusion * 10.0));

    for (int x = -4; x <= 4; x++) {
        for (int y = -4; y <= 4; y++) {
            if (x * x + y * y > radius * radius) continue;
            vec2 off = vec2(float(x) * texelW * diffusion * 3.0,
                           float(y) * texelH * diffusion * 3.0);
            vec4 s = texture(source, qt_TexCoord0 + off);
            float sl = dot(s.rgb, vec3(0.299, 0.587, 0.114));
            float w = smoothstep(threshold - 0.05, threshold + 0.05, sl);
            sum += s.rgb * w;
            total += w;
        }
    }

    vec3 blurred = (total > 0.0) ? sum / total : vec3(0.0);
    vec3 glowColor = mix(blurred, vec3(glowColor_r, glowColor_g, glowColor_b), useCustomColor);
    vec3 result = col.rgb + glowColor * glowMask * strength;

    fragColor = vec4(clamp(result, 0.0, 1.0), col.a) * qt_Opacity;
}
