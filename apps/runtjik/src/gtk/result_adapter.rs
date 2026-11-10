//! Translate launcher provider records into toolkit-neutral presentation fields.
use gtk::prelude::*;
use serde_json::Value;
use tjik_gtk::results::Presentation;
pub use tjik_gtk::results::{track_pointer, Layout};

fn text<'a>(value: &'a Value, key: &str) -> &'a str {
    value[key].as_str().unwrap_or("")
}
fn presentation(value: &Value, clipboard_icon: &str) -> Presentation {
    let provider = text(value, "provider");
    let preview = if provider == "Emoji" {
        text(value, "preview")
    } else {
        ""
    };
    let thumbnail = text(value, "thumbnail");
    let source = if !thumbnail.is_empty() {
        thumbnail
    } else if provider == "Clipboard" && !clipboard_icon.is_empty() {
        clipboard_icon
    } else if !text(value, "favicon").is_empty() {
        text(value, "favicon")
    } else {
        text(value, "icon")
    };
    Presentation {
        id: text(value, "id").into(),
        title: text(value, "name").into(),
        subtitle: text(value, "subtitle").into(),
        image: source.into(),
        fallback_icon: if provider == "Browser" {
            "web-browser"
        } else {
            "application-x-executable"
        }
        .into(),
        text_preview: preview.into(),
        large_image: !thumbnail.is_empty(),
        closing: value["closing"] == true,
        markers: u8::from(value["windows"].as_u64().unwrap_or(0) > 0)
            + u8::from(!preview.is_empty() && value["has_tones"] == true),
    }
}
pub fn factory(
    layout: Layout,
    icon_size: i32,
    selected: impl Fn(u32) + 'static,
) -> gtk::SignalListItemFactory {
    let clipboard_icon = std::env::var("RUNTJIK_HOME")
        .map(|home| format!("{home}/assets/icons/commands/runtjik-cliphist.svg"))
        .unwrap_or_default();
    tjik_gtk::results::factory(layout, icon_size, selected, move |object| {
        let value = object
            .downcast_ref::<gtk::StringObject>()
            .and_then(|item| serde_json::from_str(&item.string()).ok())
            .unwrap_or(Value::Null);
        presentation(&value, &clipboard_icon)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn image_precedence_and_browser_fallback_remain_application_policy() {
        let mut value =
            json!({"provider":"Clipboard", "thumbnail":"thumb", "favicon":"fav", "icon":"icon"});
        assert_eq!(presentation(&value, "clip").image, "thumb");
        value["thumbnail"] = json!("");
        assert_eq!(presentation(&value, "clip").image, "clip");
        value["provider"] = json!("Browser");
        let browser = presentation(&value, "clip");
        assert_eq!(browser.image, "fav");
        assert_eq!(browser.fallback_icon, "web-browser");
        value["favicon"] = json!("");
        assert_eq!(presentation(&value, "clip").image, "icon");
    }

    #[test]
    fn text_preview_is_only_for_emoji_and_does_not_discard_metadata() {
        let mut value = json!({"id":"fixture", "name":"<title>", "subtitle":"sub", "provider":"Emoji", "preview":"👍", "has_tones":true, "windows":2, "closing":true});
        let row = presentation(&value, "");
        assert_eq!(row.text_preview, "👍");
        assert_eq!(row.markers, 2);
        assert_eq!(row.title, "<title>");
        assert!(row.closing);
        value["provider"] = json!("Other");
        let row = presentation(&value, "");
        assert!(row.text_preview.is_empty());
        assert_eq!(row.markers, 1);
    }
}
