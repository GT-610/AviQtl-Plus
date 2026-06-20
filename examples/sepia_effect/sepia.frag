#version 440
layout(location=0) in vec2 qt_TexCoord0;
layout(location=0) out vec4 fragColor;
layout(std140, binding=0) uniform buf {
    mat4  qt_Matrix;
    float qt_Opacity;
    float intensity;
    float temperature;
    float targetWidth;
    float targetHeight;
};
layout(binding=1) uniform sampler2D source;

void main() {
    vec4 color = texture(source, qt_TexCoord0);

    // Sepia conversion matrix
    vec3 sepia;
    sepia.r = dot(color.rgb, vec3(0.393, 0.769, 0.189));
    sepia.g = dot(color.rgb, vec3(0.349, 0.686, 0.168));
    sepia.b = dot(color.rgb, vec3(0.272, 0.534, 0.131));

    // Apply temperature shift
    sepia.r += temperature * 0.1;
    sepia.b -= temperature * 0.1;

    // Blend between original and sepia based on intensity
    vec3 result = mix(color.rgb, sepia, intensity);

    fragColor = vec4(result, color.a) * qt_Opacity;
}
