use serde_json::Value;
use we_core::wallpaper::{
    properties::{UserProperty, UserPropertyKind, UserPropertySchema},
    settings::WallpaperSettings,
};

#[derive(Debug, Clone, Default)]
pub(crate) struct PropertyFilter {
    pub query: String,
    pub modified_only: bool,
    pub show_internal: bool,
}

pub(crate) struct PropertyMatch<'a> {
    pub property: &'a UserProperty,
    pub label: String,
    pub group: Option<String>,
    pub modified: bool,
    pub internal: bool,
    pub decorative: bool,
}

pub(crate) struct PropertyResults<'a> {
    pub entries: Vec<PropertyMatch<'a>>,
    pub total: usize,
    pub modified: usize,
}

pub(crate) fn search<'a>(
    schema: &'a UserPropertySchema,
    settings: &WallpaperSettings,
    filter: &PropertyFilter,
) -> PropertyResults<'a> {
    let query = normalize(&filter.query);
    let tokens = query.split_whitespace().collect::<Vec<_>>();
    let mut results = PropertyResults { entries: Vec::new(), total: 0, modified: 0 };
    let mut group = None;
    let mut group_visible = true;
    let values = schema
        .entries
        .iter()
        .map(|property| {
            (
                property.key.clone(),
                settings.user_properties.get(&property.key).unwrap_or(&property.default).clone(),
            )
        })
        .collect();

    // The schema is already in author order. A group marker applies to every
    // following entry until the next marker, even when a filter hides its rows.
    for property in &schema.entries {
        let label = plain_text(&property.label);
        let condition_visible =
            super::property_condition::evaluate(property.condition.as_deref(), &values);
        if matches!(&property.kind, UserPropertyKind::Unsupported(kind) if kind == "group") {
            group = (!label.is_empty()).then_some(label);
            group_visible = condition_visible;
            continue;
        }

        let decorative = label.is_empty()
            && property.label.contains('<')
            && (!matches!(property.kind, UserPropertyKind::Html)
                || plain_text(property.default.as_str().unwrap_or("")).is_empty())
            && !matches!(property.kind, UserPropertyKind::File | UserPropertyKind::Directory);
        let internal = decorative
            || !group_visible
            || !condition_visible
            || matches!(property.kind, UserPropertyKind::Unsupported(_));
        if internal && !filter.show_internal {
            continue;
        }
        let label = if label.is_empty() && !decorative { property.key.clone() } else { label };

        let editable = is_editable(property);
        let modified = is_modified(property, settings);
        results.total += usize::from(filter.show_internal || editable);
        results.modified += usize::from(modified);
        if (!editable && (!tokens.is_empty() || filter.modified_only) && !filter.show_internal)
            || (filter.modified_only && !modified)
        {
            continue;
        }

        if !tokens.is_empty() {
            let mut fields = vec![label.clone()];
            if let Some(group) = &group {
                fields.push(group.clone());
            }
            if matches!(property.kind, UserPropertyKind::Combo) {
                let current =
                    settings.user_properties.get(&property.key).unwrap_or(&property.default);
                fields.extend(
                    property
                        .options
                        .iter()
                        .filter(|option| filter.show_internal || option.value == *current)
                        .map(|option| plain_text(&option.label)),
                );
            }
            if filter.show_internal {
                fields.push(property.key.clone());
            }
            let haystack = normalize(&fields.join("\n"));
            if !tokens.iter().all(|token| haystack.contains(token)) {
                continue;
            }
        }

        results.entries.push(PropertyMatch {
            property,
            label,
            group: group.clone(),
            modified,
            internal,
            decorative,
        });
    }
    results
}

/// Extract display text without Markdown decoration, links, or HTML entities.
pub(crate) fn plain_text(value: &str) -> String {
    let rendered = html2text::config::with_decorator(html2text::render::TrivialDecorator::new())
        .raw_mode(true)
        .unicode_strikeout(false)
        .string_from_read(value.as_bytes(), value.len().max(80))
        .unwrap_or_else(|_| value.to_owned());
    let text = rendered.split_whitespace().collect::<Vec<_>>().join(" ");
    match text.as_str() {
        "ui_browse_properties_scheme_color" => "Scheme color".to_owned(),
        _ => text,
    }
}

pub(crate) fn is_editable(property: &UserProperty) -> bool {
    matches!(
        property.kind,
        UserPropertyKind::Boolean
            | UserPropertyKind::Slider
            | UserPropertyKind::Combo
            | UserPropertyKind::Color
            | UserPropertyKind::Text
            | UserPropertyKind::File
            | UserPropertyKind::Directory
    )
}

