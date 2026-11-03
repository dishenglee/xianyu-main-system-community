// eslint-disable-next-line @typescript-eslint/ban-ts-comment
// @ts-nocheck
import React from "react";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { beforeEach, describe, expect, it, vi } from "vitest";

const {
  accountStoreMock,
  apiMocks,
  notifyMock,
  loadStoredAccountSelectionMock,
  saveStoredAccountSelectionMock,
} = vi.hoisted(() => ({
  accountStoreMock: {
    accounts: [],
    loadAccounts: vi.fn().mockResolvedValue([]),
  },
  apiMocks: {
    getItemReplies: vi.fn(),
    getItemReplyModes: vi.fn(),
    createItemReply: vi.fn(),
    updateItemReply: vi.fn(),
    deleteItemReply: vi.fn(),
    upsertItemReplyMode: vi.fn(),
    deleteItemReplyMode: vi.fn(),
    uploadItemImage: vi.fn(),
    updateAccountSetting: vi.fn(),
  },
  notifyMock: vi.fn(),
  loadStoredAccountSelectionMock: vi.fn(),
  saveStoredAccountSelectionMock: vi.fn(),
}));

vi.mock("../../hooks/useAccounts", () => ({
  useAccounts: () => accountStoreMock,
}));

vi.mock("../../api/index", () => apiMocks);
vi.mock("../../components/TopBar", () => ({
  TopBar: ({ title }) => <div>{title}</div>,
}));
vi.mock("../../components/HubTabs", () => ({
  HubTabs: () => null,
  HUB_PRESETS: { automation: [] },
}));
vi.mock("../../components/EmptyState", () => ({
  EmptyState: ({ title, description }) => (
    <div>
      <div>{title}</div>
      <div>{description}</div>
    </div>
  ),
}));
vi.mock("../../components/Modal", () => ({
  Modal: ({ open, title, children }) =>
    open ? (
      <div>
        <div>{title}</div>
        {children}
      </div>
    ) : null,
}));
vi.mock("../../components/StatusBadge", () => ({
  StatusBadge: ({ label }) => <span>{label}</span>,
}));
vi.mock("../../components/Notify", () => ({
  useSafeNotify: () => notifyMock,
}));
vi.mock("../../components/AppDialog", () => ({
  useAppDialog: () => ({
    showConfirm: vi.fn().mockResolvedValue(true),
  }),
}));
vi.mock("../../hooks/useDevice", () => ({
  useDevice: () => ({
    isMobile: false,
    isTablet: false,
    isDesktop: true,
    device: "desktop",
  }),
}));
vi.mock("../automation/accountSelectionStorage", () => ({
  loadStoredAccountSelection: loadStoredAccountSelectionMock,
  saveStoredAccountSelection: saveStoredAccountSelectionMock,
}));
vi.mock("../automation/capabilities", () => ({
  diffCapabilitySelection: () => ({
    dirty: false,
    toEnable: [],
    toDisable: [],
  }),
  enabledCapabilityUids: (accounts) =>
    accounts
      .filter((account) => account.autoReply || account.replyMode === "default")
      .map((account) => account.uid),
  sanitizeSelectableUids: (uids, accounts) =>
    uids.filter((uid) => accounts.some((account) => account.uid === uid)),
}));
vi.mock("../../hooks/useAuthScope", () => ({
  useAuthScope: () => ({
    authToken: "token-a",
    captureScope: (key = "") => ({
      token: "token-a",
      key,
    }),
    isScopeCurrent: (scope, key = scope.key) => scope.key === key,
  }),
}));

import ItemRepliesPage from "./ItemRepliesPage";

function renderPage() {
  return render(
    <MemoryRouter>
      <ItemRepliesPage />
    </MemoryRouter>,
  );
}

