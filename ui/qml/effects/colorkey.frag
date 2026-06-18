#version 440
layout(location=0) in vec2 qt_TexCoord0;
layout(location=0) out vec4 fragColor;
layout(std140, binding=0) uniform buf {
    mat4  qt_Matrix;
    float qt_Opacity;
    vec4  keyColor;
    float similarity;
    float blend;
    float invert;
};
layout(binding=1) uniform sampler2D source;
void main() {
    vec4 color = texture(source, qt_TexCoord0);
    
    // Calculate color difference
    vec3 diff = abs(color.rgb - keyColor.rgb);
    float maxDiff = max(max(diff.r, diff.g), diff.b);
    
    // Apply similarity threshold
    float keyMask = smoothstep(similarity, similarity - blend, maxDiff);
    
    // Apply invert if needed
    if (invert > 0.5) {
        keyMask = 1.0 - keyMask;
    }
    
    // Apply mask to alpha channel
    fragColor = vec4(color.rgb, color.a * keyMask) * qt_Opacity;
}