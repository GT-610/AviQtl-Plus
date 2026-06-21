#version 440
layout(location = 0) in vec2 vTexCoord;
layout(location = 1) in float vOpacity;
layout(location = 0) out vec4 fragColor;
layout(binding = 1) uniform sampler2D tex;
void main() {
    vec4 c = texture(tex, vTexCoord);
    fragColor = vec4(c.rgb * vOpacity, c.a * vOpacity);
}
