// Core domain types for Xianyu V2 API

export type ReplyMode =
  | 'off'
  | 'default'
  | 'keyword'
  | 'ai'
  | 'follow_global'
  | 'item_reply'

export interface Account {
  uid: string;
  nick: string;
  displayNick?: string;
  displayName?: string;
  snsNick?: string;
  accountNick?: string;
  avatar?: string;
  cookie_id?: string;
  status?: string;
  raw_status?: string;
  restriction_end?: string;
  // UID row first creation time. Do not use it as the current user's add time.
  created_at?: number;
  added_at?: number | null;
  addedAt?: number | null;
  bound_at?: number;
  boundAt?: number;
  updated_at?: number;
  has_h5_tk?: boolean;
  last_session_status?: string;
  last_session_reason?: string;
  last_session_check_at?: number;
  last_session_ok_at?: number;
  first_session_fail_at?: number;
  liveStatus?: string;
  live_status?: string;
  liveLastConnectedAt?: number;
  live_last_connected_at?: number;
  liveLastDisconnectedAt?: number;
  live_last_disconnected_at?: number;
  liveLastError?: string;
  live_last_error?: string;
  liveUpdatedAt?: number;
  live_updated_at?: number;
  next_refresh_in?: number;
  security_score?: number;
  security_score_updated_at?: number;
  // Business metrics
  fish_power?: number;
  coin_balance?: number;
  violation_count?: number;
  seller_30d_sold?: string;
  summaryCachedAt?: number;
  totalEarned?: number;
  fishShopEnabled?: boolean;
  fishShopLevel?: string;
  // Auto-reply / ship toggles
  replyMode?: ReplyMode;
  autoReply?: boolean;
  autoShip?: boolean;
  aiEnabled?: boolean;
  autoRecallItemReplies?: boolean;
  auto_reply_enabled?: boolean;
  auto_ship_enabled?: boolean;
  /** Number of pending delivery orders whose item has no enabled configuration. */
  pendingDeliveryMismatchCount?: number;
  pending_delivery_mismatch_count?: number;
  ai_enabled?: boolean;
  auto_recall_item_replies?: boolean;
  delete_only?: boolean;
  deleteOnly?: boolean;
  remote_binding_state?: string;
  remoteBindingState?: string;
  remote_binding_message?: string;
  remoteBindingMessage?: string;
  location?: {
    name?: string;
    address?: string;
    province?: string;
    city?: string;
    area?: string;
    divisionId?: string | number;
    latitude?: string | number;
    longitude?: string | number;
  };
}

export interface MtopResponse<T = unknown> {
  ret?: string[] | string;
  data?: T;
  [key: string]: unknown;
}

export type DatacompassDateType =
  | "recent1d"
  | "recent7d"
  | "recent30d"
  | string;

export type ShopGrowthQueryOptions = RequestOptions;

export interface ClaimCoinTaskPayload {
  uid: string;
  taskId?: string;
  taskType?: string;
  benefitId?: string;
  [key: string]: unknown;
}

export interface CreatePromotionPayload {
  uid: string;
  itemId: string;
  promotionNum?: string | number;
  costCoinNum: number;
  expectedCostCoinNum?: number;
  [key: string]: unknown;
}

export interface CreateExposureOrderPayload {
  uid: string;
  itemIds: string[];
  exposureNum: number;
  expectedExposureNum?: number;
  picUrl?: string;
  title?: string;
  [key: string]: unknown;
}

export interface CompleteSafeViewTasksPayload {
  uid: string;
  scene?: string;
}

export interface ReceiveExposureRewardPayload {
  uid: string;
  taskId?: string;
  taskType?: string;
  benefitId?: string;
  [key: string]: unknown;
}

export interface RunExposureTaskPayload extends ReceiveExposureRewardPayload {
  taskInstanceId?: string;
  activityId?: string;
  benefitList?: unknown[];
}

export interface SuperExposurePayload {
  uid: string;
  [key: string]: unknown;
}

