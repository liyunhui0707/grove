# Grove — contributor and coding-agent instructions

## Read before implementation

Read README.md, docs/current-status.md, docs/product-spec.md, docs/decisions.md, and docs/spike-plan.md. Historical evaluations and private stage notes are preserved in the ignored local archive; current decisions take precedence.

The owner accepted the overall recommendation and authorized a new independent project and phased development on 2026-11-08. The earlier planning-only restriction is historical; implementation of the first Spike is now authorized. Do not ask again whether to create the project or start the accepted Spike.

Keep work inside this project. Do not modify neighboring repositories, the earlier planning folder, or the separate Android hotkey project.

## Product priorities

Beauty, simplicity, efficient keyboard review, and original-media safety come first. The product is a standalone local-first app, not a generic file manager, web mock-up, or phone-transfer tool.

One app, one brand, one repository; use clear internal modules. Photos first. Do not build a plugin platform, public SDK, cloud sync, AI features, RAW editor, video editor, or separate video app during the Spike.

Use small reviewable steps. Do not spawn subagents unless the owner explicitly requests delegation.

## Data safety

- Never read .env or .env.* except relevant clearly named .env.example, .env.sample, or .env.template files. Never print credential-like values.
- Never read secrets/**.
- Use generated or appropriately licensed test media outside the repository for interactive testing. Do not inspect personal photo directories without explicit current-task authorization.
- Do not modify, rename, move, overwrite, or delete real family media without explicit authorization of that operation.
- The Spike must not modify source media. Portable metadata tests may write only to designated test copies.
- On 2026-11-10 the owner explicitly authorized 3B development and QA on designated generated test copies, including confirmed same-volume moves, staging and withdrawal. This qualifies the preceding Spike rule for that test scope only; it does not authorize reading or changing real family media.
- Do not store photos, videos, databases, thumbnail caches, operation logs, or personal paths in source control.
- Do not use curl or wget without approval of the specific current-task destination/source and purpose.
- Do not use rm without approval of the exact deletion targets and side effects. Use reviewable patches for project-file removals; never create a persistent broad allow rule.
- Credential audits inspect names, storage, scopes, and permissions, not credential values.
- Do not create persistent broad allow rules for interpreters, shells, package managers, remote tools, container tools, cloud CLIs, or arbitrary runners.
- Remote-host work requires the remote-host-operations skill and explicit authorization. No remote-host work is authorized by the project bootstrap.
- Git pushes, GitHub mutations, deployments, Docker daemon actions, SSH/SCP/rsync, cloud operations, and production changes require explicit current-task authorization.

## Scoring and persistence

Use a nullable 0–5 rating: null means unreviewed; 0 means reject candidate. Do not collapse null and 0.

SQLite is the operational catalog. Portable XMP maps unreviewed to missing/0, app rating 0 to standard -1, and 1–5 unchanged. A reject candidate never triggers deletion.

Preserve unknown XMP fields, detect conflicts, persist pending writes, and surface sync failures. Never write ratings into original media during the Spike.

Stable file identity must not depend only on absolute paths or content hashes. Identical copies are distinct file occurrences.

## UI and performance

Use a cohesive custom design system, light/dark themes, subtle motion, visible keyboard focus, and progressive disclosure. Photos stay visually central.

Virtualize grids. Use bounded decode concurrency, cancellation/stale-result rejection, disk thumbnail cache, screen-sized previews, and explicit memory budgets. Do not load eight full-resolution originals by default.

Rating shortcuts must not act in text inputs. Undo must restore the correct file, value, focus, and review position. Comparison ratings target only the active pane.

## Engineering and evidence

On 2026-11-14 the owner explicitly moved all subsequent app acceptance and experiments to this local Mac. This supersedes the 2026-11-08 Air-default testing preference and the older control-node runtime restriction. Use existing local project tools and isolated generated or appropriately licensed test data; do not silently install tools or change global/system/privacy settings. Air is no longer the default and requires a new explicit remote task authorization. Historical Air results remain evidence only for their recorded host and candidate; they do not establish local acceptance.

The owner's question about personally using their photo library is not authorization for the agent to inspect that library or modify real media. Existing data-safety boundaries remain in force. Do not start another Codex session or agent, or infer publication authorization from the testing-host change.

Use Tauri 2 + React + TypeScript + Vite + Rust + SQLite as the accepted starting stack. Verify current dependency documentation and HEIC alternatives before committing to decoder behavior.

Reuse mature libraries; do not copy GPL application code into this Apache-2.0 project without an explicit licensing decision. Maintain provenance for dependencies, fonts, datasets, and test assets.

Write meaningful tests for ratings, persistence, XMP mapping, cancellation/races, and future file-operation recovery. Browser-only tests do not prove native permissions or cross-platform compatibility.

Mark planned, implemented, and tested capabilities separately. Record commands and real results in docs/current-status.md. Do not claim benchmarks, manual QA, Windows/Linux support, or installer acceptance without evidence.

The owner explicitly approved 0.1.0 as the initial Spike version on 2026-11-08. No further release/version policy is established: do not invent a release scheme, bump versions, tag releases, or publish without the appropriate authorization.

## Publication and continuity

The project already has an approved public repository and technical prereleases; current release identity is recorded in docs/current-status.md. Work from this repository root on the existing public history. The former `.local/public-release-0.1.0` copy is a historical reference, not a second development workspace. Review privacy, notices, source identity, and installers before an authorized upload; an existing remote does not authorize a push or publication.

.local/ contains private conversation history and local launch metadata. Keep it ignored and never force-add it. Read only the relevant planning archive if necessary; never copy its personal details into public docs or product code.

Maintain docs/current-status.md after each meaningful milestone. Decisions override the historical evaluation only when explicitly recorded with their reason and evidence.


## Git management

The owner authorized local Git consolidation and routine development-stage commits on 2026-11-16. Follow docs/git-workflow.md. Start each task by checking status, branch, upstream, and recent commits. Preserve unrelated work and use a feature branch from the appropriate baseline. Commit a coherent completed change with its relevant tests and concise evidence before calling a stage complete; do not leave product work accumulating untracked. This standing local-commit authorization does not authorize pushes, PR mutations, tags, version changes, releases, or deployments.

Use explicit file staging and inspect the staged diff. Keep `.local/`, private history, media, catalogs, artifacts, and logs ignored; never force-add them. Do not copy the product source into a release workspace. Keep website source as ordinary tracked files under `website/`; its existing Sites metadata may remain local, but never stage it as a Git submodule/gitlink. Record the source commit and dirty state alongside build IDs and existing source manifests for future candidates. Documentation-only commits do not change an already accepted binary's build identity.

Never rewrite published history or use destructive reset/clean/forced checkout to tidy a working tree. Published tags and assets are immutable. Synchronize refs read-only, distinguish local committed work from pushed work, and report both at handoff. Backups and status notes support Git history; they do not replace commits.
