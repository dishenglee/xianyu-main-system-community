//! Message content normalization — decodes the Xianyu `content` payload into
//! a flat `NormalizedMessageContent` struct based on `contentType`.

use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use serde_json::{json, Value};

use super::{json_array, json_i64, json_object, json_string, NormalizedMessageContent};

// ---------------------------------------------------------------------------
// Helper trait
// ---------------------------------------------------------------------------

// Tiny helper: String::or_default_if_empty(other) — return self when non-empty
// else fall back to `other`. Used to thread `||`-style fallbacks through
// json_string chains where we want "first non-empty" semantics.
trait OrDefaultIfEmpty {
    fn or_default_if_empty(self, other: String) -> String;
}
impl OrDefaultIfEmpty for String {
    fn or_default_if_empty(self, other: String) -> String {
        if self.is_empty() {
            other
        } else {
            self
        }
    }
}

// ---------------------------------------------------------------------------
// Content normalization
// ---------------------------------------------------------------------------

pub fn normalize_message_content(content: Option<&Value>) -> NormalizedMessageContent {
    let content = json_object(content);
    let content_type = json_i64(content.get("contentType"));
    let mut title = String::new();
    let mut text = String::new();
    let mut image_urls = Vec::new();
    let mut card_meta = String::new();

    match content_type {
        1 => {
            text = json_string(json_object(content.get("text")).get("text"));
        }
        2 => {
            image_urls = json_array(json_object(content.get("image")).get("pics"))
                .into_iter()
                .filter_map(|pic| {
                    let pic = pic.as_object().cloned().unwrap_or_default();
                    let url = json_string(pic.get("url"));
                    (!url.is_empty()).then_some(url)
                })
                .collect();
        }
        3 => {
            let audio = json_object(content.get("audio"));
            let duration = json_i64(audio.get("duration"));
            text = if duration > 0 {
                format!("[语音 {duration}″]")
            } else {
                "[语音]".to_string()
            };
        }
        4 => {
            let video = json_object(content.get("video"));
            let snapshot = json_string(video.get("snapshot"));
            if !snapshot.is_empty() {
                image_urls.push(snapshot);
            }
            text = "[视频]".to_string();
        }
        5 => {
            let expression = json_object(content.get("expression"));
            let gif_url = json_string(expression.get("url"));
            if !gif_url.is_empty() {
                image_urls.push(gif_url);
            }
            text = {
                let name = json_string(expression.get("name"));
                if name.is_empty() {
                    "[动画表情]".to_string()
                } else {
                    name
                }
            };
        }
        6 => {
            let card = json_object(content.get("textCard"));
            title = json_string(card.get("title"));
            text = json_string(card.get("content"));
        }
        7 => {
            let card = json_object(content.get("itemCard"));
            let item = json_object(card.get("item"));
            title = json_string(item.get("title"));
            let price = json_string(item.get("price"));
            let tip = json_string(card.get("itemTip"));
            let main_pic = json_string(item.get("mainPic"));
            if !main_pic.is_empty() {
                image_urls.push(main_pic.clone());
            }
            text = if !tip.is_empty() && !price.is_empty() {
                format!("{tip}\n{price}")
            } else if !price.is_empty() {
                price.clone()
            } else {
                tip.clone()
            };
            // Rich item card metadata so the frontend can render the proper
            // thumb + title + ¥ price layout instead of falling back to a
            // system-style pill (which is what kind="card" + empty card_meta
            // previously produced).
            card_meta = serde_json::to_string(&json!({
                "kind": "item",
                "itemId": json_string(item.get("itemId")),
                "title": title,
                "mainPic": main_pic,
                "price": price,
                "itemTip": tip,
            }))
            .unwrap_or_default();
        }
        8 => {
            let card = json_object(content.get("imageCard"));
            title = json_string(card.get("title"));
            text = json_string(card.get("content"));
            let url = json_string(card.get("url"));
            if !url.is_empty() {
                image_urls.push(url.clone());
            }
            // contentType=8 is the alternate item-card shape (image-led).
            // Share the same `kind="item"` schema so the frontend renderer
            // can treat 7 and 8 identically.
            card_meta = serde_json::to_string(&json!({
                "kind": "item",
                "itemId": "",
                "title": title,
                "mainPic": url,
                "price": "",
                "itemTip": text,
            }))
            .unwrap_or_default();
        }
        11 => {
            let reply = json_object(content.get("reply"));
            title = json_string(reply.get("title"));
            text = json_string(reply.get("content"));
            let media_url = json_string(reply.get("mediaUrl"));
            if !media_url.is_empty() {
                image_urls.push(media_url);
            }
        }
        14 => {
            text = json_string(json_object(content.get("tip")).get("tip"));
        }
        17 => {
            let transfer = json_object(content.get("transfer"));
            title = {
                let title = json_string(transfer.get("title"));
                if title.is_empty() {
                    "转账".to_string()
                } else {
                    title
                }
            };
            text = json_string(transfer.get("content"));
            let media_url = json_string(transfer.get("mediaUrl"));
            if !media_url.is_empty() {
                image_urls.push(media_url);
            }
        }
        18 => {
            let rtc = json_object(content.get("rtc"));
            let rtc_text = json_string(rtc.get("text"));
            text = if rtc_text.is_empty() {
                "[语音通话]".to_string()
            } else {
                rtc_text
            };
        }
        25 | 26 | 28 => {
            let dx_card = json_object(content.get("dxCard"));
            let item = json_object(dx_card.get("item"));
            let main = json_object(item.get("main"));
            let ex = json_object(main.get("exContent"));
            let head = json_object(ex.get("head"));
            title = json_string(head.get("title"));
            if title.is_empty() {
                title = json_string(ex.get("title"));
            }
            text = json_string(head.get("subtitle"));
            if text.is_empty() {
                text = json_string(ex.get("desc"));
            }
            let seller_avatar = json_string(head.get("avatarUrl"));
            let seller_nick = json_string(head.get("sellerName"))
                .or_default_if_empty(json_string(head.get("name")))
                .or_default_if_empty(json_string(head.get("title")));
            let item1 = json_object(ex.get("item1"));
            let item_image = json_string(item1.get("image"));
            let item_title = json_string(item1.get("title"));
            let item_subtitle = json_string(item1.get("subTitle"))
                .or_default_if_empty(json_string(item1.get("subtitle")))
                .or_default_if_empty(json_string(item1.get("desc")));
            // link target: check `main.targetUrl` / `item.targetUrl` / ex root
            let link_url = json_string(main.get("targetUrl"))
                .or_default_if_empty(json_string(item.get("targetUrl")))
                .or_default_if_empty(json_string(ex.get("targetUrl")));

            // Keep legacy image_urls populated (avatar first, else item pic)
            // so existing callers still see one image field.
            if !seller_avatar.is_empty() {
                image_urls.push(seller_avatar.clone());
            } else if !item_image.is_empty() {
                image_urls.push(item_image.clone());
            }

            // Serialize rich card metadata so the frontend can render the
            // seller + item sub-card layout — this data is lost otherwise.
            let meta = json!({
                "sellerNick": seller_nick,
                "sellerAvatar": seller_avatar,
                "itemTitle": item_title,
                "itemSubtitle": item_subtitle,
                "itemPic": item_image,
                "linkUrl": link_url,
            });
            card_meta = serde_json::to_string(&meta).unwrap_or_default();
        }
        27 => {
            // Message recall (撤回). Upstream pushes a tip-shaped envelope
            // with the actor's name + recall verb. We keep the original text
            // for the bubble preview but flip the `kind` downstream so the
            // frontend can render it as a centered system pill regardless of
            // sender (instead of the generic `tip` color).
            let withdraw = json_object(content.get("withdrawTip"));
            let withdraw_text = json_string(withdraw.get("withdrawText"));
            text = if withdraw_text.is_empty() {
                "撤回了一条消息".to_string()
            } else {
                withdraw_text
            };
        }
        30 => {
            let card = json_object(content.get("locationCard"));
            title = json_string(card.get("title"));
            text = json_string(card.get("content"));
            let url = json_string(card.get("url"));
            if !url.is_empty() {
                image_urls.push(url.clone());
            }
            let action_url =
                json_string(json_object(json_object(card.get("action")).get("page")).get("url"));
            // Location card metadata — frontend renders a map-preview style
            // card with a red pin overlay. action_url goes to the deep link
            // when the user taps the card.
            card_meta = serde_json::to_string(&json!({
                "kind": "location",
                "title": title,
                "content": text,
                "latitude": json_string(card.get("latitude")),
                "longitude": json_string(card.get("longitude")),
                "previewUrl": url,
                "actionUrl": action_url,
            }))
            .unwrap_or_default();
        }
        32 => {
            let dynamic = json_object(content.get("dynamicOperation"));
            let inner = normalize_message_content(dynamic.get("changeContent"));
            title = inner.title;
            text = inner.text;
            if !inner.image_url.is_empty() {
                image_urls.push(inner.image_url);
            }
            if title.is_empty() && text.is_empty() {
                text = "[动态消息]".to_string();
            }
        }
        101 => {
            // Wrapped "custom" envelope. Real content lives inside `custom.data`
            // as a base64-encoded JSON blob (e.g. `{contentType:2, image:{pics:[...]}}`
            // for images we sent ourselves). Recursively normalize the inner
            // payload so image messages don't fall through to the text branch
            // and show as "[图片]" placeholder.
            let custom = json_object(content.get("custom"));
            let decoded = custom
                .get("data")
                .and_then(Value::as_str)
                .and_then(|raw| BASE64_STANDARD.decode(raw).ok())
                .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok());
            if let Some(inner) = decoded {
                return normalize_message_content(Some(&inner));
            }
            // Fallback: use the summary string the server sets for legacy
            // custom envelopes ("[图片]" / "[语音]" etc.) when data is empty.
            text = json_string(custom.get("summary"))
                .or_default_if_empty(json_string(json_object(content.get("text")).get("text")));
        }
        _ => {
            text = json_string(json_object(content.get("text")).get("text"));
        }
    }

    let (kind, preview) = if content_type == 27 {
        // Recall tip — frontend renders as centered "你/对方撤回了一条消息"
        // pill. Distinct from generic tips so we can also bump
        // `peer_last_read_version` when the recall is from the peer.
        ("recalled".to_string(), text.clone())
    } else if content_type == 14 {
        ("tip".to_string(), text.clone())
    } else if matches!(content_type, 3 | 18) {
        ("text".to_string(), text.clone())
    } else if content_type == 5 && !image_urls.is_empty() {
        (
            "image".to_string(),
            if text.is_empty() {
                "[动画表情]".to_string()
            } else {
                text.clone()
            },
        )
    } else if content_type == 4 && !image_urls.is_empty() {
        ("image".to_string(), "[视频]".to_string())
    } else if matches!(content_type, 7 | 8 | 30) {
        (
            "card".to_string(),
            if !title.is_empty() {
                title.clone()
            } else if !text.is_empty() {
                text.clone()
            } else {
                "[卡片]".to_string()
            },
        )
    } else if matches!(content_type, 6 | 11 | 17 | 25 | 26 | 28 | 32) {
        (
            "card".to_string(),
            if !title.is_empty() {
                title.clone()
            } else if !text.is_empty() {
                text.clone()
            } else {
                "[卡片消息]".to_string()
            },
        )
    } else if !image_urls.is_empty() && title.is_empty() && text.is_empty() {
        ("image".to_string(), "[图片]".to_string())
    } else if !image_urls.is_empty() {
        (
            "card".to_string(),
            if !title.is_empty() {
                title.clone()
            } else if !text.is_empty() {
                text.clone()
            } else {
                "[图片]".to_string()
            },
        )
    } else if !text.is_empty() {
        ("text".to_string(), text.clone())
    } else {
        (
            "unknown".to_string(),
            format!("[消息类型 {}]", content_type.max(0)),
        )
    };

    NormalizedMessageContent {
        kind,
        title,
        text,
        image_url: image_urls.first().cloned().unwrap_or_default(),
        image_urls,
        content_type,
        preview,
        card_meta,
    }
}
