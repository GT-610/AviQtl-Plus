fn aviqtl_effect(
    input_color: vec4<f32>,
    uv: vec2<f32>,
    canvas_size: vec2<f32>,
    time_seconds: f32,
) -> vec4<f32> {
    let amount = clamp(aviqtl_parameter(0u).x, 0.0, 1.0);
    let tint = aviqtl_parameter(1u);
    let source = aviqtl_sample(uv);
    let color = mix(source.rgb, tint.rgb, amount * tint.a);
    let contract_values = vec3<f32>(canvas_size.x, canvas_size.y, time_seconds) * 0.0;
    return vec4<f32>(color + contract_values, input_color.a);
}
