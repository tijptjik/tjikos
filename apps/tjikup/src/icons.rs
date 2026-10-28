//! Artwork identity is independent of the human-readable status subject.
use std::collections::BTreeMap;

pub fn resolve(
    subject: &str,
    recipe_icon: &str,
    aliases: &BTreeMap<String, String>,
    contains: impl Fn(&str) -> bool,
) -> String {
    fn base(label: &str) -> &str {
        label.split(['(', '[']).next().unwrap_or(label).trim_end()
    }
    for label in [subject, base(subject), recipe_icon, base(recipe_icon)] {
        if let Some(key) = aliases.get(label).filter(|key| contains(key)) {
            return key.clone();
        }
        if !label.is_empty() && label != "*" && contains(label) {
            return label.into();
        }
    }
    "*".into()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn report_subjects_inherit_recipe_art_without_overriding_known_services() {
        let aliases = BTreeMap::from([("Cloudflare backup".into(), "Storage".into())]);
        let known = |key: &str| {
            [
                "Herdr",
                "Systemd",
                "Tailscale",
                "Collie",
                "Storage",
                "Hyprvoice",
                "Fedora",
                "llmtrim",
            ]
            .contains(&key)
        };
        for (subject, recipe, expected) in [
            ("Plugin : File viewer", "Herdr", "Herdr"),
            ("Plugin : LLMTrim [Herdr unavailable]", "llmtrim", "llmtrim"),
            ("Collie Bridge", "Collie (bridge)", "Collie"),
            ("Common Units", "Systemd (user services)", "Systemd"),
            ("Clients Units", "Systemd", "Systemd"),
            ("Cloudflare backup", "Systemd", "Storage"),
            ("Hyprvoice", "Systemd", "Hyprvoice"),
            ("Tailscale operator for any-user", "Tailscale", "Tailscale"),
            ("Tailnet DNS and routes", "Tailscale", "Tailscale"),
            ("Fedora upgrade", "Fedora (release)", "Fedora"),
            ("Unknown task", "Unknown recipe", "*"),
        ] {
            assert_eq!(resolve(subject, recipe, &aliases, known), expected);
        }
    }
}
