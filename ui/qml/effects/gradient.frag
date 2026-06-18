#version 440
layout(location=0) in vec2 qt_TexCoord0;
layout(location=0) out vec4 fragColor;
layout(std140, binding=0) uniform buf {
    mat4  qt_Matrix;
    float qt_Opacity;
    float strength;
    float center_x;
    float center_y;
    float angle;
    float width;
    int   shape;
    float colStart_r;
    float colStart_g;
    float colStart_b;
    float colStart_a;
    float colEnd_r;
    float colEnd_g;
    float colEnd_b;
    float colEnd_a;
};
layout(binding=1) uniform sampler2D source;

void main() {
    vec4 col = texture(source, qt_TexCoord0);
    vec2 uv = qt_TexCoord0 - vec2(center_x, center_y);

    float t = 0.0;
    if (shape == 0) {
        float rad = radians(angle);
        vec2 dir = vec2(cos(rad), sin(rad));
        t = dot(uv, dir) / max(width, 0.001) + 0.5;
    } else if (shape == 1) {
        t = length(uv) / max(width, 0.001);
    } else {
        t = max(abs(uv.x), abs(uv.y)) / max(width, 0.001);
    }

    t = clamp(t, 0.0, 1.0);
    vec4 grad = mix(vec4(colStart_r, colStart_g, colStart_b, colStart_a),
                    vec4(colEnd_r, colEnd_g, colEnd_b, colEnd_a), t);
    grad.a *= strength;

    vec3 result = mix(col.rgb, grad.rgb, grad.a);
    float outA = max(col.a, grad.a);
    fragColor = vec4(result, outA) * qt_Opacity;
}
