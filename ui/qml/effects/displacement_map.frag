#version 440
layout(location=0) in vec2 qt_TexCoord0;
layout(location=0) out vec4 fragColor;
layout(std140, binding=0) uniform buf {
    mat4  qt_Matrix;
    float qt_Opacity;
    float intensity;
    int   mapType;
    float scaleX;
    float scaleY;
    float targetWidth;
    float targetHeight;
};
layout(binding=1) uniform sampler2D source;

float noise(vec2 p) {
    return fract(sin(dot(p, vec2(12.9898, 78.233))) * 43758.5453);
}

float ripple(vec2 p) {
    float dist = length(p - vec2(0.5));
    return sin(dist * 20.0) * 0.5 + 0.5;
}

float checker(vec2 p) {
    vec2 q = floor(p * 10.0);
    return mod(q.x + q.y, 2.0);
}

float gradient(vec2 p) {
    return p.x;
}

void main() {
    vec2 uv = qt_TexCoord0;
    vec2 pixel = uv * vec2(targetWidth, targetHeight);
    
    // Calculate displacement map value
    float mapValue = 0.0;
    vec2 scaledUV = uv * vec2(scaleX, scaleY);
    
    if (mapType == 0) {
        // Noise
        mapValue = noise(scaledUV);
    } else if (mapType == 1) {
        // Ripple
        mapValue = ripple(scaledUV);
    } else if (mapType == 2) {
        // Checker
        mapValue = checker(scaledUV);
    } else {
        // Gradient
        mapValue = gradient(scaledUV);
    }
    
    // Apply displacement
    vec2 displacement = (vec2(mapValue) - 0.5) * intensity * 0.1;
    vec2 distortedUV = uv + displacement;
    
    // Clamp to valid UV range
    distortedUV = clamp(distortedUV, 0.0, 1.0);
    
    // Sample texture with displacement
    vec4 color = texture(source, distortedUV);
    
    fragColor = color * qt_Opacity;
}