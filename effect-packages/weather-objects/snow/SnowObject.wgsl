fn weather_hash(point: vec2<f32>, seed: f32) -> vec2<f32> {
    let first = dot(point + vec2<f32>(seed * 0.017), vec2<f32>(127.1, 311.7));
    let second = dot(point + vec2<f32>(seed * 0.031), vec2<f32>(269.5, 183.3));
    return fract(sin(vec2<f32>(first, second)) * 43758.5453);
}

fn aviqtl_effect(
    input_color: vec4<f32>,
    uv: vec2<f32>,
    canvas_size: vec2<f32>,
    time_seconds: f32,
) -> vec4<f32> {
    let count = clamp(aviqtl_parameter(0u).x, 1.0, 2000.0);
    let speed = aviqtl_parameter(1u).x;
    let particle_size = max(aviqtl_parameter(2u).x, 0.1);
    let spread = max(aviqtl_parameter(3u).x, 0.0);
    let seed = aviqtl_parameter(4u).x;
    let tint = aviqtl_parameter(5u);
    let pixel = uv * canvas_size;
    let cell_size = max(sqrt(canvas_size.x * canvas_size.y / count), 8.0);
    let base_cell = floor(pixel / cell_size);
    var coverage = 0.0;
    for (var offset_y: i32 = -1; offset_y <= 1; offset_y += 1) {
        for (var offset_x: i32 = -1; offset_x <= 1; offset_x += 1) {
            let cell = base_cell + vec2<f32>(f32(offset_x), f32(offset_y));
            let random = weather_hash(cell, seed);
            let fall = fract(random.y + time_seconds * speed * (0.12 + spread * 0.08));
            let sway = sin(time_seconds * (0.8 + random.x) + random.y * 6.2831853)
                * spread * cell_size * 0.3;
            let center = (cell + vec2<f32>(random.x, fall)) * cell_size + vec2<f32>(sway, 0.0);
            let radius = particle_size * (0.55 + random.x * 0.9);
            let distance_value = length(pixel - center);
            let flake = 1.0 - smoothstep(radius * 0.65, radius + 1.0, distance_value);
            coverage = max(coverage, flake * (0.4 + random.y * 0.6));
        }
    }
    let generated = vec4<f32>(tint.rgb, tint.a * coverage);
    return max(input_color, generated);
}