export interface DatacompassMetric {
  name: string;
  label: string;
  value?: string | number;
  display?: string;
  ratio?: string | number;
}

export interface Order {
  order_id: string;
  biz_order_id?: string;
  title?: string;
  price?: string;
  status?: string;
  buyer_nick?: string;
  buyer_uid?: string;
  item_id?: string;
  item_title?: string;
  created_at?: number;
  updated_at?: number;
}

export interface Session {
  session_id: string;
  session_type?: number;
  uid: string;
  title?: string;
  last_message?: string;
  unread_count?: number;
  updated_at?: number;
  avatar?: string;
}

export interface Message {
  msg_id: string;
  session_id?: string;
  content: string;
  sender_id?: string;
  sender_nick?: string;
  created_at?: number;
  type?: string;
}

export type DashboardRiskSeverity =
  | "info"
  | "warning"
  | "danger"
  | "critical"
  | string;

export interface DashboardAccountRow {
  uid: string;
  nick?: string;
  status?: string;
  todayMessages?: number;
  unreadMessages?: number;
  unreadSessions?: number;
  todayOrders?: number;
  todayRevenue?: number;
  waitSendOrders?: number;
  pendingPayOrders?: number;
  lastMessageAt?: string | number | null;
  lastOrderAt?: string | number | null;
}

export interface DashboardRecentEvent {
  kind?: string;
  orderId?: string;
  title?: string;
  subtitle?: string;
  uid?: string;
  nick?: string;
  buyerNick?: string;
  status?: string;
  amount?: number;
  ts?: string | number | null;
}

export interface DashboardRiskItem {
  kind?: string;
  title?: string;
  subtitle?: string;
  severity?: DashboardRiskSeverity;
  targetPath?: string;
}

export interface DashboardTrendPoint {
  date: string;
  messages?: number;
  orders?: number;
  revenue?: number;
}

export interface DashboardStats {
  date?: string;
  generatedAt?: string | number | null;
  todayMessages: number;
  unreadMessages: number;
  unreadSessions: number;
  overdueReplySessions: number;
  todayOrders: number;
  todayRevenue: number;
  pendingPayOrders: number;
  waitSendOrders: number;
  waitReceiveOrders: number;
  closedOrders: number;
  accountTotal: number;
  activeAccounts: number;
  inactiveAccounts: number;
  restrictedAccounts: number;
  sessionErrorAccounts: number;
  accountRows: DashboardAccountRow[];
  recentEvents: DashboardRecentEvent[];
  riskItems: DashboardRiskItem[];
  trend7d: DashboardTrendPoint[];
}

export interface DataCenterTotals {
  todayRevenue: number;
  todayOrders: number;
  waitSendOrders: number;
  waitSendEarliestAt: number;
  pendingPayOrders: number;
  pendingPayAmount: number;
  unreadSessions: number;
  unreadMessages: number;
  overdueReplySessions: number;
  onlineItems: number;
  totalItems: number;
  aftersaleOrders: number;
  closedOrders: number;
  accountTotal: number;
  activeAccounts: number;
  liveOnlineAccounts: number;
  sessionErrorAccounts: number;
}

export interface DataCenterAccountRow {
  uid: string;
  nick: string;
  avatar?: string;
  status: string;
  liveStatus?: string;
  lastError?: string;
  todayRevenue: number;
  weekRevenue: number;
  totalRevenue: number;
  todayOrders: number;
  weekOrders: number;
  totalOrders: number;
  unreadMessages: number;
  unreadSessions: number;
  waitSendOrders: number;
  pendingPayOrders: number;
  onlineItems: number;
  totalItems: number;
  lastMessageAt: number;
  lastOrderAt: number;
  itemsSyncedAt: number;
  ordersSyncedAt: number;
}

export interface DataCenterQueueItem {
  id: string;
  kind: "order" | "message" | string;
  uid: string;
  accountNick: string;
  title: string;
  subtitle: string;
  customer: string;
  amount: number;
  status: string;
  statusLabel: string;
  targetPath: string;
  actionLabel: string;
  ts: number;
  itemId?: string;
  itemPic?: string;
  sessionId?: string;
  orderId?: string;
}

