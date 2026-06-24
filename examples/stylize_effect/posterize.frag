#version 440
layout(location=0) in vec2 qt_TexCoord0;
layout(location=0) out vec4 fragColor;
layout(std140, binding=0) uniform buf {
    mat4  qt_Matrix;
    float qt_Opacity;
    float levels;
    float dither;
    float targetWidth;
    float targetHeight;
};
layout(binding=1) uniform sampler2D source;

// Simple hash for ordered dithering
float hash(vec2 p) {
    return fract(sin(dot(p, vec2(12.9898, 78.233))) * 43758.5453);
}

void main() {
    vec4 color = texture(source, qt_TexCoord0);

    float n = max(levels, 2.0);

    // Apply ordered dithering before quantization
    if (dither > 0.0) {
        vec2 pixel = qt_TexCoord0 * vec2(targetWidth, targetHeight);
        float noise = (hash(floor(pixel)) - 0.5) * dither / n;
        color.rgb += noise;
    }

    // Quantize each channel to n levels
    vec3 result = floor(color.rgb * n + 0.5) / n;

    fragColor = vec4(result, color.a) * qt_Opacity;
}
