#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrderConfirmation {
    Ready,
    Cancelled,
    AlreadyConsigned { safe_to_send: bool },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrderSnapshot {
    pub confirmation: OrderConfirmation,
    pub purchase_quantity: Option<i64>,
    pub purchase_quantity_source: String,
    pub refreshed_at: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConsignOutcome {
    Consigned,
    AlreadyConsigned { safe_to_send: bool },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GatewayError {
    Transient { code: String, message: String },
    Permanent { code: String, message: String },
}

impl GatewayError {
    pub fn transient(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self::Transient {
            code: code.into(),
            message: message.into(),
        }
    }

    pub fn permanent(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self::Permanent {
            code: code.into(),
            message: message.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MessageDeliveryResult {
    Acked { message_id: String, uuid: String },
    Uncertain,
    TransientFailure { code: String, message: String },
    PermanentFailure { code: String, message: String },
}

#[allow(async_fn_in_trait)]
pub trait OrderGateway: Send + Sync {
    async fn refresh_and_confirm_order(
        &self,
        uid: &str,
        order_id: &str,
    ) -> Result<OrderSnapshot, GatewayError>;

    async fn consign_without_shipping(
        &self,
        uid: &str,
        order_id: &str,
    ) -> Result<ConsignOutcome, GatewayError>;
}

#[allow(async_fn_in_trait)]
pub trait BuyerMessenger: Send + Sync {
    async fn send_text_and_wait_ack(
        &self,
        uid: &str,
        buyer_id: &str,
        content: &str,
    ) -> MessageDeliveryResult;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StockInsufficientNotice {
    pub alert_id: i64,
    pub uid: String,
    pub order_id: String,
    pub item_id: String,
    pub title: String,
    pub body: String,
    pub available: i64,
    pub required: i64,
}

#[allow(async_fn_in_trait)]
pub trait AdminNotifier: Send + Sync {
    /// Best-effort admin notification for waiting_stock. Must be idempotent.
    async fn notify_stock_insufficient(&self, notice: &StockInsufficientNotice);
}

/// Default no-op used by unit tests and compatibility entry points.
pub struct NoopAdminNotifier;

impl AdminNotifier for NoopAdminNotifier {
    async fn notify_stock_insufficient(&self, _notice: &StockInsufficientNotice) {}
}
