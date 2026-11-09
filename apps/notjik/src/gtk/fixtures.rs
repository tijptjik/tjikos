//! Synthetic, account-free cards used by the gallery and widget acceptance tests.
use super::model::Notice;
use serde_json::json;
use std::path::Path;
pub fn notices(root: &Path, now: f64, media_root: Option<&Path>) -> Vec<Notice> {
    let photo = url::Url::from_file_path(root.join("qml/icons/wifi-cover.png"))
        .unwrap()
        .to_string();
    let media = |file: &str| {
        let path = media_root
            .map(|p| p.join(file))
            .filter(|p| p.is_file())
            .unwrap_or_else(|| {
                root.join("examples").join(match file {
                    "voice.ogg" => "demo-voice.wav",
                    "motion.gif" => "demo-animated-sticker.gif",
                    "motion.mp4" => "demo-animation.mp4",
                    _ => file,
                })
            });
        url::Url::from_file_path(path).unwrap().to_string()
    };
    let mut cards = vec![
        json!({"type":"standard","title":"Relay link established","body":"Earth, eventually.\nThe survey team is ready.","actions":[{"label":"Open manifest"},{"label":"Open home folder"},{"label":"A longer action label"}]}),
        json!({"type":"status","title":"Reactor fault","body":"Engineering · deck 4","urgency":"critical","data":{"state":"Failed","tone":"critical","detail":"Containment field unstable"}}),
        json!({"type":"progress","title":"Star chart transfer","data":{"value":75,"label":"Resolving jump coordinates"}}),
        json!({"type":"timer","title":"Jump window","data":{"seconds":30,"label":"Spooling the jump drive","doneLabel":"Coordinates locked"}}),
        json!({"type":"results-table","title":"Surface survey complete","data":{"results":[{"label":"Sectors mapped","value":12,"ingredients":[{"label":"New signals","value":3}]},{"label":"Unresolved readings","value":0}],"state":"ALL SYSTEMS GO","tone":"success","duration":"42s","rainbow-text":true}}),
        json!({"type":"key-value","title":"Invoice received","body":"This raw body is replaced by structured fields","data":{"heading":"Deep-space logistics","fields":[{"label":"Invoice","value":"DS-2048"},{"label":"Total","value":"HK$ 1,250.00"}]}}),
        json!({"type":"auth-code","title":"Sign-in verification","data":{"code":"804 216"}}),
        json!({"type":"todo","title":"Review the sector map","body":"Check the three new signals before departure.","data":{"taskId":"synthetic-task","period":"synthetic-period","group":"Navigation","due":"today"}}),
        json!({"type":"telegram","title":"Maya Chen","body":"That moon was not on the chart.\nCan you check the readings?","data":{"kind":"text","styledText":"That <b>moon</b> was not on the chart.<br>Can you check the readings?"}}),
        json!({"type":"telegram","title":"Survey team","body":"First light on Nacre.","data":{"kind":"photo","source":photo}}),
        json!({"type":"telegram","title":"Maya Chen","body":"See you at the café tomorrow.","data":{"kind":"voice","source":media("voice.ogg"),"duration":8,"waveform":[4,8,20,31,18,9,22,30,16,8,4,19,27,30,14,5]}}),
        json!({"type":"telegram","title":"Survey team","data":{"kind":"sticker","source":media("demo-sticker.png"),"format":"png","width":128,"height":128,"fallback":"🛰️"}}),
        json!({"type":"telegram","title":"Maya Chen","data":{"kind":"emoji","source":media("motion.gif"),"format":"gif","fallback":"✨"}}),
        json!({"type":"telegram","title":"Survey team","body":"Orbit acquired.","data":{"kind":"animation","source":media("demo-robot-dog.gif"),"format":"gif","width":128,"height":128,"fallback":"Animation preview"}}),
        json!({"type":"telegram","title":"Maya Chen","data":{"kind":"sticker","source":media("demo-animated-sticker.gif"),"format":"gif","width":128,"height":128}}),
        json!({"type":"telegram","title":"Survey team","data":{"kind":"animation","source":media("motion.mp4"),"format":"video","width":240,"height":144}}),
        json!({"type":"telegram","title":"Maya Chen","body":"Hello 🙂 from Telegram","data":{"kind":"text","parts":[{"kind":"text","text":"Hello "},{"kind":"emoji","text":"🙂","source":media("demo-animation.gif"),"format":"gif"},{"kind":"text","text":" from Telegram"}]}}),
        json!({"type":"telegram","title":"Maya Chen","data":{"kind":"sticker","source":media("demo-video-sticker.webm"),"format":"video","width":128,"height":128}}),
        json!({"type":"telegram","title":"Maya Chen","data":{"kind":"sticker","source":media("demo-sticker.webp"),"format":"webp","width":128,"height":128}}),
    ];
    for (i, n) in cards.iter_mut().enumerate() {
        n["id"] = json!(format!("demo-{i}"));
        n["app"] = json!("notjik demo");
        n["received"] = json!(now);
        n["updatedAt"] = json!(now);
        n["robotIndex"] = json!(i);
        if !n["data"].is_object() {
            n["data"] = json!({});
        }
        n["data"]["demo"] = json!(true);
        if n["type"] == "telegram" {
            n["data"]["chatId"] = json!("-100000");
            n["data"]["messageId"] = json!((i + 1).to_string());
        }
    }
    cards.into_iter().map(Notice).collect()
}