export interface DataCenterShareSegment {
  uid: string;
  nick: string;
  amount: number;
  orders: number;
  percent: number;
}

export interface DataCenterShareRange {
  range: "day" | "week" | "total" | string;
  label: string;
  totalAmount: number;
  totalOrders: number;
  segments: DataCenterShareSegment[];
}

export interface DataCenterShareBundle {
  day: DataCenterShareRange;
  week: DataCenterShareRange;
  total: DataCenterShareRange;
}

export interface DataCenterTrendPoint {
  date: string;
  label: string;
  amount: number;
  orders: number;
  messages: number;
}

export interface DataCenterTrendRange {
  range: "week" | "month" | "total" | string;
  label: string;
  totalAmount: number;
  totalOrders: number;
  points: DataCenterTrendPoint[];
}

export interface DataCenterTrendBundle {
  week: DataCenterTrendRange;
  month: DataCenterTrendRange;
  total: DataCenterTrendRange;
}

export interface DataCenterSellerTrendRange {
  available: boolean;
  range: "week" | "halfMonth" | "month" | string;
  label: string;
  totalAmount: number;
  totalOrders: number;
  points: DataCenterTrendPoint[];
  syncedAt: number;
}

export interface DataCenterSellerTrendBundle {
  week: DataCenterSellerTrendRange;
  halfMonth: DataCenterSellerTrendRange;
  month: DataCenterSellerTrendRange;
}

export interface DataCenterDataCompassSummary {
  status: string;
  accountCount: number;
  successCount: number;
  failedCount: number;
  lastSuccessAt: number;
  errorSummary: string;
}

export interface DataCenterSyncSummary {
  status: string;
  subtitle: string;
  sqliteOk: boolean;
  liveOnline: number;
  liveTotal: number;
  staleAccounts: number;
  signServiceOk?: boolean;
  signServiceStatusCode?: number;
  licenseServiceOk?: boolean;
  licenseServiceStatusCode?: number;
  dataCompass: DataCenterDataCompassSummary;
}

export interface DataCenterAutomationMetric {
  enabled?: number | null;
  running?: number | null;
  pending?: number | null;
  errors?: number | null;
}

export interface DataCenterAutomationSummary {
  delivery: DataCenterAutomationMetric;
  scheduledTasks: DataCenterAutomationMetric;
  replies: DataCenterAutomationMetric;
}

export interface DataCenterStats {
  date: string;
  generatedAt: number;
  totals: DataCenterTotals;
  accounts: DataCenterAccountRow[];
  queue: DataCenterQueueItem[];
  share: DataCenterShareBundle;
  trend: DataCenterTrendBundle;
  sellerTrend: DataCenterSellerTrendBundle;
  sync: DataCenterSyncSummary;
  automation?: DataCenterAutomationSummary;
}

export interface Item {
  item_id: string;
  uid: string;
  title?: string;
  price?: string;
  orig_price?: string;
  status?: string;
  sold_count?: number;
  view_count?: number;
  cover_url?: string;
  desc?: string;
  created_at?: number;
}

export interface Keyword {
  id?: number;
  cookie_id?: string;
  cookieId?: string;
  item_id?: string;
  itemId?: string;
  keyword: string;
  reply?: string;
  type?: string;
  enabled?: boolean;
  task_group_id?: string;
  taskGroupId?: string;
  created_at?: number;
  createdAt?: number;
}

export interface ItemReply {
  id?: number;
  cookie_id?: string;
  cookieId?: string;
  item_id?: string;
  itemId?: string;
  reply?: string;
  type?: string;
  enabled?: boolean;
  autoRecall?: boolean;
  auto_recall?: boolean;
  taskGroupId?: string;
  task_group_id?: string;
  replyCount?: number;
  reply_count?: number;
  created_at?: number;
  createdAt?: number;
}

