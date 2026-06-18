#version 440
layout(location=0) in vec2 qt_TexCoord0;
layout(location=0) out vec4 fragColor;
layout(std140, binding=0) uniform buf {
    mat4  qt_Matrix;
    float qt_Opacity;
    float rasterW;
    float rasterH;
    float speed;
    float angle;
    float time;
    float targetWidth;
    float targetHeight;
};
layout(binding=1) uniform sampler2D source;
void main() {
    vec2 uv = qt_TexCoord0;
    vec2 pixel = uv * vec2(targetWidth, targetHeight);
    
    // Convert angle to radians
    float rad = radians(angle);
    float cosA = cos(rad);
    float sinA = sin(rad);
    
    // Rotate pixel coordinates
    vec2 center = vec2(targetWidth, targetHeight) * 0.5;
    vec2 rotated = pixel - center;
    rotated = vec2(
        rotated.x * cosA - rotated.y * sinA,
        rotated.x * sinA + rotated.y * cosA
    );
    pixel = rotated + center;
    
    // Apply raster effect
    float timeOffset = time * speed * 0.1;
    float rasterX = sin((pixel.x + timeOffset) * 3.14159 / rasterW);
    float rasterY = sin((pixel.y + timeOffset) * 3.14159 / rasterH);
    
    // Combine raster effects
    float raster = rasterX * rasterY;
    
    // Sample original texture
    vec4 color = texture(source, uv);
    
    // Apply raster modulation
    vec3 finalColor = color.rgb * (0.8 + 0.2 * raster);
    
    fragColor = vec4(finalColor, color.a) * qt_Opacity;
}