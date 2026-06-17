#version 440
layout(location=0) in vec2 qt_TexCoord0;
layout(location=0) out vec4 fragColor;
layout(std140, binding=0) uniform buf {
    mat4  qt_Matrix;
    float qt_Opacity;
    float hue;
    float hueRange;
    float similarity;
    float blend;
    float invert;
};
layout(binding=1) uniform sampler2D source;

vec3 rgb2hsl(vec3 c) {
    float hi = max(max(c.r, c.g), c.b);
    float lo = min(min(c.r, c.g), c.b);
    float d = hi - lo;
    float l = (hi + lo) * 0.5;
    float s = (d < 0.0001) ? 0.0 : d / (1.0 - abs(2.0 * l - 1.0));
    float h = 0.0;
    if (d > 0.0001) {
        if (hi == c.r)      h = fract((c.g - c.b) / d / 6.0);
        else if (hi == c.g) h = ((c.b - c.r) / d + 2.0) / 6.0;
        else                h = ((c.r - c.g) / d + 4.0) / 6.0;
    }
    return vec3(h, s, l);
}

void main() {
    vec4 col = texture(source, qt_TexCoord0);
    vec3 hsl = rgb2hsl(col.rgb);

    float hueDiff = abs(hsl.x - hue);
    if (hueDiff > 0.5) hueDiff = 1.0 - hueDiff;

    float hueMask = 1.0 - smoothstep(hueRange, hueRange + blend, hueDiff);
    float satMask = smoothstep(0.0, similarity * 0.5, hsl.y);

    float key = hueMask * satMask;

    if (invert > 0.5)
        key = 1.0 - key;

    fragColor = vec4(col.rgb, col.a * (1.0 - key)) * qt_Opacity;
}
