//! Offline Blitz rendering. Only already-approved, decoded raster resources enter the DOM.
use anyhow::{Result, ensure};
use anyrender::ImageRenderer;
use blitz_dom::DocumentConfig;
use blitz_html::HtmlDocument;
use blitz_traits::{
    net::{NetHandler, NetProvider, Request},
    shell::{ColorScheme, Viewport},
};
use std::{
    collections::{HashMap, HashSet},
    hash::{Hash, Hasher},
    sync::{Arc, mpsc},
};

pub type Resources = Vec<(String, Vec<u8>)>;
pub struct LinkArea {
    pub url: String,
    pub rect: [f32; 4],
}
pub struct Page {
    pub link_areas: Vec<LinkArea>,
    pub tiles: Vec<(u32, Vec<u8>)>,
    pub links: Vec<(String, String)>,
}
struct Offline(HashMap<String, Vec<u8>>);
impl NetProvider for Offline {
    fn fetch(&self, _: usize, request: Request, handler: Box<dyn NetHandler>) {
        // Never use the network or filesystem; CSS URLs are subject to the same rule.
        handler.bytes(
            request.url.to_string(),
            self.0
                .get(request.url.as_str())
                .cloned()
                .unwrap_or_default()
                .into(),
        );
    }
}

pub fn source(message: &crate::model::Message) -> Option<String> {
    message
        .payload
        .walk()
        .into_iter()
        .find(|p| p.mime_type == "text/html" && p.filename.is_empty())
        .map(|p| p.decoded_text())
}

fn prepare(
    html: &str,
    expanded: bool,
    light: bool,
    resources: &Resources,
) -> (String, Offline, Vec<(String, String)>) {
    let mut doc = scraper::Html::parse_document(html);
    let selector = scraper::Selector::parse(if expanded {
        "script, iframe, object, embed, form, input, button, textarea, select, meta, base, link, audio, video, svg"
    } else {
        "script, iframe, object, embed, form, input, button, textarea, select, meta, base, link, audio, video, svg, .gmail_quote, .yahoo_quoted, blockquote[type=cite]"
    }).unwrap();
    let removed: Vec<_> = doc.select(&selector).map(|n| n.id()).collect();
    for id in removed {
        doc.tree.get_mut(id).unwrap().detach();
    }
    let mut allowed = HashMap::new();
    let images: Vec<_> = doc
        .select(&scraper::Selector::parse("img").unwrap())
        .map(|n| (n.id(), n.attr("src").unwrap_or("").to_string()))
        .collect();
    for (id, src) in images {
        let url = format!("https://mail-image.invalid/{}", allowed.len());
        let bytes = resources
            .iter()
            .find(|(name, _)| name == &src)
            .map(|(_, b)| b);
        let mut node = doc.tree.get_mut(id).unwrap();
        let scraper::Node::Element(element) = node.value() else {
            continue;
        };
        element
            .attrs
            .retain(|(key, _)| !matches!(key.local.as_ref(), "src" | "srcset" | "sizes"));
        if let Some(bytes) = bytes {
            allowed.insert(url.clone(), bytes.clone());
            // Reparse a tiny element to obtain the qualified attribute name without a second html5ever version.
            let fragment = scraper::Html::parse_fragment(&format!("<img src='{url}'>"));
            let image = fragment
                .select(&scraper::Selector::parse("img").unwrap())
                .next()
                .unwrap();
            element.attrs.extend(image.value().attrs.clone());
        }
    }
    let links = doc
        .select(&scraper::Selector::parse("a[href]").unwrap())
        .filter_map(|n| {
            let href = n.attr("href")?;
            let url = url::Url::parse(href).ok()?;
            if !matches!(url.scheme(), "https" | "http" | "mailto") {
                return None;
            }
            Some((n.text().collect::<String>(), url.to_string()))
        })
        .take(100)
        .collect();
    let (bg, fg) = if light {
        ("#f4f1ec", "#252334")
    } else {
        ("#26233a", "#e5e5e5")
    };
    // Keep author colours paired on their own panels; never invert image pixels.
    let css = format!(
        "<style>html{{background:{bg};color:{fg};font:15px sans-serif}}body{{margin:0;padding:4px;overflow-wrap:anywhere}}img{{max-width:100%;height:auto}}table{{max-width:100%}}a{{color:#4e9bd9}}pre{{white-space:pre-wrap}}</style>"
    );
    (format!("{css}{}", doc.html()), Offline(allowed), links)
}

