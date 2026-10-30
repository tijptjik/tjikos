use crate::util::*;
use anyhow::{bail, Result};
use serde_json::Value;
fn lua(value: &str) -> String {
    let mut out = String::from("\"");
    for c in value.chars() {
        if c == '\\' || c == '"' {
            out.push('\\');
            out.push(c);
        } else if (c as u32) < 32 {
            out.push_str(&format!("\\{:03}", c as u32));
        } else {
            out.push(c);
        }
    }
    out.push('"');
    out
}
pub fn focus_commands(
    client: &Value,
    monitors: &[Value],
    is_lua: bool,
) -> Result<Vec<Vec<String>>> {
    let address = s(&client["address"]);
    if !address.starts_with("0x")
        || address.len() < 3
        || !address[2..].bytes().all(|c| c.is_ascii_hexdigit())
    {
        bail!("Invalid compositor window address");
    }
    let monitor = monitors.iter().find(|m| m["id"] == client["monitor"]);
    let workspace = s(&client["workspace"]["name"]);
    let mut commands = vec![];
    let focus = |name: &str| {
        if is_lua {
            vec![format!("hl.dsp.focus({{ monitor = {} }})", lua(name))]
        } else {
            vec!["focusmonitor".into(), name.into()]
        }
    };
    let toggle = |name: &str| {
        let name = if name == "special" {
            ""
        } else {
            name.strip_prefix("special:").unwrap_or(name)
        };
        if is_lua {
            vec![format!("hl.dsp.workspace.toggle_special({})", lua(name))]
        } else {
            vec!["togglespecialworkspace".into(), name.into()]
        }
    };
    if let Some(m) = monitor {
        for other in monitors {
            let special = s(&other["specialWorkspace"]["name"]);
            if special.is_empty() || (other["id"] == m["id"] && special == workspace) {
                continue;
            }
            commands.push(focus(s(&other["name"])));
            commands.push(toggle(special));
        }
        commands.push(focus(s(&m["name"])));
        if (workspace == "special" || workspace.starts_with("special:"))
            && s(&m["specialWorkspace"]["name"]) != workspace
        {
            commands.push(toggle(workspace));
        }
    }
    commands.push(if is_lua {
        vec![format!(
            "hl.dsp.focus({{ window = {} }})",
            lua(&format!("address:{address}"))
        )]
    } else {
        vec!["focuswindow".into(), format!("address:{address}")]
    });
    Ok(commands
        .into_iter()
        .map(|mut args| {
            let mut prefix = vec!["hyprctl".into(), "dispatch".into()];
            prefix.append(&mut args);
            prefix
        })
        .collect())
}
pub fn focus(client: &Value) -> Result<()> {
    let is_lua = command(&["hyprctl", "eval", "return"])
        .map(|v| v.trim() == "ok")
        .unwrap_or(false);
    let monitors: Vec<Value> = serde_json::from_str(&command(&["hyprctl", "-j", "monitors"])?)?;
    for argv in focus_commands(client, &monitors, is_lua)? {
        let result = run(&argv, None, 3)?;
        if result.trim() != "ok" {
            bail!("Window switch failed: {}", result.trim());
        }
    }
    Ok(())
}
pub fn clients() -> Result<Vec<Value>> {
    if std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_none() {
        return Ok(vec![]);
    }
    Ok(serde_json::from_str(&command(&[
        "hyprctl", "-j", "clients",
    ])?)?)
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn special_workspace_focus() {
        let c = json!({"address":"0xabc","monitor":2,"workspace":{"name":"special:notes"}});
        let m = vec![
            json!({"id":1,"name":"DP-1","specialWorkspace":{"name":"special:other"}}),
            json!({"id":2,"name":"DP-2","specialWorkspace":{"name":""}}),
        ];
        let commands = focus_commands(&c, &m, false).unwrap();
        assert_eq!(
            commands
                .iter()
                .map(|v| v[2..].join(" "))
                .collect::<Vec<_>>(),
            [
                "focusmonitor DP-1",
                "togglespecialworkspace other",
                "focusmonitor DP-2",
                "togglespecialworkspace notes",
                "focuswindow address:0xabc"
            ]
        );
    }
    #[test]
    fn rejects_address_injection() {
        assert!(focus_commands(&json!({"address":"0xabc;exec bad"}), &[], true).is_err());
        assert_eq!(lua("a\"\n\\b"), "\"a\\\"\\010\\\\b\"");
    }
}
