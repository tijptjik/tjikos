//! Host-supplied page metadata. Providers own transport, storage and cancellation.
use std::{future::Future, pin::Pin};

#[derive(Clone, Debug, Default)]
pub struct PagePreview {
    pub url: String,
    pub description: String,
    /// Local thumbnail URI; remote image fetching belongs to the provider.
    pub image: String,
}

pub type PreviewFuture = Pin<Box<dyn Future<Output = Vec<PagePreview>>>>;

/// Called on the GTK thread with at most four distinct URLs. Return immediately;
/// perform I/O asynchronously. Dropping the future must cancel its owned work.
/// The view runs one request at a time and ignores results for removed references.
///
/// A downstream adapter needs only the public trait and value types:
/// ```
/// use tjik_task_gtk::previews::{PagePreview, PreviewFuture, PreviewProvider};
/// use std::rc::Rc;
///
/// struct Offline;
/// impl PreviewProvider for Offline {
///     fn load(&self, urls: Vec<String>) -> PreviewFuture {
///         Box::pin(async move {
///             urls.into_iter().map(|url| PagePreview {
///                 url, description: "Offline preview".into(), image: String::new(),
///             }).collect()
///         })
///     }
/// }
/// let provider: Rc<dyn PreviewProvider> = Rc::new(Offline);
/// let pending = provider.load(vec!["https://example.invalid/".into()]);
/// drop(pending); // No transport work survives cancellation.
/// ```
pub trait PreviewProvider {
    fn load(&self, urls: Vec<String>) -> PreviewFuture;
}
