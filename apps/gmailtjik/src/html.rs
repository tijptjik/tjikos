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
    pub width: u32,
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
        .root_element()
        .select(&scraper::Selector::parse("img").unwrap())
        .map(|n| (n.id(), n.attr("src").unwrap_or("").to_string()))
        .collect();
    for (id, src) in images {
        let url = format!("https://mail-image.invalid/{}", allowed.len());
        let source = crate::reader::image_key(&src);
        let bytes = resources
            .iter()
            .find(|(name, _)| name == &source)
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
        .root_element()
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
    // Author colours are retained in light mode. Dark mode normalizes computed
    // colours below, after class rules, inline styles and legacy attributes resolve.
    // Email bodies are documents rather than fixed-size web viewports. Contain
    // images, but keep wide table content available to the reader's scrollbar.
    let css = format!(
        "<style>html{{background:{bg};color:{fg};font:16px/1.5 sans-serif}}html,body{{height:auto!important;overflow:visible!important}}body{{margin:0!important;padding:12px!important;box-sizing:border-box;width:auto!important;min-width:0!important;overflow-wrap:anywhere}}img{{max-width:100%!important;height:auto!important}}table{{max-width:100%!important}}a{{color:#2166ac}}pre{{white-space:pre-wrap}}</style>"
    );
    let defaults = scraper::Html::parse_fragment(&css);
    let style = defaults
        .select(&scraper::Selector::parse("style").unwrap())
        .next()
        .unwrap();
    let head = doc
        .select(&scraper::Selector::parse("head").unwrap())
        .next()
        .unwrap()
        .id();
    doc.tree
        .get_mut(head)
        .unwrap()
        .prepend(defaults.tree.get(style.id()).unwrap().value().clone())
        .append(style.first_child().unwrap().value().clone());
    // Prepending a style before <!doctype> silently puts HTML into quirks mode.
    // A single standards document also gives fragments the same layout rules.
    (
        format!("<!doctype html>{}", doc.root_element().html()),
        Offline(allowed),
        links,
    )
}

