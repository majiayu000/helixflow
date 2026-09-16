# Helixflow canvas design.md

This file is the brand contract for the workbench UI. Coding agents must follow it instead of inventing a new look.

It exists because agents kept producing Comfy-style node chrome, generic SaaS dashboards, and extra trays. The process matches [Vercel’s design.md loop](https://vercel.com/blog/how-our-agents-build-on-brand-pages-with-design-md): encode the last review corrections as observable rules, keep mechanics in the existing stylesheet, and do not add a second token set.

## Scope

- In: `web/src` workbench shell, canvas overlays, media cards, chat pane, top bar, left add rail.
- Out: Rust graph authority, SQLite versions, `upload://` media refs, Queue/cost confirmation, operator node types (`image.generate`, `llm.*`, `video.*` as workflow steps).
- Do not copy Vercel marketing pages, Inter/Roboto, purple gradients, or a light dashboard.

## Reader and task

The reader is on the canvas to make or edit a picture, video, or audio clip, then ask the agent about the selected nodes. The canvas is the job. Chrome must stay quieter than the nodes.

## Two node families

1. **Media cards** (`input.image`, `input.video`, `input.audio`, and the visual shell of `image.generate` / `image.edit`): the content *is* the node.
2. **Operator nodes** (prompt writers, video steps, save): keep ports and params. Do not restyle them into media cards.

## Observable decisions

Rewrite of the last review corrections. Each line is checkable in the DOM or a screenshot.

1. Empty canvas does not mount a centered “描述结果，工作流随后出现” agent card. A drop hint is allowed only when `nodeCount === 0`.
2. Left add rail and 添加节点 tray map 文本/图片/视频/音频 to one card each (`input.text` / `input.image` / `input.video` / `input.audio`). The tray must not list `image.edit`, `image.generate`, or other operator types. Clicking 图片 must not open an Edit vs Generate tray.
3. A media card has no title bar, no node id, no `SELECTED` badge, no `Input`/`image` port labels.
4. An empty media card is a dashed square with a centered media glyph and a 图片 / 视频 / 音频 caption. Clicking the card selects it and does not open a file picker. Upload is a separate 上传 control above the selected empty card.
5. A filled media card is the media, edge to edge, `object-fit: contain`, no overlay title. It resizes to the photo aspect (`fitMediaNodeSize`); it does not stay on the empty 280×280 square. Dropping a file onto an empty media card fills that card. Selected cards expose corner handles above the pixels so the card can be stretched.
6. Selecting an image card shows a prompt composer **below** the card, wider than the card, with the existing model picker (text-to-image models), an aspect select (1:1 / 16:9 / 9:16), and a round send control. It does not show the compact inspector pill (参数 / 对话 / 宫格). Generate still adds a new card to the right **and a lineage edge from the source** (`image` → `in` for empty/text-to-image, `image` → `image` for a filled source). Generating from an existing `image.generate` / `image.edit` card also spawns a new connected card; it does not rewrite the source in place. If the user picked a model, that generate node is pinned with catalog `set_semantics`; it does not invent a fake `model` param. Moving or resizing the card must not clear selection or unmount the toolbar/composer.
7. Media cards expose TapNow-style `+` handles on hover/select. Dragging `+` onto another card creates a reference edge into `in`. Same-type is preferred; mismatched types still land on `in`. Dropping on the card body is enough; the user does not have to hit a 10px port.
8. Image crop/split/replace tools appear only when the selected card already has image pixels.
9. Clicking a media-card `+` without dragging opens 添加节点 (文本/图片/视频/音频). Choosing a type adds the card beside the source and a same-type edge when ports match.
10. Double-clicking empty canvas opens the same add menu at the click.
11. Selected media toolbar includes 复制 / 下载 / 删除. Download is disabled when the card has no pixels.
12. 扩图 opens an 8-handle frame on the card. 擦除 opens a brush/rect overlay. 2× / 4× and 画质 create `image.edit` to the right. They do not replace the source card.
13. Double-click or right-click empty canvas opens 添加节点.
14. `input.text` is edited on the card after double-click. Empty cards show 双击开始编辑. The editor stores markdown in `text` and shows a format toolbar (H1 / H2 / H3 / paragraph / bold / italic / lists). It does not open the inspector pill stack.
15. ⌘Z undoes a pending layout move first, then the workspace version. The left rail 素材库 lists IndexedDB persistent assets plus current-graph uploads. 提示词 / 搜索 / 历史 / 模板 stay browse panels, not a second inspector.
16. Top bar height is 48px. Chat header is sentence case, not `CONVERSATION · CODEX` with tracking. Composer chips do not print raw node ids.
17. One primary accent (`--accent`) per view. Do not add a second brand color on the shell.
18. New UI uses tokens already declared on `:root` in `src/styles.css`. Do not introduce a parallel palette, a new display font, or ad-hoc hex colors in components.
19. Edit mode defaults to pan. Space or Ctrl temporarily swaps to box-select. The view dock can switch tools; a minimap is available.
20. Hover or single-select lights related nodes and edges. ⌘G groups the current selection spatially; ⌘⇧G ungroups. Groups are canvas chrome, not executable nodes.
21. Composer accepts `@` mentions and a 选参考 picker that connects another card as a reference. Image toolbar 存素材 writes into the persistent asset library. Model execution uses the catalog and backend run lifecycle.

## Named anti-patterns (never ship)

- **Comfy Shell**: title row + uppercase id + port names + SELECTED on a media card.
- **Status Marquee**: uppercase tracked labels such as `CONVERSATION · CODEX` or `idle · 观察中`.
- **Split Catalog**: 图片 opening `image.edit` vs `image.generate`.
- **Inspector Pill Stack**: 参数/修图/宫格/生成/对话 floating above a media card.
- **Inner Well**: a dashed rectangle inset inside a solid media card.
- **Token Drift**: a component that defines its own background/font instead of `--bg`, `--surface`, `--text`, `--accent`.

## Available primitives

Use these, documented here so agents do not read the whole CSS into context.

Tokens (`src/styles.css` `:root`): `--bg`, `--surface`, `--surface-2`, `--surface-3`, `--border`, `--border-2`, `--text`, `--text-2`, `--text-3`, `--accent`, `--accent-ink`, `--accent-soft`, `--font-sans`, `--font-mono`, `--r-sm`, `--r-md`, `--r-lg`.

Shell classes: `.wb`, `.wb-top`, `.wb-chat`, `.brand`, `.project-chip`, `.btn`, `.btn--primary`, `.ibtn`, `.node-library-bar`, `.node--media`, `.media-card--empty`, `.media-card-kind`, `.media-card-frame`, `.media-card-composer`, `.canvas-media-upload`, `.canvas-add-menu`, `.canvas-image-toolbar`, `.canvas-text-toolbar`, `.model-picker`.

## Composition

- Canvas fills the remaining viewport. Left rail and right chat overlay or sit beside it; they must not cover the selected card’s prompt bar.
- Media card default size is the square `MEDIA_CARD_WIDTH` × `MEDIA_CARD_HEIGHT` in `graph-canvas-navigation.ts` (280×280). Filled cards may still follow `fitMediaNodeSize`.
- Generate from an empty image card adds `image.generate` to the right (Helixflow graph) and connects `source.image → generate.in`. Do not pretend the empty card becomes the artifact in place.

## Human canvas loop

The canvas is the product. Agent later commands these same verbs. Do not add Agent-only graph APIs while this loop is incomplete.

| Verb | Status |
|------|--------|
| Add 文本/图片/视频/音频 card | Walkable. Tray and 添加节点 hide operators. |
| Upload / drop file | Walkable. Empty-card click does not upload. Drop on empty card fills it. |
| Card follows photo aspect | Walkable after measure fix. Layout reads `workflowGraph` size so persist does not snap back to 280×280. |
| Connect cards | Walkable. Drop on card body lands on `in`; mismatched types still connect. Derived cards emit `spawn_node`; the graph crate resolves the lineage edge. |
| Click `+` / double-click / right-click add | Walkable. |
| Prompt + aspect + generate to the right | Walkable. Result is a new card connected to the source, not in-place. |
| Model picker on generate | Walkable when the user picked a catalog model: `set_semantics` pins the binding. No pick → catalog default. |
| Crop / split / outpaint / erase / 2× 4× / enhance | Walkable. Tools write a new card to the right and a lineage edge from the source. |
| Duplicate / download / delete | Walkable. |
| Stretch card corners | Walkable. Handles sit above pixels; size persists on the workflow graph. |
| Undo | Walkable as version undo, not a canvas op-log. |
| Refresh still there | Walkable for size after graph payload includes `size`. |
| Library / search / history | Walkable. 素材库 includes persistent IndexedDB assets; 提示词库 inserts into the selected composer. |
| Templates | Leak. Stubs drop empty cards, not a real starter layout. |

Do not invest in Agent graph-building until the walkable verbs stay walkable after refresh without a second projection.

## Copy

- Chat header: `Chat`.
- Chat empty: one short line, not a feature list.
- Composer placeholder: `说说要做什么`.
- Selected context: `@ 1 个节点`, never `@选中 input_image_…`.