export interface ItemReplyMode {
  cookie_id?: string;
  cookieId?: string;
  item_id?: string;
  itemId?: string;
  reply_mode?: ReplyMode;
  replyMode?: ReplyMode;
  updated_at?: number;
  updatedAt?: number;
}

export interface OfflineReplySyncPayload {
  itemId?: string;
  enableOfflineReply?: boolean;
  offlineReplyText?: string;
}

export interface OfflineReplyResult {
  status?: string;
  name?: string;
  transport?: string;
  results?: Record<string, unknown>;
  [key: string]: unknown;
}

export interface Card {
  id?: number;
  cookie_id?: string;
  cookieId?: string;
  title?: string;
  content?: string;
  card_pool_tag?: string;
  cardPoolTag?: string;
  card_content?: string;
  cardContent?: string;
  reusable?: boolean;
  used?: boolean;
  usedOrderId?: string;
  used_order_id?: string;
  createdAt?: number;
  created_at?: number;
  enabled?: boolean;
}

export interface DeliveryRule {
  id?: number;
  cookie_id?: string;
  cookieId?: string;
  item_id?: string;
  itemId?: string;
  keyword?: string;
  card_pool_tag?: string;
  cardPoolTag?: string;
  card_pool_scope?: string;
  cardPoolScope?: string;
  auto_ship?: boolean;
  enabled?: boolean;
}

export interface DeliveryProduct {
  itemId: string;
  image: string;
  title: string;
  price: string;
  quantity?: number | null;
  online: boolean;
}

export interface DeliveryPendingOrderMismatch {
  orderId: string;
  itemId: string;
  title: string;
  status: string;
  statusText: string;
}

export interface DeliveryConfiguration {
  id: number;
  uid: string;
  itemId: string;
  fixedMessage: string;
  oneTimeEnabled: boolean;
  enabled: boolean;
  needsRepair: boolean;
  poolTag: string;
  poolTags?: string[];
  fixedPoolTag?: string;
  fixedPoolTags?: string[];
  availableCardCount: number;
  product: DeliveryProduct;
  createdAt: number;
  /**
   * Account-level pending orders which do not match an enabled delivery
   * configuration. The backend includes the same snapshot on each config for
   * that account, so consumers must aggregate it at the account level.
   */
  pendingOrderMismatches?: DeliveryPendingOrderMismatch[];
}

export interface SaveDeliveryConfigurationPayload {
  uid: string;
  itemId: string;
  fixedMessage?: string;
  poolTags: string[];
  fixedPoolTags?: string[];
  oneTimeEnabled?: boolean;
  cards?: string[];
  enabled?: boolean;
}

export interface UpdateDeliveryConfigurationPayload {
  fixedMessage?: string;
  poolTags?: string[];
  fixedPoolTags?: string[];
  oneTimeEnabled?: boolean;
  cards?: string[];
  enabled?: boolean;
}

export interface DeliveryRunSummary {
  id: number;
  uid: string;
  configurationId: number;
  orderId: string;
  buyerId: string;
  itemId: string;
  purchaseQuantity?: number | null;
  requiredCardCount: number;
  status: string;
  attemptCount: number;
  lastErrorMessage: string;
  createdAt: number;
  updatedAt: number;
}

export interface AiSettings {
  cookie_id?: string;
  cookieId?: string;
  reply_mode?: ReplyMode;
  replyMode?: ReplyMode;
  ai_enabled?: boolean;
  aiEnabled?: boolean;
  auto_reply_enabled?: boolean;
  autoReplyEnabled?: boolean;
  auto_ship_enabled?: boolean;
  autoShipEnabled?: boolean;
  api_type?: string;
  apiType?: string;
  api_key?: string;
  apiKey?: string;
  base_url?: string;
  baseUrl?: string;
  model_name?: string;
  modelName?: string;
  max_bargain_rounds?: number;
  maxBargainRounds?: number;
  max_discount_percent?: number;
  maxDiscountPercent?: number;
  max_discount_amount?: number;
  maxDiscountAmount?: number;
  custom_prompts?: Record<string, unknown>;
  customPrompts?: Record<string, unknown>;
  updated_at?: number;
  updatedAt?: number;
}

