#version 440
layout(location=0) in vec2 qt_TexCoord0;
layout(location=0) out vec4 fragColor;
layout(std140, binding=0) uniform buf {
    mat4  qt_Matrix;
    float qt_Opacity;
    float strength;
    float threshold;
    float luminanceEdge;
    float alphaEdge;
    vec3 edgeColor;
    float texelW;
    float texelH;
};
layout(binding=1) uniform sampler2D source;

float sampleVal(vec2 uv, float useLuma, float useAlpha) {
    vec4 s = texture(source, uv);
    if (useAlpha > 0.5) return s.a;
    if (useLuma > 0.5) return dot(s.rgb, vec3(0.299, 0.587, 0.114));
    return (s.r + s.g + s.b) * 0.333;
}

void main() {
    vec4 col = texture(source, qt_TexCoord0);
    vec2 dx = vec2(texelW, 0.0);
    vec2 dy = vec2(0.0, texelH);

    float tl = sampleVal(qt_TexCoord0 - dx - dy, luminanceEdge, alphaEdge);
    float tc = sampleVal(qt_TexCoord0 - dy,        luminanceEdge, alphaEdge);
    float tr = sampleVal(qt_TexCoord0 + dx - dy,   luminanceEdge, alphaEdge);
    float ml = sampleVal(qt_TexCoord0 - dx,         luminanceEdge, alphaEdge);
    float mr = sampleVal(qt_TexCoord0 + dx,         luminanceEdge, alphaEdge);
    float bl = sampleVal(qt_TexCoord0 - dx + dy,    luminanceEdge, alphaEdge);
    float bc = sampleVal(qt_TexCoord0 + dy,         luminanceEdge, alphaEdge);
    float br = sampleVal(qt_TexCoord0 + dx + dy,    luminanceEdge, alphaEdge);

    float gx = -tl - 2.0*ml - bl + tr + 2.0*mr + br;
    float gy = -tl - 2.0*tc - tr + bl + 2.0*bc + br;
    float edge = sqrt(gx*gx + gy*gy) * strength;
    edge = smoothstep(threshold, threshold + 0.05, edge);

    vec3 ec = edgeColor;
    vec3 result = mix(col.rgb, ec, edge);
    float outA = mix(col.a, 1.0, edge);

    fragColor = vec4(result, outA) * qt_Opacity;
}