pub fn render(
    html: &str,
    width: u32,
    expanded: bool,
    light: bool,
    resources: &Resources,
) -> Result<Page> {
    ensure!(html.len() <= 2_000_000, "HTML is too large; use Text view");
    let width = width.clamp(160, 2400);
    let (html, provider, links) = prepare(html, expanded, light, resources);
    let mut doc = HtmlDocument::from_html(
        &html,
        DocumentConfig {
            viewport: Some(Viewport::new(
                width,
                1,
                1.,
                if light {
                    ColorScheme::Light
                } else {
                    ColorScheme::Dark
                },
            )),
            net_provider: Some(Arc::new(provider)),
            ..Default::default()
        },
    );
    doc.handle_messages();
    doc.resolve(0.);
    let height = doc.root_element().final_layout().size.height.ceil().max(1.) as u32;
    ensure!(
        height <= 24000,
        "HTML exceeds the reader limit; use Text view"
    );
    let mut link_areas = Vec::new();
    if let Ok(nodes) = doc.query_selector_all("a[href]") {
        for node in nodes.into_iter().take(100) {
            let Some(url) = doc
                .get_node(node)
                .and_then(|node| node.attr("href".into()))
                .and_then(|url| url::Url::parse(url).ok())
            else {
                continue;
            };
            if !matches!(url.scheme(), "http" | "https" | "mailto") {
                continue;
            }
            for rect in doc.node_client_rects(node) {
                if rect.width > 0. && rect.height > 0. {
                    link_areas.push(LinkArea {
                        url: url.to_string(),
                        rect: [
                            rect.x as f32,
                            rect.y as f32,
                            rect.width as f32,
                            rect.height as f32,
                        ],
                    });
                }
            }
        }
    }
    let mut tiles = Vec::new();
    for y in (0..height).step_by(1024) {
        let h = (height - y).min(1024);
        let mut renderer = anyrender_vello_cpu::VelloCpuImageRenderer::new(width, h);
        let mut rgba = Vec::new();
        renderer.render_to_vec(
            |scene| blitz_paint::paint_scene(scene, &mut doc, 1., width, h, 0, y),
            &mut rgba,
        );
        let mut png = std::io::Cursor::new(Vec::new());
        image::RgbaImage::from_raw(width, h, rgba)
            .unwrap()
            .write_to(&mut png, image::ImageFormat::Png)?;
        tiles.push((h, png.into_inner()));
    }
    Ok(Page {
        tiles,
        links,
        link_areas,
    })
}

struct Job {
    key: u64,
    html: String,
    width: u32,
    expanded: bool,
    light: bool,
    resources: Resources,
}
pub struct Renderer {
    sender: mpsc::SyncSender<Job>,
    receiver: mpsc::Receiver<(u64, Result<Page>)>,
    pending: HashSet<u64>,
    pub cache: HashMap<u64, Result<Page>>,
}
impl Renderer {
    pub fn new() -> Self {
        let (sender, jobs) = mpsc::sync_channel::<Job>(4);
        let (results, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            for job in jobs {
                let result = std::panic::catch_unwind(|| {
                    render(
                        &job.html,
                        job.width,
                        job.expanded,
                        job.light,
                        &job.resources,
                    )
                })
                .unwrap_or_else(|_| {
                    Err(anyhow::anyhow!(
                        "Blitz could not render this message; use Text view"
                    ))
                });
                if results.send((job.key, result)).is_err() {
                    break;
                }
            }
        });
        Self {
            sender,
            receiver,
            pending: HashSet::new(),
            cache: HashMap::new(),
        }
    }
    pub fn poll(&mut self) -> bool {
        let mut changed = false;
        for (key, result) in self.receiver.try_iter() {
            self.pending.remove(&key);
            if self.cache.len() >= 12 {
                self.cache.clear();
            }
            self.cache.insert(key, result);
            changed = true;
        }
        changed
    }
    pub fn request(
        &mut self,
        html: String,
        width: u32,
        expanded: bool,
        light: bool,
        resources: Resources,
    ) -> u64 {
        let mut hash = std::collections::hash_map::DefaultHasher::new();
        (&html, width, expanded, light, &resources).hash(&mut hash);
        let key = hash.finish();
        if !self.pending.contains(&key) && !self.cache.contains_key(&key) {
            if self
                .sender
                .try_send(Job {
                    key,
                    html,
                    width,
                    expanded,
                    light,
                    resources,
                })
                .is_ok()
            {
                self.pending.insert(key);
            }
        }
        key
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lays_out_tables_and_reflows_at_reader_width() {
        let html = include_str!("../assets/demo-email.html");
        let narrow = render(html, 260, false, false, &vec![]).unwrap();
        let wide = render(html, 650, false, false, &vec![]).unwrap();
        assert!(
            narrow.tiles.iter().map(|t| t.0).sum::<u32>() > wide.tiles.iter().map(|t| t.0).sum()
        );
        assert_eq!(wide.links[0].1, "https://gpui.rs/");
        assert!(!wide.link_areas.is_empty());
        let image = image::load_from_memory(&wide.tiles[0].1)
            .unwrap()
            .to_rgba8();
        assert_eq!(image.width(), 650);
        assert!(
            image
                .pixels()
                .any(|p| p[0] > 240 && p[1] > 240 && p[2] > 240)
        );
    }
    #[test]
    fn removes_active_content_and_quotes_and_only_maps_approved_images() {
        let input = "<script>alert(1)</script><iframe src='file:///etc/passwd'></iframe><p>New</p><div class='gmail_quote'>Old</div><img src='https://tracker.test/a' srcset='https://tracker.test/b 2x'>";
        let (html, net, _) = prepare(input, false, false, &vec![]);
        assert!(!html.contains("<script"));
        assert!(!html.contains("<iframe"));
        assert!(!html.contains("Old"));
        assert!(!html.contains("tracker.test"));
        assert!(net.0.is_empty());
        assert!(prepare(input, true, false, &vec![]).0.contains("Old"));
    }
    #[test]
    fn renders_approved_inline_image_without_inverting_it() {
        let mut bytes = std::io::Cursor::new(Vec::new());
        image::RgbaImage::from_pixel(20, 20, image::Rgba([255, 0, 0, 255]))
            .write_to(&mut bytes, image::ImageFormat::Png)
            .unwrap();
        let page = render(
            "<img src='cid:red' width='20' height='20'>",
            300,
            false,
            false,
            &vec![("cid:red".into(), bytes.into_inner())],
        )
        .unwrap();
        let image = image::load_from_memory(&page.tiles[0].1)
            .unwrap()
            .to_rgba8();
        assert!(
            image
                .pixels()
                .filter(|p| p[0] == 255 && p[1] == 0 && p[2] == 0)
                .count()
                >= 350
        );
    }
}
