//! Translation tables between ExEdit object files and AviQtl-Plus catalogue entries.
//!
//! ExEdit identifies an object or filter by its localized display name (`_name`), because
//! that name is a pointer into the running `exedit.auf`'s own table. The Chinese translation
//! pack therefore rewrote every built-in name, while script-backed objects keep an ASCII
//! `name=` identifier. A reader has to accept every localization that exists in the wild, so
//! each table below lists the Chinese names of the pack this work targets, the original
//! Japanese names, and the English names some tools emit.
//!
//! Parameter tables follow the same idea: `轨` entries name the parameter in each language and
//! describe the linear conversion into our units. Anything not listed here is preserved by the
//! importer as a disabled placeholder instead of being guessed at.

/// How one ExEdit trackbar maps onto a catalogue parameter.
#[derive(Debug, Clone, Copy)]
pub struct TrackMapping {
    /// Accepted source keys, in the order they are tried.
    pub keys: &'static [&'static str],
    /// Catalogue parameter name.
    pub param: &'static str,
    /// `value * scale + offset`.
    pub scale: f64,
    pub offset: f64,
}

/// How one ExEdit colour parameter maps onto a catalogue parameter.
#[derive(Debug, Clone, Copy)]
pub struct ColorMapping {
    pub keys: &'static [&'static str],
    pub param: &'static str,
}

/// How one ExEdit integer parameter maps onto a catalogue parameter.
#[derive(Debug, Clone, Copy)]
pub struct IntegerMapping {
    pub keys: &'static [&'static str],
    pub param: &'static str,
    pub scale: f64,
    pub offset: f64,
}

/// A built-in ExEdit object this importer understands.
pub struct ObjectMapping {
    /// Catalogue id.
    pub object_id: &'static str,
    /// Accepted `_name` values.
    pub aliases: &'static [&'static str],
    pub tracks: &'static [TrackMapping],
    pub colors: &'static [ColorMapping],
    pub integers: &'static [IntegerMapping],
    /// Accepted boolean keys, each written to a catalogue parameter when present.
    pub flags: &'static [(&'static [&'static str], &'static str)],
    /// Whether the referenced media path lives in the filter's `_exdata` blob.
    pub media_path: bool,
}

/// A built-in ExEdit filter this importer understands.
pub struct FilterMapping {
    /// Catalogue id.
    pub effect_id: &'static str,
    pub aliases: &'static [&'static str],
    pub tracks: &'static [TrackMapping],
    pub colors: &'static [ColorMapping],
    pub integers: &'static [IntegerMapping],
    pub flags: &'static [(&'static [&'static str], &'static str)],
}

const fn track(keys: &'static [&'static str], param: &'static str, scale: f64) -> TrackMapping {
    TrackMapping {
        keys,
        param,
        scale,
        offset: 0.0,
    }
}

/// ExEdit's 透明度 runs the other way round from our `opacity`: there, 0 is opaque and 100 is
/// invisible, while here 1 is opaque. The negative scale flips the direction and the offset
/// puts full opacity at zero, so a stock object — written with 透明度=0.0 — arrives visible.
///
/// Reading it the other way made every imported object fully transparent, which is why a
/// freshly imported project previewed black, and made an object the author had faded out
/// arrive opaque.
const fn transparency_track(keys: &'static [&'static str], param: &'static str) -> TrackMapping {
    TrackMapping {
        keys,
        param,
        scale: -0.01,
        offset: 1.0,
    }
}

/// The drawing/transform filter. ExEdit stores it last in every object that has effects.
pub const DRAWING_ALIASES: &[&str] = &[
    // The CN pack calls it 标准属性, stock ExEdit 標準描画; both ship in the wild.
    "标准属性",
    "標準描画",
    "標準描画(拡張)",
    "Standard drawing",
    "standard_draw",
];

