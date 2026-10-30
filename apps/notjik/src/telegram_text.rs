//! Display-only Telegram payloads. UTF-16 entities never become executable HTML.
use crate::util::merge;
use base64::Engine;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
pub fn text(v: &Value) -> &str {
    v.as_str().unwrap_or("")
}
pub fn trunc(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}
pub fn waveform(v: &Value) -> Vec<u8> {
    let s = text(v);
    if s.is_empty() || s.len() > 4096 {
        return vec![];
    }
    let Ok(b) = base64::engine::general_purpose::STANDARD.decode(s) else {
        return vec![];
    };
    (0..b.len() * 8)
        .step_by(5)
        .filter(|i| i + 5 <= b.len() * 8)
        .map(|i| {
            let word = b[i / 8] as u16 | ((b.get(i / 8 + 1).copied().unwrap_or(0) as u16) << 8);
            ((word >> (i % 8)) & 31) as u8
        })
        .collect()
}
pub fn transcription(v: &Value) -> Value {
    match text(&v["@type"]) {
        "speechRecognitionResultPending" => {
            json!({"transcriptionState":"pending","transcript":trunc(text(&v["partial_text"]),65536)})
        }
        "speechRecognitionResultText" => {
            json!({"transcriptionState":"done","transcript":trunc(text(&v["text"]),65536)})
        }
        "speechRecognitionResultError" => {
            json!({"transcriptionState":"error","transcriptionError":trunc(v["error"]["message"].as_str().unwrap_or("Transcription failed"),4096)})
        }
        _ => json!({"transcriptionState":"idle","transcript":""}),
    }
}
fn tags(s: &str) -> Option<(&'static str, &'static str)> {
    Some(match s {
        "textEntityTypeBold" => ("<b>", "</b>"),
        "textEntityTypeItalic" => ("<i>", "</i>"),
        "textEntityTypeUnderline" | "textEntityTypeTextUrl" | "textEntityTypeUrl" => {
            ("<u>", "</u>")
        }
        "textEntityTypeStrikethrough" => ("<s>", "</s>"),
        "textEntityTypeCode" | "textEntityTypePre" | "textEntityTypePreCode" => {
            ("<font face=\"monospace\">", "</font>")
        }
        _ => return None,
    })
}
fn positions(s: &str) -> BTreeMap<usize, usize> {
    let mut p = BTreeMap::from([(0, 0)]);
    let mut units = 0;
    for (i, c) in s.char_indices() {
        units += c.len_utf16();
        p.insert(units, i + c.len_utf8());
    }
    p
}
pub fn styled(f: &Value, start: usize, end: Option<usize>) -> String {
    let s = trunc(text(&f["text"]), 65536);
    let p = positions(&s);
    let end = end
        .unwrap_or(*p.last_key_value().unwrap().0)
        .min(*p.last_key_value().unwrap().0);
    if start > end || !p.contains_key(&start) || !p.contains_key(&end) {
        return String::new();
    }
    let mut ranges = vec![];
    for e in f["entities"].as_array().into_iter().flatten() {
        if let (Some(tag), Some(a), Some(len)) = (
            tags(text(&e["type"]["@type"])),
            e["offset"].as_u64(),
            e["length"].as_u64(),
        ) {
            let Some(b) = a.checked_add(len) else {
                continue;
            };
            let (a, b) = (a as usize, b as usize);
            if len > 0 && p.contains_key(&a) && p.contains_key(&b) {
                ranges.push((a, b, tag));
            }
        }
    }
    let mut bounds = BTreeSet::from([start, end]);
    for (a, b, _) in &ranges {
        if *a < end && *b > start {
            bounds.insert(start.max(*a));
            bounds.insert(end.min(*b));
        }
    }
    let bounds = bounds.into_iter().collect::<Vec<_>>();
    let mut out = String::new();
    for w in bounds.windows(2) {
        let mut active = vec![];
        for (a, b, t) in &ranges {
            if *a <= w[0] && w[1] <= *b && !active.contains(t) {
                active.push(*t);
            }
        }
        for t in &active {
            out.push_str(t.0)
        }
        let val = s[p[&w[0]]..p[&w[1]]]
            .replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('\t', "    ")
            .replace('\n', "<br>");
        let re = regex::Regex::new(r" {2,}").unwrap();
        out.push_str(&re.replace_all(&val, |c: &regex::Captures| {
            format!("{} ", "&nbsp;".repeat(c[0].len() - 1))
        }));
        for t in active.iter().rev() {
            out.push_str(t.1)
        }
    }
    out
}
fn plain_parts(parts: &mut Vec<Value>, s: &str, mut offset: usize, f: &Value) {
    let re = regex::Regex::new(r"\n|[^\S\n]+|[^\s]+").unwrap();
    for m in re.find_iter(s) {
        let word = m.as_str();
        let stop = offset + word.encode_utf16().count();
        let mut v = json!({"kind":"text","text":word});
        if !word.chars().all(char::is_whitespace) {
            v["styledText"] = json!(styled(f, offset, Some(stop)));
        }
        parts.push(v);
        offset = stop;
    }
}
pub fn emoji_parts(f: &Value) -> Value {
    let s = trunc(text(&f["text"]), 4096);
    let p = positions(&s);
    let mut entities = f["entities"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|e| e["type"]["@type"] == "textEntityTypeCustomEmoji")
        .collect::<Vec<_>>();
    entities.sort_by_key(|e| e["offset"].as_i64().unwrap_or(0));
    entities.truncate(24);
    if entities.is_empty() {
        return json!([]);
    }
    let mut parts = vec![];
    let mut end = 0;
    for e in entities {
        let (Some(a), Some(len)) = (e["offset"].as_u64(), e["length"].as_u64()) else {
            continue;
        };
        let Some(b) = a.checked_add(len) else {
            continue;
        };
        let (a, b) = (a as usize, b as usize);
        if a < end || b <= a || !p.contains_key(&a) || !p.contains_key(&b) {
            continue;
        }
        plain_parts(&mut parts, &s[p[&end]..p[&a]], end, f);
        let id = e["type"]["custom_emoji_id"]
            .as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| e["type"]["custom_emoji_id"].to_string());
        parts.push(json!({"kind":"emoji","text":&s[p[&a]..p[&b]],"styledText":styled(f,a,Some(b)),"emojiId":id,"source":"","format":""}));
        end = b;
    }
    plain_parts(&mut parts, &s[p[&end]..], end, f);
    json!(parts)
}
pub fn sticker(s: &Value, kind: &str) -> (Value, Value) {
    let format = match text(&s["format"]["@type"]) {
        "stickerFormatWebp" => "webp",
        "stickerFormatTgs" => "tgs",
        "stickerFormatWebm" => "webm",
        _ => "",
    };
    (
        json!({"kind":kind,"source":"","format":format,"status":"Downloading sticker…","fallback":s.get("emoji").unwrap_or(&json!("")),"width":s.get("width").unwrap_or(&json!(256)),"height":s.get("height").unwrap_or(&json!(256))}),
        s["sticker"].clone(),
    )
}
pub fn describe(m: &Value, preview: bool) -> (String, Value, Value) {
    let c = &m["content"];
    let kind = text(&c["@type"]);
    let mut d = json!({"kind":"text","source":"","status":""});
    let plain = |body: &str| {
        (
            body.to_string(),
            json!({"kind":"text","source":"","status":""}),
            Value::Null,
        )
    };
    if !preview {
        return plain("New message");
    }
    if !m["self_destruct_type"].is_null()
        || m["self_destruct_in"].as_f64().unwrap_or(0.) > 0.
        || c["is_secret"] == true
        || m["can_be_saved"] == false
    {
        return plain("Protected message · open in Telegram");
    }
    let caption = text(&c["caption"]["text"]).to_string();
    let f = if kind == "messageText" {
        &c["text"]
    } else {
        &c["caption"]
    };
    if f.is_object() {
        if f["entities"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|e| e["type"]["@type"] == "textEntityTypeSpoiler")
        {
            return plain("Message with spoiler · open in Telegram");
        }
        if f["entities"].as_array().is_some_and(|a| !a.is_empty()) {
            d["styledText"] = json!(styled(f, 0, None));
        }
        d["parts"] = emoji_parts(f);
    }
    match kind {
        "messageSticker" | "messageAnimatedEmoji" => {
            let animated = kind == "messageAnimatedEmoji";
            let (media, file) = sticker(
                if animated {
                    &c["animated_emoji"]["sticker"]
                } else {
                    &c["sticker"]
                },
                if animated { "emoji" } else { "sticker" },
            );
            merge(&mut d, &media);
            if animated {
                d["fallback"] = json!(text(&c["emoji"]));
            }
            (String::new(), d, file)
        }
        "messageAnimation" => {
            if c["has_spoiler"] == true {
                return plain("Animation with spoiler · open in Telegram");
            }
            let a = &c["animation"];
            merge(
                &mut d,
                &json!({"kind":"animation","format":if a["mime_type"]=="image/gif"{"gif"}else{"video"},"width":a.get("width").unwrap_or(&json!(320)),"height":a.get("height").unwrap_or(&json!(200)),"status":"Downloading animation…"}),
            );
            (caption, d, a["animation"].clone())
        }
        "messagePhoto" => {
            if c["has_spoiler"] == true {
                return plain("Photo with spoiler · open in Telegram");
            }
            let sizes = c["photo"]["sizes"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|s| s["photo"]["id"].as_i64().is_some_and(|i| i != 0))
                .collect::<Vec<_>>();
            let preferred = sizes
                .iter()
                .copied()
                .filter(|s| {
                    s["width"]
                        .as_u64()
                        .unwrap_or(0)
                        .max(s["height"].as_u64().unwrap_or(0))
                        <= 1280
                })
                .collect::<Vec<_>>();
            let size = if preferred.is_empty() {
                sizes
            } else {
                preferred
            }
            .into_iter()
            .max_by_key(|s| {
                s["width"]
                    .as_u64()
                    .unwrap_or(0)
                    .saturating_mul(s["height"].as_u64().unwrap_or(0))
            });
            merge(
                &mut d,
                &json!({"kind":"photo","status":"Downloading photo…"}),
            );
            (
                caption,
                d,
                size.map(|s| s["photo"].clone()).unwrap_or(Value::Null),
            )
        }
        "messageVoiceNote" => {
            let v = &c["voice_note"];
            merge(
                &mut d,
                &json!({"kind":"voice","duration":v.get("duration").unwrap_or(&json!(0)),"waveform":waveform(&v["waveform"]),"status":"Downloading voice message…"}),
            );
            merge(&mut d, &transcription(&v["speech_recognition_result"]));
            (caption, d, v["voice"].clone())
        }
        "messageText" => (text(&c["text"]["text"]).to_string(), d, Value::Null),
        _ => {
            let label = match kind {
                "messageVideo" => "Video",
                "messageVideoNote" => "Video message",
                "messageDocument" => "Document",
                "messageAudio" => "Audio",
                "messageLocation" => "Location",
                "messagePoll" => "Poll",
                "messageContact" => "Contact",
                _ => "New message · open in Telegram",
            };
            (
                if caption.is_empty() {
                    label.to_string()
                } else {
                    caption
                },
                d,
                Value::Null,
            )
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn escaped_nested_utf16() {
        let f = json!({"text":"😀<hi>","entities":[{"offset":2,"length":4,"type":{"@type":"textEntityTypeBold"}},{"offset":3,"length":2,"type":{"@type":"textEntityTypeItalic"}}]});
        assert_eq!(
            styled(&f, 0, None),
            "😀<b>&lt;</b><b><i>hi</i></b><b>&gt;</b>"
        );
    }
    #[test]
    fn protected_media_has_no_download() {
        let m = json!({"can_be_saved":false,"content":{"@type":"messagePhoto","photo":{"sizes":[{"photo":{"id":1}}]}}});
        assert!(describe(&m, true).2.is_null());
    }
}
#[cfg(test)]
mod reference_contracts {
    use super::*;
    #[test]
    fn matches_frozen_display_contract() {
        let cases: Value =
            serde_json::from_str(include_str!("../tests/fixtures/telegram-display.json")).unwrap();
        for (i, c) in cases.as_array().unwrap().iter().enumerate() {
            let args = &c["args"];
            let actual = match text(&c["function"]) {
                "describe" => {
                    let (body, data, file) = describe(&args[0], args[1].as_bool().unwrap_or(true));
                    json!([body, data, file])
                }
                "styled_text" => json!(styled(&args[0], 0, None)),
                "emoji_parts" => emoji_parts(&args[0]),
                "decode_waveform" => json!(waveform(&args[0])),
                "transcription_data" => transcription(&args[0]),
                other => panic!("Unknown fixture {other}"),
            };
            assert_eq!(actual, c["expected"], "display contract case {i}");
        }
    }
}
