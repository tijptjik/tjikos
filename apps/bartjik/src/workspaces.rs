use crate::util::*;
use anyhow::{ensure, Result};
use serde_json::{json, Value};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::UnixStream,
};
pub async fn query(kind: &str) -> Result<Value> {
    let p = std::path::PathBuf::from(std::env::var("XDG_RUNTIME_DIR")?)
        .join("hypr")
        .join(std::env::var("HYPRLAND_INSTANCE_SIGNATURE")?)
        .join(".socket.sock");
    tokio::time::timeout(std::time::Duration::from_millis(500), async {
        let mut stream = UnixStream::connect(p).await?;
        stream.write_all(format!("j/{kind}").as_bytes()).await?;
        let mut data = Vec::new();
        stream.take(1048577).read_to_end(&mut data).await?;
        ensure!(data.len() <= 1048576, "compositor response too large");
        Ok(serde_json::from_slice(&data)?)
    })
    .await?
}
pub fn local_position(point: &Value, monitors: &Value) -> Result<Value> {
    let x = point["x"]
        .as_f64()
        .ok_or_else(|| anyhow::anyhow!("invalid cursor"))?;
    let y = point["y"]
        .as_f64()
        .ok_or_else(|| anyhow::anyhow!("invalid cursor"))?;
    for m in arr(monitors) {
        let scale = num(&m["scale"]);
        ensure!(scale > 0., "invalid scale");
        let (mut w, mut h) = (num(&m["width"]) / scale, num(&m["height"]) / scale);
        if m["transform"].as_u64().unwrap_or(0) % 2 == 1 {
            std::mem::swap(&mut w, &mut h);
        }
        if x >= num(&m["x"]) && x < num(&m["x"]) + w && y >= num(&m["y"]) && y < num(&m["y"]) + h {
            return Ok(
                json!({"valid":true,"monitor":m["name"],"x":x-num(&m["x"]),"y":y-num(&m["y"])}),
            );
        }
    }
    Ok(json!({"valid":true,"monitor":"","x":0,"y":0}))
}
pub fn offscreen(snapshot: &Value, m: &Value, wid: &Value) -> Value {
    let x = num(&m["x"]);
    let scale = num(&m["scale"]).max(0.01);
    let width = num(&m[if m["transform"].as_u64().unwrap_or(0) % 2 == 1 {
        "height"
    } else {
        "width"
    }]) / scale;
    let mut left = vec![];
    let mut right = vec![];
    if width > 0. {
        for c in arr(&snapshot["clients"]) {
            if c["monitor"] != m["id"]
                || c["workspace"]["id"] != *wid
                || c["mapped"] != true
                || truth(&c["hidden"])
                || truth(&c["floating"])
                || truth(&c["pinned"])
                || c["fullscreenHandler"] != "scrolling"
                || arr(&c["at"]).len() != 2
                || arr(&c["size"]).len() != 2
                || num(&c["size"][0]) <= 0.
            {
                continue;
            }
            let at = num(&c["at"][0]);
            let row = json!({"address":c["address"],"app":if truth(&c["class"]){c["class"].clone()}else if truth(&c["initialClass"]){c["initialClass"].clone()}else{json!("Application")},"title":s(&c["title"]),"x":at,"y":c["at"][1]});
            if at + num(&c["size"][0]) <= x + 8. {
                left.push(row)
            } else if at >= x + width - 8. {
                right.push(row)
            }
        }
    }
    for rows in [&mut left, &mut right] {
        rows.sort_by(|a, b| {
            num(&a["x"])
                .total_cmp(&num(&b["x"]))
                .then(num(&a["y"]).total_cmp(&num(&b["y"])))
                .then(s(&a["address"]).cmp(s(&b["address"])))
        });
    }
    json!({"left":left,"right":right})
}
pub fn view(snapshot: &Value, screen: &str, o: &Value) -> Vec<Value> {
    let Some(m) = arr(&snapshot["monitors"])
        .iter()
        .find(|m| m["name"] == screen)
    else {
        return vec![];
    };
    let mut owned: Vec<Value> = arr(&snapshot["workspaces"])
        .iter()
        .filter(|w| w["monitor"] == screen)
        .cloned()
        .collect();
    for name in arr(&o["persistent"][screen]) {
        let name = st(name);
        if !arr(&snapshot["workspaces"])
            .iter()
            .any(|w| st(&w["name"]) == name)
        {
            owned.push(json!({"id":null,"name":name,"monitor":screen,"placeholder":true}));
        }
    }
    owned.retain(|w| {
        (!truth(&o["occupiedOnly"])
            || num(&w["windows"]) > 0.
            || (!w["id"].is_null()
                && (w["id"] == m["activeWorkspace"]["id"]
                    || w["id"] == m["specialWorkspace"]["id"])))
            && (o["special"] != false || !s(&w["name"]).starts_with("special:"))
    });
    fn order(w: &Value) -> (i32, i64, String) {
        let name = s(&w["name"]);
        if let Ok(n) = name.parse::<i64>() {
            (0, n, String::new())
        } else {
            (
                if name.starts_with("special:") { 2 } else { 1 },
                0,
                name.to_lowercase(),
            )
        }
    }
    owned.sort_by_key(order);
    owned.iter().map(|w|{let name=s(&w["name"]);let special=name.starts_with("special:");let id=if let Ok(n)=name.parse::<u64>(){n.to_string()}else if w["id"].is_null(){format!("empty:{name}")}else{st(&w["id"])};let active=!w["id"].is_null()&&w["id"]==m[if special{"specialWorkspace"}else{"activeWorkspace"}]["id"];let focused=active&&truth(&m["focused"]);let label=o["labels"][name].as_str().unwrap_or(name.trim_start_matches("special:"));let circles=["circles","circles_name"].contains(&s(&o["label"]));let ico=icon(if focused{"circle-filled"}else{"circle-open"});let mut row=item(&id,if o["label"]=="circles"{None}else{Some(label)},if circles{Some(&ico)}else{None},"image",json!({"active":active,"focused":focused,"urgent":arr(&snapshot["urgent"]).contains(&w["id"]),"placeholder":truth(&w["placeholder"])}),&format!("{name} · {screen}"),json!({"leftClick":"workspace.activate","scrollUp":"workspace.previous","scrollDown":"workspace.next"}));row["workspace"]=name.parse::<u64>().map(|n|json!(n)).unwrap_or(json!(name));row["special"]=json!(special);if truth(&o["offscreen"])&&!w["id"].is_null()&&!special{row["offscreen"]=offscreen(snapshot,m,&w["id"]);}row}).collect()
}
pub async fn snapshot() -> Result<Value> {
    let (m, w, c) = tokio::try_join!(query("monitors"), query("workspaces"), query("clients"))?;
    ensure!(
        m.is_array() && w.is_array() && c.is_array(),
        "invalid compositor snapshot"
    );
    let urgent: Vec<_> = arr(&c)
        .iter()
        .filter(|c| truth(&c["urgent"]))
        .map(|c| c["workspace"]["id"].clone())
        .collect();
    let select = |rows: &Value, keys: &[&str]| {
        json!(arr(rows)
            .iter()
            .map(|row| keys
                .iter()
                .map(|k| (k.to_string(), row[k].clone()))
                .collect::<serde_json::Map<_, _>>())
            .collect::<Vec<_>>())
    };
    Ok(
        json!({"monitors":select(&m,&["id","name","x","y","width","height","scale","transform","activeWorkspace","specialWorkspace","focused"]),"workspaces":select(&w,&["id","name","monitor","windows"]),"clients":select(&c,&["address","monitor","workspace","at","size","mapped","hidden","floating","pinned","fullscreenHandler","class","initialClass","title"]),"urgent":urgent}),
    )
}
pub fn lua(v: &str) -> String {
    let mut r = "\"".to_owned();
    for c in v.chars() {
        match c {
            '\\' | '"' => {
                r.push('\\');
                r.push(c)
            }
            c if (c as u32) < 32 => r.push_str(&format!("\\{:03}", c as u32)),
            _ => r.push(c),
        }
    }
    r.push('"');
    r
}