export interface AiModelsDiscoverPayload {
  api_type?: string;
  apiType?: string;
  api_key?: string;
  apiKey?: string;
  base_url?: string;
  baseUrl?: string;
}

export interface AiModelsDiscoverResult {
  models: string[];
  endpoint?: string;
}

export interface AiConnectionTestPayload {
  api_type?: string;
  apiType?: string;
  api_key?: string;
  apiKey?: string;
  base_url?: string;
  baseUrl?: string;
  model_name?: string;
  modelName?: string;
}

export interface AiConnectionTestResult {
  ok: boolean;
  message: string;
  endpoint?: string;
  model_name?: string;
  modelName?: string;
  models?: string[];
}

export interface TaskProgress {
  status: string;
  total_targets: number;
  completed_targets: number;
  successful_targets: number;
  failed_targets: number;
  skipped_targets: number;
  pending_targets: number;
  percent: number;
  current_target_id: string;
  current_account_uid: string;
  last_message: string;
  updated_at: number;
}

export interface TaskAttemptSummary {
  total: number;
  success: number;
  failed: number;
  skipped: number;
  latest_created_at: number;
}

export interface TaskDeleteJobSummary {
  total: number;
  pending: number;
  running: number;
  retryable: number;
  succeeded: number;
  failed: number;
  cancelled: number;
  already_removed: number;
  item_gone: number;
  latest_created_at: number;
}

export interface TaskTarget {
  item_id: string;
  position: number;
  title: string;
  price: string;
  seller_id: string;
  seller_nick: string;
  status: string;
  created_at: number;
  updated_at: number;
}

export interface Task {
  task_id: string;
  title: string;
  status: string;
  source: string;
  target_total: number;
  account_total: number;
  created_at: number;
  updated_at: number;
  started_at: number;
  paused_at: number;
  finished_at: number;
  last_error: string;
  last_transition_at: number;
  last_log_at: number;
  comment_templates: string[];
  publish_interval_min_seconds: number;
  publish_interval_max_seconds: number;
  delete_interval_min_seconds: number;
  delete_interval_max_seconds: number;
  delete_after_seconds: number;
  enable_delete_comments: boolean;
  account_uids: string[];
  targets: TaskTarget[];
  progress: TaskProgress | null;
  attempt_summary: TaskAttemptSummary;
  delete_job_summary: TaskDeleteJobSummary;
}

export interface TaskDeleteJob {
  id: number;
  task_id: string;
  attempt_id: number;
  account_uid: string;
  item_id: string;
  comment_id: string;
  status: string;
  payload: Record<string, unknown>;
  scheduled_at: number;
  retry_count: number;
  started_at: number;
  finished_at: number;
  cancelled_at: number;
  last_error_at: number;
  last_error_code: string;
  last_error_message: string;
  claimed_by: string;
  claim_token: string;
  created_at: number;
  updated_at: number;
}

export interface TaskAttempt {
  id: number;
  task_id: string;
  account_uid: string;
  item_id: string;
  target_status: string;
  success: boolean;
  comment_content: string;
  comment_content_hash: string;
  source_comment: Record<string, unknown>;
  published_comment_id: string;
  published_at: number;
  failure_reason_code: string;
  failure_reason_message: string;
  failure_scope: string;
  request_payload: Record<string, unknown>;
  response_payload: Record<string, unknown>;
  delete_job_payload: Record<string, unknown>;
  created_at: number;
  updated_at: number;
}

export interface TaskLog {
  id: number;
  task_id: string;
  level: string;
  event: string;
  message: string;
  context: Record<string, unknown>;
  created_at: number;
}

export interface Pagination {
  page: number;
  page_size: number;
  total: number;
}

export interface PaginatedResponse<T> {
  items: T[];
  pagination: Pagination;
}

export interface RequestOptions {
  signal?: AbortSignal;
  headers?: Record<string, string>;
  timeoutMs?: number;
  timeoutMessage?: string;
  traceId?: string;
  requestId?: string;
}
