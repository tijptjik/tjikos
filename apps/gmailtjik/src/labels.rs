use crate::model::Label;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};

#[derive(Clone, Debug)]
pub struct Row {
    pub path: String,
    pub name: String,
    pub id: Option<String>,
    pub depth: usize,
    pub children: bool,
    pub color: u32,
}

// TjikOS rainbow: red, amber, green, cyan, blue, violet.
const PALETTE: [u32; 6] = [0xff5f5f, 0xffaf00, 0x5fff87, 0x5fd7ff, 0x5f87ff, 0xd787ff];
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Visibility {
    Hidden,
    #[default]
    Visible,
    Unread,
}
pub type Preferences = BTreeMap<String, Visibility>;
pub fn preference(path: &str, preferences: &Preferences) -> Visibility {
    preferences.get(path).copied().unwrap_or_else(|| {
        if ["auth", "closed", "offers", "personal"].contains(&path.to_lowercase().as_str()) {
            Visibility::Unread
        } else {
            Visibility::Visible
        }
    })
}
fn rainbow(index: usize, count: usize) -> u32 {
    let position =
        index as f32 * (PALETTE.len() - 1) as f32 / count.saturating_sub(1).max(1) as f32;
    let low = position.floor() as usize;
    let high = (low + 1).min(PALETTE.len() - 1);
    let blend = position - low as f32;
    let mut color = 0;
    for shift in [0, 8, 16] {
        let a = ((PALETTE[low] >> shift) & 255) as f32;
        let b = ((PALETTE[high] >> shift) & 255) as f32;
        color |= ((a + (b - a) * blend).round() as u32) << shift;
    }
    color
}
#[cfg(test)]
pub fn rows(labels: &[Label], collapsed: &HashSet<String>) -> Vec<Row> {
    visible_rows(labels, collapsed, &Preferences::new(), false)
}
pub fn visible_rows(
    labels: &[Label],
    collapsed: &HashSet<String>,
    preferences: &Preferences,
    show_all: bool,
) -> Vec<Row> {
    let mut roots = Vec::new();
    let mut folders = BTreeMap::<String, Option<String>>::new();
    for label in labels {
        if ["YELLOW_STAR", "YELLOR_STAR"].contains(&label.name.to_uppercase().as_str()) {
            continue;
        }
        if matches!(label.id.as_str(), "UNREAD" | "IMPORTANT" | "CHAT")
            || label.id.starts_with("CATEGORY_")
        {
            continue;
        }
        if matches!(
            label.id.as_str(),
            "INBOX" | "STARRED" | "SENT" | "DRAFT" | "SPAM" | "TRASH"
        ) {
            let name = match label.id.as_str() {
                "INBOX" => "Inbox",
                "STARRED" => "Starred",
                "SENT" => "Sent",
                "DRAFT" => "Drafts",
                "SPAM" => "Spam",
                _ => "Trash",
            };
            roots.push(Row {
                path: format!("system:{}", label.id),
                name: name.into(),
                id: Some(label.id.clone()),
                depth: 0,
                children: false,
                color: 0xa5a0ba,
            });
            continue;
        }
        let parts: Vec<_> = label.name.split('/').collect();
        for depth in 1..=parts.len() {
            folders.entry(parts[..depth].join("/")).or_insert(None);
        }
        folders.insert(label.name.clone(), Some(label.id.clone()));
    }
    roots.sort_by_key(|r| match r.id.as_deref() {
        Some("INBOX") => 0,
        Some("STARRED") => 1,
        Some("SENT") => 2,
        Some("DRAFT") => 3,
        Some("SPAM") => 4,
        _ => 5,
    });
    let root_names: Vec<_> = folders
        .keys()
        .filter(|p| !p.contains('/'))
        .cloned()
        .collect();
    let colors: BTreeMap<_, _> = root_names
        .iter()
        .enumerate()
        .map(|(i, p)| (p.clone(), rainbow(i, root_names.len())))
        .collect();
    if !show_all {
        roots.retain(|row| match preference(&row.path, preferences) {
            Visibility::Hidden => false,
            Visibility::Visible => true,
            Visibility::Unread => labels
                .iter()
                .any(|l| Some(&l.id) == row.id.as_ref() && l.messages_unread > 0),
        });
        let hidden: Vec<_> = folders
            .keys()
            .filter(|path| match preference(path, preferences) {
                Visibility::Hidden => true,
                Visibility::Visible => false,
                Visibility::Unread => !labels.iter().any(|l| {
                    (l.name == **path || l.name.starts_with(&format!("{path}/")))
                        && l.messages_unread > 0
                }),
            })
            .cloned()
            .collect();
        folders.retain(|path, _| {
            !hidden
                .iter()
                .any(|h| path == h || path.starts_with(&format!("{h}/")))
        });
    }
    fn append(
        parent: Option<&str>,
        folders: &BTreeMap<String, Option<String>>,
        collapsed: &HashSet<String>,
        out: &mut Vec<Row>,
        colors: &BTreeMap<String, u32>,
    ) {
        for (path, id) in folders {
            if path.rsplit_once('/').map(|(p, _)| p) != parent {
                continue;
            }
            let depth = path.matches('/').count();
            let root = path.split('/').next().unwrap_or(path);
            let base = colors[root];
            let amount = (depth as f32 * 0.22).min(0.66);
            let mut color = 0;
            for shift in [0, 8, 16] {
                let channel = ((base >> shift) & 255) as f32;
                color |= ((channel + (255. - channel) * amount).round() as u32) << shift;
            }
            let children = folders
                .keys()
                .any(|p| p.rsplit_once('/').is_some_and(|(parent, _)| parent == path));
            out.push(Row {
                path: path.clone(),
                name: path.rsplit('/').next().unwrap_or(path).into(),
                id: id.clone(),
                depth,
                children,
                color,
            });
            if !collapsed.contains(path) {
                append(Some(path), folders, collapsed, out, colors);
            }
        }
    }
    append(None, &folders, collapsed, &mut roots, &colors);
    roots
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn nested_holders_preserve_ids_and_inherit_lighter_colors() {
        let labels = vec![
            Label {
                messages_unread: 0,
                id: "child".into(),
                name: "Mail/Work/Projects".into(),
            },
            Label {
                messages_unread: 0,
                id: "parent".into(),
                name: "Mail".into(),
            },
            Label {
                messages_unread: 0,
                id: "other".into(),
                name: "Other".into(),
            },
        ];
        let rows = rows(&labels, &HashSet::new());
        assert_eq!(
            rows.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(),
            ["Mail", "Work", "Projects", "Other"]
        );
        assert_eq!(rows[0].id.as_deref(), Some("parent"));
        assert!(rows[1].id.is_none());
        assert_eq!(rows[2].id.as_deref(), Some("child"));
        for shift in [0, 8, 16] {
            assert!(((rows[0].color >> shift) & 255) <= ((rows[1].color >> shift) & 255));
            assert!(((rows[1].color >> shift) & 255) <= ((rows[2].color >> shift) & 255));
        }
        let collapsed = super::rows(&labels, &HashSet::from(["Mail".into()]));
        assert_eq!(collapsed.len(), 2);
        assert_eq!(collapsed[0].color, rows[0].color);
    }
    #[test]
    fn visibility_and_rainbow_follow_order_not_hashes() {
        let labels = vec![
            Label {
                id: "auth".into(),
                name: "AUTH".into(),
                messages_unread: 0,
            },
            Label {
                id: "closed".into(),
                name: "Closed/Child".into(),
                messages_unread: 1,
            },
            Label {
                id: "mail".into(),
                name: "Mail".into(),
                messages_unread: 0,
            },
            Label {
                id: "duplicate".into(),
                name: "YELLOW_STAR".into(),
                messages_unread: 5,
            },
        ];
        let visible = rows(&labels, &HashSet::new());
        assert!(
            !visible
                .iter()
                .any(|r| r.path == "AUTH" || r.path == "YELLOW_STAR")
        );
        assert!(visible.iter().any(|r| r.path == "Closed"));
        let mut preferences = Preferences::new();
        preferences.insert("Mail".into(), Visibility::Hidden);
        let hidden = visible_rows(&labels, &HashSet::new(), &preferences, false);
        assert!(!hidden.iter().any(|r| r.path == "Mail"));
        let all = visible_rows(&labels, &HashSet::new(), &preferences, true);
        assert!(all.iter().any(|r| r.path == "Mail"));
        assert_eq!(
            all.iter().find(|r| r.path == "Closed").unwrap().color,
            visible.iter().find(|r| r.path == "Closed").unwrap().color
        );
        assert_eq!(rainbow(0, 6), PALETTE[0]);
        assert_eq!(rainbow(5, 6), PALETTE[5]);
    }
}
