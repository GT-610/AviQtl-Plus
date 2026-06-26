#version 440
layout(location=0) in vec2 qt_TexCoord0;
layout(location=0) out vec4 fragColor;
layout(std140, binding=0) uniform buf {
    mat4 qt_Matrix;
    float qt_Opacity;
    float cx;
    float cy;
    float wipeRadius;
    float targetWidth;
    float targetHeight;
};
layout(binding=1) uniform sampler2D prevTexture;
layout(binding=2) uniform sampler2D nextTexture;

void main() {
    vec4 prev = texture(prevTexture, qt_TexCoord0);
    vec4 next = texture(nextTexture, qt_TexCoord0);

    vec2 pixelCoord = qt_TexCoord0 * vec2(targetWidth, targetHeight);
    vec2 center = vec2(cx * targetWidth, cy * targetHeight);
    float dist = length(pixelCoord - center);

    float mask = step(dist, wipeRadius);
    fragColor = mix(prev, next, mask) * qt_Opacity;
}