fn luminance(color: [f32; 4]) -> f32 {
    let linear = |v: f32| {
        let v = v.clamp(0., 1.);
        if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * linear(color[0]) + 0.7152 * linear(color[1]) + 0.0722 * linear(color[2])
}

fn dark_surface(mut color: [f32; 4]) -> [f32; 4] {
    if luminance(color) > 0.08 {
        // Preserve panel hue without allowing bright backgrounds. Neutral panels
        // remain slightly raised from the app surface rather than disappearing.
        let min = color[..3].iter().copied().fold(1., f32::min);
        for (channel, base) in color[..3].iter_mut().zip([46., 42., 64.]) {
            *channel = base / 255. + (*channel - min).clamp(0., 1.) * 0.10;
        }
    }
    color
}

fn dark_foreground(mut color: [f32; 4]) -> [f32; 4] {
    let min = color[..3].iter().copied().fold(1., f32::min);
    let max = color[..3].iter().copied().fold(0., f32::max);
    if max - min < 0.12 {
        return [224. / 255., 222. / 255., 244. / 255., 1.];
    }
    // Keep meaningful author colour differences, lifting them enough for at
    // least 5:1 contrast against every normalized surface (luminance <= .08).
    for _ in 0..16 {
        if luminance(color) >= 0.65 {
            break;
        }
        for channel in &mut color[..3] {
            *channel = channel.clamp(0., 1.) * 0.85 + 0.15;
        }
    }
    color[3] = 1.;
    color
}

fn dark_border(mut color: [f32; 4]) -> [f32; 4] {
    let min = color[..3].iter().copied().fold(1., f32::min);
    for (channel, base) in color[..3].iter_mut().zip([96., 90., 121.]) {
        *channel = base / 255. + (*channel - min).clamp(0., 1.) * 0.20;
    }
    color
}

fn css_color(color: [f32; 4]) -> String {
    let rgb = color[..3]
        .iter()
        .map(|v| (v.clamp(0., 1.) * 255.).round() as u8)
        .collect::<Vec<_>>();
    format!("rgba({},{},{},{})", rgb[0], rgb[1], rgb[2], color[3])
}

fn normalize_dark(doc: &mut HtmlDocument) {
    // Read computed values before mutating any styles so inherited colours and
    // currentColor still refer to the author's original palette.
    let changes: Vec<_> = doc
        .query_selector_all("*")
        .unwrap_or_default()
        .into_iter()
        .filter_map(|id| {
            let node = doc.get_node(id)?;
            let element = node.element_data()?;
            let styles = node.primary_styles()?;
            let foreground = styles.clone_color();
            let background = styles
                .get_background()
                .background_color
                .resolve_to_absolute(&foreground)
                .into_srgb_legacy();
            let borders = styles.get_border();
            let border_colors = [
                &borders.border_top_color,
                &borders.border_right_color,
                &borders.border_bottom_color,
                &borders.border_left_color,
            ]
            .map(|color| {
                css_color(dark_border(
                    *color
                        .resolve_to_absolute(&foreground)
                        .into_srgb_legacy()
                        .raw_components(),
                ))
            });
            let mut ancestor = Some(id);
            let mut link = false;
            while let Some(id) = ancestor {
                let node = doc.get_node(id)?;
                if node
                    .element_data()
                    .is_some_and(|e| e.name.local == blitz_dom::local_name!("a"))
                    && node.attr("href".into()).is_some()
                {
                    link = true;
                    break;
                }
                ancestor = node.parent;
            }
            let foreground = if link {
                [156. / 255., 207. / 255., 216. / 255., 1.]
            } else {
                dark_foreground(*foreground.into_srgb_legacy().raw_components())
            };
            let background = if matches!(element.name.local.as_ref(), "html" | "body") {
                [38. / 255., 35. / 255., 58. / 255., 1.]
            } else {
                dark_surface(*background.raw_components())
            };
            Some((
                id,
                node.attr("style".into()).unwrap_or("").to_string(),
                css_color(foreground),
                css_color(background),
                border_colors,
            ))
        })
        .collect();
    for (id, mut style, foreground, background, border_colors) in changes {
        for (property, value) in [
            ("color", foreground.as_str()),
            ("background-color", background.as_str()),
            ("border-top-color", border_colors[0].as_str()),
            ("border-right-color", border_colors[1].as_str()),
            ("border-bottom-color", border_colors[2].as_str()),
            ("border-left-color", border_colors[3].as_str()),
            ("outline-color", "#908caa"),
            ("text-decoration-color", "currentColor"),
            // Decorative gradients, shadows and inherited inversion filters can
            // restore a bright panel or alter image pixels after colour mapping.
            ("background-image", "none"),
            ("box-shadow", "none"),
            ("text-shadow", "none"),
            ("filter", "none"),
            ("mix-blend-mode", "normal"),
        ] {
            if let Some(updated) = doc.style_attr_set_property(&style, property, value, true) {
                style = updated;
            }
        }
        doc.mutate().set_attribute(
            id,
            blitz_dom::QualName::new(None, Default::default(), "style".into()),
            &style,
        );
    }
    doc.resolve(0.);
}

pub fn render(
    html: &str,
    width: u32,
    expanded: bool,
    light: bool,
    resources: &Resources,
) -> Result<Page> {
    ensure!(
        html.len() <= 2_000_000,
        "HTML is too large; use Readable view"
    );
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
    if !light {
        normalize_dark(&mut doc);
    }
    // Fixed boxes belong to the email document, not to each raster tile's
    // viewport. Resolve authored CSS before normalizing so class rules and
    // !important declarations are handled without parsing CSS ourselves.
    let fixed: Vec<_> = doc
        .query_selector_all("*")
        .unwrap_or_default()
        .into_iter()
        .filter(|&id| {
            doc.get_node(id)
                .is_some_and(|node| node.primary_styles().is_some())
                && doc.resolved_style_value(id, "position") == "fixed"
        })
        .collect();
    if !fixed.is_empty() {
        for id in fixed {
            let style = doc
                .get_node(id)
                .and_then(|node| node.attr("style".into()))
                .unwrap_or("");
            if let Some(style) = doc.style_attr_set_property(style, "position", "absolute", true) {
                doc.mutate().set_attribute(
                    id,
                    blitz_dom::QualName::new(None, Default::default(), "style".into()),
                    &style,
                );
            }
        }
        doc.resolve(0.);
    }
    let root = doc.root_element();
    let height = root.scroll_height().ceil().max(1.) as u32;
    let width = root.scroll_width().ceil().max(width as f32) as u32;
    ensure!(
        height <= 24000,
        "HTML exceeds the reader limit; use Readable view"
    );
    ensure!(
        width <= 4800 && u64::from(width) * u64::from(height) <= 48_000_000,
        "HTML layout is too large; use Readable view"
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
        // Painter offsets translate the scene, rather than selecting a slice.
        // Scroll the document so every tile paints its own content at y = 0.
        doc.set_viewport_scroll(blitz_dom::Point {
            x: 0.,
            y: f64::from(y),
        });
        let mut renderer = anyrender_vello_cpu::VelloCpuImageRenderer::new(width, h);
        let mut rgba = Vec::new();
        renderer.render_to_vec(
            |scene| blitz_paint::paint_scene(scene, &mut doc, 1., width, h, 0, 0),
            &mut rgba,
        );
        let mut png = std::io::Cursor::new(Vec::new());
        image::RgbaImage::from_raw(width, h, rgba)
            .unwrap()
            .write_to(&mut png, image::ImageFormat::Png)?;
        tiles.push((h, png.into_inner()));
    }
    Ok(Page {
        width,
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
                        "Blitz could not render this message; use Readable view"
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
    fn defaults_do_not_put_the_document_into_quirks_mode() {
        for input in ["<!doctype html><p>Hello</p>", "<p>Hello</p>"] {
            let (html, _, _) = prepare(input, false, true, &vec![]);
            assert!(html.starts_with("<!doctype html><html><head><style>"));
            assert_eq!(html.matches("<!doctype").count(), 1);
        }
    }

    #[test]
    fn light_surface_preserves_authored_text_colors() {
        let page = render(
            "<p style='color:#333'>Readable reply</p>",
            300,
            false,
            true,
            &vec![],
        )
        .unwrap();
        let image = image::load_from_memory(&page.tiles[0].1)
            .unwrap()
            .to_rgba8();
        assert_eq!(image.get_pixel(0, 0).0, [244, 241, 236, 255]);
        assert!(image.pixels().any(|p| p.0 == [51, 51, 51, 255]));
    }

    #[test]
    fn dark_surface_overrides_legacy_and_important_authored_colors() {
        let page = render(
            "<style>.panel{background:#fff!important;color:#000!important}</style><body bgcolor='#ffffff' style='background:white!important;color:black!important'><table class='panel' width='260'><tr><td bgcolor='#fff' style='color:#000!important;padding:16px'><span style='color:rgb(0,0,0)!important'>Readable dark reply</span></td></tr></table></body>",
            300, false, false, &vec![],
        ).unwrap();
        let image = image::load_from_memory(&page.tiles[0].1)
            .unwrap()
            .to_rgba8();
        assert_eq!(image.get_pixel(0, 0).0, [38, 35, 58, 255]);
        assert!(
            image
                .pixels()
                .filter(|p| p.0 == [224, 222, 244, 255])
                .count()
                > 30
        );
        assert!(
            image
                .pixels()
                .all(|p| p.0 != [255, 255, 255, 255] && p.0 != [0, 0, 0, 255])
        );
    }

    #[test]
    fn dark_panels_keep_distinct_tints_and_readable_links_and_borders() {
        let page = render(
            "<div style='background:#ffdddd;padding:12px;border:2px solid black;border-right:8px solid transparent'><a href='https://example.test' style='color:black!important'><span style='color:#111!important'>Open the reference</span></a><div style='background:#ddffdd;height:50px;color:#770000'>Important detail</div></div>",
            300, false, false, &vec![],
        ).unwrap();
        let image = image::load_from_memory(&page.tiles[0].1)
            .unwrap()
            .to_rgba8();
        let outer = dark_surface([1., 221. / 255., 221. / 255., 1.]);
        let inner = dark_surface([221. / 255., 1., 221. / 255., 1.]);
        let rgba = |color: [f32; 4]| color.map(|v| (v * 255.).round() as u8);
        assert_ne!(rgba(outer), rgba(inner));
        assert!(image.pixels().any(|p| p.0 == rgba(outer)));
        assert!(image.pixels().any(|p| p.0 == rgba(inner)));
        assert_eq!(image.get_pixel(284, 20).0, rgba(outer));
        assert!(
            image
                .pixels()
                .filter(|p| p.0 == [156, 207, 216, 255])
                .count()
                > 10
        );
        assert!(image.pixels().any(|p| p.0 == [96, 90, 121, 255]));
        let link = [156. / 255., 207. / 255., 216. / 255., 1.];
        assert!((luminance(link) + 0.05) / (luminance(outer) + 0.05) > 7.);
        assert!(
            (luminance(dark_foreground([119. / 255., 0., 0., 1.])) + 0.05)
                / (luminance(inner) + 0.05)
                > 7.
        );
        assert_eq!(page.link_areas.len(), 1);
    }

    #[test]
    fn dark_view_removes_bright_decorative_gradients() {
        let page = render(
            "<div style='height:60px;background:linear-gradient(white,#ffffee)!important;box-shadow:0 0 10px 10px white'>Body</div>",
            300, false, false, &vec![],
        ).unwrap();
        let image = image::load_from_memory(&page.tiles[0].1)
            .unwrap()
            .to_rgba8();
        assert!(
            image
                .pixels()
                .all(|p| !(p[0] > 240 && p[1] > 240 && p[2] > 240))
        );
    }

    #[test]
    fn oversized_inline_images_keep_both_edges_and_aspect_ratio() {
        let mut bytes = std::io::Cursor::new(Vec::new());
        image::RgbaImage::from_fn(960, 240, |x, _| {
            image::Rgba(if x < 480 {
                [255, 0, 0, 255]
            } else {
                [0, 0, 255, 255]
            })
        })
        .write_to(&mut bytes, image::ImageFormat::Png)
        .unwrap();
        let page = render(
            "<img src='cid:wide' width='960' height='240' style='width:960px;height:240px'>",
            300,
            false,
            true,
            &vec![("cid:wide".into(), bytes.into_inner())],
        )
        .unwrap();
        assert_eq!(page.width, 300);
        assert!(page.tiles.iter().map(|t| t.0).sum::<u32>() < 120);
        let image = image::load_from_memory(&page.tiles[0].1)
            .unwrap()
            .to_rgba8();
        let red = image.pixels().filter(|p| p.0 == [255, 0, 0, 255]).count();
        let blue = image.pixels().filter(|p| p.0 == [0, 0, 255, 255]).count();
        assert!(
            red > 8000 && blue > 8000,
            "Both halves must remain visible: {red}, {blue}"
        );
        assert!(red.abs_diff(blue) < 150);
    }

    #[test]
    fn wide_tables_retain_rightmost_content_and_link_coordinates() {
        let page = render(
            "<table style='min-width:900px;table-layout:fixed'><tr><td>Left</td><td style='background:#0000ff'><a href='https://example.test/right'>Rightmost value</a></td></tr></table>",
            300, false, true, &vec![],
        ).unwrap();
        assert!(page.width >= 912);
        let link = &page.link_areas[0];
        assert!(link.rect[0] > 300.);
        assert!(link.rect[0] + link.rect[2] <= page.width as f32);
        let image = image::load_from_memory(&page.tiles[0].1)
            .unwrap()
            .to_rgba8();
        assert!(
            image
                .enumerate_pixels()
                .any(|(x, _, p)| x > 800 && p.0 == [0, 0, 255, 255])
        );
    }

    #[test]
    fn captures_overflow_below_the_root_across_tile_boundaries() {
        let page = render(
            "<div style='position:relative;height:10px'><a href='https://example.test/end' style='position:absolute;top:1200px;width:80px;height:50px;background:#ff0000'>End</a></div>",
            300, false, true, &vec![],
        ).unwrap();
        assert!(page.tiles.iter().map(|t| t.0).sum::<u32>() >= 1262);
        assert!(page.link_areas[0].rect[1] >= 1200.);
        let image = image::load_from_memory(&page.tiles[1].1)
            .unwrap()
            .to_rgba8();
        assert!(image.pixels().any(|p| p.0 == [255, 0, 0, 255]));
        assert!(
            image.pixels().all(|p| p[3] == 255),
            "The reading surface must continue beyond the root box"
        );
    }

    #[test]
    fn adjacent_tiles_have_no_missing_or_repeated_rows() {
        let page = render(
            "<div style='height:1012px;background:#ff0000'></div><div style='height:60px;background:#0000ff'></div>",
            300, false, true, &vec![],
        ).unwrap();
        assert_eq!(page.tiles.iter().map(|tile| tile.0).sum::<u32>(), 1096);
        let first = image::load_from_memory(&page.tiles[0].1)
            .unwrap()
            .to_rgba8();
        let second = image::load_from_memory(&page.tiles[1].1)
            .unwrap()
            .to_rgba8();
        assert_eq!(first.get_pixel(20, 1023).0, [255, 0, 0, 255]);
        assert_eq!(second.get_pixel(20, 0).0, [0, 0, 255, 255]);
        assert_eq!(second.get_pixel(20, 59).0, [0, 0, 255, 255]);
        assert_eq!(second.get_pixel(20, 60).0, [244, 241, 236, 255]);
    }

    #[test]
    fn fixed_content_and_its_link_appear_only_once() {
        for position in [
            "<style>.fixed { position:fixed!important }</style>",
            "<style>.fixed { position:fixed }</style>",
        ] {
            let page = render(
                &format!("{position}<a class='fixed' href='https://example.test/fixed' style='top:24px;left:24px;width:60px;height:40px;background:#ff00ff'>Fixed</a><div style='height:2000px'>Body</div>"),
                300, false, true, &vec![],
            ).unwrap();
            assert_eq!(page.link_areas.len(), 1);
            assert_eq!(page.link_areas[0].rect[..2], [24., 24.]);
            let first = image::load_from_memory(&page.tiles[0].1)
                .unwrap()
                .to_rgba8();
            let second = image::load_from_memory(&page.tiles[1].1)
                .unwrap()
                .to_rgba8();
            assert!(first.pixels().any(|pixel| pixel.0 == [255, 0, 255, 255]));
            assert!(second.pixels().all(|pixel| pixel.0 != [255, 0, 255, 255]));
        }
    }

    #[test]
    fn lays_out_tables_and_reflows_at_reader_width() {
        let html = include_str!("../assets/demo-email.html");
        let narrow = render(html, 260, false, false, &vec![]).unwrap();
        let wide = render(html, 900, false, false, &vec![]).unwrap();
        assert!(
            narrow.tiles.iter().map(|t| t.0).sum::<u32>() > wide.tiles.iter().map(|t| t.0).sum()
        );
        assert_eq!(wide.links[0].1, "https://gpui.rs/");
        assert!(!wide.link_areas.is_empty());
        let image = image::load_from_memory(&wide.tiles[0].1)
            .unwrap()
            .to_rgba8();
        assert_eq!(image.width(), 900);
        assert!(image.pixels().any(|p| p.0 == [224, 222, 244, 255]));
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
    fn detached_quote_and_active_content_do_not_supply_links_or_images() {
        let input = "<p><a href='https://example.test/new'>New</a><img src='cid:new'></p><div class='gmail_quote'><a href='https://example.test/old'>Old</a><img src='cid:old'></div><form><a href='https://example.test/form'>Form</a><img src='cid:form'></form>";
        let resources = vec![
            ("cid:new".into(), vec![1]),
            ("cid:old".into(), vec![2]),
            ("cid:form".into(), vec![3]),
        ];
        let (_, collapsed, links) = prepare(input, false, false, &resources);
        assert_eq!(
            links,
            vec![("New".into(), "https://example.test/new".into())]
        );
        assert_eq!(collapsed.0.len(), 1);
        assert_eq!(collapsed.0.values().next(), Some(&vec![1]));
        let (_, expanded, links) = prepare(input, true, false, &resources);
        assert_eq!(
            links,
            vec![
                ("New".into(), "https://example.test/new".into()),
                ("Old".into(), "https://example.test/old".into()),
            ]
        );
        assert_eq!(expanded.0.len(), 2);
        assert!(expanded.0.values().any(|bytes| bytes == &vec![2]));
        assert!(expanded.0.values().all(|bytes| bytes != &vec![3]));
    }
    #[test]
    fn encoded_image_references_match_only_the_approved_resource() {
        let resources = vec![
            ("cid:Part+one@mail".into(), vec![1]),
            ("cid:Part%40mail".into(), vec![2]),
            ("https://example.test/art".into(), vec![3]),
        ];
        let (_, provider, _) = prepare(
            "<img src='CID:Part+one%40mail'><img src='cid:Part%2540mail'><img src='HTTPS://EXAMPLE.TEST:443/art'><img src='file:///not-approved'>",
            false,
            false,
            &resources,
        );
        assert_eq!(provider.0.len(), 3);
        for bytes in [vec![1], vec![2], vec![3]] {
            assert!(provider.0.values().any(|approved| approved == &bytes));
        }
    }

    #[test]
    fn renders_approved_inline_image_without_inverting_it() {
        let mut bytes = std::io::Cursor::new(Vec::new());
        image::RgbaImage::from_pixel(20, 20, image::Rgba([255, 0, 0, 255]))
            .write_to(&mut bytes, image::ImageFormat::Png)
            .unwrap();
        let page = render(
            "<div style='filter:invert(1);background:white'><img src='cid:red' width='20' height='20' style='filter:invert(1)'></div>",
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
