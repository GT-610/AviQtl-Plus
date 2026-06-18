#version 440
layout(location=0) in vec2 qt_TexCoord0;
layout(location=0) out vec4 fragColor;
layout(std140, binding=0) uniform buf {
    mat4  qt_Matrix;
    float qt_Opacity;
    int   lightType;
    float intensity;
    float radius;
    float lightX;
    float lightY;
    vec4  lightColor;
    float targetWidth;
    float targetHeight;
};
layout(binding=1) uniform sampler2D source;
void main() {
    vec4 color = texture(source, qt_TexCoord0);
    vec2 uv = qt_TexCoord0;
    vec2 lightPos = vec2(lightX, lightY);
    
    float lightEffect = 0.0;
    
    if (lightType == 0) {
        // Point light
        float dist = length(uv - lightPos);
        lightEffect = smoothstep(radius, 0.0, dist) * intensity;
    } else if (lightType == 1) {
        // Spotlight
        vec2 dir = normalize(uv - lightPos);
        float angle = dot(dir, vec2(0.0, -1.0)); // Pointing up
        float spotEffect = smoothstep(0.7, 1.0, angle);
        float dist = length(uv - lightPos);
        lightEffect = spotEffect * smoothstep(radius, 0.0, dist) * intensity;
    } else {
        // Ambient light
        lightEffect = intensity * 0.5;
    }
    
    // Apply light color
    vec3 lightContribution = lightColor.rgb * lightEffect;
    
    // Add light to original color
    vec3 finalColor = color.rgb + lightContribution * color.a;
    
    fragColor = vec4(finalColor, color.a) * qt_Opacity;
}