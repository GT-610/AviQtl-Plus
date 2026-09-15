fn weather_hash(point: vec2<f32>, seed: f32) -> vec2<f32> {
    let first = dot(point + vec2<f32>(seed * 0.013), vec2<f32>(127.1, 311.7));
    let second = dot(point + vec2<f32>(seed * 0.029), vec2<f32>(269.5, 183.3));
    return fract(sin(vec2<f32>(first, second)) * 43758.5453);
}

fn weather_segment_distance(point: vec2<f32>, start: vec2<f32>, end: vec2<f32>) -> f32 {
    let segment = end - start;
    let projection = clamp(dot(point - start, segment) / max(dot(segment, segment), 0.0001), 0.0, 1.0);
    return length(point - (start + segment * projection));
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
            let fall = fract(random.y + time_seconds * speed * (0.35 + spread * 0.2));
            let drift = (random.x - 0.5) * spread * cell_size * 0.8;
            let center = (cell + vec2<f32>(random.x, fall)) * cell_size + vec2<f32>(drift, 0.0);
            let length_value = particle_size * (3.0 + spread * 2.0) * (0.65 + random.y * 0.7);
            let start = center - vec2<f32>(length_value * 0.2, length_value * 0.5);
            let end = center + vec2<f32>(length_value * 0.2, length_value * 0.5);
            let distance_value = weather_segment_distance(pixel, start, end);
            let thickness = max(0.75, particle_size * 0.2);
            let drop = 1.0 - smoothstep(thickness, thickness + 1.25, distance_value);
            coverage = max(coverage, drop * (0.45 + random.x * 0.55));
        }
    }
    let generated = vec4<f32>(tint.rgb, tint.a * coverage);
    return max(input_color, generated);
}
