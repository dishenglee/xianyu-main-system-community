# Contributor Notes

## Product Boundary

- This public community edition is a local, single-user desktop application.
- Do not reintroduce product-account login, device activation, license gates, or per-user tenant isolation.
- The embedded API must bind only to a loopback address. Do not add a browser-based product flow or expose it as a shared network service.
- Xianyu account credentials are separate from product authentication. Keep account cookies local and never commit them.
- Do not include local databases, secrets, signing material, logs, or release artifacts in commits.

## Engineering

- Read the surrounding code and tests before changing behavior.
- Keep changes focused and follow existing Rust, TypeScript, and Tauri patterns.
- Add or update focused tests for behavior changes. Run the relevant Rust tests and `npm --prefix frontend-v2 run build` before reporting completion.
- Preserve unrelated user changes. Do not use destructive git commands.
- Use Chinese commit messages for repository commits.

## Frontend

- The supported product experience is the Tauri application only.
- Keep desktop and mobile navigation consistent with the existing route map.
- Avoid changing API behavior, data permissions, or product flows beyond the explicit task.

## Packaging

- Do not create or publish production release artifacts unless the user explicitly requests a release.
- Never package local user data or credentials.
- Keep GitHub automation manual unless explicitly approved for a specific workflow.
