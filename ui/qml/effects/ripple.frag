#version 440
layout(location=0) in vec2 qt_TexCoord0;
layout(location=0) out vec4 fragColor;
layout(std140, binding=0) uniform buf {
    mat4  qt_Matrix;
    float qt_Opacity;
    float amplitude;
    float frequency;
    float speed;
    float centerX;
    float centerY;
    float time;
    float targetWidth;
    float targetHeight;
};
layout(binding=1) uniform sampler2D source;
void main() {
    vec2 uv = qt_TexCoord0;
    vec2 center = vec2(centerX, centerY);
    
    // Calculate distance from center
    vec2 diff = uv - center;
    float dist = length(diff);
    float angle = atan(diff.y, diff.x);
    
    // Apply ripple distortion
    float ripple = sin(dist * frequency * 3.14159 * 2.0 - time * speed * 0.1) * amplitude;
    
    // Convert back to Cartesian coordinates
    vec2 distortion = vec2(cos(angle), sin(angle)) * ripple * 0.01;
    vec2 distortedUV = uv + distortion;
    
    // Clamp to valid UV range
    distortedUV = clamp(distortedUV, 0.0, 1.0);
    
    // Sample texture with distortion
    vec4 color = texture(source, distortedUV);
    
    fragColor = color * qt_Opacity;
}