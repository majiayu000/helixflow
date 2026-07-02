# Tech Spec

## Linked Issue

GH-66

## Product Spec

`specs/GH66/product.md`

## Codebase Context

| Area | Files | Current behavior | Why relevant |
| --- | --- | --- | --- |
| Canvas | `web/src/components/graph-canvas.tsx` | 有选择/拖拽/复制文本基础 | 添加删除粘贴入口 |
| Catalog | `web/src/api.ts`, node catalog types | registry catalog 已暴露节点定义 | 节点库数据源 |
| Store/API | `web/src/store.ts`, GH-59 endpoint | 批量 ops 可直接成版 | add/delete/paste 都走批量 op |
| Tests | `web/src/app.test.tsx` | 覆盖基础画布交互 | 需要新增节点库/粘贴测试 |

## 设计方案

### 1. Node library panel

新增节点库面板组件,从现有 catalog state 读取节点定义,按 category/group 渲染并支持搜索。点击/双击或拖到画布构造 `add_node` op。

### 2. ID and placement

前端生成临时唯一 node id,基于 node type slug + counter/uuid。drop point 转换为 graph coordinate;双击添加到 viewport center。

### 3. Delete selection

Delete 键读取 selection,构造删除选中 nodes 和关联 edges 的批量 ops。后端仍负责最终 graph validation。成功后刷新 state 并清空/更新 selection。

### 4. Copy/paste

复制选区写入 app-specific JSON 到 clipboard,包含 nodes、params、positions、内部 edges、source ids。粘贴时重映射 ids,offset 位置,构造 `[add_node..., add_edge...]` 批量 op。外部/无效 JSON 提示错误,不调用 API。

### 5. Error handling

400/409 显示画布级错误,保留 selection。catalog 加载失败时节点库展示 empty/error state。

## Product-to-Test Mapping

| Product invariant | Implementation area | Verification |
| --- | --- | --- |
| P1 catalog panel | node library component | search/category tests |
| P2 add_node unique id | add node helper/API | add node test |
| P3 delete batch | selection/delete ops | delete node with edges test |
| P4 paste remap | clipboard helper | 2 node 1 edge paste test |
| P5 invalid clipboard no write | paste validation | no API call test |
| P6 undo/restore | state refresh/version history | existing version route smoke |

## 数据流

catalog -> node library -> add/delete/copy/paste helpers -> `/versions/ops` batch -> workspace state refresh -> canvas rerender.

## 备选方案

- 继续使用手动 proposal 表单:无法满足画布闭环,放弃。
- 直接复用系统剪贴板纯文本:无法安全保留内部边/id remap,放弃。

## 风险

- Security: clipboard JSON 解析需防御异常,不使用 `eval`/`innerHTML`。
- Compatibility: 不破坏现有选择快捷键。
- Performance: catalog 搜索需对大 catalog 保持响应。
- Maintenance: id remap/helper 需测试覆盖。

## 测试计划

- [ ] Unit tests: id generation、delete op builder、clipboard remap。
- [ ] Integration tests: add/delete/paste UI flows。
- [ ] Manual verification: drag add、keyboard delete、copy/paste、undo。

## 回滚方案

隐藏节点库与 paste/delete handlers,保留已有 canvas render 和 proposal/API。