/// The transform's trackbars, in ExEdit's storage order, for objects that use
/// `track0..track5` instead of the localized names.
pub const DRAWING_TRACKS: &[TrackMapping] = &[
    track(&["X", "x", "track0"], "x", 1.0),
    track(&["Y", "y", "track1"], "y", 1.0),
    track(&["Z", "z", "track2"], "z", 1.0),
    track(&["缩放率", "拡大率", "scale", "track3"], "scale", 1.0),
    transparency_track(&["透明度", "opacity", "track4"], "opacity"),
    track(
        &["旋转", "回転", "rotation", "rotationZ", "track5"],
        "rotationZ",
        1.0,
    ),
];

pub const OBJECTS: &[ObjectMapping] = &[
    ObjectMapping {
        object_id: "text",
        aliases: &["文本", "テキスト", "Text", "text"],
        tracks: &[
            track(
                &["大小", "サイズ", "size", "fontSize", "track0"],
                "fontSize",
                1.0,
            ),
            track(
                &["spacing_x", "文字間隔", "letterSpacing"],
                "letterSpacing",
                1.0,
            ),
            track(&["spacing_y", "行間隔", "lineSpacing"], "lineSpacing", 1.0),
        ],
        colors: &[
            ColorMapping {
                keys: &["color", "文字色"],
                param: "color",
            },
            ColorMapping {
                keys: &["color2", "縁色", "影色"],
                param: "outlineColor",
            },
        ],
        // `align` is not passed through: ExEdit numbers 文字揃え as an index into its list,
        // which `apply_text_alignment` decodes. Copying the value across mapped 中央揃え to
        // whatever number the list position happened to be.
        integers: &[],
        flags: &[
            (&["B", "太字", "bold"], "fontBold"),
            (&["I", "斜体", "italic"], "fontItalic"),
        ],
        media_path: false,
    },
    ObjectMapping {
        object_id: "image",
        // Stock ExEdit names the object 画像ファイル; the CN pack calls it 图片文件.
        aliases: &["图像", "图片文件", "画像", "画像ファイル", "Image", "image"],
        tracks: &[transparency_track(
            &["透明度", "opacity", "track0"],
            "opacity",
        )],
        colors: &[],
        integers: &[],
        flags: &[],
        media_path: true,
    },
    ObjectMapping {
        object_id: "video",
        // Stock ExEdit names the object 動画ファイル; the CN pack calls it 视频文件.
        aliases: &["视频", "视频文件", "動画", "動画ファイル", "Video", "video"],
        tracks: &[transparency_track(&["透明度", "opacity"], "opacity")],
        colors: &[],
        integers: &[],
        flags: &[],
        media_path: true,
    },
    ObjectMapping {
        object_id: "audio",
        // Stock ExEdit names the object 音声ファイル; the CN pack calls it 音频文件.
        aliases: &["音频", "音频文件", "音声", "音声ファイル", "Audio", "audio"],
        tracks: &[
            // ExEdit stores 音量 as a percentage: 100.0 is unity gain, which is our 1.0.
            track(&["音量", "volume"], "volume", 0.01),
            track(&["左右", "pan", "声道"], "pan", 0.01),
        ],
        colors: &[],
        integers: &[],
        flags: &[],
        media_path: true,
    },
];

/// ExEdit's 図形 object and its built-in figure types.
///
/// `FUN_10073470` turns the stored figure type into a vertex count:
///
/// ```text
/// if (type == 1) { fill the whole object rectangle }
/// else if (2 < type && type < 7) {
///     vertices = type;            // 3, 4, 5, 6
///     if (type == 6) vertices = 10;   // the star
///     else if (type != 3) vertices = type + 1;
/// }
/// else { use the object's own rectangle }
/// ```
///
/// The stored value is zero-based and follows the order the 図形 dialog lists the types in:
/// 背景 (0), 円 (1), 四角形 (2), 三角形 (3), 五角形 (4), 六角形 (5), 星型 (6). Reading it as
/// one-based swapped 円 and 四角形, so a stock circle arrived as a rectangle and a square as
/// a 64-gon; the types from 三角形 up happened to line up either way.
///
/// 背景 and 円 both take the non-polygon path — an ellipse inscribed in the object rectangle,
/// for 円 — so a dense polygon stands in for the circle here. A four-sided 四边形 that is not
/// the built-in 四角形 is a bitmap figure loaded from the `figure` folder, named in `name`.
pub struct ShapeMapping {
    /// Catalogue object id.
    pub object_id: &'static str,
    /// `shapeType` value, when the catalogue object has one.
    pub shape_type: Option<&'static str>,
    /// Vertex count for `sides`.
    pub sides: i32,
}

