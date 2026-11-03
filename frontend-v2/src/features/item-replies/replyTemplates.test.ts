import { describe, expect, it } from "vitest";

import {
  buildAccountItemReplyPayloads,
  groupAccountReplyTasks,
  resolveReplyTargetUids,
  selectSavedReplyTemplates,
} from "./replyTemplates";

describe("item reply templates", () => {
  it("selects enabled saved reply templates by id and ignores disabled ones", () => {
    const replies = [
      { id: 1, itemId: "", reply: "文字回复", type: "text", enabled: true },
      {
        id: 2,
        itemId: "",
        reply: "https://example.com/a.png",
        type: "image",
        enabled: true,
      },
      { id: 3, itemId: "", reply: "关闭的回复", type: "text", enabled: false },
      {
        id: 4,
        itemId: "item-1",
        reply: "商品专用",
        type: "text",
        enabled: true,
      },
    ];

    expect(
      selectSavedReplyTemplates(replies, [1, 2, 3, 4]).map((reply) => reply.id),
    ).toEqual([1, 2, 4]);
  });

  it("builds account item reply payloads from selected saved templates", () => {
    expect(
      buildAccountItemReplyPayloads(
        [
          {
            id: 1,
            itemId: "",
            reply: "文字回复",
            type: "text",
            enabled: true,
            autoRecall: true,
          },
          {
            id: 2,
            itemId: "item-1",
            reply: "https://example.com/a.png",
            type: "image",
            enabled: false,
            autoRecall: false,
          },
        ],
        {
          autoRecall: true,
          taskGroupId: "task-1",
          replyCount: 3,
        },
      ),
    ).toEqual([
      {
        itemId: "",
        reply: "文字回复",
        type: "text",
        enabled: true,
        autoRecall: true,
        taskGroupId: "task-1",
        replyCount: 3,
      },
      {
        itemId: "item-1",
        reply: "https://example.com/a.png",
        type: "image",
        enabled: false,
        autoRecall: true,
        taskGroupId: "task-1",
        replyCount: 3,
      },
    ]);
  });

  it("uses the selected account list as the only reply write target", () => {
    expect(resolveReplyTargetUids(["seller-a", "seller-b"])).toEqual([
      "seller-a",
      "seller-b",
    ]);
    expect(
      resolveReplyTargetUids(["seller-a", "seller-a", "", " seller-b "]),
    ).toEqual(["seller-a", "seller-b"]);
  });

  it("groups replies created in the same applied task into one saved task", () => {
    expect(
      groupAccountReplyTasks([
        {
          id: 11,
          cookieId: "seller-a",
          itemId: "",
          reply: "统一默认回复",
          type: "text",
          enabled: true,
          autoRecall: true,
          taskGroupId: "task-1",
          replyCount: 2,
          createdAt: 30,
        },
        {
          id: 12,
          cookieId: "seller-a",
          itemId: "item-1",
          reply: "商品专用",
          type: "text",
          enabled: true,
          autoRecall: true,
          taskGroupId: "task-1",
          replyCount: 2,
          createdAt: 30,
        },
        {
          id: 13,
          cookieId: "seller-b",
          itemId: "",
          reply: "统一默认回复",
          type: "text",
          enabled: true,
          autoRecall: true,
          taskGroupId: "task-1",
          replyCount: 2,
          createdAt: 30,
        },
        {
          id: 14,
          cookieId: "seller-b",
          itemId: "item-1",
          reply: "商品专用",
          type: "text",
          enabled: true,
          autoRecall: true,
          taskGroupId: "task-1",
          replyCount: 2,
          createdAt: 30,
        },
        {
          id: 15,
          cookieId: "seller-c",
          itemId: "",
          reply: "另一组默认回复",
          type: "text",
          enabled: true,
          autoRecall: false,
          taskGroupId: "task-2",
          replyCount: 1,
          createdAt: 20,
        },
      ]),
    ).toEqual([
      {
        key: "task-1",
        taskGroupId: "task-1",
        enabled: true,
        autoRecall: true,
        partialAutoRecall: false,
        replyCount: 2,
        partialReplyCount: false,
        ruleIds: [11, 12, 13, 14],
        accountUids: ["seller-a", "seller-b"],
        entries: [
          {
            key: "\u0001统一默认回复\u0001text\u00011\u00011",
            itemId: "",
            reply: "统一默认回复",
            type: "text",
            enabled: true,
            autoRecall: true,
            ruleIds: [11, 13],
            accountUids: ["seller-a", "seller-b"],
            records: [
              {
                id: 11,
                cookieId: "seller-a",
                itemId: "",
                reply: "统一默认回复",
                type: "text",
                enabled: true,
                autoRecall: true,
                taskGroupId: "task-1",
                replyCount: 2,
                createdAt: 30,
              },
              {
                id: 13,
                cookieId: "seller-b",
                itemId: "",
                reply: "统一默认回复",
                type: "text",
                enabled: true,
                autoRecall: true,
                taskGroupId: "task-1",
                replyCount: 2,
                createdAt: 30,
              },
            ],
          },
          {
            key: "item-1\u0001商品专用\u0001text\u00011\u00011",
            itemId: "item-1",
            reply: "商品专用",
            type: "text",
            enabled: true,
            autoRecall: true,
            ruleIds: [12, 14],
            accountUids: ["seller-a", "seller-b"],
            records: [
              {
                id: 12,
                cookieId: "seller-a",
                itemId: "item-1",
                reply: "商品专用",
                type: "text",
                enabled: true,
                autoRecall: true,
                taskGroupId: "task-1",
                replyCount: 2,
                createdAt: 30,
              },
              {
                id: 14,
                cookieId: "seller-b",
                itemId: "item-1",
                reply: "商品专用",
                type: "text",
                enabled: true,
                autoRecall: true,
                taskGroupId: "task-1",
                replyCount: 2,
                createdAt: 30,
              },
            ],
          },
        ],
        records: [
          {
            id: 11,
            cookieId: "seller-a",
            itemId: "",
            reply: "统一默认回复",
            type: "text",
            enabled: true,
            autoRecall: true,
            taskGroupId: "task-1",
            replyCount: 2,
            createdAt: 30,
          },
          {
            id: 12,
            cookieId: "seller-a",
            itemId: "item-1",
            reply: "商品专用",
            type: "text",
            enabled: true,
            autoRecall: true,
            taskGroupId: "task-1",
            replyCount: 2,
            createdAt: 30,
          },
          {
            id: 13,
            cookieId: "seller-b",
            itemId: "",
            reply: "统一默认回复",
            type: "text",
            enabled: true,
            autoRecall: true,
            taskGroupId: "task-1",
            replyCount: 2,
            createdAt: 30,
          },
          {
            id: 14,
            cookieId: "seller-b",
            itemId: "item-1",
            reply: "商品专用",
            type: "text",
            enabled: true,
            autoRecall: true,
            taskGroupId: "task-1",
            replyCount: 2,
            createdAt: 30,
          },
        ],
      },
      {
        key: "task-2",
        taskGroupId: "task-2",
            enabled: true,
            autoRecall: false,
            partialAutoRecall: false,
            replyCount: 1,
            partialReplyCount: false,
            ruleIds: [15],
        accountUids: ["seller-c"],
        entries: [
          {
            key: "\u0001另一组默认回复\u0001text\u00011\u00010",
            itemId: "",
            reply: "另一组默认回复",
            type: "text",
            enabled: true,
            autoRecall: false,
            ruleIds: [15],
            accountUids: ["seller-c"],
            records: [
              {
                id: 15,
                cookieId: "seller-c",
                itemId: "",
                reply: "另一组默认回复",
                type: "text",
                enabled: true,
                autoRecall: false,
                taskGroupId: "task-2",
                replyCount: 1,
                createdAt: 20,
              },
            ],
          },
        ],
        records: [
          {
            id: 15,
            cookieId: "seller-c",
            itemId: "",
            reply: "另一组默认回复",
            type: "text",
            enabled: true,
            autoRecall: false,
            taskGroupId: "task-2",
            replyCount: 1,
            createdAt: 20,
          },
        ],
      },
    ]);
  });

  it("separates legacy tasks with different reply counts", () => {
    const tasks = groupAccountReplyTasks([
      {
        id: 1,
        cookieId: "seller-a",
        itemId: "",
        reply: "同一内容",
        type: "text",
        enabled: true,
        autoRecall: false,
        replyCount: 1,
      },
      {
        id: 2,
        cookieId: "seller-a",
        itemId: "",
        reply: "同一内容",
        type: "text",
        enabled: true,
        autoRecall: false,
        replyCount: 3,
      },
    ]);

    expect(tasks).toHaveLength(2);
    expect(tasks.map((task) => task.replyCount)).toEqual([1, 3]);
  });
});
