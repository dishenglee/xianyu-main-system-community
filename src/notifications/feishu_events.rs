use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use sqlx::Row;

use crate::state::AppState;

use super::{
    driver::{patch_runtime_error, patch_runtime_success},
    feishu_actions::handle_feishu_business_action,
    feishu_business::render_feishu_business_reply,
    feishu_group_events::handle_feishu_group_event,
    feishu_group_router::sync_member_into_active_groups,
    feishu_members::{
        generate_collaborator_invite, join_collaboration_via_invite, member_can_manage_channel,
        member_is_primary, provision_allowed_member_binding, resolve_member_for_sender,
        JoinCollaborationOutcome,
    },
    feishu_native::{FeishuNativeError, MessageSendRequest, MessageSendResponse},
    feishu_notify_cards::{
        build_feishu_open_chat_card, FeishuChatCardContext, FeishuChatCardMessage,
    },
    im_commands::{
        bind_rejected_reply, bind_success_reply, bound_feishu_reply, invite_join_code,
        open_chat_route_id, parse_feishu_im_command, recent_reply_body, selected_reply_route_id,
        unbound_feishu_reply, FeishuImCommand,
    },
    reply_router::{account_uid_belongs_to_owner, send_reply_via_route},
    sanitize::safe_worker_error_message,
};

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct FeishuImEvent {
    pub channel_id: i64,
    #[serde(default)]
    pub event_id: String,
    #[serde(default)]
    pub chat_id: String,
    #[serde(default)]
    pub message_id: String,
    #[serde(default)]
    pub thread_id: String,
    #[serde(default)]
    pub chat_type: String,
    #[serde(default)]
    pub sender_id: String,
    #[serde(default)]
    pub sender_name: String,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub mentioned_bot: bool,
    #[serde(default)]
    pub mention_all: bool,
    #[serde(default)]
    pub is_from_bot: bool,
    #[serde(default)]
    pub action_form_values: Map<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeishuImEventOutcome {
    pub status: String,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct FeishuEventReply {
    msg_type: String,
    content: Value,
}

impl FeishuEventReply {
    pub(crate) fn text(text: impl Into<String>) -> Self {
        Self {
            msg_type: "text".to_string(),
            content: json!({ "text": text.into() }),
        }
    }

    pub(crate) fn interactive(content: Value) -> Self {
        Self {
            msg_type: "interactive".to_string(),
            content,
        }
    }

    fn card(title: &str, body: &str, actions: Vec<FeishuCardAction>) -> Self {
        let mut elements = vec![json!({
            "tag": "div",
            "text": {
                "tag": "lark_md",
                "content": body,
            }
        })];
        if !actions.is_empty() {
            elements.push(json!({
                "tag": "action",
                "actions": actions.into_iter().map(|action| action.to_value()).collect::<Vec<_>>(),
            }));
        }
        Self {
            msg_type: "interactive".to_string(),
            content: json!({
                "config": {
                    "wide_screen_mode": true,
                    "enable_forward": true,
                },
                "header": {
                    "template": "blue",
                    "title": {
                        "tag": "plain_text",
                        "content": title,
                    },
                },
                "elements": elements,
            }),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct FeishuHandledEvent {
    pub(crate) status: String,
    pub(crate) reply: Option<FeishuEventReply>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FeishuCardAction {
    label: String,
    command: String,
    primary: bool,
}

impl FeishuCardAction {
    fn new(label: impl Into<String>, command: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            command: command.into(),
            primary: false,
        }
    }

    fn primary(label: impl Into<String>, command: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            command: command.into(),
            primary: true,
        }
    }

    fn to_value(self) -> Value {
        json!({
            "tag": "button",
            "type": if self.primary { "primary" } else { "default" },
            "text": {
                "tag": "plain_text",
                "content": self.label,
            },
            "value": {
                "command": self.command,
            },
        })
    }
}

pub async fn handle_feishu_im_event(
    state: &AppState,
    event: FeishuImEvent,
) -> Result<FeishuImEventOutcome, String> {
    let channel = state
        .storage()
        .get_notification_channel(event.channel_id)
        .await
        .map_err(|err| err.to_string())?
        .ok_or_else(|| format!("Feishu channel {} not found", event.channel_id))?;

    if channel.channel_type != "feishu" && channel.channel_type != "lark" {
        return Err(format!(
            "channel {} is not a Feishu/Lark channel",
            event.channel_id
        ));
    }

    if should_ignore_event(state, &event).await? {
        return Ok(FeishuImEventOutcome {
            status: "ignored".to_string(),
        });
    }

    if !state
        .storage()
        .try_claim_feishu_event(
            event.channel_id,
            &event.event_id,
            &event.message_id,
            &event.chat_id,
            &event.sender_id,
        )
        .await
        .map_err(|err| err.to_string())?
    {
        return Ok(FeishuImEventOutcome {
            status: "duplicate_ignored".to_string(),
        });
    }

    let mut runtime_patch = safe_event_runtime_patch(&event);
    runtime_patch.insert(
        "last_feishu_event_status".to_string(),
        Value::String("received".to_string()),
    );

    if is_group_chat(&event.chat_type) {
        let handled = match handle_feishu_group_event(
            state,
            &channel,
            &event,
            &mut runtime_patch,
        )
        .await
        {
            Ok(Some(handled)) => handled,
            Ok(None) => FeishuHandledEvent {
                status: "scope_rejected".to_string(),
                reply: Some(FeishuEventReply::text(
                    "为了保护 owner 的商品、订单和客户会话数据，请私聊鱼管家机器人完成绑定、查询、回复和确认操作。"
                        .to_string(),
                )),
            },
            Err(error) => {
                finish_feishu_event_status(state, &event, "processing_error").await;
                return Err(error);
            }
        };
        return finalize_feishu_handled_event(state, &event, &mut runtime_patch, handled).await;
    }
    let command = parse_feishu_im_command(&event.text);
    let reply =
        match build_feishu_im_reply(state, &channel, &event, command, &mut runtime_patch).await {
            Ok(reply) => reply,
            Err(error) => {
                finish_feishu_event_status(state, &event, "processing_error").await;
                return Err(error);
            }
        };
    let status = runtime_patch
        .get("last_feishu_event_status")
        .and_then(Value::as_str)
        .unwrap_or("replied")
        .to_string();
    finalize_feishu_handled_event(
        state,
        &event,
        &mut runtime_patch,
        FeishuHandledEvent {
            status,
            reply: Some(reply),
        },
    )
    .await
}

async fn finish_feishu_event_status(state: &AppState, event: &FeishuImEvent, status: &str) {
    if let Err(error) = state
        .storage()
        .finish_feishu_event(event.channel_id, &event.event_id, &event.message_id, status)
        .await
    {
        eprintln!(
            "[feishu.event] finish receipt failed channel={} event_id={} err={}",
            event.channel_id,
            event.event_id,
            safe_worker_error_message(&error.to_string())
        );
    }
}

async fn finalize_feishu_handled_event(
    state: &AppState,
    event: &FeishuImEvent,
    runtime_patch: &mut Map<String, Value>,
    handled: FeishuHandledEvent,
) -> Result<FeishuImEventOutcome, String> {
    runtime_patch.insert(
        "last_feishu_event_status".to_string(),
        Value::String(handled.status.clone()),
    );

    let Some(reply) = handled.reply else {
        patch_runtime_success(state, event.channel_id, runtime_patch).await;
        finish_feishu_event_status(state, event, &handled.status).await;
        return Ok(FeishuImEventOutcome {
            status: handled.status,
        });
    };

    match send_feishu_event_reply(state, event, reply).await {
        Ok(response) => {
            runtime_patch.insert(
                "last_feishu_reply_message_id".to_string(),
                Value::String(response.message_id),
            );
            runtime_patch.insert(
                "last_feishu_reply_fell_back".to_string(),
                Value::Bool(response.fell_back),
            );
            patch_runtime_success(state, event.channel_id, runtime_patch).await;
            finish_feishu_event_status(state, event, &handled.status).await;
            Ok(FeishuImEventOutcome {
                status: handled.status,
            })
        }
        Err(error) => {
            let err = safe_worker_error_message(&error.to_string());
            runtime_patch.insert(
                "last_feishu_event_status".to_string(),
                Value::String("reply_error".to_string()),
            );
            patch_runtime_error(state, event.channel_id, &err, runtime_patch).await;
            finish_feishu_event_status(state, event, "reply_error").await;
            Err(err)
        }
    }
}

async fn build_feishu_im_reply(
    state: &AppState,
    channel: &crate::models::notifications::NotificationChannel,
    event: &FeishuImEvent,
    command: FeishuImCommand,
    runtime_patch: &mut Map<String, Value>,
) -> Result<FeishuEventReply, String> {
    if command == FeishuImCommand::JoinInvite {
        return Ok(handle_feishu_join_invite(state, channel, event, runtime_patch).await?);
    }
    if command == FeishuImCommand::Bind {
        return Ok(
            match bind_feishu_sender_to_channel_owner(state, channel, event).await {
                Ok(binding) => {
                    record_operator_activity_on_event(
                        state,
                        channel.id,
                        &binding.owner_username,
                        &event.sender_id,
                        &event.sender_name,
                        &event.chat_id,
                        &event.event_id,
                    )
                    .await?;
                    runtime_patch.insert(
                        "last_feishu_event_status".to_string(),
                        Value::String("bound".to_string()),
                    );
                    runtime_patch.insert(
                        "last_feishu_bound_owner".to_string(),
                        Value::String(binding.owner_username.clone()),
                    );
                    runtime_patch.insert("last_feishu_binding_id".to_string(), json!(binding.id));
                    FeishuEventReply::text(bind_success_reply(&binding.owner_username))
                }
                Err(reason) => {
                    runtime_patch.insert(
                        "last_feishu_event_status".to_string(),
                        Value::String("bind_rejected".to_string()),
                    );
                    FeishuEventReply::text(bind_rejected_reply(&reason))
                }
            },
        );
    }
    if command == FeishuImCommand::Unbind {
        return Ok(FeishuEventReply::text(
            unbind_feishu_sender(state, channel, event, runtime_patch).await?,
        ));
    }

    if let Some(member) = feishu_bound_member_for_sender(state, channel, event).await? {
        let owner = member.owner_username.clone();
        record_operator_activity_on_event(
            state,
            channel.id,
            &owner,
            &event.sender_id,
            &event.sender_name,
            &event.chat_id,
            &event.event_id,
        )
        .await?;
        if command == FeishuImCommand::Collaborators {
            runtime_patch.insert(
                "last_feishu_event_status".to_string(),
                Value::String("member_list_replied".to_string()),
            );
            runtime_patch.insert(
                "last_feishu_bound_owner".to_string(),
                Value::String(owner.clone()),
            );
            return Ok(FeishuEventReply::text(
                render_feishu_members_reply(state, channel.id, &owner).await?,
            ));
        }
        if command == FeishuImCommand::GenerateInvite {
            if !member_can_manage_channel(&member) {
                runtime_patch.insert(
                    "last_feishu_event_status".to_string(),
                    Value::String("permission_denied".to_string()),
                );
                return Ok(FeishuEventReply::text(
                    "只有主用户可以生成协作者邀请码。".to_string(),
                ));
            }
            let invite =
                generate_collaborator_invite(state, channel, &event.sender_id, &event.sender_name)
                    .await?;
            runtime_patch.insert(
                "last_feishu_event_status".to_string(),
                Value::String("invite_generated".to_string()),
            );
            runtime_patch.insert(
                "last_feishu_bound_owner".to_string(),
                Value::String(owner.clone()),
            );
            let app_id = non_empty_config(&channel.config, "app_id");
            let body = if let Some(open_url) = feishu_bot_open_chat_url(app_id.as_deref()) {
                format!(
                    "协作者邀请码：`{}`\n\n1800 秒内有效，只能使用一次。\n\n1. 把这条消息直接转发给协作者。\n2. 让协作者点开下面链接进入机器人会话。\n3. 进入后发送「加入 {}」即可完成加入。\n\n打开机器人会话：{}",
                    invite.code, invite.code, open_url
                )
            } else {
                format!(
                    "协作者邀请码：`{}`\n\n1800 秒内有效，只能使用一次。请把邀请码发给协作者；协作者打开与机器人的会话后，发送「加入 {}」即可加入。",
                    invite.code, invite.code
                )
            };
            return Ok(FeishuEventReply::card("协作者邀请码", &body, Vec::new()));
        }
        if command == FeishuImCommand::LeaveCollaboration {
            if member_is_primary(&member) {
                runtime_patch.insert(
                    "last_feishu_event_status".to_string(),
                    Value::String("permission_denied".to_string()),
                );
                return Ok(FeishuEventReply::text(
                    "主用户不能直接退出协作；如需更换主用户，请使用重绑流程。".to_string(),
                ));
            }
            state
                .storage()
                .delete_feishu_user_binding(channel.id, &event.sender_id)
                .await
                .map_err(|err| err.to_string())?;
            runtime_patch.insert(
                "last_feishu_event_status".to_string(),
                Value::String("left_collaboration".to_string()),
            );
            return Ok(FeishuEventReply::text("已退出当前飞书协作。".to_string()));
        }
        if command == FeishuImCommand::OpenChatRoute {
            return open_feishu_reply_route_for_bound_sender(
                state,
                channel,
                event,
                &owner,
                runtime_patch,
            )
            .await;
        }
        if command == FeishuImCommand::SelectReplyRoute {
            let reply = select_feishu_reply_route_for_bound_sender(
                state,
                channel,
                event,
                &owner,
                runtime_patch,
            )
            .await?;
            return Ok(FeishuEventReply::text(reply));
        }
        if command == FeishuImCommand::RecentReply {
            if let Some(reply) = try_send_xianyu_reply_to_latest_chat_for_bound_sender(
                state,
                channel,
                event,
                &owner,
                runtime_patch,
            )
            .await?
            {
                return Ok(FeishuEventReply::text(reply));
            }
        }
        if command == FeishuImCommand::RecentMessages {
            runtime_patch.insert(
                "last_feishu_event_status".to_string(),
                Value::String("recent_message_replied".to_string()),
            );
            runtime_patch.insert(
                "last_feishu_bound_owner".to_string(),
                Value::String(owner.clone()),
            );
            let reply =
                render_recent_feishu_route_for_bound_sender(state, channel, event, &owner).await?;
            return Ok(interactive_business_reply(command, &reply));
        }
        if let Some(action_reply) = handle_feishu_business_action(
            state,
            channel.id,
            &owner,
            &event.sender_id,
            &event.chat_id,
            &event.message_id,
            &event.text,
        )
        .await?
        {
            runtime_patch.insert(
                "last_feishu_event_status".to_string(),
                Value::String(action_reply.status.to_string()),
            );
            runtime_patch.insert(
                "last_feishu_bound_owner".to_string(),
                Value::String(owner.clone()),
            );
            return Ok(FeishuEventReply::text(action_reply.reply));
        }
        if let Some(reply) =
            render_feishu_business_reply(state, &owner, command, &event.text).await?
        {
            runtime_patch.insert(
                "last_feishu_event_status".to_string(),
                Value::String("business_replied".to_string()),
            );
            runtime_patch.insert(
                "last_feishu_bound_owner".to_string(),
                Value::String(owner.clone()),
            );
            return Ok(interactive_business_reply(command, &reply));
        }
        if command == FeishuImCommand::Ack {
            if let Some(reply) =
                try_send_xianyu_reply_for_bound_sender(state, channel, event, &owner, runtime_patch)
                    .await?
            {
                return Ok(FeishuEventReply::text(reply));
            }
            if let Some(reply) = try_send_xianyu_plain_reply_to_latest_chat_for_bound_sender(
                state,
                channel,
                event,
                &owner,
                runtime_patch,
            )
            .await?
            {
                return Ok(FeishuEventReply::text(reply));
            }
        }
        runtime_patch.insert(
            "last_feishu_event_status".to_string(),
            Value::String("bound_reply".to_string()),
        );
        runtime_patch.insert(
            "last_feishu_bound_owner".to_string(),
            Value::String(owner.clone()),
        );
        return Ok(FeishuEventReply::text(bound_feishu_reply(command, &owner)));
    }

    runtime_patch.insert(
        "last_feishu_event_status".to_string(),
        Value::String("replied".to_string()),
    );
    Ok(FeishuEventReply::text(unbound_feishu_reply(command)))
}

fn interactive_business_reply(command: FeishuImCommand, body: &str) -> FeishuEventReply {
    match command {
        FeishuImCommand::Menu => FeishuEventReply::card(
            "鱼管家飞书菜单",
            body,
            vec![
                FeishuCardAction::primary("最近消息", "最近消息"),
                FeishuCardAction::primary("商品列表", "商品列表"),
                FeishuCardAction::primary("订单列表", "订单列表"),
                FeishuCardAction::new("帮助", "帮助"),
            ],
        ),
        FeishuImCommand::ItemList => FeishuEventReply::card(
            "鱼管家商品",
            body,
            with_body_actions(
                vec![
                    FeishuCardAction::primary("刷新商品", "商品列表"),
                    FeishuCardAction::new("订单列表", "订单列表"),
                    FeishuCardAction::new("最近消息", "最近消息"),
                ],
                item_list_card_actions(body),
            ),
        ),
        FeishuImCommand::OrderList => FeishuEventReply::card(
            "鱼管家订单",
            body,
            with_body_actions(
                vec![
                    FeishuCardAction::primary("刷新订单", "订单列表"),
                    FeishuCardAction::new("商品列表", "商品列表"),
                    FeishuCardAction::new("最近消息", "最近消息"),
                ],
                order_list_card_actions(body),
            ),
        ),
        FeishuImCommand::RecentMessages => FeishuEventReply::card(
            "最近闲鱼消息",
            body,
            vec![
                FeishuCardAction::primary("商品列表", "商品列表"),
                FeishuCardAction::new("订单列表", "订单列表"),
                FeishuCardAction::new("帮助", "帮助"),
            ],
        ),
        _ => FeishuEventReply::text(body.to_string()),
    }
}

fn with_body_actions(
    mut base: Vec<FeishuCardAction>,
    mut body_actions: Vec<FeishuCardAction>,
) -> Vec<FeishuCardAction> {
    base.append(&mut body_actions);
    base
}

fn item_list_card_actions(body: &str) -> Vec<FeishuCardAction> {
    let mut actions = Vec::new();
    let mut detail_index = 1usize;
    let mut down_index = 1usize;
    for line in body.lines().map(str::trim) {
        if detail_index <= 2 {
            if let Some(command) = line
                .strip_prefix("详情：")
                .map(str::trim)
                .filter(|command| command.starts_with("商品详情 "))
            {
                actions.push(FeishuCardAction::new(
                    format!("商品详情 {detail_index}"),
                    command,
                ));
                detail_index += 1;
                continue;
            }
        }
        if down_index <= 2 {
            if let Some(command) = first_operation_command(line, "下架商品 ") {
                actions.push(FeishuCardAction::new(
                    format!("下架商品 {down_index}"),
                    command,
                ));
                down_index += 1;
            }
        }
        if detail_index > 2 && down_index > 2 {
            break;
        }
    }
    actions
}

fn order_list_card_actions(body: &str) -> Vec<FeishuCardAction> {
    let mut actions = Vec::new();
    let mut detail_index = 1usize;
    let mut consign_index = 1usize;
    for line in body.lines().map(str::trim) {
        if detail_index <= 2 {
            if let Some(command) = line
                .strip_prefix("详情：")
                .map(str::trim)
                .filter(|command| command.starts_with("订单详情 "))
            {
                actions.push(FeishuCardAction::new(
                    format!("订单详情 {detail_index}"),
                    command,
                ));
                detail_index += 1;
                continue;
            }
        }
        if consign_index <= 2 {
            if let Some(command) = first_operation_command(line, "无需快递发货 ") {
                actions.push(FeishuCardAction::new(
                    format!("无需发货 {consign_index}"),
                    command,
                ));
                consign_index += 1;
            }
        }
        if detail_index > 2 && consign_index > 2 {
            break;
        }
    }
    actions
}

fn first_operation_command(line: &str, expected_prefix: &str) -> Option<String> {
    line.strip_prefix("操作：")?
        .split('/')
        .map(str::trim)
        .find(|command| command.starts_with(expected_prefix))
        .map(str::to_string)
}

async fn render_recent_feishu_route_for_bound_sender(
    state: &AppState,
    channel: &crate::models::notifications::NotificationChannel,
    event: &FeishuImEvent,
    owner_username: &str,
) -> Result<String, String> {
    let Some(route) = state
        .storage()
        .get_latest_feishu_im_reply_route_for_chat(channel.id, &event.chat_id)
        .await
        .map_err(|err| err.to_string())?
    else {
        return Ok("当前飞书会话没有可用的最近闲鱼消息。".to_string());
    };
    if route.owner_username.trim() != owner_username.trim()
        || !account_uid_belongs_to_owner(state, &route.uid, owner_username).await?
    {
        return Ok("最近消息不属于当前绑定 owner。".to_string());
    }
    Ok(format!(
        "最近闲鱼消息\n账号：{}\n会话：{}\n客户：{}\n商品：{}\n\n发送「回复 <内容>」可回复该会话。",
        route.uid,
        route.session_id,
        display_or_default(&route.customer_name, "客户"),
        display_or_default(&route.item_title, "未命名商品")
    ))
}

async fn try_send_xianyu_reply_to_latest_chat_for_bound_sender(
    state: &AppState,
    channel: &crate::models::notifications::NotificationChannel,
    event: &FeishuImEvent,
    owner_username: &str,
    runtime_patch: &mut Map<String, Value>,
) -> Result<Option<String>, String> {
    let Some(reply_text) = recent_reply_body(&event.text) else {
        return Ok(None);
    };
    let Some(route) = state
        .storage()
        .get_latest_feishu_im_reply_route_for_chat(channel.id, &event.chat_id)
        .await
        .map_err(|err| err.to_string())?
    else {
        runtime_patch.insert(
            "last_feishu_event_status".to_string(),
            Value::String("xianyu_reply_route_missing".to_string()),
        );
        return Ok(Some(
            "无法回复：当前飞书会话没有可用的最近闲鱼消息，请引用鱼管家推送的客户消息后回复。"
                .to_string(),
        ));
    };
    send_xianyu_reply_via_checked_route(state, route, owner_username, &reply_text, runtime_patch)
        .await
        .map(Some)
}

async fn try_send_xianyu_plain_reply_to_latest_chat_for_bound_sender(
    state: &AppState,
    channel: &crate::models::notifications::NotificationChannel,
    event: &FeishuImEvent,
    owner_username: &str,
    runtime_patch: &mut Map<String, Value>,
) -> Result<Option<String>, String> {
    let reply_text = event.text.trim();
    if reply_text.is_empty() {
        return Ok(None);
    }
    let Some(route) = state
        .storage()
        .get_latest_feishu_im_reply_route_for_chat(channel.id, &event.chat_id)
        .await
        .map_err(|err| err.to_string())?
    else {
        return Ok(None);
    };
    send_xianyu_reply_via_checked_route(state, route, owner_username, reply_text, runtime_patch)
        .await
        .map(Some)
}

enum FeishuReplyRouteSelection {
    Selected(crate::models::notifications::FeishuImReplyRoute),
    UserReply { status: &'static str, reply: String },
}

async fn select_feishu_reply_route_for_bound_sender(
    state: &AppState,
    channel: &crate::models::notifications::NotificationChannel,
    event: &FeishuImEvent,
    owner_username: &str,
    runtime_patch: &mut Map<String, Value>,
) -> Result<String, String> {
    let Some(route_id) = selected_reply_route_id(&event.text) else {
        runtime_patch.insert(
            "last_feishu_event_status".to_string(),
            Value::String("reply_route_missing".to_string()),
        );
        return Ok("无法选择：缺少闲鱼消息标识。".to_string());
    };
    let route = match select_feishu_reply_route_for_bound_sender_inner(
        state,
        channel,
        event,
        owner_username,
        &route_id,
    )
    .await?
    {
        FeishuReplyRouteSelection::Selected(route) => route,
        FeishuReplyRouteSelection::UserReply { status, reply } => {
            runtime_patch.insert(
                "last_feishu_event_status".to_string(),
                Value::String(status.to_string()),
            );
            return Ok(reply);
        }
    };

    runtime_patch.insert(
        "last_feishu_event_status".to_string(),
        Value::String("reply_route_selected".to_string()),
    );
    runtime_patch.insert(
        "last_feishu_bound_owner".to_string(),
        Value::String(owner_username.to_string()),
    );
    runtime_patch.insert(
        "last_feishu_selected_reply_route".to_string(),
        Value::String(route_id),
    );
    runtime_patch.insert(
        "last_xianyu_reply_session_id".to_string(),
        Value::String(route.session_id.clone()),
    );
    runtime_patch.insert(
        "last_xianyu_reply_customer_name".to_string(),
        Value::String(route.customer_name.clone()),
    );

    Ok(format!(
        "已选择闲鱼会话 `{}`（{}）。现在发送「回复 <内容>」会回复该客户。",
        route.session_id,
        display_or_default(&route.customer_name, "客户")
    ))
}

async fn open_feishu_reply_route_for_bound_sender(
    state: &AppState,
    channel: &crate::models::notifications::NotificationChannel,
    event: &FeishuImEvent,
    owner_username: &str,
    runtime_patch: &mut Map<String, Value>,
) -> Result<FeishuEventReply, String> {
    let Some(route_id) = open_chat_route_id(&event.text) else {
        runtime_patch.insert(
            "last_feishu_event_status".to_string(),
            Value::String("reply_route_missing".to_string()),
        );
        return Ok(FeishuEventReply::text("无法打开：缺少闲鱼消息标识。"));
    };
    let route = match select_feishu_reply_route_for_bound_sender_inner(
        state,
        channel,
        event,
        owner_username,
        &route_id,
    )
    .await?
    {
        FeishuReplyRouteSelection::Selected(route) => route,
        FeishuReplyRouteSelection::UserReply { status, reply } => {
            runtime_patch.insert(
                "last_feishu_event_status".to_string(),
                Value::String(status.to_string()),
            );
            return Ok(FeishuEventReply::text(reply));
        }
    };
    let account_name = load_account_name_for_uid(state, &route.uid).await?;
    let history = load_feishu_chat_card_history(state, &route).await?;
    runtime_patch.insert(
        "last_feishu_event_status".to_string(),
        Value::String("reply_route_opened".to_string()),
    );
    runtime_patch.insert(
        "last_feishu_bound_owner".to_string(),
        Value::String(owner_username.to_string()),
    );
    runtime_patch.insert(
        "last_feishu_selected_reply_route".to_string(),
        Value::String(route_id.clone()),
    );
    runtime_patch.insert(
        "last_xianyu_reply_session_id".to_string(),
        Value::String(route.session_id.clone()),
    );
    runtime_patch.insert(
        "last_xianyu_reply_customer_name".to_string(),
        Value::String(route.customer_name.clone()),
    );
    Ok(FeishuEventReply::interactive(build_feishu_open_chat_card(
        FeishuChatCardContext {
            uid: route.uid.clone(),
            account_name,
            customer_name: route.customer_name.clone(),
            session_id: route.session_id.clone(),
            item_id: route.item_id.clone(),
            item_title: route.item_title.clone(),
            latest_route_key: route_id,
        },
        &history,
    )))
}

async fn select_feishu_reply_route_for_bound_sender_inner(
    state: &AppState,
    channel: &crate::models::notifications::NotificationChannel,
    event: &FeishuImEvent,
    owner_username: &str,
    route_id: &str,
) -> Result<FeishuReplyRouteSelection, String> {
    let Some(route) = state
        .storage()
        .get_feishu_im_reply_route(channel.id, route_id)
        .await
        .map_err(|err| err.to_string())?
    else {
        return Ok(FeishuReplyRouteSelection::UserReply {
            status: "reply_route_missing",
            reply: "无法选择：这条聚合消息已过期或不存在。".to_string(),
        });
    };
    if route.owner_username.trim() != owner_username.trim()
        || !account_uid_belongs_to_owner(state, &route.uid, owner_username).await?
    {
        return Ok(FeishuReplyRouteSelection::UserReply {
            status: "reply_route_rejected",
            reply: "无法选择：这条闲鱼消息不属于当前绑定 owner。".to_string(),
        });
    }

    let chat_id = if event.chat_id.trim().is_empty() {
        route.chat_id.as_str()
    } else {
        event.chat_id.as_str()
    };
    if !event.message_id.trim().is_empty() {
        state
            .storage()
            .upsert_feishu_im_reply_route(
                channel.id,
                owner_username,
                &event.message_id,
                chat_id,
                &route.uid,
                &route.session_id,
                &route.peer_uid,
                &route.customer_name,
                &route.item_id,
                &route.item_title,
                &route.xianyu_message_id,
                &route.xianyu_upstream_message_id,
            )
            .await
            .map_err(|err| err.to_string())?;
    }
    state
        .storage()
        .upsert_feishu_im_reply_route(
            channel.id,
            owner_username,
            &selected_route_marker_id(event, route_id),
            chat_id,
            &route.uid,
            &route.session_id,
            &route.peer_uid,
            &route.customer_name,
            &route.item_id,
            &route.item_title,
            &route.xianyu_message_id,
            &route.xianyu_upstream_message_id,
        )
        .await
        .map_err(|err| err.to_string())?;
    Ok(FeishuReplyRouteSelection::Selected(route))
}

async fn try_send_xianyu_reply_for_bound_sender(
    state: &AppState,
    channel: &crate::models::notifications::NotificationChannel,
    event: &FeishuImEvent,
    owner_username: &str,
    runtime_patch: &mut Map<String, Value>,
) -> Result<Option<String>, String> {
    let reply_text = event.text.trim();
    if reply_text.is_empty() {
        return Ok(None);
    }
    let Some(route) = lookup_feishu_reply_route_for_event(state, channel.id, event).await? else {
        return Ok(None);
    };
    send_xianyu_reply_via_checked_route(state, route, owner_username, reply_text, runtime_patch)
        .await
        .map(Some)
}

fn selected_route_marker_id(event: &FeishuImEvent, route_id: &str) -> String {
    let source = if !event.event_id.trim().is_empty() {
        event.event_id.trim()
    } else if !event.message_id.trim().is_empty() {
        event.message_id.trim()
    } else {
        "manual"
    };
    format!("selected:{source}:{}", route_id.trim())
}

async fn send_xianyu_reply_via_checked_route(
    state: &AppState,
    route: crate::models::notifications::FeishuImReplyRoute,
    owner_username: &str,
    reply_text: &str,
    runtime_patch: &mut Map<String, Value>,
) -> Result<String, String> {
    if route.owner_username.trim() != owner_username.trim() {
        runtime_patch.insert(
            "last_feishu_event_status".to_string(),
            Value::String("xianyu_reply_rejected".to_string()),
        );
        return Ok("无法回复：这条飞书消息不属于当前绑定 owner。".to_string());
    }
    if !account_uid_belongs_to_owner(state, &route.uid, owner_username).await? {
        runtime_patch.insert(
            "last_feishu_event_status".to_string(),
            Value::String("xianyu_reply_rejected".to_string()),
        );
        return Ok("无法回复：闲鱼账号不属于当前 owner。".to_string());
    }

    let delivery = send_reply_via_route(
        state,
        &route.uid,
        &route.session_id,
        &route.peer_uid,
        &route.customer_name,
        &route.item_id,
        &route.item_title,
        reply_text,
    )
    .await?;
    runtime_patch.insert(
        "last_feishu_event_status".to_string(),
        Value::String("xianyu_replied".to_string()),
    );
    runtime_patch.insert(
        "last_xianyu_reply_session_id".to_string(),
        Value::String(delivery.session_id.clone()),
    );
    runtime_patch.insert(
        "last_xianyu_reply_customer_name".to_string(),
        Value::String(delivery.customer_name.clone()),
    );
    Ok(format!(
        "已回复闲鱼会话 `{}`（{}）。",
        delivery.session_id,
        display_or_default(&delivery.customer_name, "客户")
    ))
}

async fn lookup_feishu_reply_route_for_event(
    state: &AppState,
    channel_id: i64,
    event: &FeishuImEvent,
) -> Result<Option<crate::models::notifications::FeishuImReplyRoute>, String> {
    for candidate in [&event.thread_id, &event.message_id] {
        let candidate = candidate.trim();
        if candidate.is_empty() {
            continue;
        }
        if let Some(route) = state
            .storage()
            .get_feishu_im_reply_route(channel_id, candidate)
            .await
            .map_err(|err| err.to_string())?
        {
            return Ok(Some(route));
        }
    }
    Ok(None)
}

async fn load_account_name_for_uid(state: &AppState, uid: &str) -> Result<String, String> {
    sqlx::query_scalar::<_, String>("SELECT nick FROM accounts WHERE uid = ? LIMIT 1")
        .bind(uid.trim())
        .fetch_optional(state.storage().accounts())
        .await
        .map(|value| value.unwrap_or_else(|| uid.trim().to_string()))
        .map_err(|err| err.to_string())
}

async fn load_feishu_chat_card_history(
    state: &AppState,
    route: &crate::models::notifications::FeishuImReplyRoute,
) -> Result<Vec<FeishuChatCardMessage>, String> {
    let rows = sqlx::query(
        r#"
        SELECT ts, title, text, preview, sender_nick, is_self, kind
        FROM message_history_cache
        WHERE uid = ? AND session_id = ?
        ORDER BY version DESC, ts DESC
        LIMIT 12
        "#,
    )
    .bind(route.uid.trim())
    .bind(route.session_id.trim())
    .fetch_all(state.storage().live_state())
    .await
    .map_err(|err| err.to_string())?;
    let mut history = rows
        .into_iter()
        .map(|row| {
            let is_self = row.try_get::<i64, _>("is_self").unwrap_or(0) != 0;
            let body = first_non_empty(&[
                row.try_get::<String, _>("text").unwrap_or_default(),
                row.try_get::<String, _>("title").unwrap_or_default(),
                row.try_get::<String, _>("preview").unwrap_or_default(),
            ])
            .unwrap_or_else(|| {
                let kind = row.try_get::<String, _>("kind").unwrap_or_default();
                if kind.trim().is_empty() {
                    "[新消息]".to_string()
                } else {
                    format!("[{}]", kind.trim())
                }
            });
            let sender_nick = row.try_get::<String, _>("sender_nick").unwrap_or_default();
            FeishuChatCardMessage {
                ts: row.try_get::<i64, _>("ts").unwrap_or_default(),
                sender_label: if is_self {
                    "我".to_string()
                } else {
                    display_or_default(&sender_nick, &route.customer_name)
                },
                body,
            }
        })
        .collect::<Vec<_>>();
    history.reverse();
    if history.is_empty() {
        if let Some(fallback) = load_live_message_summary_fallback(state, route).await? {
            history.push(fallback);
        }
    }
    Ok(history)
}

async fn load_live_message_summary_fallback(
    state: &AppState,
    route: &crate::models::notifications::FeishuImReplyRoute,
) -> Result<Option<FeishuChatCardMessage>, String> {
    let row = sqlx::query(
        r#"
        SELECT summary, last_message_ts, peer_nick
        FROM message_live_state
        WHERE uid = ? AND session_id = ?
        LIMIT 1
        "#,
    )
    .bind(route.uid.trim())
    .bind(route.session_id.trim())
    .fetch_optional(state.storage().live_state())
    .await
    .map_err(|err| err.to_string())?;
    let Some(row) = row else {
        return Ok(None);
    };
    let summary = row.try_get::<String, _>("summary").unwrap_or_default();
    if summary.trim().is_empty() {
        return Ok(None);
    }
    let peer_nick = row.try_get::<String, _>("peer_nick").unwrap_or_default();
    Ok(Some(FeishuChatCardMessage {
        ts: row.try_get::<i64, _>("last_message_ts").unwrap_or_default(),
        sender_label: display_or_default(&peer_nick, &route.customer_name),
        body: summary.trim().to_string(),
    }))
}

fn first_non_empty(values: &[String]) -> Option<String> {
    values
        .iter()
        .map(|value| value.trim())
        .find(|value| !value.is_empty())
        .map(|value| value.to_string())
}

async fn record_operator_activity_on_event(
    state: &AppState,
    channel_id: i64,
    owner_username: &str,
    sender_id: &str,
    sender_name: &str,
    chat_id: &str,
    event_id: &str,
) -> Result<(), String> {
    if owner_username.trim().is_empty() || sender_id.trim().is_empty() {
        return Ok(());
    }
    state
        .storage()
        .upsert_feishu_operator_activity(
            channel_id,
            owner_username,
            sender_id,
            sender_name,
            chat_id,
            event_id,
        )
        .await
        .map(|_| ())
        .map_err(|err| err.to_string())
}

async fn bind_feishu_sender_to_channel_owner(
    state: &AppState,
    channel: &crate::models::notifications::NotificationChannel,
    event: &FeishuImEvent,
) -> Result<crate::models::notifications::FeishuUserBinding, String> {
    let owner = channel.owner_username.trim();
    if owner.is_empty() {
        return Err("当前飞书渠道没有 owner".to_string());
    }
    let sender_id = event.sender_id.trim();
    if sender_id.is_empty() {
        return Err("缺少飞书用户 ID".to_string());
    }
    if let Some(existing) = state
        .storage()
        .get_feishu_user_binding(channel.id, sender_id)
        .await
        .map_err(|err| err.to_string())?
    {
        if existing.enabled && existing.owner_username.trim() == owner {
            return Ok(existing);
        }
    }
    provision_allowed_member_binding(
        state,
        channel,
        sender_id,
        &event.sender_name,
        &event.chat_id,
        &event.chat_type,
        &event.message_id,
        &event.event_id,
        "manual_bind",
    )
    .await?
    .ok_or_else(|| "当前飞书用户不在该机器人的绑定授权名单中".to_string())
}

async fn unbind_feishu_sender(
    state: &AppState,
    channel: &crate::models::notifications::NotificationChannel,
    event: &FeishuImEvent,
    runtime_patch: &mut Map<String, Value>,
) -> Result<String, String> {
    let sender_id = event.sender_id.trim();
    if sender_id.is_empty() {
        runtime_patch.insert(
            "last_feishu_event_status".to_string(),
            Value::String("unbind_rejected".to_string()),
        );
        return Ok(bind_rejected_reply("缺少飞书用户 ID"));
    }
    let deleted = state
        .storage()
        .delete_feishu_user_binding(channel.id, sender_id)
        .await
        .map_err(|err| err.to_string())?;
    runtime_patch.insert(
        "last_feishu_event_status".to_string(),
        Value::String(if deleted { "unbound" } else { "unbind_noop" }.to_string()),
    );
    if deleted {
        Ok("已解绑当前飞书用户。再次发送「绑定」可重新绑定。".to_string())
    } else {
        Ok("当前飞书用户尚未绑定，无需解绑。".to_string())
    }
}

async fn feishu_bound_member_for_sender(
    state: &AppState,
    channel: &crate::models::notifications::NotificationChannel,
    event: &FeishuImEvent,
) -> Result<Option<crate::models::notifications::FeishuUserBinding>, String> {
    resolve_member_for_sender(
        state,
        channel,
        &event.sender_id,
        &event.sender_name,
        &event.chat_id,
        &event.chat_type,
        &event.message_id,
        &event.event_id,
    )
    .await
}

async fn handle_feishu_join_invite(
    state: &AppState,
    channel: &crate::models::notifications::NotificationChannel,
    event: &FeishuImEvent,
    runtime_patch: &mut Map<String, Value>,
) -> Result<FeishuEventReply, String> {
    let Some(code) = invite_join_code(&event.text) else {
        runtime_patch.insert(
            "last_feishu_event_status".to_string(),
            Value::String("join_rejected".to_string()),
        );
        return Ok(FeishuEventReply::text(
            "请让要加入的人本人私聊这个机器人，并发送「加入 <邀请码>」完成协作者加入。".to_string(),
        ));
    };
    match join_collaboration_via_invite(
        state,
        channel,
        &event.sender_id,
        &event.sender_name,
        &event.chat_id,
        &event.chat_type,
        &event.message_id,
        &event.event_id,
        &code,
    )
    .await
    {
        Ok(outcome) => {
            let binding = match outcome {
                JoinCollaborationOutcome::AlreadyJoined(binding) => binding,
                JoinCollaborationOutcome::Joined(binding) => {
                    if let Err(error) = sync_member_into_active_groups(
                        state,
                        channel,
                        &binding.owner_username,
                        &binding.sender_id,
                    )
                    .await
                    {
                        eprintln!(
                            "[feishu.join] channel={} owner={} sender={} sync active groups failed: {}",
                            channel.id, binding.owner_username, binding.sender_id, error
                        );
                    }
                    binding
                }
            };
            runtime_patch.insert(
                "last_feishu_event_status".to_string(),
                Value::String("joined_collaboration".to_string()),
            );
            runtime_patch.insert(
                "last_feishu_bound_owner".to_string(),
                Value::String(binding.owner_username.clone()),
            );
            Ok(FeishuEventReply::text(bind_success_reply(
                &binding.owner_username,
            )))
        }
        Err(reason) => {
            runtime_patch.insert(
                "last_feishu_event_status".to_string(),
                Value::String("join_rejected".to_string()),
            );
            Ok(FeishuEventReply::text(bind_rejected_reply(&reason)))
        }
    }
}

async fn render_feishu_members_reply(
    state: &AppState,
    channel_id: i64,
    owner_username: &str,
) -> Result<String, String> {
    let members = state
        .storage()
        .list_feishu_user_bindings_for_owner(channel_id, owner_username)
        .await
        .map_err(|err| err.to_string())?;
    if members.is_empty() {
        return Ok("当前还没有飞书协作者。".to_string());
    }

    let mut lines = vec!["飞书协作者".to_string()];
    for member in members {
        let role = if member_is_primary(&member) {
            "主用户"
        } else {
            "协作者"
        };
        let name = if member.sender_name.trim().is_empty() {
            member.sender_id.as_str()
        } else {
            member.sender_name.as_str()
        };
        lines.push(format!("- {} | {} | {}", role, name, member.sender_id));
    }
    Ok(lines.join("\n"))
}

async fn send_feishu_event_reply(
    state: &AppState,
    event: &FeishuImEvent,
    reply: FeishuEventReply,
) -> Result<MessageSendResponse, FeishuNativeError> {
    let channel = state
        .storage()
        .get_notification_channel(event.channel_id)
        .await
        .map_err(|err| FeishuNativeError::Unavailable(err.to_string()))?
        .ok_or_else(|| FeishuNativeError::Unavailable("Feishu channel not found".to_string()))?;
    state
        .feishu_native()
        .message_send(MessageSendRequest {
            channel_id: event.channel_id,
            receive_id: event_reply_receive_id(event),
            receive_id_type: event_reply_receive_id_type(event).to_string(),
            msg_type: reply.msg_type,
            content: reply.content,
            app_id: non_empty_config(&channel.config, "app_id"),
            app_secret: non_empty_config(&channel.config, "app_secret"),
            domain: non_empty_config(&channel.config, "domain"),
            reply_to_message_id: (!is_group_chat(&event.chat_type))
                .then(|| non_empty(&event.message_id))
                .flatten(),
            reply_in_thread: ((!event.thread_id.trim().is_empty())
                && !is_group_chat(&event.chat_type))
            .then_some(true),
            fallback_to_plain_chat: Some(true),
            source_event_id: non_empty(&event.event_id),
        })
        .await
}

async fn should_ignore_event(state: &AppState, event: &FeishuImEvent) -> Result<bool, String> {
    if event.is_from_bot {
        return Ok(true);
    }
    if !is_group_chat(&event.chat_type) {
        return Ok(false);
    }
    if event.mentioned_bot || event.mention_all {
        return Ok(false);
    }
    if event.chat_id.trim().is_empty() {
        return Ok(true);
    }
    let mapped = state
        .storage()
        .get_feishu_session_group_chat_by_chat_id(event.channel_id, &event.chat_id)
        .await
        .map_err(|err| err.to_string())?
        .is_some();
    Ok(!mapped)
}

fn event_reply_receive_id(event: &FeishuImEvent) -> String {
    let chat_id = event.chat_id.trim();
    if !chat_id.is_empty() {
        chat_id.to_string()
    } else {
        event.sender_id.trim().to_string()
    }
}

fn event_reply_receive_id_type(event: &FeishuImEvent) -> &'static str {
    if event.chat_id.trim().is_empty() {
        "open_id"
    } else {
        "chat_id"
    }
}

fn safe_event_runtime_patch(event: &FeishuImEvent) -> Map<String, Value> {
    Map::from_iter([
        (
            "last_feishu_event_id".to_string(),
            Value::String(event.event_id.trim().to_string()),
        ),
        (
            "last_feishu_message_id".to_string(),
            Value::String(event.message_id.trim().to_string()),
        ),
        (
            "last_chat_id".to_string(),
            Value::String(event.chat_id.trim().to_string()),
        ),
        (
            "last_sender_id".to_string(),
            Value::String(event.sender_id.trim().to_string()),
        ),
        (
            "last_chat_type".to_string(),
            Value::String(event.chat_type.trim().to_string()),
        ),
    ])
}

fn is_group_chat(chat_type: &str) -> bool {
    matches!(
        chat_type.trim().to_ascii_lowercase().as_str(),
        "group" | "chat" | "supergroup"
    )
}

fn non_empty(value: &str) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

fn non_empty_config(config: &Map<String, Value>, key: &str) -> Option<String> {
    config.get(key).and_then(Value::as_str).and_then(non_empty)
}

fn feishu_bot_open_chat_url(app_id: Option<&str>) -> Option<String> {
    let app_id = app_id.and_then(non_empty)?;
    Some(format!(
        "https://applink.feishu.cn/client/bot/open?appId={}",
        urlencoding::encode(&app_id)
    ))
}

fn display_or_default(value: &str, fallback: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        fallback.to_string()
    } else {
        trimmed.to_string()
    }
}