/// The 図形 object's display names.
pub const FIGURE_ALIASES: &[&str] = &["图形", "図形", "Shape", "figure"];

/// Built-in figure types, indexed by the zero-based `type` value stored in the object file.
pub const FIGURE_TYPES: [ShapeMapping; 7] = [
    // 0 背景: the object's own rectangle, filled.
    ShapeMapping {
        object_id: "rect",
        shape_type: Some("polygon"),
        sides: 4,
    },
    // 1 円 has no vertex count; a dense polygon is the closest primitive we have.
    ShapeMapping {
        object_id: "polygon_shape",
        shape_type: None,
        sides: 64,
    },
    // 2 四角形
    ShapeMapping {
        object_id: "rect",
        shape_type: Some("polygon"),
        sides: 4,
    },
    // 3 三角形
    ShapeMapping {
        object_id: "polygon_shape",
        shape_type: None,
        sides: 3,
    },
    // 4 五角形
    ShapeMapping {
        object_id: "polygon_shape",
        shape_type: None,
        sides: 5,
    },
    // 5 六角形
    ShapeMapping {
        object_id: "polygon_shape",
        shape_type: None,
        sides: 6,
    },
    // 6 星型
    ShapeMapping {
        object_id: "rect",
        shape_type: Some("star"),
        sides: 5,
    },
];

/// Looks up the built-in figure type for a stored `type` value.
///
/// Values outside 0..=6 are not built-in figures — ExEdit draws the object's own rectangle and
/// takes the picture, if any, from `name` — so they fall back to 背景, which is that rectangle.
pub fn figure_mapping(kind: i32) -> &'static ShapeMapping {
    usize::try_from(kind)
        .ok()
        .and_then(|index| FIGURE_TYPES.get(index))
        .unwrap_or(&FIGURE_TYPES[0])
}

