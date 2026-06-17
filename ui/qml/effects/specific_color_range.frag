#version 440
layout(location=0) in vec2 qt_TexCoord0;
layout(location=0) out vec4 fragColor;
layout(std140, binding=0) uniform buf {
    mat4  qt_Matrix;
    float qt_Opacity;
    float targetHue;
    float hueRange;
    vec4  targetColor;
    float strength;
};
layout(binding=1) uniform sampler2D source;

vec3 rgb2hsv(vec3 c) {
    vec4 K = vec4(0.0, -1.0/3.0, 2.0/3.0, -1.0);
    vec4 p = mix(vec4(c.bg, K.wz), vec4(c.gb, K.xy), step(c.b, c.g));
    vec4 q = mix(vec4(p.xyw, c.r), vec4(c.r, p.yzx), step(p.x, c.r));
    float d = q.x - min(q.w, q.y);
    float e = 1.0e-10;
    return vec3(abs(q.z + (q.w - q.y) / (6.0 * d + e)), d / (q.x + e), q.x);
}

void main() {
    vec4 color = texture(source, qt_TexCoord0);
    
    // Convert to HSV
    vec3 hsv = rgb2hsv(color.rgb);
    
    // Calculate hue difference
    float hueDiff = abs(hsv.x - targetHue);
    if (hueDiff > 0.5) hueDiff = 1.0 - hueDiff;
    
    // Create mask for target hue range
    float mask = smoothstep(hueRange, hueRange * 0.5, hueDiff);
    
    // Apply color transformation
    vec3 transformedColor = mix(color.rgb, targetColor.rgb, mask * strength);
    
    fragColor = vec4(transformedColor, color.a) * qt_Opacity;
}