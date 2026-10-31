//! Read structured sections from the English Wiktionary's rendered entry HTML.
//! Only text and explicit dictionary links reach QML; remote markup is never rendered.
use crate::{
    util::*,
    words::{encode, Row},
};
use scraper::{ElementRef, Html, Selector};
use serde_json::{json, Value};

pub const SECTIONS: &[&str] = &[
    "definitions",
    "synonyms",
    "related_terms",
    "derived_terms",
    "pronunciation",
    "etymology",
    "alternative_forms",
    "antonyms",
    "usage_notes",
    "inflections",
    "translations",
];
fn selector(css: &str) -> Selector {
    Selector::parse(css).unwrap()
}
fn normalize(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}
fn skip(element: ElementRef<'_>) -> bool {
    ["script", "style", "sup", "audio", "video"].contains(&element.value().name())
        || element.value().classes().any(|c| {
            [
                "mw-editsection",
                "citation-whole",
                "noprint",
                "HQToggle",
                "maintenance-line",
                "audiometa",
            ]
            .contains(&c)
        })
}
fn text(element: ElementRef<'_>, shallow: bool) -> String {
    let mut output = String::new();
    for child in element.children() {
        if let Some(value) = child.value().as_text() {
            output.push_str(value);
        } else if let Some(child) = ElementRef::wrap(child) {
            if skip(child) || (shallow && ["ol", "ul", "dl"].contains(&child.value().name())) {
                continue;
            }
            output.push_str(&text(child, shallow));
            if ["br", "p", "li", "td", "dd"].contains(&child.value().name()) {
                output.push(' ');
            }
        }
    }
    if shallow {
        output
    } else {
        normalize(&output)
    }
}
fn own_sense(element: ElementRef<'_>, sense: ElementRef<'_>) -> bool {
    element
        .ancestors()
        .filter_map(ElementRef::wrap)
        .find(|e| {
            e.value().name() == "li"
                && e.parent()
                    .and_then(ElementRef::wrap)
                    .is_some_and(|p| p.value().name() == "ol")
        })
        .is_some_and(|e| e.id() == sense.id())
}
fn terms(element: ElementRef<'_>) -> Vec<Value> {
    let mut seen = std::collections::HashSet::new();
    element
        .select(&selector("a[href]"))
        .filter_map(|a| {
            let href = a.value().attr("href")?;
            let title = a.value().attr("title").unwrap_or("");
            if !href.starts_with("/wiki/") || title.contains(':') {
                return None;
            }
            let name = text(a, false);
            if name.is_empty()
                || ["edit", "show", "hide"].contains(&name.as_str())
                || !seen.insert(name.clone())
            {
                return None;
            }
            Some(json!({"text":name,"term":name,"url":format!("https://en.wiktionary.org{href}")}))
        })
        .collect()
}
#[derive(Default)]
struct Context {
    language: String,
    etymology: String,
    part: String,
    headword: String,
    anchor: String,
    section: String,
}
impl Context {
    fn row(&self, term: &str, name: String, section: &str, details: Value) -> Row {
        let url = format!(
            "https://en.wiktionary.org/wiki/{}#{}",
            encode(term),
            encode(&self.anchor)
        );
        let mut data = json!({"language":self.language,"section":section,"part_of_speech":self.part,
            "etymology":self.etymology,"headword":self.headword,"group":format!("{}:{}:{}:{}",self.language,self.etymology,self.part,if section=="definitions" {self.anchor.as_str()} else {section})});
        for (key, value) in details.as_object().unwrap() {
            data[key] = value.clone();
        }
        (
            name,
            format!("Wiktionary · {} · {}", self.language, self.part),
            url,
            data,
        )
    }
}
fn senses(
    list: ElementRef<'_>,
    ctx: &Context,
    term: &str,
    sections: &[String],
    synonym_mode: bool,
    prefix: &str,
    depth: usize,
    limit: usize,
    rows: &mut Vec<Row>,
    related: &mut Vec<Row>,
) {
    if depth > 8 {
        return;
    }
    for (i, (sense, name)) in list
        .children()
        .filter_map(ElementRef::wrap)
        .filter(|e| e.value().name() == "li")
        .filter(|e| !skip(*e))
        .map(|sense| (sense, normalize(&text(sense, true))))
        // MediaWiki inserts empty list items around sense IDs. They are not
        // definitions and must not consume either a number or the result limit.
        .filter(|(_, name)| !name.is_empty())
        .take(limit)
        .enumerate()
    {
        let number = if prefix.is_empty() {
            (i + 1).to_string()
        } else {
            format!("{prefix}.{}", i + 1)
        };
        let examples: Vec<_> = sense
            .select(&selector(".e-example"))
            .filter(|e| own_sense(*e, sense))
            .map(|e| text(e, false))
            .filter(|s| !s.is_empty())
            .collect();
        let synonyms: Vec<_> = sense
            .select(&selector(".nyms.synonym"))
            .filter(|e| own_sense(*e, sense))
            .flat_map(terms)
            .collect();
        if !synonym_mode && sections.iter().any(|s| s == "definitions") {
            rows.push(ctx.row(term,name.clone(),"definitions",json!({"sense_number":number,"sense_depth":depth,"examples":examples,
                "sense_synonyms":if sections.iter().any(|s|s=="synonyms"){synonyms.clone()}else{vec![]}})));
        }
        if !synonyms.is_empty()
            && sections.iter().any(|s| s == "synonyms")
            && (synonym_mode || !sections.iter().any(|s| s == "definitions"))
        {
            related.push(ctx.row(
                term,
                format!("Sense {number}"),
                "synonyms",
                json!({"terms":synonyms,"context":name}),
            ));
        }
        for nested in sense
            .children()
            .filter_map(ElementRef::wrap)
            .filter(|e| e.value().name() == "ol")
        {
            senses(
                nested,
                ctx,
                term,
                sections,
                synonym_mode,
                &number,
                depth + 1,
                limit,
                rows,
                related,
            );
        }
    }
}
pub fn parse(term: &str, data: &Value, options: &Value, synonym_mode: bool) -> Vec<Row> {
    let document = Html::parse_fragment(s(&data["parse"]["text"]));
    let Some(root) = document.select(&selector(".mw-parser-output")).next() else {
        return vec![];
    };
    let languages = strings(&options["languages"]);
    let sections = strings(&options["sections"]);
    let limit = options["max_results_per_source"].as_u64().unwrap_or(12) as usize;
    let mut ctx = Context::default();
    let mut rows = vec![];
    let heading_selector = selector("h2,h3,h4,h5,h6");
    let parts = [
        "Noun",
        "Proper noun",
        "Verb",
        "Adjective",
        "Adverb",
        "Pronoun",
        "Preposition",
        "Postposition",
        "Conjunction",
        "Interjection",
        "Determiner",
        "Article",
        "Numeral",
        "Number",
        "Participle",
        "Particle",
        "Phrase",
        "Proverb",
        "Idiom",
        "Letter",
        "Symbol",
        "Abbreviation",
        "Initialism",
        "Acronym",
        "Prefix",
        "Suffix",
        "Infix",
        "Affix",
        "Root",
        "Combining form",
        "Contraction",
        "Romanization",
        "Syllable",
        "Punctuation mark",
    ];
    for element in root.children().filter_map(ElementRef::wrap) {
        let heading = if ["h2", "h3", "h4", "h5", "h6"].contains(&element.value().name()) {
            Some(element)
        } else {
            element.select(&heading_selector).next()
        };
        if let Some(heading) = heading {
            let title = text(heading, false);
            let level = heading.value().name();
            if level == "h2" {
                ctx = Context {
                    language: title.clone(),
                    ..Context::default()
                };
            } else if parts.contains(&title.as_str()) {
                ctx.part = title.clone();
                ctx.headword.clear();
                ctx.section = "definitions".into();
            } else {
                ctx.section = title.to_lowercase().replace(' ', "_");
                if title.starts_with("Etymology") {
                    ctx.etymology = title.clone();
                    ctx.part.clear();
                    ctx.headword.clear();
                    ctx.section = "etymology".into();
                }
                if title.starts_with("Pronunciation") {
                    ctx.section = "pronunciation".into();
                }
                if ["Declension", "Conjugation", "Inflection"].contains(&title.as_str()) {
                    ctx.section = "inflections".into();
                }
                if level == "h3" && ctx.section != "definitions" {
                    ctx.part.clear();
                    ctx.headword.clear();
                }
            }
            ctx.anchor = heading.value().attr("id").unwrap_or(&title).to_owned();
            continue;
        }
        if !languages
            .iter()
            .any(|l| l.eq_ignore_ascii_case(&ctx.language))
            || skip(element)
        {
            continue;
        }
        if ctx.section == "definitions" {
            if let Some(headword) = element.select(&selector(".headword-line")).next() {
                ctx.headword = text(headword, false);
            }
            if element.value().name() == "ol" {
                let mut related = vec![];
                senses(
                    element,
                    &ctx,
                    term,
                    &sections,
                    synonym_mode,
                    "",
                    0,
                    limit,
                    &mut rows,
                    &mut related,
                );
                rows.extend(related);
            }
            continue;
        }
        if !sections.contains(&ctx.section)
            || (synonym_mode
                && !["synonyms", "related_terms", "derived_terms", "antonyms"]
                    .contains(&ctx.section.as_str()))
        {
            continue;
        }
        if [
            "synonyms",
            "related_terms",
            "derived_terms",
            "alternative_forms",
            "antonyms",
        ]
        .contains(&ctx.section.as_str())
        {
            let items: Vec<_> = if element.value().name() == "li" {
                vec![element]
            } else {
                element.select(&selector("li")).collect()
            };
            let mut links = vec![];
            for item in items {
                // Keep sense qualifiers beside their own relation, rather than flattening meanings.
                let qualifier = item
                    .select(&selector(".qualifier-content,.label-content"))
                    .map(|e| text(e, false))
                    .collect::<Vec<_>>()
                    .join(", ");
                for mut link in terms(item) {
                    if !qualifier.is_empty() {
                        link["qualifier"] = json!(qualifier);
                    }
                    links.push(link);
                }
            }
            links.truncate(limit * 10);
            if !links.is_empty() {
                rows.push(ctx.row(term, String::new(), &ctx.section, json!({"terms":links})));
            }
        } else if ctx.section == "pronunciation" {
            for item in element.select(&selector("li")) {
                let name = text(item, false);
                if !name.is_empty() {
                    rows.push(ctx.row(term, name, "pronunciation", json!({})));
                }
            }
        } else if ["etymology", "usage_notes"].contains(&ctx.section.as_str()) {
            if ["p", "ul", "dl"].contains(&element.value().name()) {
                let name = text(element, false);
                if !name.is_empty() {
                    rows.push(ctx.row(term, name, &ctx.section, json!({})));
                }
            }
        } else if ctx.section == "translations" {
            for table in element.select(&selector("table.translations")) {
                let context = table.value().attr("data-gloss").unwrap_or("");
                for item in table.select(&selector("li")).take(limit * 10) {
                    let name = text(item, false);
                    if !name.is_empty() {
                        rows.push(ctx.row(term, name, "translations", json!({"context":context})));
                    }
                }
            }
        } else if ctx.section == "inflections" {
            for tr in element.select(&selector("tr")).take(limit * 10) {
                let cells = tr
                    .children()
                    .filter_map(ElementRef::wrap)
                    .filter(|e| ["td", "th"].contains(&e.value().name()))
                    .map(|e| text(e, false))
                    .collect::<Vec<_>>();
                let name = cells.join(" · ");
                if !name.is_empty() {
                    rows.push(ctx.row(term, name, "inflections", json!({})));
                }
            }
        }
    }
    rows.sort_by_key(|row| {
        languages
            .iter()
            .position(|l| l.eq_ignore_ascii_case(s(&row.3["language"])))
            .unwrap_or(usize::MAX)
    });
    rows
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> Value {
        json!({"parse":{"text":include_str!("../tests/fixtures/word-entry.html").replace("{{term}}","glimmer")}})
    }
    fn options() -> Value {
        crate::config::contract()["DEFAULTS"]["plugin_settings"]["Words"].clone()
    }
    #[test]
    fn keeps_nested_senses_examples_and_parts_separate() {
        let rows = parse("glimmer", &fixture(), &options(), false);
        let definitions: Vec<_> = rows
            .iter()
            .filter(|r| r.3["section"] == "definitions")
            .collect();
        assert_eq!(definitions.len(), 5);
        assert_eq!(definitions[0].3["sense_number"], "1");
        assert_eq!(definitions[1].3["sense_number"], "1.1");
        assert_eq!(definitions[1].3["sense_depth"], 1);
        assert_eq!(
            definitions[0].3["examples"],
            json!(["A glimmer appeared beyond the trees."])
        );
        assert!(!definitions[0].0.contains("hoped for"));
        assert_eq!(definitions[0].3["sense_synonyms"][0]["term"], "gleam");
        assert_eq!(definitions[3].3["part_of_speech"], "Verb");
        assert_ne!(definitions[0].3["group"], definitions[4].3["group"]);
        assert!(rows.iter().all(|r| r.3["language"] == "English"));
        assert!(rows
            .iter()
            .all(|r| ["definitions", "synonyms", "related_terms"].contains(&s(&r.3["section"]))));
    }
    #[test]
    fn empty_sense_placeholders_do_not_consume_numbers_or_limits() {
        let data = json!({"parse":{"text":r#"
            <div class="mw-parser-output"><h2 id="English">English</h2><h3 id="Adjective">Adjective</h3>
            <ol><li class="mw-empty-elt"></li><li>First synthetic meaning.
                <ol><li class="mw-empty-elt"></li><li>Nested synthetic meaning.
                    <dl><dd><span class="nyms synonym"><a href="/wiki/example">example</a></span></dd></dl>
                </li></ol></li>
                <li> </li><li><span class="maintenance-line">Maintenance only</span></li>
                <li>Second synthetic meaning.</li><li>Beyond the configured limit.</li>
            </ol></div>"#}});
        let mut options = options();
        options["max_results_per_source"] = json!(2);
        let rows = parse("synthetic", &data, &options, false);
        assert_eq!(rows.len(), 3);
        assert_eq!(
            rows.iter()
                .map(|row| s(&row.3["sense_number"]))
                .collect::<Vec<_>>(),
            ["1", "1.1", "2"]
        );
        assert_eq!(rows[1].3["sense_depth"], 1);
        assert_eq!(rows[2].0, "Second synthetic meaning.");
        let synonyms = parse("synthetic", &data, &options, true);
        assert_eq!(synonyms.len(), 1);
        assert_eq!(synonyms[0].0, "Sense 1.1");
    }
    #[test]
    fn filters_languages_sections_and_thesaurus_without_inventing_terms() {
        let mut options = options();
        options["languages"] = json!(["French"]);
        let rows = parse("glimmer", &fixture(), &options, false);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].3["language"], "French");
        options["languages"] = json!(["English"]);
        options["sections"] = json!(SECTIONS);
        let rows = parse("glimmer", &fixture(), &options, false);
        for section in SECTIONS {
            assert!(
                rows.iter().any(|r| r.3["section"] == *section),
                "missing {section}"
            );
        }
        assert!(!rows
            .iter()
            .any(|r| r.0.contains("never display style text")));
        let rows = parse("glimmer", &fixture(), &options, true);
        assert!(rows.iter().all(
            |r| ["synonyms", "related_terms", "derived_terms", "antonyms"]
                .contains(&s(&r.3["section"]))
        ));
        assert!(rows.iter().any(|r| r.3["terms"]
            .as_array()
            .unwrap()
            .iter()
            .any(|t| t["term"] == "gleam")));
        options["sections"] = json!(["definitions"]);
        assert!(parse("glimmer", &fixture(), &options, true).is_empty());
    }
    #[test]
    fn validates_new_word_settings_and_defaults() {
        assert!(crate::config::parse(
            "[plugin_settings.Words]\nlanguages=[\"French\"]\nsections=[\"etymology\"]"
        )
        .is_ok());
        for source in [
            "languages=[]",
            "languages=[3]",
            "languages=[\"English\",\"english\"]",
            "sections=[\"unknown\"]",
        ] {
            assert!(crate::config::parse(&format!("[plugin_settings.Words]\n{source}")).is_err());
        }
        assert_eq!(
            options()["define_sources"],
            json!(["wiktionary", "urban_dictionary"])
        );
        assert_eq!(options()["synonym_sources"], json!(["wiktionary"]));
    }
    #[test]
    fn live_response_fixture_when_supplied() {
        let Ok(path) = std::env::var("RUNTJIK_WIKTIONARY_FIXTURE") else {
            return;
        };
        let data: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        let rows = parse("monster", &data, &options(), false);
        assert!(rows.iter().any(|r| r.3["sense_depth"] == 1));
        assert!(rows.iter().any(|r| r.3["part_of_speech"] == "Verb"));
        assert!(rows.iter().any(|r| r.3["section"] == "related_terms"));
        assert!(rows.iter().all(|r| r.3["language"] == "English"));
        println!(
            "Parsed {} structured rows from the public monster entry",
            rows.len()
        );
    }
}
