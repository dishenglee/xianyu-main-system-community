// eslint-disable-next-line @typescript-eslint/ban-ts-comment
// @ts-nocheck
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Image as ImageIcon, Plus } from "lucide-react";
import { useAccounts } from "../../hooks/useAccounts";
import {
  getItemReplies,
  getItemReplyModes,
  createItemReply,
  updateItemReply,
  deleteItemReply,
  deleteItemReplyMode,
  uploadItemImage,
  updateAccountSetting,
} from "../../api/index";
import { TopBar } from "../../components/TopBar";
import { HubTabs, HUB_PRESETS } from "../../components/HubTabs";
import { Modal } from "../../components/Modal";
import { StatusBadge } from "../../components/StatusBadge";
import { useSafeNotify } from "../../components/Notify";
import { useAppDialog } from "../../components/AppDialog";
import { useDevice } from "../../hooks/useDevice";
import { sanitizeSelectableUids } from "../automation/capabilities";
import { AccountConfigurationSplit } from "../automation/AccountConfigurationSplit";
import {
  loadStoredAccountSelection,
  saveStoredAccountSelection,
} from "../automation/accountSelectionStorage";
import {
  buildAccountItemReplyPayloads,
  normalizeReplyCount,
  resolveReplyTargetUids,
  selectSavedReplyTemplates,
} from "./replyTemplates";
import { useAuthScope } from "../../hooks/useAuthScope";
import { filterBusinessAccounts } from "../../utils/accountAccess";

const GLOBAL_SCOPE = "__global__";
const ACCOUNT_SELECTION_STORAGE_KEY = "item-replies";
const emptyForm = {
  mode: "default",
  itemId: "",
  reply: "",
  type: "text",
  enabled: true,
  autoRecall: false,
  replyCount: 1,
};


function createTaskGroupId() {
  if (
    typeof crypto !== "undefined" &&
    typeof crypto.randomUUID === "function"
  ) {
    return crypto.randomUUID();
  }
  return `reply-task-${Date.now()}-${Math.random().toString(36).slice(2, 10)}`;
}

function normalizeReply(rule) {
  return {
    ...rule,
    itemId: rule?.itemId || "",
    type: rule?.type || "text",
    cookieId: rule?.cookieId || rule?.cookie_id || "",
    reply: rule?.reply || "",
    enabled: Boolean(rule?.enabled),
    autoRecall: Boolean(rule?.autoRecall ?? rule?.auto_recall),
    taskGroupId: String(rule?.taskGroupId || rule?.task_group_id || "").trim(),
    replyCount: normalizeReplyCount(rule?.replyCount ?? rule?.reply_count),
  };
}

function normalizeItemReplyModeEntry(entry) {
  return {
    ...entry,
    cookieId: entry?.cookieId || entry?.cookie_id || "",
    itemId: entry?.itemId || entry?.item_id || "",
    replyMode: String(entry?.replyMode || entry?.reply_mode || "follow_global"),
    updatedAt: Number(entry?.updatedAt ?? entry?.updated_at ?? 0) || 0,
  };
}

function accountReplyMode(account) {
  return String(account?.replyMode || account?.reply_mode || "").trim();
}

function isItemReplyAccount(account) {
  const mode = accountReplyMode(account);
  return mode === "default" || mode === "item_reply";
}


