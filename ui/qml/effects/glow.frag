#version 440
layout(location=0) in vec2 qt_TexCoord0;
layout(location=0) out vec4 fragColor;
layout(std140, binding=0) uniform buf {
    mat4  qt_Matrix;
    float qt_Opacity;
    float intensity;
    float radius;
    float threshold;
    vec4  glowColor;
    float targetWidth;
    float targetHeight;
};
layout(binding=1) uniform sampler2D source;
void main() {
    vec2 texel = vec2(1.0 / targetWidth, 1.0 / targetHeight);
    vec4 color = texture(source, qt_TexCoord0);
    
    // Calculate luminance
    float luminance = dot(color.rgb, vec3(0.299, 0.587, 0.114));
    
    // Apply threshold
    float glowAmount = smoothstep(threshold, threshold + 0.1, luminance);
    
    // Apply blur for glow effect
    vec4 sum = vec4(0.0);
    float total = 0.0;
    int r = int(clamp(radius, 1.0, 16.0));
    
    for (int x = -r; x <= r; x++) {
        for (int y = -r; y <= r; y++) {
            float dist = float(x*x + y*y);
            if (dist > float(r*r)) continue;
            float w = exp(-dist / float(r * r) * 2.0);
            vec4 s = texture(source, qt_TexCoord0 + vec2(float(x), float(y)) * texel);
            sum += s * w;
            total += w;
        }
    }
    
    vec4 blurred = sum / max(total, 0.0001);
    
    // Apply glow color
    vec3 glow = blurred.rgb * glowColor.rgb * glowAmount * intensity;
    
    // Combine with original
    vec3 finalColor = color.rgb + glow * color.a;
    
    fragColor = vec4(finalColor, color.a) * qt_Opacity;
}