describe("ItemRepliesPage", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    accountStoreMock.accounts = [];
    accountStoreMock.loadAccounts.mockResolvedValue([]);
    loadStoredAccountSelectionMock.mockReturnValue([]);
    apiMocks.getItemReplies.mockResolvedValue([]);
    apiMocks.getItemReplyModes.mockResolvedValue([]);
    apiMocks.createItemReply.mockImplementation(async (cookieId, payload) => ({
      id: Date.now(),
      cookieId,
      ...payload,
    }));
    apiMocks.updateAccountSetting.mockResolvedValue({ status: "ok" });
    apiMocks.deleteItemReply.mockResolvedValue({ status: "ok" });
  });

  it("moves a pending account into configured accounts and enables default mode without templates", async () => {
    accountStoreMock.accounts = [
      { uid: "uid-new", nick: "待配置店铺", status: "active", autoReply: false },
      { uid: "uid-live", nick: "已启用店铺", status: "active", replyMode: "default" },
    ];
    loadStoredAccountSelectionMock.mockReturnValue([]);
    renderPage();

    const pending = screen.getByText("待配置账号").closest("section");
    const configured = screen.getByText("已配置账号").closest("section");
    expect(screen.getByTestId("account-configuration-split").className).toContain(
      "lg:grid-cols-2",
    );
    expect(screen.getByTestId("pending-account-grid").className).toContain(
      "grid-cols-2",
    );
    expect(screen.getByTestId("configured-account-grid").className).toContain(
      "grid-cols-2",
    );
    expect(await within(pending).findByText("待配置店铺")).toBeTruthy();
    expect(within(pending).queryByText("已启用店铺")).toBeNull();
    expect(within(configured).getByText("已启用店铺")).toBeTruthy();

    fireEvent.click(
      within(pending).getByRole("checkbox", {
        name: "选择待配置账号 待配置店铺",
      }),
    );
    fireEvent.click(within(pending).getByRole("button", { name: "保存配置" }));

    await waitFor(() => {
      expect(apiMocks.updateAccountSetting).toHaveBeenCalledWith("uid-new", {
        replyMode: "default",
      });
      expect(within(pending).queryByText("待配置店铺")).toBeNull();
      expect(within(configured).getByText("待配置店铺")).toBeTruthy();
    });
    expect(apiMocks.createItemReply).not.toHaveBeenCalled();
    expect(saveStoredAccountSelectionMock).toHaveBeenCalledWith(
      "item-replies",
      ["uid-live", "uid-new"],
    );
  });

  it("does not treat stored keyword or AI accounts as default-reply configuration", async () => {
    accountStoreMock.accounts = [
      { uid: "uid-keyword", nick: "关键词店铺", status: "active", replyMode: "keyword" },
      { uid: "uid-ai", nick: "AI 店铺", status: "active", replyMode: "ai" },
      { uid: "uid-default", nick: "默认回复店铺", status: "active", replyMode: "default" },
    ];
    loadStoredAccountSelectionMock.mockReturnValue(["uid-keyword", "uid-ai", "uid-default"]);
    renderPage();

    const pending = screen.getByText("待配置账号").closest("section");
    const configured = screen.getByText("已配置账号").closest("section");
    expect(await within(configured).findByText("默认回复店铺")).toBeTruthy();
    expect(within(configured).queryByText("关键词店铺")).toBeNull();
    expect(within(configured).queryByText("AI 店铺")).toBeNull();
    expect(within(pending).getByText("关键词店铺")).toBeTruthy();
    expect(within(pending).getByText("AI 店铺")).toBeTruthy();
  });

  it("moves successful accounts right and leaves failed accounts selected for retry", async () => {
    accountStoreMock.accounts = [
      { uid: "uid-ok", nick: "成功店铺", status: "active", replyMode: "off" },
      { uid: "uid-fail", nick: "失败店铺", status: "active", replyMode: "off" },
    ];
    apiMocks.updateAccountSetting.mockImplementation(async (uid) => {
      if (uid === "uid-fail") throw new Error("启用失败");
      return { status: "ok" };
    });
    renderPage();

    const pending = screen.getByText("待配置账号").closest("section");
    const configured = screen.getByText("已配置账号").closest("section");
    fireEvent.click(within(pending).getByRole("checkbox", { name: "选择待配置账号 成功店铺" }));
    fireEvent.click(within(pending).getByRole("checkbox", { name: "选择待配置账号 失败店铺" }));
    fireEvent.click(within(pending).getByRole("button", { name: "保存配置" }));

    await waitFor(() => {
      expect(within(configured).getByText("成功店铺")).toBeTruthy();
      expect(within(pending).getByText("失败店铺")).toBeTruthy();
      expect(within(pending).getByRole("checkbox", { name: "选择待配置账号 失败店铺" }).checked).toBe(true);
    });
    expect(notifyMock).toHaveBeenCalledWith("已配置 1 个账号，1 个失败并保留待重试", "error");
  });

  it("removes a configured account and deletes all of its reply configuration", async () => {
    accountStoreMock.accounts = [
      { uid: "uid-1", nick: "已配置店铺", status: "active", replyMode: "default" },
    ];
    loadStoredAccountSelectionMock.mockReturnValue(["uid-1"]);
    apiMocks.getItemReplies.mockImplementation(async (cookieId) =>
      cookieId === "uid-1"
        ? [{ id: 91, cookieId, itemId: "", reply: "旧回复", type: "text" }]
        : [],
    );
    apiMocks.getItemReplyModes.mockImplementation(async (cookieId) =>
      cookieId === "uid-1"
        ? [{ cookieId, itemId: "item-9", replyMode: "item_reply" }]
        : [],
    );
    renderPage();

    const pending = screen.getByText("待配置账号").closest("section");
    const configured = screen.getByText("已配置账号").closest("section");
    expect(await within(configured).findByText("已配置店铺")).toBeTruthy();
    fireEvent.click(
      within(configured).getByRole("button", { name: "移除已配置账号 已配置店铺" }),
    );

    await waitFor(() => {
      expect(apiMocks.updateAccountSetting).toHaveBeenCalledWith("uid-1", {
        replyMode: "off",
      });
      expect(apiMocks.deleteItemReply).toHaveBeenCalledWith(91);
      expect(apiMocks.deleteItemReplyMode).toHaveBeenCalledWith(
        "uid-1",
        "item-9",
      );
      expect(apiMocks.deleteItemReply.mock.invocationCallOrder[0]).toBeLessThan(
        apiMocks.getItemReplyModes.mock.invocationCallOrder[0],
      );
      expect(within(configured).queryByText("已配置店铺")).toBeNull();
      expect(within(pending).getByText("已配置店铺")).toBeTruthy();
    });
    expect(saveStoredAccountSelectionMock).toHaveBeenCalledWith(
      "item-replies",
      [],
    );
  });

  it("creates a default reply template in global scope without selecting an account first", async () => {
    renderPage();

    fireEvent.click(
      screen.getAllByRole("button", { name: "新增默认回复模板" })[0],
    );
    fireEvent.change(screen.getByPlaceholderText("买家咨询时自动回复的话术…"), {
      target: { value: "默认回复内容" },
    });
    fireEvent.click(screen.getByRole("button", { name: "保存" }));

    await waitFor(() => {
      expect(apiMocks.createItemReply).toHaveBeenCalledWith("__global__", {
        itemId: "",
        reply: "默认回复内容",
        type: "text",
        enabled: true,
        autoRecall: false,
      });
    });
  });

  it("creates an item-specific reply template in global scope without selecting an account first", async () => {
    renderPage();

    fireEvent.click(
      screen.getAllByRole("button", { name: "新增商品回复模板" })[0],
    );
    fireEvent.change(screen.getByPlaceholderText("填写后只作用于该商品"), {
      target: { value: "item-1001" },
    });
    fireEvent.change(screen.getByPlaceholderText("买家咨询时自动回复的话术…"), {
      target: { value: "商品专用回复" },
    });
    fireEvent.click(screen.getByRole("button", { name: "保存" }));

    await waitFor(() => {
      expect(apiMocks.createItemReply).toHaveBeenCalledWith("__global__", {
        itemId: "item-1001",
        reply: "商品专用回复",
        type: "text",
        enabled: true,
        autoRecall: false,
      });
    });
  });

  it("keeps edit and delete actions in the saved template list without rendering a duplicate template pool section", async () => {
    apiMocks.getItemReplies.mockResolvedValue([
      {
        id: 7,
        cookieId: "__global__",
        itemId: "",
        reply: "已保存模板",
        type: "text",
        enabled: true,
      },
    ]);

    renderPage();

    expect(await screen.findByText("已保存模板")).toBeTruthy();
    expect(screen.getByRole("button", { name: "编辑" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "删除" })).toBeTruthy();
    expect(screen.queryByText("回复模板池")).toBeNull();
  });

  it("does not render image url text in the saved reply template list", async () => {
    apiMocks.getItemReplies.mockResolvedValue([
      {
        id: 8,
        cookieId: "__global__",
        itemId: "",
        reply: "https://cdn.example.com/reply-image.png",
        type: "image",
        enabled: true,
      },
    ]);

    renderPage();

    expect(await screen.findByAltText("已保存回复图片")).toBeTruthy();
    expect(
      screen.queryByText("https://cdn.example.com/reply-image.png"),
    ).toBeNull();
  });

  it("does not render the removed item override and saved task sections", async () => {
    accountStoreMock.accounts = [
      { uid: "uid-1", nick: "账号一", status: "active" },
      { uid: "uid-2", nick: "账号二", status: "active" },
      { uid: "uid-3", nick: "账号三", status: "active" },
    ];
    loadStoredAccountSelectionMock.mockReturnValue(["uid-1", "uid-2", "uid-3"]);
    apiMocks.getItemReplies.mockImplementation(async (cookieId) => {
      if (cookieId === "__global__") return [];
      if (cookieId === "uid-1") {
        return [
          {
            id: 101,
            cookieId,
            itemId: "",
            reply: "统一回复",
            type: "text",
            enabled: true,
            autoRecall: true,
            taskGroupId: "task-a",
          },
          {
            id: 102,
            cookieId,
            itemId: "item-9",
            reply: "商品专用回复",
            type: "text",
            enabled: true,
            autoRecall: true,
            taskGroupId: "task-a",
          },
        ];
      }
      if (cookieId === "uid-2") {
        return [
          {
            id: 103,
            cookieId,
            itemId: "",
            reply: "统一回复",
            type: "text",
            enabled: true,
            autoRecall: true,
            taskGroupId: "task-a",
          },
          {
            id: 104,
            cookieId,
            itemId: "item-9",
            reply: "商品专用回复",
            type: "text",
            enabled: true,
            autoRecall: true,
            taskGroupId: "task-a",
          },
        ];
      }
      if (cookieId === "uid-3")
        return [
          {
            id: 105,
            cookieId,
            itemId: "",
            reply: "第三组回复",
            type: "text",
            enabled: true,
            autoRecall: false,
            taskGroupId: "task-b",
          },
        ];
      return [];
    });

    renderPage();

    expect(await screen.findByText("已配置账号")).toBeTruthy();
    expect(screen.queryByText("商品覆盖模式")).toBeNull();
    expect(screen.queryByText("已保存回复任务")).toBeNull();
    expect(screen.queryByText("已保存任务 · 2 条内容")).toBeNull();
    expect(screen.getAllByText("账号一").length).toBeGreaterThan(0);
    expect(screen.getAllByText("账号二").length).toBeGreaterThan(0);
    expect(
      screen.getAllByRole("button", { name: "新增默认回复模板" }),
    ).toHaveLength(1);
    expect(
      screen.getAllByRole("button", { name: "新增商品回复模板" }),
    ).toHaveLength(1);
  });

  it("applies saved reply templates to selected accounts and switches them to default reply mode", async () => {
    accountStoreMock.accounts = [
      { uid: "uid-1", nick: "账号一", status: "active", autoReply: false },
      { uid: "uid-2", nick: "账号二", status: "active", autoReply: false },
    ];
    loadStoredAccountSelectionMock.mockReturnValue([]);
    const accountRepliesByUid = {
      "uid-1": [],
      "uid-2": [],
    };
    let createdId = 900;
    apiMocks.getItemReplies.mockImplementation(async (cookieId) => {
      if (cookieId === "__global__") {
        return [
          {
            id: 301,
            cookieId,
            itemId: "",
            reply: "默认自动回复",
            type: "text",
            enabled: true,
          },
        ];
      }
      return accountRepliesByUid[cookieId] || [];
    });
    apiMocks.createItemReply.mockImplementation(async (cookieId, payload) => {
      const record = {
        id: createdId++,
        cookieId,
        ...payload,
      };
      if (cookieId !== "__global__") {
        accountRepliesByUid[cookieId] = [record];
      }
      return record;
    });

    renderPage();

    await screen.findByText("默认自动回复");
    accountStoreMock.loadAccounts.mockClear();

    const templateCheckbox = screen.getAllByRole("checkbox")[0];
    fireEvent.click(templateCheckbox);
    fireEvent.click(
      screen.getByRole("checkbox", { name: "选择待配置账号 账号一" }),
    );
    fireEvent.click(screen.getByRole("button", { name: "保存配置" }));

    await waitFor(() => {
      expect(apiMocks.createItemReply).toHaveBeenCalledWith("uid-1", {
        itemId: "",
        reply: "默认自动回复",
        type: "text",
        enabled: true,
        autoRecall: false,
        taskGroupId: expect.any(String),
        replyCount: 1,
      });
      expect(apiMocks.updateAccountSetting).toHaveBeenCalledWith("uid-1", {
        replyMode: "default",
      });
      expect(apiMocks.updateAccountSetting).toHaveBeenCalledTimes(1);
      expect(saveStoredAccountSelectionMock).toHaveBeenCalledWith(
        "item-replies",
        ["uid-1"],
      );
      expect(accountStoreMock.loadAccounts).toHaveBeenCalled();
    });

    const configured = screen.getByText("已配置账号").closest("section");
    expect(await within(configured).findByText("账号一")).toBeTruthy();
    expect(saveStoredAccountSelectionMock).toHaveBeenLastCalledWith(
      "item-replies",
      ["uid-1"],
    );
  });

  it("shows reply-count reminder when both text and image templates are selected", async () => {
    accountStoreMock.accounts = [
      { uid: "uid-1", nick: "账号一", status: "active", autoReply: false },
    ];
    loadStoredAccountSelectionMock.mockReturnValue(["uid-1"]);
    apiMocks.getItemReplies.mockImplementation(async (cookieId) => {
      if (cookieId === "__global__") {
        return [
          {
            id: 301,
            cookieId,
            itemId: "",
            reply: "默认自动回复",
            type: "text",
            enabled: true,
          },
          {
            id: 302,
            cookieId,
            itemId: "",
            reply: "https://cdn.example.com/reply.png",
            type: "image",
            enabled: true,
          },
        ];
      }
      return [];
    });

    renderPage();

    await screen.findByText("默认自动回复");
    const templateCheckboxes = screen.getAllByRole("checkbox");
    fireEvent.click(templateCheckboxes[0]);
    fireEvent.click(templateCheckboxes[1]);

    expect(
      screen.getByText(
        '当前已勾选 1 条文字和 1 张图片；每次触发会随机发送 1 段文字 + 1 张图片，回复次数按整条任务累计。',
      ),
    ).toBeTruthy();
  });

});
