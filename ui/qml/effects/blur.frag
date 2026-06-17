#version 440
layout(location=0) in vec2 qt_TexCoord0;
layout(location=0) out vec4 fragColor;
layout(std140, binding=0) uniform buf {
    mat4  qt_Matrix;
    float qt_Opacity;
    float size;
    float quality;
    float targetWidth;
    float targetHeight;
};
layout(binding=1) uniform sampler2D source;
void main() {
    vec2 texel = vec2(1.0 / targetWidth, 1.0 / targetHeight);
    int r = int(clamp(size, 1.0, 64.0));
    vec4 sum = vec4(0.0);
    float total = 0.0;
    
    // Apply quality multiplier for sampling radius
    int sampleRadius = r * int(quality);
    
    for (int x = -sampleRadius; x <= sampleRadius; x++) {
        for (int y = -sampleRadius; y <= sampleRadius; y++) {
            float dist = float(x*x + y*y);
            if (dist > float(sampleRadius*sampleRadius)) continue;
            float w = exp(-dist / float(sampleRadius * sampleRadius) * 2.0);
            vec4 s = texture(source, qt_TexCoord0 + vec2(float(x), float(y)) * texel);
            sum += s * w;
            total += w;
        }
    }
    
    vec4 blurred = sum / max(total, 0.0001);
    fragColor = blurred * qt_Opacity;
}