pub const FILTERS: &[FilterMapping] = &[
    FilterMapping {
        effect_id: "blur",
        aliases: &["模糊", "ぼかし", "Blur"],
        tracks: &[track(&["范围", "範囲", "size"], "size", 1.0)],
        colors: &[],
        integers: &[],
        flags: &[],
    },
    FilterMapping {
        effect_id: "border_blur",
        aliases: &["边缘模糊", "境界ぼかし", "Border blur"],
        tracks: &[track(&["范围", "範囲"], "size", 1.0)],
        colors: &[],
        integers: &[],
        flags: &[],
    },
    FilterMapping {
        effect_id: "mosaic",
        aliases: &["马赛克", "モザイク", "Mosaic"],
        tracks: &[track(&["大小", "サイズ", "size"], "size", 1.0)],
        colors: &[],
        integers: &[],
        flags: &[],
    },
    FilterMapping {
        effect_id: "monochrome",
        aliases: &["单色化", "単色化", "Monochrome"],
        tracks: &[track(&["强度", "強さ", "strength"], "strength", 0.1)],
        colors: &[ColorMapping {
            keys: &["颜色", "色", "color"],
            param: "color",
        }],
        integers: &[],
        flags: &[(&["保持亮度", "輝度を保持"], "preserveLuma")],
    },
    FilterMapping {
        effect_id: "diffuse_light",
        aliases: &["扩散光", "拡散光", "Diffuse light"],
        tracks: &[
            track(&["强度", "強さ", "strength"], "strength", 0.1),
            track(&["扩散度", "拡散", "diffusion"], "diffusion", 1.0),
        ],
        colors: &[],
        integers: &[],
        flags: &[],
    },
    FilterMapping {
        effect_id: "edge_detection",
        aliases: &["提取边缘", "エッジ抽出", "Edge detection"],
        tracks: &[
            track(&["强度", "強さ", "strength"], "strength", 0.1),
            track(&["阈值", "しきい値", "threshold"], "threshold", 1.0),
        ],
        colors: &[ColorMapping {
            keys: &["颜色", "色", "color"],
            param: "color",
        }],
        integers: &[],
        flags: &[
            (&["提取亮度边缘", "輝度エッジ"], "luminanceEdge"),
            (&["提取透明度边缘", "透明度エッジ"], "alphaEdge"),
        ],
    },
    FilterMapping {
        effect_id: "diagonal_clipping",
        aliases: &["斜向裁剪", "斜めクリッピング", "Diagonal clipping"],
        tracks: &[
            track(&["中心X", "中心X"], "centerX", 1.0),
            track(&["中心Y", "中心Y"], "centerY", 1.0),
            track(&["角度", "角度", "angle"], "angle", 1.0),
            track(&["宽", "幅", "width"], "width", 1.0),
            track(&["模糊", "ぼかし", "blur"], "blur", 1.0),
        ],
        colors: &[],
        integers: &[],
        flags: &[],
    },
    FilterMapping {
        effect_id: "clipping",
        aliases: &["裁剪遮罩", "クリッピング", "Clipping"],
        tracks: &[
            track(&["上", "top"], "top", 1.0),
            track(&["下", "bottom"], "bottom", 1.0),
            track(&["左", "left"], "left", 1.0),
            track(&["右", "right"], "right", 1.0),
        ],
        colors: &[],
        integers: &[],
        flags: &[],
    },
    FilterMapping {
        effect_id: "color_correction",
        aliases: &["色调修正", "色調補正", "Color correction"],
        tracks: &[
            // Both sides store these as percentages against 100, so the values carry over
            // unchanged. The Japanese names were on the wrong pair: 明るさ is brightness and
            // 輝度 is lightness, the same way the CN pack's 亮度 and 发光度 divide them.
            track(&["亮度", "明るさ", "brightness"], "brightness", 1.0),
            track(&["对比度", "コントラスト", "contrast"], "contrast", 1.0),
            track(&["色相", "色相", "hue"], "hue", 1.0),
            track(&["发光度", "輝度", "lightness"], "lightness", 1.0),
            track(&["色度", "彩度", "saturation"], "saturation", 1.0),
        ],
        colors: &[],
        integers: &[],
        flags: &[],
    },
    FilterMapping {
        effect_id: "emission",
        aliases: &["辉光", "発光", "Emission"],
        tracks: &[
            track(&["强度", "強さ", "strength"], "strength", 0.1),
            track(&["扩散度", "拡散", "diffusion"], "diffusion", 0.1),
            track(&["阈值", "しきい値", "threshold"], "threshold", 0.1),
        ],
        colors: &[ColorMapping {
            keys: &["颜色", "色", "color"],
            param: "color",
        }],
        integers: &[],
        flags: &[],
    },
    FilterMapping {
        effect_id: "noise",
        aliases: &["噪波", "ノイズ", "Noise"],
        tracks: &[
            track(&["强度", "強さ", "strength"], "strength", 0.1),
            track(&["速度X", "速度X"], "speed", 1.0),
        ],
        colors: &[],
        integers: &[],
        flags: &[],
    },
    FilterMapping {
        effect_id: "fade",
        aliases: &["淡化", "フェード", "Fade"],
        tracks: &[
            track(&["入场", "イン", "fadeIn"], "fadeIn", 1.0),
            track(&["出场", "アウト", "fadeOut"], "fadeOut", 1.0),
        ],
        colors: &[],
        integers: &[],
        flags: &[],
    },
    FilterMapping {
        effect_id: "sharpen",
        aliases: &["锐化", "シャープ", "Sharpen"],
        tracks: &[
            track(&["强度", "強さ", "strength"], "strength", 0.1),
            track(&["范围", "範囲", "range"], "range", 1.0),
        ],
        colors: &[],
        integers: &[],
        flags: &[],
    },
    FilterMapping {
        effect_id: "vibration",
        aliases: &["振动", "振動", "Vibration"],
        tracks: &[
            track(&["X", "x"], "x", 1.0),
            track(&["Y", "y"], "y", 1.0),
            track(&["周期", "周期", "speed"], "speed", 1.0),
        ],
        colors: &[],
        integers: &[],
        flags: &[],
    },
    FilterMapping {
        effect_id: "mirror",
        aliases: &["镜像", "ミラー", "Mirror"],
        tracks: &[
            track(&["透明度", "透明度"], "transparency", 0.01),
            track(&["淡化", "フェード"], "decay", 0.01),
        ],
        colors: &[],
        integers: &[],
        flags: &[],
    },
    FilterMapping {
        effect_id: "polar_transform",
        aliases: &["极坐标变换", "極座標変換", "Polar transform"],
        tracks: &[
            track(&["中心径", "中心径"], "centerX", 1.0),
            track(&["缩放率", "拡大率"], "scale", 0.01),
            track(&["旋转", "回転"], "angleOffset", 1.0),
        ],
        colors: &[],
        integers: &[],
        flags: &[],
    },
    FilterMapping {
        effect_id: "raster",
        aliases: &["光栅", "ラスター", "Raster"],
        tracks: &[
            track(&["宽度", "幅", "width"], "width", 1.0),
            track(&["高度", "高さ", "height"], "height", 1.0),
            track(&["周期", "周期", "speed"], "speed", 1.0),
        ],
        colors: &[],
        integers: &[],
        flags: &[],
    },
    FilterMapping {
        effect_id: "directional_blur",
        aliases: &["方向模糊", "方向ブラー", "Directional blur"],
        tracks: &[
            track(&["范围", "範囲", "length"], "length", 1.0),
            track(&["角度", "角度", "angle"], "angle", 1.0),
        ],
        colors: &[],
        integers: &[],
        flags: &[],
    },
    FilterMapping {
        effect_id: "radial_blur",
        aliases: &["放射模糊", "放射ブラー", "Radial blur"],
        tracks: &[
            track(&["范围", "範囲"], "strength", 1.0),
            track(&["X", "x"], "x", 1.0),
            track(&["Y", "y"], "y", 1.0),
        ],
        colors: &[],
        integers: &[],
        flags: &[],
    },
    FilterMapping {
        effect_id: "lens_blur",
        aliases: &["镜头模糊", "レンズブラー", "Lens blur"],
        tracks: &[
            track(&["范围", "範囲"], "radius", 1.0),
            track(&["发光强度", "明るさ"], "brightness", 1.0),
        ],
        colors: &[],
        integers: &[],
        flags: &[],
    },
    FilterMapping {
        effect_id: "glow",
        aliases: &["发光", "グロー", "Glow"],
        tracks: &[
            track(&["强度", "強さ", "intensity"], "intensity", 0.1),
            track(&["扩散度", "拡散", "radius"], "radius", 0.1),
            track(&["阈值", "しきい値", "threshold"], "threshold", 0.1),
        ],
        colors: &[ColorMapping {
            keys: &["颜色", "色", "color"],
            param: "color",
        }],
        integers: &[],
        flags: &[],
    },
    FilterMapping {
        effect_id: "border",
        aliases: &["描边", "縁取り", "Border"],
        tracks: &[
            track(&["大小", "サイズ", "size"], "size", 1.0),
            track(&["模糊", "ぼかし", "blur"], "blur", 1.0),
        ],
        colors: &[ColorMapping {
            keys: &["颜色", "色", "color"],
            param: "color",
        }],
        integers: &[],
        flags: &[],
    },
    FilterMapping {
        effect_id: "drop_shadow",
        aliases: &["阴影", "影", "Shadow"],
        tracks: &[
            track(&["X", "x"], "x", 1.0),
            track(&["Y", "y"], "y", 1.0),
            track(&["浓度", "濃度", "opacity"], "opacity", 0.01),
            track(&["扩散度", "拡散", "radius"], "radius", 1.0),
        ],
        colors: &[ColorMapping {
            keys: &["颜色", "色", "color"],
            param: "color",
        }],
        integers: &[],
        flags: &[],
    },
    FilterMapping {
        effect_id: "chroma_key",
        aliases: &["色度键", "クロマキー", "Chroma key"],
        tracks: &[
            track(&["色相范围", "色相範囲"], "hueRange", 1.0),
            track(&["色度范围", "彩度範囲"], "similarity", 0.01),
        ],
        colors: &[],
        integers: &[],
        flags: &[],
    },
    FilterMapping {
        effect_id: "luminance_key",
        aliases: &["亮度键", "ルミナンスキー", "Luminance key"],
        tracks: &[
            track(&["基础亮度", "基準輝度"], "threshold", 0.001),
            track(&["模糊", "ぼかし"], "blend", 0.001),
        ],
        colors: &[],
        integers: &[],
        flags: &[],
    },
    FilterMapping {
        effect_id: "gradient",
        aliases: &["渐变", "グラデーション", "Gradient"],
        tracks: &[
            track(&["强度", "強さ"], "strength", 0.1),
            track(&["中心X", "中心X"], "centerX", 1.0),
            track(&["中心Y", "中心Y"], "centerY", 1.0),
            track(&["角度", "角度"], "angle", 1.0),
            track(&["宽", "幅"], "width", 1.0),
        ],
        colors: &[],
        integers: &[],
        flags: &[],
    },
    FilterMapping {
        effect_id: "image_loop",
        aliases: &["图像拼贴", "イメージループ", "Image loop"],
        tracks: &[
            track(&["横向数", "横"], "countX", 1.0),
            track(&["纵向数", "縦"], "countY", 1.0),
            track(&["速度X", "速度X"], "intervalX", 1.0),
            track(&["速度Y", "速度Y"], "intervalY", 1.0),
        ],
        colors: &[],
        integers: &[],
        flags: &[],
    },
];

/// Finds the object mapping for an ExEdit `_name`.
pub fn object_mapping(display_name: &str) -> Option<&'static ObjectMapping> {
    OBJECTS
        .iter()
        .find(|mapping| mapping.aliases.contains(&display_name))
}

/// Finds the filter mapping for an ExEdit `_name`.
pub fn filter_mapping(display_name: &str) -> Option<&'static FilterMapping> {
    FILTERS
        .iter()
        .find(|mapping| mapping.aliases.contains(&display_name))
}

/// Reports whether a display name is ExEdit's drawing/transform filter.
pub fn is_drawing_filter(display_name: &str) -> bool {
    DRAWING_ALIASES.contains(&display_name)
}

/// The blend modes ExEdit stores as an index in the drawing filter's `blend` key.
///
/// The order matches `transform.json`'s `blendMode` options, which is the order AviUtl uses
/// for its own 合成モード list.
pub const BLEND_MODES: [&str; 16] = [
    "通常",
    "スクリーン",
    "乗算",
    "オーバーレイ",
    "加算",
    "減算",
    "比較（明）",
    "比較（暗）",
    "色反転",
    "ソフトライト",
    "ハードライト",
    "差の絶対値",
    "色相",
    "彩度",
    "カラー",
    "輝度",
];