export default function ItemRepliesPage() {
  const { accounts, loadAccounts } = useAccounts();
  const notify = useSafeNotify();
  const { showConfirm } = useAppDialog();
  const { isMobile, isTablet } = useDevice();
  const [saving, setSaving] = useState(false);
  const [globalReplies, setGlobalReplies] = useState([]);
  const [showModal, setShowModal] = useState(false);
  const [editingRuleIds, setEditingRuleIds] = useState([]);
  const [form, setForm] = useState({ ...emptyForm });
  const [error, setError] = useState("");
  const [uploadingImage, setUploadingImage] = useState(false);
  const [savedGlobalUids, setSavedGlobalUids] = useState([]);
  const [globalSelectedUids, setGlobalSelectedUids] = useState([]);
  const [globalSelectionSaving, setGlobalSelectionSaving] = useState(false);
  const [removingConfiguredUid, setRemovingConfiguredUid] = useState("");
  const [selectedTemplateIds, setSelectedTemplateIds] = useState([]);
  const [applyTaskAutoRecall, setApplyTaskAutoRecall] = useState(false);
  const [applyTaskReplyCount, setApplyTaskReplyCount] = useState(1);
  const loadSeqRef = useRef(0);
  const removedConfiguredUidsRef = useRef(new Set());
  const compact = isMobile || isTablet;

  const activeAccounts = filterBusinessAccounts(accounts);
  const activeAccountKey = useMemo(
    () => activeAccounts.map((a) => `${a.uid}:${accountReplyMode(a)}`).join("|"),
    [activeAccounts],
  );
  const savedGlobalUidSet = useMemo(
    () => new Set(savedGlobalUids),
    [savedGlobalUids],
  );
  const globalReplyTemplates = useMemo(
    () =>
      globalReplies.filter(
        (rule) => rule.enabled && String(rule.reply || "").trim(),
      ),
    [globalReplies],
  );

  const selectedTemplates = useMemo(
    () => selectSavedReplyTemplates(globalReplyTemplates, selectedTemplateIds),
    [globalReplyTemplates, selectedTemplateIds],
  );
  const selectedTextTemplateCount = useMemo(
    () =>
      selectedTemplates.filter((template) => (template.type || "text") !== "image")
        .length,
    [selectedTemplates],
  );
  const selectedImageTemplateCount = useMemo(
    () =>
      selectedTemplates.filter((template) => (template.type || "text") === "image")
        .length,
    [selectedTemplates],
  );
  const replyTargetUids = useMemo(
    () => resolveReplyTargetUids(globalSelectedUids),
    [globalSelectedUids],
  );
  const replyTargetKey = replyTargetUids.join("|");
  const pageScopeKey = replyTargetKey;
  const pageScopeKeyRef = useRef(pageScopeKey);
  pageScopeKeyRef.current = pageScopeKey;
  const clearPageState = useCallback(() => {
    loadSeqRef.current += 1;
    setGlobalReplies([]);
    setShowModal(false);
    setEditingRuleIds([]);
    setForm({ ...emptyForm });
    setError("");
    setUploadingImage(false);
    setGlobalSelectedUids([]);
    setSavedGlobalUids([]);
    setGlobalSelectionSaving(false);
    setRemovingConfiguredUid("");
    removedConfiguredUidsRef.current.clear();
    setSelectedTemplateIds([]);
    setApplyTaskAutoRecall(false);
    setApplyTaskReplyCount(1);
    setSaving(false);
  }, []);
  const { authToken, captureScope, isScopeCurrent } =
    useAuthScope(clearPageState);

  useEffect(() => {
    loadAccounts();
  }, [authToken]);
  useEffect(() => {
    // Existing enabled auto-reply accounts must be visible even when the
    // browser storage was cleared or was saved before that account was
    // configured.  Persist the merged selection so a later reload keeps the
    // same account scope.
    if (!activeAccounts.length) return;
    const realConfiguredSet = new Set(
      activeAccounts.filter(isItemReplyAccount).map((account) => account.uid),
    );
    const mergedSelection = [
      ...loadStoredAccountSelection(ACCOUNT_SELECTION_STORAGE_KEY).filter((uid) =>
        realConfiguredSet.has(uid),
      ),
      ...realConfiguredSet,
    ].filter((uid) => !removedConfiguredUidsRef.current.has(uid));
    const storedSelection = sanitizeSelectableUids(
      [...new Set(mergedSelection)],
      activeAccounts,
      "autoReply",
      { allowEnableDisabledAccounts: true },
    );
    saveStoredAccountSelection(ACCOUNT_SELECTION_STORAGE_KEY, storedSelection);
    setSavedGlobalUids(storedSelection);
    setGlobalSelectedUids((uids) => {
      const savedSet = new Set(storedSelection);
      return sanitizeSelectableUids(uids, activeAccounts, "autoReply", {
        allowEnableDisabledAccounts: true,
      }).filter((uid) => !savedSet.has(uid));
    });
  }, [activeAccountKey, authToken]);
  useEffect(() => {
    void loadReplies();
  }, [authToken, activeAccountKey]);
  async function loadReplies() {
    const scope = captureScope(pageScopeKey);
    const loadSeq = ++loadSeqRef.current;
    const isCurrent = () =>
      isScopeCurrent(scope, pageScopeKeyRef.current) &&
      loadSeqRef.current === loadSeq;
    try {
      const globalList = await getItemReplies(GLOBAL_SCOPE);
      if (!isCurrent()) return;
      setGlobalReplies(
        (globalList || [])
          .map(normalizeReply)
          .filter((rule) => rule.cookieId === GLOBAL_SCOPE),
      );
    } catch (e) {
      if (isCurrent()) notify(e?.message || "加载回复规则失败", "error");
    }
  }

  function openCreate(mode = "default") {
    setForm({ ...emptyForm, mode, itemId: "" });
    setEditingRuleIds([]);
    setError("");
    setShowModal(true);
  }

  function openEditTemplate(rule) {
    setForm({
      mode: rule.itemId ? "item" : "default",
      itemId: rule.itemId || "",
      reply: rule.reply || "",
      type: rule.type || "text",
      enabled: Boolean(rule.enabled),
      autoRecall: false,
      replyCount: 1,
    });
    setEditingRuleIds(
      Number.isFinite(Number(rule.id)) ? [Number(rule.id)] : [],
    );
    setError("");
    setShowModal(true);
  }


  function closeModal() {
    setShowModal(false);
    setEditingRuleIds([]);
    setForm({ ...emptyForm });
    setError("");
    setUploadingImage(false);
  }

  function uploadCandidates() {
    const ordered = [];
    const seen = new Set();
    const pushUid = (uid) => {
      const trimmed = String(uid || "").trim();
      if (!trimmed || seen.has(trimmed)) return;
      seen.add(trimmed);
      ordered.push(trimmed);
    };

    replyTargetUids.forEach(pushUid);
    activeAccounts.forEach((account) => pushUid(account.uid));
    return ordered;
  }

  async function handleImagePick(event) {
    const file = event.target.files?.[0];
    event.target.value = "";
    if (!file) return;
    const candidates = uploadCandidates();
    if (!candidates.length) {
      setError("没有可用账号，无法上传图片");
      return;
    }
    setError("");
    setUploadingImage(true);
    const scope = captureScope(pageScopeKey);
    try {
      let lastError = "";
      for (const uid of candidates) {
        try {
          const uploaded = await uploadItemImage(uid, file);
          if (!isScopeCurrent(scope, pageScopeKeyRef.current)) return;
          const imageUrl = String(uploaded?.url || "").trim();
          if (!imageUrl) {
            throw new Error("图片上传未返回可用地址");
          }
          setForm((f) => ({ ...f, reply: imageUrl }));
          setError("");
          return;
        } catch (error) {
          lastError = error?.message || String(error);
        }
      }
      throw new Error(
        `图片上传失败，已尝试 ${candidates.length} 个账号；最后错误：${lastError}`,
      );
    } catch (e) {
      if (isScopeCurrent(scope, pageScopeKeyRef.current))
        setError(e.message || "图片上传失败");
    } finally {
      if (isScopeCurrent(scope, pageScopeKeyRef.current))
        setUploadingImage(false);
    }
  }

  function handleGlobalAccountSelection(nextUids) {
    const sanitized = sanitizeSelectableUids(nextUids, activeAccounts, "autoReply", {
      allowEnableDisabledAccounts: true,
    }).filter((uid) => !savedGlobalUidSet.has(uid));
    setGlobalSelectedUids(sanitized);
  }

  async function replaceAccountItemReplies(uid, payloads) {
    const existing = await getItemReplies(uid);
    const accountRules = (existing || [])
      .map(normalizeReply)
      .filter((rule) => rule.cookieId === uid);
    await Promise.all(accountRules.map((rule) => deleteItemReply(rule.id)));
    await Promise.all(payloads.map((payload) => createItemReply(uid, payload)));
  }

  async function applyRepliesToAccount(uid, payloads) {
    await replaceAccountItemReplies(uid, payloads);
    await updateAccountSetting(uid, { replyMode: "default" });
  }

  async function saveGlobalAccountSelection() {
    const newUids = sanitizeSelectableUids(
      replyTargetUids,
      activeAccounts,
      "autoReply",
      { allowEnableDisabledAccounts: true },
    ).filter(
      (uid) =>
        !savedGlobalUidSet.has(uid) &&
        String(activeAccounts.find((account) => account.uid === uid)?.status || "") ===
          "active",
    );
    if (!newUids.length) {
      notify(
        selectedTemplates.length
          ? "请先在待配置账号中选择账号并保存配置"
          : "请先选择待配置账号",
        "error",
      );
      return;
    }
    setGlobalSelectionSaving(true);
    const scope = captureScope("save-accounts");
    try {
      const taskGroupId = selectedTemplates.length ? createTaskGroupId() : "";
      const payloads = buildAccountItemReplyPayloads(selectedTemplates, {
        autoRecall: applyTaskAutoRecall,
        taskGroupId,
        replyCount: applyTaskReplyCount,
      });
      const results = await Promise.allSettled(
        newUids.map((uid) =>
          payloads.length
            ? applyRepliesToAccount(uid, payloads)
            : updateAccountSetting(uid, { replyMode: "default" }),
        ),
      );
      if (!isScopeCurrent(scope, "save-accounts")) return;

      const succeededUids = newUids.filter((_, index) => results[index].status === "fulfilled");
      const failedUids = newUids.filter((_, index) => results[index].status === "rejected");
      const nextSavedUids = [...new Set([...savedGlobalUids, ...succeededUids])];
      succeededUids.forEach((uid) => removedConfiguredUidsRef.current.delete(uid));
      saveStoredAccountSelection(ACCOUNT_SELECTION_STORAGE_KEY, nextSavedUids);
      setSavedGlobalUids(nextSavedUids);
      setGlobalSelectedUids(failedUids);
      void loadAccounts({ force: true });
      void loadReplies();

      if (!failedUids.length) {
        setSelectedTemplateIds([]);
        setApplyTaskAutoRecall(false);
        setApplyTaskReplyCount(1);
        notify(
          payloads.length
            ? `已配置 ${succeededUids.length} 个账号并应用 ${payloads.length} 条回复`
            : `已配置 ${succeededUids.length} 个账号`,
          "success",
        );
      } else if (succeededUids.length) {
        notify(`已配置 ${succeededUids.length} 个账号，${failedUids.length} 个失败并保留待重试`, "error");
      } else {
        const firstFailure = results.find((result) => result.status === "rejected");
        notify(firstFailure?.reason?.message || "保存账号应用范围失败", "error");
      }
    } catch (e) {
      if (isScopeCurrent(scope, "save-accounts"))
        notify(e?.message || "保存账号应用范围失败", "error");
    } finally {
      if (isScopeCurrent(scope, "save-accounts"))
        setGlobalSelectionSaving(false);
    }
  }

  async function removeConfiguredAccount(uid) {
    if (!savedGlobalUidSet.has(uid) || removingConfiguredUid) return;
    const account = activeAccounts.find((entry) => entry.uid === uid);
    const confirmed = await showConfirm(
      `确认移除“${account?.nick || uid}”的自动回复配置？`,
      {
        title: "移除自动回复账号",
        confirmLabel: "移除并删除",
        danger: true,
      },
    );
    if (!confirmed) return;
    setRemovingConfiguredUid(uid);
    const scope = captureScope(`remove-account:${uid}`);
    try {
      const existingReplies = await getItemReplies(uid);
      const replyIds = (existingReplies || [])
        .map(normalizeReply)
        .filter((rule) => rule.cookieId === uid && Number.isFinite(Number(rule.id)))
        .map((rule) => Number(rule.id));
      await Promise.all(replyIds.map((id) => deleteItemReply(id)));

      const existingModes = await getItemReplyModes(uid);
      const itemIds = [
        ...new Set(
          (existingModes || [])
            .map(normalizeItemReplyModeEntry)
            .filter((entry) => entry.cookieId === uid && entry.itemId)
            .map((entry) => entry.itemId),
        ),
      ];
      await Promise.all(itemIds.map((itemId) => deleteItemReplyMode(uid, itemId)));
      await updateAccountSetting(uid, { replyMode: "off" });
      if (!isScopeCurrent(scope, `remove-account:${uid}`)) return;
      removedConfiguredUidsRef.current.add(uid);
      setSavedGlobalUids((current) => {
        const nextSavedUids = current.filter((item) => item !== uid);
        saveStoredAccountSelection(ACCOUNT_SELECTION_STORAGE_KEY, nextSavedUids);
        return nextSavedUids;
      });
      void loadAccounts({ force: true });
      void loadReplies();
      notify("已移除账号及全部回复配置", "success");
    } catch (error) {
      if (isScopeCurrent(scope, `remove-account:${uid}`)) {
        void loadAccounts({ force: true });
        notify(error?.message || "移除已配置账号失败，已重新同步账号状态", "error");
      }
    } finally {
      if (isScopeCurrent(scope, `remove-account:${uid}`)) setRemovingConfiguredUid("");
    }
  }

  function toggleTemplate(ruleId) {
    setSelectedTemplateIds((ids) => {
      const id = Number(ruleId);
      if (!Number.isFinite(id)) return ids;
      return ids.includes(id)
        ? ids.filter((item) => item !== id)
        : [...ids, id];
    });
  }

  async function submit() {
    const isEditing = editingRuleIds.length > 0;
    setError("");
    if (!form.reply.trim()) {
      setError(form.type === "image" ? "图片地址不能为空" : "回复内容不能为空");
      return;
    }
    if (
      form.mode === "item" &&
      !String(form.itemId || "").trim()
    ) {
      setError("商品专用回复必须填写商品 ID");
      return;
    }
    setSaving(true);
    const scope = captureScope(pageScopeKey);
    try {
      if (isEditing) {
        const payload = {
          itemId: form.mode === "item" ? form.itemId : "",
          reply: form.reply.trim(),
          type: form.type,
          enabled: Boolean(form.enabled),
          autoRecall: false,
        };
        await Promise.all(
          editingRuleIds.map((id) => updateItemReply(id, payload)),
        );
      } else {
        const payload = {
          itemId: form.mode === "item" ? form.itemId : "",
          reply: form.reply.trim(),
          type: form.type,
          enabled: Boolean(form.enabled),
          autoRecall: false,
        };
        await createItemReply(GLOBAL_SCOPE, payload);
      }
      if (!isScopeCurrent(scope, pageScopeKeyRef.current)) return;
      closeModal();
      await loadReplies();
      notify(
        isEditing ? "回复规则已更新" : "回复模板已新增",
        "success",
      );
    } catch (e) {
      if (isScopeCurrent(scope, pageScopeKeyRef.current)) setError(e.message);
    } finally {
      if (isScopeCurrent(scope, pageScopeKeyRef.current)) setSaving(false);
    }
  }

  async function removeTemplate(rule) {
    const isTemplate = rule.cookieId === GLOBAL_SCOPE;
    const label = String(rule.itemId || "").trim()
      ? `商品 ${rule.itemId} 的${isTemplate ? "回复模板" : "专用回复"}`
      : `这条${isTemplate ? "默认回复模板" : "账号默认回复"}`;
    const confirmed = await showConfirm(`确认删除${label}？`, {
      title: "删除回复规则",
      confirmLabel: "删除",
      danger: true,
    });
    if (!confirmed) return;
    const scope = captureScope(pageScopeKey);
    try {
      await deleteItemReply(rule.id);
      if (!isScopeCurrent(scope, pageScopeKeyRef.current)) return;
      await loadReplies();
    } catch (e) {
      if (isScopeCurrent(scope, pageScopeKeyRef.current))
        notify(e?.message || "删除回复规则失败", "error");
    }
  }


  function renderTemplateOption(rule) {
    const id = Number(rule.id);
    const checked = selectedTemplateIds.includes(id);
    return (
      <div
        key={rule.id}
        className={`${compact ? "mobile-soft-card p-3.5" : "input"} ${checked ? "ring-2" : ""}`}
        style={
          checked ? { "--tw-ring-color": "var(--primary-light)" } : undefined
        }
      >
        <div className="flex items-start gap-3">
          <input
            type="checkbox"
            aria-label={`选择回复模板 ${rule.itemId ? `商品 ${rule.itemId}` : "默认回复"}`}
            className="mt-1 h-4 w-4 shrink-0 rounded"
            style={{ accentColor: "var(--primary)" }}
            checked={checked}
            onChange={() => toggleTemplate(rule.id)}
          />
          <div className="min-w-0 flex-1">
            <div className="mb-2 flex flex-wrap items-center gap-2">
              <span
                className="rounded-full px-2 py-0.5 text-[11px] font-semibold"
                style={{
                  background: "var(--primary-light)",
                  color: "var(--primary)",
                }}
              >
                {rule.itemId ? `商品 ${rule.itemId}` : "默认回复"}
              </span>
              <span
                className="rounded-full px-2 py-0.5 text-[11px] font-medium"
                style={{
                  background: "var(--surface-soft)",
                  color: "var(--text-secondary)",
                }}
              >
                {rule.type === "image" ? "图片" : "文字"}
              </span>
              <StatusBadge
                status={rule.enabled ? "info" : "neutral"}
                label={rule.enabled ? "启用中" : "已关闭"}
              />
            </div>
            {rule.type === "image" ? (
              <div className="space-y-2">
                <img
                  src={rule.reply}
                  alt="已保存回复图片"
                  className="max-h-[96px] rounded-xl border object-contain bg-[var(--surface-soft)]"
                  style={{ borderColor: "var(--border-light)" }}
                />
              </div>
            ) : (
              <div
                className="line-clamp-3 whitespace-pre-wrap text-[13px] leading-5"
                style={{ color: "var(--text-main)" }}
              >
                {rule.reply}
              </div>
            )}
          </div>
        </div>
        <div
          className={
            compact
              ? "mt-3 mobile-soft-card flex items-center gap-2 p-2.5"
              : "mt-3 flex items-center gap-2 border-t pt-3"
          }
          style={compact ? undefined : { borderColor: "var(--border-light)" }}
        >
          <button
            type="button"
            onClick={() => openEditTemplate(rule)}
            className="btn-ghost btn-sm flex-1 md:flex-initial"
          >
            编辑
          </button>
          <button
            type="button"
            onClick={() => removeTemplate(rule)}
            className="btn-sm flex-1 md:flex-initial"
            style={{ color: "var(--danger)" }}
          >
            删除
          </button>
        </div>
      </div>
    );
  }

  return (
    <div
      className={
        compact ? "h-full flex flex-col mobile-page-shell" : "contents"
      }
      style={compact ? { backgroundColor: "var(--bg-page)" } : undefined}
    >
      {compact ? <TopBar title="默认 / 商品回复" /> : null}

      {!compact && <HubTabs tabs={HUB_PRESETS.automation} />}

      <div
        className={
          compact
            ? "flex-1 overflow-y-auto mobile-page-body"
            : "flex-1 overflow-y-auto px-4 py-4 md:px-6"
        }
      >
        <div className={compact ? "mobile-page-stack" : "space-y-6"}>
          <section
            className={
              compact
                ? "mobile-surface-card p-4 space-y-4"
                : "card p-4 md:p-5 mb-4 space-y-4"
            }
          >
            {compact ? (
              <div className="mobile-section-head items-start">
                <div className="mobile-section-copy">
                  <h2 className="mobile-section-title">回复配置</h2>
                </div>
                <span className="mobile-pill mobile-muted-pill shrink-0">
                  {globalReplies.length} 条模板
                </span>
              </div>
            ) : (
              <div className="flex flex-col md:flex-row md:items-start md:justify-between gap-3">
                <div>
                  <div
                    className="text-[15px] font-semibold"
                    style={{ color: "var(--text-main)" }}
                  >
                    回复配置
                  </div>
                </div>
              </div>
            )}

            <div className="space-y-4">
              <div
                className={
                  compact
                    ? "mobile-soft-card p-3.5 space-y-3"
                    : "surface-card rounded-2xl p-4 space-y-3"
                }
              >
                <div
                  className={
                    compact
                      ? "mobile-section-head items-start"
                      : "flex flex-col gap-2 md:flex-row md:items-start md:justify-between"
                  }
                >
                  <div className={compact ? "mobile-section-copy" : undefined}>
                    <div
                      className={
                        compact
                          ? "mobile-section-title text-[1.05rem]"
                          : "text-[14px] font-semibold"
                      }
                      style={{
                        color: compact ? undefined : "var(--text-primary)",
                      }}
                    >
                      已保存回复模板
                    </div>
                    {!compact ? (
                      <div
                        className="text-[12px] mt-1"
                        style={{ color: "var(--text-secondary)" }}
                      >
                        先保存默认回复或商品专用回复模板；在待配置账号中勾选账号后，保存配置时可同时应用所选模板。
                      </div>
                    ) : null}
                  </div>
                  <div className="flex flex-wrap items-center gap-2 md:justify-end">
                    <button
                      onClick={() => openCreate("default")}
                      className="btn-primary btn-sm flex items-center gap-1"
                    >
                      <Plus className="w-4 h-4" /> 新增默认回复模板
                    </button>
                    <button
                      onClick={() => openCreate("item")}
                      className="btn-primary btn-sm"
                    >
                      新增商品回复模板
                    </button>
                    <div
                      className={
                        compact
                          ? "mobile-pill mobile-muted-pill shrink-0"
                          : "text-[12px]"
                      }
                      style={{
                        color: selectedTemplates.length
                          ? "var(--primary)"
                          : "var(--text-secondary)",
                      }}
                    >
                      已选择 {selectedTemplates.length} 条
                    </div>
                  </div>
                </div>
                {!globalReplies.length ? (
                  <div
                    className={
                      compact
                        ? "mobile-soft-card p-4 text-[13px]"
                        : "rounded-xl border border-dashed p-4 text-[13px]"
                    }
                    style={{ color: "var(--text-secondary)" }}
                  >
                    先保存一条默认回复或商品专用回复模板，再从这里选择要批量应用到账号的内容。
                  </div>
                ) : (
                  <div className="grid gap-2 md:grid-cols-5">
                    {globalReplies.map(renderTemplateOption)}
                  </div>
                )}
              </div>

              <div
                className={
                  compact
                    ? "mobile-soft-card p-3.5 space-y-3"
                    : "surface-card rounded-2xl p-4 space-y-3"
                }
              >
                <div
                  className={compact ? "mobile-section-head items-start" : "flex flex-col gap-2"}
                >
                  <div className={compact ? "mobile-section-copy" : undefined}>
                    <div
                      className={
                        compact
                          ? "mobile-section-title text-[1.05rem]"
                          : "text-[14px] font-semibold"
                      }
                      style={{
                        color: compact ? undefined : "var(--text-primary)",
                      }}
                    >
                      配置
                    </div>
                    {!compact ? (
                      <div
                        className="text-[12px] mt-1"
                        style={{ color: "var(--text-secondary)" }}
                      >
                        从待配置账号中选择并保存；选中的模板只会应用到本次新增账号。
                      </div>
                    ) : null}
                  </div>
                </div>

                <AccountConfigurationSplit
                  accounts={activeAccounts}
                  configuredUids={savedGlobalUids}
                  selectedPendingUids={globalSelectedUids}
                  onSelectionChange={handleGlobalAccountSelection}
                  onSave={saveGlobalAccountSelection}
                  onRemove={removeConfiguredAccount}
                  saving={globalSelectionSaving}
                  removingUid={removingConfiguredUid}
                  pendingDescription={selectedTemplates.length ? `保存时会同时应用 ${selectedTemplates.length} 条模板。` : "只显示尚未配置的活跃账号。"}
                  configuredDescription="移除会删除该账号的默认回复、商品回复和商品覆盖配置，并关闭回复模式。"
                />

                <div
                  className={`${compact ? "mobile-soft-card" : "rounded-2xl border"} space-y-3 px-4 py-3`}
                  style={
                    compact ? undefined : { borderColor: "var(--border-light)" }
                  }
                >
                  <div className="flex flex-col gap-3 md:flex-row md:items-end">
                    <label className="flex-1 min-w-0">
                      <span className="label">回复次数</span>
                      <input
                        type="number"
                        min="1"
                        max="999"
                        className="input"
                        value={applyTaskReplyCount}
                        onChange={(e) =>
                          setApplyTaskReplyCount(normalizeReplyCount(e.target.value))
                        }
                      />
                    </label>
                    <label className="flex items-center gap-3 px-1 py-2 text-[13px] cursor-pointer">
                      <input
                        type="checkbox"
                        checked={applyTaskAutoRecall}
                        onChange={(e) => setApplyTaskAutoRecall(e.target.checked)}
                        className="h-4 w-4 rounded"
                        style={{ accentColor: "var(--primary)" }}
                      />
                      自动撤回已回复内容
                    </label>
                  </div>
                  <div
                    className="text-[12px] leading-relaxed"
                    style={{ color: "var(--text-secondary)" }}
                  >
                    {selectedTextTemplateCount > 0 && selectedImageTemplateCount > 0
                      ? `当前已勾选 ${selectedTextTemplateCount} 条文字和 ${selectedImageTemplateCount} 张图片；每次触发会随机发送 1 段文字 + 1 张图片，回复次数按整条任务累计。`
                      : "若同时勾选多条文字和多张图片，每次触发会随机发送 1 段文字 + 1 张图片，回复次数按整条任务累计。"}
                  </div>
                </div>

                <div
                  className={
                    compact
                      ? "mobile-soft-card px-4 py-3 text-[12px]"
                      : "surface-card rounded-2xl px-4 py-3 text-[12px]"
                  }
                  style={{ color: "var(--text-secondary)" }}
                >
                  待保存 {replyTargetUids.length} 个账号，已配置 {savedGlobalUids.length} 个账号。勾选模板后保存配置，只会把模板应用到本次新增账号。
                </div>
              </div>


            </div>
          </section>

        </div>
      </div>

      <Modal
        open={showModal}
        onClose={closeModal}
        title={
          editingRuleIds.length
            ? "编辑回复"
            : form.mode === "item"
              ? "新增商品回复模板"
              : "新增默认回复模板"
        }
      >
        <div className="space-y-4 py-2">
          <>
              <label className="block">
                <span className="label">回复类型</span>
                <select
                  className="input"
                  value={form.type}
                  onChange={(e) =>
                    setForm((f) => ({ ...f, type: e.target.value }))
                  }
                >
                  <option value="text">文字回复</option>
                  <option value="image">图片回复</option>
                </select>
              </label>

              {form.mode === "item" ? (
                <label className="block">
                  <span className="label">商品 ID</span>
                  <input
                    className="input"
                    value={form.itemId}
                    onChange={(e) =>
                      setForm((f) => ({
                        ...f,
                        itemId: e.target.value,
                        mode: "item",
                      }))
                    }
                    placeholder="填写后只作用于该商品"
                  />
                </label>
              ) : null}

              <label className="block">
                <span className="label">
                  {form.type === "image" ? "图片地址" : "回复内容"}
                </span>
                {form.type === "image" ? (
                  <div className="space-y-3">
                    <label
                      className="inline-flex items-center gap-2 rounded-2xl border px-4 py-3 text-[13px] cursor-pointer"
                      style={{ borderColor: "var(--border-light)" }}
                    >
                      <input
                        type="file"
                        accept="image/*"
                        className="hidden"
                        onChange={handleImagePick}
                        disabled={uploadingImage}
                      />
                      <ImageIcon
                        className="w-4 h-4"
                        style={{ color: "var(--primary)" }}
                      />
                      {uploadingImage ? "上传中…" : "选择本地图片"}
                    </label>
                    <div
                      className="text-[12px]"
                      style={{ color: "var(--text-secondary)" }}
                    >
                      会优先使用当前勾选账号上传；如果没有勾选账号或上传失败，会自动换其他活跃账号重试。
                    </div>
                    <input
                      className="input"
                      value={form.reply}
                      onChange={(e) =>
                        setForm((f) => ({ ...f, reply: e.target.value }))
                      }
                      placeholder="上传后会自动回填图片地址；也可手动粘贴已上传 URL"
                    />
                  </div>
                ) : (
                  <textarea
                    className="input min-h-[120px] resize-y"
                    value={form.reply}
                    onChange={(e) =>
                      setForm((f) => ({ ...f, reply: e.target.value }))
                    }
                    placeholder="买家咨询时自动回复的话术…"
                  />
                )}
              </label>

              {form.type === "image" && form.reply.trim() ? (
                <div
                  className="rounded-2xl border bg-[var(--surface-soft)] p-3"
                  style={{ borderColor: "var(--border-light)" }}
                >
                  <div
                    className="text-[12px] mb-2"
                    style={{ color: "var(--text-secondary)" }}
                  >
                    图片预览
                  </div>
                  <img
                    src={form.reply}
                    alt="预览"
                    className="max-h-[200px] rounded-xl object-contain"
                  />
                </div>
              ) : null}
          </>


          <label
            className="flex items-center gap-3 rounded-2xl border px-4 py-3 text-[13px] cursor-pointer"
            style={{ borderColor: "var(--border-light)" }}
          >
            <input
              type="checkbox"
              checked={form.enabled}
              onChange={(e) =>
                setForm((f) => ({ ...f, enabled: e.target.checked }))
              }
              className="h-4 w-4 rounded"
              style={{ accentColor: "var(--primary)" }}
            />
            {String(form.itemId || "").trim()
              ? "模板启用后，应用到账号时该商品会优先触发这条专用回复"
              : "模板启用后，应用到账号时会在关键词和 AI 都没命中时触发这条默认回复"}
          </label>


          {error && (
            <p className="liquid-notice liquid-notice-danger rounded-2xl px-4 py-3 text-[13px]">
              {error}
            </p>
          )}

          {!editingRuleIds.length ? (
            <p
              className="text-[12px] leading-relaxed"
              style={{ color: "var(--text-secondary)" }}
            >
              保存后先进入已保存回复模板。勾选账号和模板，点击"应用回复到账号"后才会写入对应账号。
            </p>
          ) : null}

          <div className="flex flex-col-reverse sm:flex-row items-stretch sm:items-center sm:justify-end gap-2 sm:gap-3 pt-2">
            <button
              onClick={closeModal}
              className="btn-secondary btn-sm w-full sm:w-auto"
            >
              取消
            </button>
            <button
              onClick={submit}
              disabled={saving || uploadingImage}
              className="btn-primary btn-sm w-full sm:w-auto disabled:opacity-40"
            >
              {saving ? "保存中…" : "保存"}
            </button>
          </div>
        </div>
      </Modal>
    </div>
  );
}
