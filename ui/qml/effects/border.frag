#version 440
layout(location=0) in vec2 qt_TexCoord0;
layout(location=0) out vec4 fragColor;
layout(std140, binding=0) uniform buf {
    mat4  qt_Matrix;
    float qt_Opacity;
    float size;
    float blur;
    vec3 borderColor;
    float texelW;
    float texelH;
};
layout(binding=1) uniform sampler2D source;

void main() {
    vec4 col = texture(source, qt_TexCoord0);
    float maxDist = 0.0;
    float sumAlpha = 0.0;
    int radius = int(max(1.0, size + blur));

    for (int x = -10; x <= 10; x++) {
        for (int y = -10; y <= 10; y++) {
            if (x * x + y * y > radius * radius) continue;
            vec2 off = vec2(float(x) * texelW, float(y) * texelH);
            float a = texture(source, qt_TexCoord0 + off).a;
            float dist = sqrt(float(x*x + y*y));
            if (a > 0.1) {
                maxDist = max(maxDist, dist);
                sumAlpha += a;
            }
        }
    }

    float borderMask = smoothstep(size + blur, size, maxDist) * (1.0 - col.a);
    vec3 bc = borderColor;
    vec3 result = mix(col.rgb, bc, borderMask);
    float outA = max(col.a, borderMask);

    fragColor = vec4(result, outA) * qt_Opacity;
}
