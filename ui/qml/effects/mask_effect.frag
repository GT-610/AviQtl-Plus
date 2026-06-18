#version 440
layout(location=0) in vec2 qt_TexCoord0;
layout(location=0) out vec4 fragColor;
layout(std140, binding=0) uniform buf {
    mat4  qt_Matrix;
    float qt_Opacity;
    int   maskType;
    float invertMask;
    float maskStrength;
    float targetWidth;
    float targetHeight;
};
layout(binding=1) uniform sampler2D source;

float circleMask(vec2 uv, vec2 center, float radius) {
    float dist = length(uv - center);
    return smoothstep(radius, radius - 0.01, dist);
}

float rectMask(vec2 uv, vec2 center, vec2 size) {
    vec2 d = abs(uv - center) - size;
    return smoothstep(0.01, 0.0, max(d.x, d.y));
}

float starMask(vec2 uv, vec2 center, float radius, int points) {
    float angle = atan(uv.y - center.y, uv.x - center.x);
    float dist = length(uv - center);
    float r = radius * (0.5 + 0.5 * cos(float(points) * angle));
    return smoothstep(r, r - 0.01, dist);
}

float heartMask(vec2 uv, vec2 center, float size) {
    vec2 p = (uv - center) / size;
    p.y = -p.y;
    float a = p.x * p.x + p.y * p.y - 1.0;
    return smoothstep(0.01, 0.0, a * a * a - p.x * p.x * p.y * p.y * p.y);
}

void main() {
    vec4 color = texture(source, qt_TexCoord0);
    vec2 uv = qt_TexCoord0;
    vec2 center = vec2(0.5, 0.5);
    
    float mask = 0.0;
    
    if (maskType == 0) {
        // Circle mask
        mask = circleMask(uv, center, 0.4);
    } else if (maskType == 1) {
        // Rectangle mask
        mask = rectMask(uv, center, vec2(0.3, 0.2));
    } else if (maskType == 2) {
        // Star mask
        mask = starMask(uv, center, 0.4, 5);
    } else if (maskType == 3) {
        // Heart mask
        mask = heartMask(uv, center, 0.4);
    }
    
    // Apply invert if needed
    if (invertMask > 0.5) {
        mask = 1.0 - mask;
    }
    
    // Apply mask strength
    mask *= maskStrength;
    
    // Apply mask to alpha channel
    fragColor = vec4(color.rgb, color.a * mask) * qt_Opacity;
}