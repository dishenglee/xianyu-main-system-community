use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NotificationContentKind {
    Text,
    Html,
    Markdown,
    MediaPreview,
}

impl NotificationContentKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Html => "html",
            Self::Markdown => "markdown",
            Self::MediaPreview => "media_preview",
        }
    }
}

impl fmt::Display for NotificationContentKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotificationContent {
    pub kind: NotificationContentKind,
    pub body: String,
}

impl NotificationContent {
    pub fn new(kind: NotificationContentKind, body: impl Into<String>) -> Self {
        Self {
            kind,
            body: body.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotificationTemplate {
    fallback_text: String,
    text: Option<String>,
    html: Option<String>,
    markdown: Option<String>,
    media_preview: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedNotificationContent {
    pub requested_kind: NotificationContentKind,
    pub selected_kind: NotificationContentKind,
    pub body: String,
    pub fallback_text: String,
}

impl NotificationTemplate {
    pub fn new(fallback_text: impl Into<String>) -> Self {
        let fallback_text = fallback_text.into();
        Self {
            text: Some(fallback_text.clone()),
            fallback_text,
            html: None,
            markdown: None,
            media_preview: None,
        }
    }

    pub fn from_text_only(body: impl Into<String>) -> Self {
        let body = body.into();
        Self::new(body.clone()).with_text(body)
    }

    pub fn with_text(mut self, body: impl Into<String>) -> Self {
        self.text = Some(body.into());
        self
    }

    pub fn with_html(mut self, body: impl Into<String>) -> Self {
        self.html = Some(body.into());
        self
    }

    pub fn with_markdown(mut self, body: impl Into<String>) -> Self {
        self.markdown = Some(body.into());
        self
    }

    pub fn with_media_preview(mut self, body: impl Into<String>) -> Self {
        self.media_preview = Some(body.into());
        self
    }

    pub fn fallback_text(&self) -> &str {
        &self.fallback_text
    }

    pub fn text(&self) -> Option<&str> {
        self.text.as_deref()
    }

    pub fn html(&self) -> Option<&str> {
        self.html.as_deref()
    }

    pub fn markdown(&self) -> Option<&str> {
        self.markdown.as_deref()
    }

    pub fn media_preview(&self) -> Option<&str> {
        self.media_preview.as_deref()
    }

    pub fn variant(&self, kind: NotificationContentKind) -> Option<&str> {
        match kind {
            NotificationContentKind::Text => self.text(),
            NotificationContentKind::Html => self.html(),
            NotificationContentKind::Markdown => self.markdown(),
            NotificationContentKind::MediaPreview => self.media_preview(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DriverContentCapabilities {
    pub preferred: NotificationContentKind,
    pub supported: &'static [NotificationContentKind],
    pub downgrade_order: &'static [NotificationContentKind],
}

impl DriverContentCapabilities {
    pub fn supports(&self, kind: NotificationContentKind) -> bool {
        self.supported.contains(&kind)
    }

    pub fn selection_order_for(
        &self,
        requested: NotificationContentKind,
    ) -> Vec<NotificationContentKind> {
        let mut order = Vec::with_capacity(self.downgrade_order.len() + 2);
        order.push(requested);
        if requested != self.preferred {
            order.push(self.preferred);
        }
        for candidate in self.downgrade_order {
            if !order.contains(candidate) {
                order.push(*candidate);
            }
        }
        if !order.contains(&NotificationContentKind::Text) {
            order.push(NotificationContentKind::Text);
        }
        order
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NegotiatedNotificationContent {
    pub requested: NotificationContentKind,
    pub selected: NotificationContent,
    pub fallback_text: String,
}

pub fn negotiate_template(
    template: &NotificationTemplate,
    capabilities: DriverContentCapabilities,
    requested: NotificationContentKind,
) -> NegotiatedNotificationContent {
    for kind in capabilities.selection_order_for(requested) {
        if !capabilities.supports(kind) {
            continue;
        }
        if let Some(body) = template.variant(kind) {
            return NegotiatedNotificationContent {
                requested,
                selected: NotificationContent::new(kind, body),
                fallback_text: template.fallback_text().to_string(),
            };
        }
    }

    NegotiatedNotificationContent {
        requested,
        selected: NotificationContent::new(NotificationContentKind::Text, template.fallback_text()),
        fallback_text: template.fallback_text().to_string(),
    }
}
