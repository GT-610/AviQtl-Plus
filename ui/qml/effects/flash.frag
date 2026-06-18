#version 440
layout(location=0) in vec2 qt_TexCoord0;
layout(location=0) out vec4 fragColor;
layout(std140, binding=0) uniform buf {
    mat4  qt_Matrix;
    float qt_Opacity;
    float intensity;
    float speed;
    vec4  flashColor;
    int   type;
    float time;
};
layout(binding=1) uniform sampler2D source;
void main() {
    vec4 color = texture(source, qt_TexCoord0);
    vec2 uv = qt_TexCoord0;
    vec2 center = vec2(0.5, 0.5);
    
    // Calculate flash effect based on time and speed
    float t = mod(time * speed * 0.1, 1.0);
    float flashEffect = 0.0;
    
    if (type == 0) {
        // Radial flash
        float dist = length(uv - center);
        flashEffect = smoothstep(t, t - 0.1, dist) * intensity;
    } else if (type == 1) {
        // Linear flash
        float pos = uv.x;
        flashEffect = smoothstep(t, t - 0.1, pos) * intensity;
    } else {
        // Circular flash
        float dist = length(uv - center);
        float ring = abs(dist - t);
        flashEffect = smoothstep(0.1, 0.0, ring) * intensity;
    }
    
    // Apply flash color
    vec3 flashContribution = flashColor.rgb * flashEffect;
    
    // Add flash to original color
    vec3 finalColor = color.rgb + flashContribution * color.a;
    
    fragColor = vec4(finalColor, color.a) * qt_Opacity;
}