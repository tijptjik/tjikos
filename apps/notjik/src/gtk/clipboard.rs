use super::model::*;
fn clean(s: String) -> String {
    s.replace("\r\n", "\n").replace('\r', "\n").trim().into()
}
fn inline(s: &str) -> String {
    let s = clean(s.into())
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;");
    let mut out = String::new();
    for c in s.chars() {
        if "\\`*_[]{}|".contains(c) {
            out.push('\\');
        }
        if c == '\n' {
            out.push_str("<br>");
        } else {
            out.push(c);
        }
    }
    out
}
fn field(k: &str, v: &str) -> String {
    format!("**{}:** {}", inline(k), inline(v))
}
fn table(rows: Vec<(String, String)>) -> String {
    if rows.is_empty() {
        return String::new();
    }
    let mut s = "| Field | Value |\n| --- | --- |".to_string();
    for (k, v) in rows {
        s.push_str(&format!("\n| {} | {} |", inline(&k), inline(&v)));
    }
    s
}
pub fn markdown(n: &Notice, now: f64) -> String {
    let d = n.data();
    let mut parts = vec![clean(n.body())];
    match n.kind() {
        "status" => {
            parts.push(field("Status", &fallback(&d["state"], "Status")));
            parts.push(clean(text(&d["detail"])));
        }
        "progress" => parts.push(field(
            &fallback(&d["label"], "Progress"),
            &format!("{}%", number(&d["value"]).round()),
        )),
        "timer" => {
            let s = n.remaining(now);
            parts.push(field(
                &fallback(
                    &d[if s == 0 { "doneLabel" } else { "label" }],
                    if s == 0 { "Ready" } else { "Remaining" },
                ),
                &stamp(s),
            ));
        }
        "results-table" => {
            let mut rows = vec![];
            for r in d["results"].as_array().into_iter().flatten() {
                rows.push((fallback(&r["label"], "Result"), text(&r["value"])));
                for i in r["ingredients"].as_array().into_iter().flatten() {
                    rows.push((format!("↳ {}", text(&i["label"])), text(&i["value"])));
                }
            }
            parts.push(table(rows));
            parts.push(field("Status", &fallback(&d["state"], "Running")));
            if !text(&d["duration"]).is_empty() {
                parts.push(field("Duration", &text(&d["duration"])));
            }
        }
        "key-value" => {
            parts.clear();
            if !text(&d["heading"]).is_empty() {
                parts.push(format!("### {}", inline(&text(&d["heading"]))));
            }
            parts.push(table(
                d["fields"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|r| (text(&r["label"]), text(&r["value"])))
                    .collect(),
            ));
        }
        "todo" => {
            parts.push(field("Group", &fallback(&d["group"], "General")));
            parts.push(field("Due", &fallback(&d["due"], "today")));
        }
        "telegram" => {
            if n.body().trim().is_empty() {
                parts.push(clean(text(&d["caption"])));
            }
            let media = match d["kind"].as_str().unwrap_or("") {
                "photo" => "Photo",
                "voice" => "Voice message",
                "sticker" => "Sticker",
                "emoji" => "Animated emoji",
                "animation" => "Animation",
                _ => "",
            };
            if !media.is_empty() {
                let mut description = media.to_string();
                if number(&d["duration"]) > 0. {
                    description.push_str(&format!(
                        " ({})",
                        stamp(number(&d["duration"]).round() as u64)
                    ));
                }
                if !text(&d["fallback"]).is_empty() {
                    description.push_str(&format!(" · {}", text(&d["fallback"])));
                }
                parts.push(field("Media", &description));
            }
            if !text(&d["transcript"]).is_empty() {
                parts.push(format!(
                    "**Transcript:**\n\n{}",
                    clean(text(&d["transcript"]))
                ));
            }
        }
        _ => {}
    }
    let s = parts
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n");
    if s.is_empty() {
        clean(n.title())
    } else {
        s
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn exports_content_without_execution_or_private_metadata() {
        let n = Notice(
            json!({"type":"results-table","body":"Full **body**","data":{"results":[{"label":"<x>|","value":0,"ingredients":[{"label":"a","value":"b\nc"}]}],"state":"Done","source":"/private"},"actions":[{"command":["secret"]}]}),
        );
        let s = markdown(&n, 0.);
        assert!(s.contains("&lt;x&gt;\\|"));
        assert!(s.contains("b<br>c"));
        assert!(s.contains("| 0 |"));
        assert!(!s.contains("private"));
        assert!(!s.contains("secret"));
    }
    #[test]
    fn parsed_content_replaces_raw_body() {
        let n = Notice(
            json!({"type":"key-value","body":"raw secret","data":{"heading":"Parsed","fields":[{"label":"Count","value":false}]}}),
        );
        let s = markdown(&n, 0.);
        assert!(!s.contains("raw secret"));
        assert!(s.contains("false"));
    }
}
