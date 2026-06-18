#version 440
layout(location=0) in vec2 qt_TexCoord0;
layout(location=0) out vec4 fragColor;
layout(std140, binding=0) uniform buf {
    mat4  qt_Matrix;
    float qt_Opacity;
    float threshold;
    float blend;
    float invert;
};
layout(binding=1) uniform sampler2D source;
void main() {
    vec4 color = texture(source, qt_TexCoord0);
    
    // Calculate luminance
    float luminance = dot(color.rgb, vec3(0.299, 0.587, 0.114));
    
    // Apply threshold
    float keyMask = smoothstep(threshold, threshold - blend, luminance);
    
    // Apply invert if needed
    if (invert > 0.5) {
        keyMask = 1.0 - keyMask;
    }
    
    // Apply mask to alpha channel
    fragColor = vec4(color.rgb, color.a * keyMask) * qt_Opacity;
}