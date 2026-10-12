//! Chat resources are inert until an explicit, validated user link activation.
//!
//! Parser hooks replace images/reference images and raw HTML with native text.
//! Base's authoritative image-source hook is also installed: the initial parse
//! precedes installing Markdown extensions, and asynchronous reparsing must never
//! expose a document-selected URI (including data URLs) during that interval.

use std::sync::Arc;
use std::sync::LazyLock;
use std::sync::atomic::{AtomicUsize, Ordering};

use gpui_kit::base::TextView;
use gpui_kit::component::text::{
    MarkdownExtensions, MarkdownNode, MarkdownParseContext, MarkdownPlugin, markdown_ast,
};
use gpui_kit::*;

pub struct ChatResourcePolicy {
    extensions: MarkdownExtensions,
    image_overrides: Arc<AtomicUsize>,
}

static POLICY: LazyLock<ChatResourcePolicy> = LazyLock::new(|| ChatResourcePolicy {
    extensions: MarkdownExtensions::default()
        .plugin(BlockedInline)
        .plugin(BlockedHtmlBlock),
    image_overrides: Arc::default(),
});

// A bundled transparent pixel, never a renderer URL, path, SVG, or data URL.
static BLOCKED_IMAGE: LazyLock<Arc<Image>> = LazyLock::new(|| {
    let mut png = std::io::Cursor::new(Vec::new());
    image::DynamicImage::new_rgba8(1, 1)
        .write_to(&mut png, image::ImageFormat::Png)
        .expect("one-pixel inert image");
    Arc::new(Image::from_bytes(ImageFormat::Png, png.into_inner()))
});

pub fn chat_policy() -> &'static ChatResourcePolicy {
    &POLICY
}

impl ChatResourcePolicy {
    pub fn apply(&self, view: TextView) -> TextView {
        let overrides = self.image_overrides.clone();
        view.markdown_extensions(self.extensions.clone())
            .image_source(move |_| {
                overrides.fetch_add(1, Ordering::Relaxed);
                ImageSource::Image(BLOCKED_IMAGE.clone())
            })
    }

    /// Diagnostic count only; never expose resource URLs or document contents.
    pub fn image_override_count(&self) -> usize {
        self.image_overrides.load(Ordering::Relaxed)
    }
}

fn blocked_node(node: &markdown_ast::Node, cx: &MarkdownParseContext<'_>) -> Option<MarkdownNode> {
    let text = match node {
        markdown_ast::Node::Image(image) => {
            format!("[Image blocked: {}]", short_label(&image.alt))
        }
        markdown_ast::Node::ImageReference(image) => {
            format!("[Image blocked: {}]", short_label(&image.alt))
        }
        markdown_ast::Node::Html(_) => "[Embedded HTML blocked; original source retained]".into(),
        _ => return None,
    };
    Some(
        MarkdownNode::new("flint-inert-resource", ())
            .text(text)
            .markdown(cx.node_source(node).unwrap_or_default()),
    )
}

fn short_label(text: &str) -> String {
    text.chars()
        .filter(|ch| !ch.is_control())
        .take(160)
        .collect()
}

struct BlockedInline;

impl MarkdownPlugin for BlockedInline {
    fn name(&self) -> &str {
        "flint-inert-resource"
    }

    fn parse(
        &self,
        node: &markdown_ast::Node,
        cx: &MarkdownParseContext<'_>,
    ) -> Option<MarkdownNode> {
        blocked_node(node, cx)
    }

    fn render(&self, node: &MarkdownNode, _: &mut Window, _: &mut App) -> impl IntoElement {
        // Literal GPUI text, not another Markdown/HTML view.
        div()
            .text_color(crate::theme::palette().text_subtle)
            .child(node.as_text().to_string())
    }
}

struct BlockedHtmlBlock;

impl MarkdownPlugin for BlockedHtmlBlock {
    fn is_block(&self) -> bool {
        true
    }

    fn name(&self) -> &str {
        "flint-inert-resource"
    }

    fn parse(
        &self,
        node: &markdown_ast::Node,
        cx: &MarkdownParseContext<'_>,
    ) -> Option<MarkdownNode> {
        matches!(node, markdown_ast::Node::Html(_))
            .then(|| blocked_node(node, cx))
            .flatten()
    }

    fn render(&self, node: &MarkdownNode, _: &mut Window, _: &mut App) -> impl IntoElement {
        div()
            .text_color(crate::theme::palette().text_subtle)
            .child(node.as_text().to_string())
    }
}

/// A credential-bearing link is not safe merely because the user clicked it.
pub fn has_embedded_credentials(url: &reqwest::Url) -> bool {
    !url.username().is_empty()
        || url.password().is_some()
        || url.query_pairs().any(|(key, _)| {
            matches!(
                key.to_ascii_lowercase().replace('-', "_").as_str(),
                "access_token"
                    | "token"
                    | "api_key"
                    | "apikey"
                    | "key"
                    | "password"
                    | "secret"
                    | "authorization"
                    | "credential"
            )
        })
}

#[cfg(test)]
mod tests {
    use super::{BLOCKED_IMAGE, has_embedded_credentials};

    #[test]
    fn inert_image_is_valid_bounded_and_transparent() {
        let decoded = image::load_from_memory(&BLOCKED_IMAGE.bytes)
            .unwrap()
            .into_rgba8();
        assert_eq!(decoded.dimensions(), (1, 1));
        assert_eq!(decoded.get_pixel(0, 0).0[3], 0);
        assert!(BLOCKED_IMAGE.bytes.len() < 128);
    }

    #[test]
    fn external_credentials_are_rejected_after_url_decoding() {
        for url in [
            "https://user:sentinel@example.com/",
            "https://user@example.com/",
            "https://example.com/?api%5Fkey=sentinel",
            "https://example.com/?Access-Token=sentinel",
        ] {
            assert!(has_embedded_credentials(&reqwest::Url::parse(url).unwrap()));
        }
        assert!(!has_embedded_credentials(
            &reqwest::Url::parse("https://example.com/docs?q=rust#heading").unwrap()
        ));
    }
}
