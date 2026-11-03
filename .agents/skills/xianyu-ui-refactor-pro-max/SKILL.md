---
name: xianyu-ui-refactor-pro-max
description: Use when planning, auditing, or implementing the frontend-v2 UI/UX refactor for xianyu-main-system, especially navigation, responsive shell, module-by-module redesign, and document-driven agent workflows.
---

# Xianyu UI Refactor Pro Max

Use this skill for any `xianyu-main-system` UI/UX refactor work in `frontend-v2`.

This skill is document-first. It assumes future sessions may have zero useful chat history.

## Read First

Always read these files before acting:

1. `AGENTS.md`
2. `docs/ui-refactor/GOAL_CONTRACT.md`
3. `docs/ui-refactor/00-master-plan.md`
4. `docs/ui-refactor/00-baseline-check.md`
5. `docs/ui-refactor/01-current-ui-map.md`
6. `docs/ui-refactor/02-target-navigation.md`
7. `docs/ui-refactor/03-route-menu-mapping.md`
8. `docs/ui-refactor/04-refactor-rules.md`
9. `docs/ui-refactor/05-refactor-progress.md`
10. `docs/ui-refactor/08-risk-register.md`
11. `docs/ui-refactor/09-visual-direction.md`
12. `docs/ui-refactor/10-mobile-navigation-strategy.md`

Then inspect these real frontend files:

- `frontend-v2/src/navigation.ts`
- `frontend-v2/src/router.tsx`
- `frontend-v2/src/layouts/AppShell.tsx`
- `frontend-v2/src/layouts/DesktopLayout.tsx`
- `frontend-v2/src/layouts/MobileLayout.tsx`
- `frontend-v2/src/components/HubTabs.tsx`
- `frontend-v2/src/components/CommandPalette.tsx`
- `frontend-v2/src/styles/main.css`

## Workflow

### Planning and audit mode

Use the read-only agents:

- `codebase_cartographer`
- `navigation_architect`
- `route_mapper`
- `uiux_designer`
- `mobile_responsive_auditor`
- `business_risk_auditor`

The controller reconciles their output into the docs.

### Implementation mode

Only `ui_implementation_worker` may edit production UI code.

Before implementation:

- Select exactly one active module in `docs/ui-refactor/05-refactor-progress.md`.
- Confirm the route family exists in `docs/ui-refactor/03-route-menu-mapping.md`.
- Confirm mobile access path exists in `docs/ui-refactor/10-mobile-navigation-strategy.md`.
- Confirm guardrails in `docs/ui-refactor/04-refactor-rules.md`.
- Confirm open risks in `docs/ui-refactor/08-risk-register.md`.

After implementation:

- Run validation commands.
- Handoff to `ui_reviewer`.
- Update progress, handoff, and risk docs.

## Hard Rules

- No chat-memory dependency.
- No hallucinated business features.
- No business-logic changes unless explicitly required.
- No API behavior changes.
- No permission or route-guard changes unless explicitly required.
- No mirroring of full desktop nav into mobile tabs.
- Mobile bottom tabs stay between 4 and 5 items.
- Every desktop first-level feature needs a mobile access path.
- Admin-only features remain permission-controlled.
- One module at a time.

## Product Direction

- Build a modern Xianyu seller operation console.
- Keep the UI work-focused and dense enough for operations work.
- Do not make it feel like a generic admin template.
- Use warm Xianyu yellow only as an accent.
- Prefer practical tables on desktop and structured cards or sheets on mobile.

## Validation

For production UI changes, run:

```bash
cd frontend-v2 && npm run lint
cd frontend-v2 && npm run build
cd frontend-v2 && npm run test
```

For docs-only runs, verify that production UI code stayed untouched:

```bash
git diff --name-only
```

## Stop Conditions

Stop and log a blocker when:

- the task would change API behavior, auth, permissions, or route guards without explicit approval
- the active module is unclear
- the mobile access path is not defined
- the change would span multiple modules

