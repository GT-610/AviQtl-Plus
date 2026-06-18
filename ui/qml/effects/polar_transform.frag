#version 440
layout(location=0) in vec2 qt_TexCoord0;
layout(location=0) out vec4 fragColor;
layout(std140, binding=0) uniform buf {
    mat4  qt_Matrix;
    float qt_Opacity;
    float centerX;
    float centerY;
    float scale;
    float angleOffset;
    float targetWidth;
    float targetHeight;
};
layout(binding=1) uniform sampler2D source;
void main() {
    vec2 uv = qt_TexCoord0;
    vec2 center = vec2(centerX, centerY);
    
    // Convert to polar coordinates
    vec2 diff = uv - center;
    float dist = length(diff);
    float angle = atan(diff.y, diff.x);
    
    // Apply angle offset
    angle += radians(angleOffset);
    
    // Convert back to Cartesian with scale
    vec2 polarUV = vec2(
        cos(angle) * dist * scale,
        sin(angle) * dist * scale
    );
    
    // Map to texture coordinates
    vec2 texUV = polarUV + center;
    
    // Clamp to valid UV range
    texUV = clamp(texUV, 0.0, 1.0);
    
    // Sample texture
    vec4 color = texture(source, texUV);
    
    fragColor = color * qt_Opacity;
}