pub(crate) fn is_modified(property: &UserProperty, settings: &WallpaperSettings) -> bool {
    is_editable(property)
        && settings
            .user_properties
            .get(&property.key)
            .is_some_and(|value| !equivalent(value, &property.default))
}

fn equivalent(left: &Value, right: &Value) -> bool {
    if left == right {
        return true;
    }
    let (Value::Number(left), Value::Number(right)) = (left, right) else {
        return false;
    };
    let (float, integer) = match (left.is_f64(), right.is_f64()) {
        (true, false) => (left.as_f64().unwrap(), right),
        (false, true) => (right.as_f64().unwrap(), left),
        _ => return false,
    };
    if float.fract() != 0.0 {
        return false;
    }
    // Compare in the integer domain so distinct values beyond f64's exact
    // integer range do not become equal after rounding or saturating casts.
    if let Some(value) = integer.as_u64() {
        float >= 0.0 && float < u64::MAX as f64 && float as u64 == value
    } else if let Some(value) = integer.as_i64() {
        float >= i64::MIN as f64 && float < 0.0 && float as i64 == value
    } else {
        false
    }
}

fn normalize(value: &str) -> String {
    value
        .chars()
        .flat_map(char::to_lowercase)
        .filter_map(|character| {
            Some(match character {
                'à' | 'á' | 'ả' | 'ã' | 'ạ' | 'ă' | 'ằ' | 'ắ' | 'ẳ' | 'ẵ' | 'ặ' | 'â' | 'ầ'
                | 'ấ' | 'ẩ' | 'ẫ' | 'ậ' => 'a',
                'è' | 'é' | 'ẻ' | 'ẽ' | 'ẹ' | 'ê' | 'ề' | 'ế' | 'ể' | 'ễ' | 'ệ' => {
                    'e'
                }
                'ì' | 'í' | 'ỉ' | 'ĩ' | 'ị' => 'i',
                'ò' | 'ó' | 'ỏ' | 'õ' | 'ọ' | 'ô' | 'ồ' | 'ố' | 'ổ' | 'ỗ' | 'ộ' | 'ơ' | 'ờ'
                | 'ớ' | 'ở' | 'ỡ' | 'ợ' => 'o',
                'ù' | 'ú' | 'ủ' | 'ũ' | 'ụ' | 'ư' | 'ừ' | 'ứ' | 'ử' | 'ữ' | 'ự' => {
                    'u'
                }
                'ỳ' | 'ý' | 'ỷ' | 'ỹ' | 'ỵ' => 'y',
                'đ' => 'd',
                '\u{0300}' | '\u{0301}' | '\u{0302}' | '\u{0303}' | '\u{0306}' | '\u{0309}'
                | '\u{031b}' | '\u{0323}' => return None,
                _ => character,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use serde_json::{json, Value};
    use we_core::wallpaper::properties::UserPropertyKind;

    use super::*;

    fn fixture() -> UserPropertySchema {
        UserPropertySchema::from_project_json(
            &json!({"general": {"properties": {
                "intro": {"type": "html", "text": "Instructions", "value": "Read me", "order": 0},
                "loose": {"type": "textinput", "text": "Ungrouped", "value": "", "order": 1},
                "scene": {"type": "group", "text": "<h4>Scene &amp; sky</h4>", "order": 100},
                "theme": {"type": "combo", "text": "<hr><h4>Time Of Day</h4><hr>", "value": 1, "order": 101,
                    "options": [{"label": "<b>Sunrise &amp; sunset</b>", "value": 1}, {"label": "Night", "value": 2}]},
                "scheme": {"type": "color", "text": "ui_browse_properties_scheme_color", "value": "1 1 1", "order": 102},
                "scene_color": {"type": "color", "text": "Color", "value": "1 1 1", "order": 103},
                "clock": {"type": "group", "text": "Clock", "order": 109},
                "clock_color": {"type": "color", "text": "<b>Color</b>", "value": "1 1 1", "order": 110},
                "seconds": {"type": "bool", "text": "Show Seconds", "value": true, "order": 111},
                "note": {"type": "html", "text": "Clock secret instructions", "value": "Read me", "order": 112},
                "unknown": {"type": "vector", "text": "Clock secret vector", "value": "0 0", "order": 113},
                "date": {"type": "group", "text": "<h4>Date 日期</h4>", "order": 115},
                "date_color": {"type": "color", "text": "Color", "value": "1 1 1", "order": 116},
                "caption": {"type": "textinput", "text": "Ngày tháng 中文", "value": "", "order": 117},
                "audio": {"type": "group", "text": "Audio Visualization", "order": 125},
                "gain": {"type": "slider", "text": "<hr>Audio Pulse Threshold<hr>", "value": 1.0, "order": 126},
                "image": {"type": "file", "text": "Poster", "value": "", "order": 127},
                "folder": {"type": "directory", "text": "Pictures", "value": "", "order": 128}
            }}}).to_string(),
        ).expect("fixture should parse")
    }

    #[test]
    fn normal_search_does_not_expose_image_markup_or_hidden_technical_keys() {
        let schema = UserPropertySchema::from_project_json(&json!({"general":{"properties":{
            "img_src_separator":{"type":"color","text":"<img src='https://example.invalid/line.png' width='2000' height='1'>","value":"1 0 0","order":1},
            "img_gain":{"type":"slider","text":"Audio gain","value":1,"order":2},
            "mode":{"type":"combo","text":"Mode","value":"day","order":3,
                "options":[{"label":"Day","value":"day"},{"label":"Night","value":"night"}]}
        }}}).to_string()).unwrap();
        let visible = filtered(&schema, "");
        assert_eq!(keys(&visible), ["img_gain", "mode"]);
        assert_eq!(visible.total, 2);
        assert!(filtered(&schema, "img").entries.is_empty());
        assert!(filtered(&schema, "night").entries.is_empty());
        assert_eq!(keys(&filtered(&schema, "audio")), ["img_gain"]);
    }

    fn keys<'a>(results: &PropertyResults<'a>) -> Vec<&'a str> {
        results.entries.iter().map(|entry| entry.property.key.as_str()).collect()
    }

    fn filtered<'a>(schema: &'a UserPropertySchema, query: &str) -> PropertyResults<'a> {
        search(
            schema,
            &WallpaperSettings::default(),
            &PropertyFilter { query: query.to_owned(), modified_only: false, ..Default::default() },
        )
    }

    #[test]
    fn unfiltered_preserves_author_order_and_excludes_group_markers_from_rows_and_counts() {
        let schema = fixture();
        let results = filtered(&schema, "");
        assert_eq!(
            keys(&results),
            [
                "intro",
                "loose",
                "theme",
                "scheme",
                "scene_color",
                "clock_color",
                "seconds",
                "note",
                "date_color",
                "caption",
                "gain",
                "image",
                "folder"
            ]
        );
        assert_eq!(results.total, 11);
        assert_eq!(results.modified, 0);
        assert_eq!(results.entries[1].group, None);
        assert_eq!(results.entries[2].group.as_deref(), Some("Scene & sky"));
        assert_eq!(results.entries[5].group.as_deref(), Some("Clock"));
        assert_eq!(results.entries[8].group.as_deref(), Some("Date 日期"));
        assert_eq!(results.entries[10].group.as_deref(), Some("Audio Visualization"));
        assert_eq!(results.entries[2].label, "Time Of Day");
        assert_eq!(results.entries[3].label, "Scheme color");
    }

    #[test]
    fn tokens_are_case_insensitive_and_may_match_different_fields() {
        let schema = fixture();
        assert_eq!(keys(&filtered(&schema, "  cLoCk \t COLOR  ")), ["clock_color"]);
        assert_eq!(keys(&filtered(&schema, "sunset SCENE time")), ["theme"]);
        assert!(filtered(&schema, "clock sunset").entries.is_empty());
        assert!(filtered(&schema, "scene_color").entries.is_empty());
    }

    #[test]
    fn repeated_labels_are_disambiguated_by_latest_group_including_hidden_headers() {
        let schema = fixture();
        assert_eq!(
            keys(&filtered(&schema, "color")),
            ["scheme", "scene_color", "clock_color", "date_color"]
        );
        assert_eq!(keys(&filtered(&schema, "date color")), ["date_color"]);
        assert_eq!(keys(&filtered(&schema, "audio visualization")), ["gain", "image", "folder"]);
        assert_eq!(keys(&filtered(&schema, "clock")), ["clock_color", "seconds"]);
    }

    #[test]
    fn matching_option_labels_preserves_the_whole_combo() {
        let schema = fixture();
        let results = filtered(&schema, "SUNSET &");
        assert_eq!(keys(&results), ["theme"]);
        let original = schema.entries.iter().find(|entry| entry.key == "theme").unwrap();
        assert!(std::ptr::eq(results.entries[0].property, original));
        assert_eq!(results.entries[0].property.options.len(), 2);
        assert_eq!(results.entries[0].property.options[1].label, "Night");
    }

    #[test]
    fn plain_text_decodes_entities_and_removes_html_without_markdown_or_link_targets() {
        assert_eq!(plain_text("<hr><h4>Time &amp; Day</h4><hr>"), "Time & Day");
        assert_eq!(plain_text("<b>Color</b>&nbsp;&#x4e2d;&#25991;<br>Đêm"), "Color 中文 Đêm");
        assert_eq!(plain_text("<a href='https://example.invalid/secret'>Clock</a>"), "Clock");
        assert_eq!(plain_text("  Night\n  lights  "), "Night lights");
        assert_eq!(plain_text(""), "");
        assert_eq!(plain_text("ui_browse_properties_scheme_color"), "Scheme color");
    }

    #[test]
    fn unicode_is_retained_and_vietnamese_accents_fold_for_search_only() {
        let schema = fixture();
        assert_eq!(keys(&filtered(&schema, "中文")), ["caption"]);
        assert_eq!(keys(&filtered(&schema, "日期 color")), ["date_color"]);
        for query in ["ngày THÁNG", "ngay thang", "nga\u{0300}y tha\u{0301}ng"] {
            let results = filtered(&schema, query);
            assert_eq!(keys(&results), ["caption"], "query: {query}");
            assert_eq!(results.entries[0].label, "Ngày tháng 中文");
        }
        let mut schema = schema;
        schema.entries.iter_mut().find(|entry| entry.key == "caption").unwrap().label =
            "Độ sáng".into();
        assert_eq!(keys(&filtered(&schema, "do SANG")), ["caption"]);
    }

    #[test]
    fn readonly_rows_appear_only_without_filters_and_are_never_modified() {
        let schema = fixture();
        let mut settings = WallpaperSettings::default();
        for key in ["intro", "note", "unknown", "scene"] {
            settings.user_properties.insert(key.into(), json!("override"));
        }
        assert!(filtered(&schema, "secret").entries.is_empty());
        let results = search(&schema, &settings, &PropertyFilter::default());
        assert_eq!(results.modified, 0);
        for entry in results.entries.iter().filter(|entry| !is_editable(entry.property)) {
            assert!(!entry.modified);
            assert!(!is_modified(entry.property, &settings));
        }
        let results = search(
            &schema,
            &settings,
            &PropertyFilter { modified_only: true, ..Default::default() },
        );
        assert!(results.entries.is_empty());
    }

    #[test]
    fn modified_uses_effective_values_and_global_counts_even_when_query_has_no_hits() {
        let schema = fixture();
        let mut settings = WallpaperSettings::default();
        settings.user_properties.insert("gain".into(), json!(1));
        settings.user_properties.insert("seconds".into(), json!(false));
        settings.user_properties.insert("date_color".into(), json!("0 0 0"));
        settings.user_properties.insert("missing".into(), json!(123));
        let results = search(
            &schema,
            &settings,
            &PropertyFilter { query: "clock".into(), modified_only: true, ..Default::default() },
        );
        assert_eq!(keys(&results), ["seconds"]);
        assert!(results.entries[0].modified);
        assert_eq!(results.total, 11);
        assert_eq!(results.modified, 2);
        let results = search(
            &schema,
            &settings,
            &PropertyFilter {
                query: "no-such-property".into(),
                modified_only: false,
                ..Default::default()
            },
        );
        assert!(results.entries.is_empty());
        assert_eq!(results.total, 11);
        assert_eq!(results.modified, 2);
    }

    #[test]
    fn changing_filters_and_resetting_overrides_recomputes_results_without_mutation() {
        let schema = fixture();
        let original_schema = schema.clone();
        let mut settings = WallpaperSettings::default();
        settings.user_properties.insert("clock_color".into(), json!("0 0 0"));
        let original_settings = settings.clone();
        let mut filter =
            PropertyFilter { query: "date".into(), modified_only: true, ..Default::default() };
        assert!(search(&schema, &settings, &filter).entries.is_empty());
        filter.query = "clock".into();
        assert_eq!(keys(&search(&schema, &settings, &filter)), ["clock_color"]);
        filter.modified_only = false;
        assert_eq!(keys(&search(&schema, &settings, &filter)), ["clock_color", "seconds"]);
        filter.query = " \t\n ".into();
        assert_eq!(search(&schema, &settings, &filter).entries.len(), 13);
        assert_eq!(schema, original_schema);
        assert_eq!(settings, original_settings);
        settings.user_properties.clear();
        assert_eq!(search(&schema, &settings, &filter).modified, 0);
    }

    #[test]
    fn every_supported_control_is_editable_and_unknown_kinds_are_readonly() {
        let mut property = fixture().entries.remove(0);
        for kind in [
            UserPropertyKind::Boolean,
            UserPropertyKind::Slider,
            UserPropertyKind::Combo,
            UserPropertyKind::Color,
            UserPropertyKind::Text,
            UserPropertyKind::File,
            UserPropertyKind::Directory,
        ] {
            property.kind = kind;
            assert!(is_editable(&property));
        }
        for kind in [
            UserPropertyKind::Html,
            UserPropertyKind::Unsupported("group".into()),
            UserPropertyKind::Unsupported("vector".into()),
        ] {
            property.kind = kind;
            assert!(!is_editable(&property));
        }
    }

    #[test]
    fn numeric_defaults_compare_by_value_without_rounding_distinct_large_integers() {
        let mut property = fixture().entries.into_iter().find(|entry| entry.key == "gain").unwrap();
        let mut settings = WallpaperSettings::default();
        let cases: Vec<(Value, Value, bool)> = vec![
            (json!(1.0), json!(1), false),
            (json!(1), json!(1.0), false),
            (json!(-1), json!(-1.0), false),
            (json!(0), json!(-0.0), false),
            (json!(1.5), json!(1), true),
            (json!(1), json!(1.000001), true),
            (json!(1), json!("1"), true),
            (json!(true), json!(1), true),
            (json!(9_007_199_254_740_993_u64), json!(9_007_199_254_740_992_u64), true),
            (json!(9_007_199_254_740_993_u64), json!(9_007_199_254_740_992_f64), true),
            (json!(u64::MAX), json!(u64::MAX as f64), true),
        ];
        for (default, current, modified) in cases {
            property.default = default;
            settings.user_properties.insert(property.key.clone(), current.clone());
            assert_eq!(
                is_modified(&property, &settings),
                modified,
                "default: {}, current: {current}",
                property.default
            );
        }
    }

    #[test]
    fn internal_mode_reveals_hidden_rows_and_technical_keys_without_changing_values() {
        let schema = UserPropertySchema::from_project_json(&json!({"general":{"properties":{
            "enabled":{"type":"bool","text":"Enable dock","value":false,"order":1},
            "secret_path":{"type":"file","text":"Dock image","value":"a.png","condition":"enabled.value","order":2},
            "img_src_line":{"type":"color","text":"<img src='line.png' width='2000' height='1'>","value":"1 0 0","order":3}
        }}}).to_string()).unwrap();
        let mut settings = WallpaperSettings::default();
        let unchanged = schema.values_json(&settings.user_properties);
        let normal = filtered(&schema, "");
        assert_eq!(keys(&normal), ["enabled"]);
        assert!(filtered(&schema, "dock image").entries.is_empty());
        let internal = search(
            &schema,
            &settings,
            &PropertyFilter { show_internal: true, ..Default::default() },
        );
        assert_eq!(keys(&internal), ["enabled", "secret_path", "img_src_line"]);
        assert_eq!(internal.total, 3);
        assert!(internal.entries[1].internal);
        assert!(internal.entries[2].decorative);
        let filtered = search(
            &schema,
            &settings,
            &PropertyFilter { query: "img_src".into(), show_internal: true, ..Default::default() },
        );
        assert_eq!(keys(&filtered), ["img_src_line"]);
        assert_eq!(schema.values_json(&settings.user_properties), unchanged);
        settings.user_properties.insert("enabled".into(), json!(true));
        let normal = search(&schema, &settings, &PropertyFilter::default());
        assert_eq!(keys(&normal), ["enabled", "secret_path"]);
    }

    #[test]
    fn every_normal_search_result_is_in_the_same_modes_cleared_list() {
        let schema = fixture();
        for show_internal in [false, true] {
            let cleared = search(
                &schema,
                &WallpaperSettings::default(),
                &PropertyFilter { show_internal, ..Default::default() },
            );
            let baseline = keys(&cleared);
            for query in ["img", "clock", "color", "secret", "unknown", "folder", "中文", "night"]
            {
                let results = search(
                    &schema,
                    &WallpaperSettings::default(),
                    &PropertyFilter { query: query.into(), show_internal, ..Default::default() },
                );
                assert!(keys(&results).iter().all(|key| baseline.contains(key)), "{query}");
                assert_eq!(results.total, cleared.total);
            }
        }
    }

    #[test]
    fn empty_schema_has_no_rows_or_counts() {
        let schema = UserPropertySchema { entries: Vec::new() };
        let results = filtered(&schema, "anything");
        assert!(results.entries.is_empty());
        assert_eq!(results.total, 0);
        assert_eq!(results.modified, 0);
    }
}
