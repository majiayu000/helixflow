# Helixflow 图片编辑能力 · Task Plan

## 已完成基线

- [x] 五种产品 intent 与编辑器交互统一。
- [x] 扩图框、擦除蒙版、profile/质量/尺寸控制已接入。
- [x] workspace-scoped durable 图片任务表和节点状态投影已建立。

## 当前迁回

| ID | 任务 | Done When |
| --- | --- | --- |
| IEU-S1 | 删除外部服务边界 | ✅ 无 `/media-processor-api`、5578、Vite proxy 或 processor client |
| IEU-S2 | Gateway 内建五种图片执行 | ✅ Atlas profile、预处理、submit/poll、下载和输出校验均有测试 |
| IEU-S3 | Server 接管任务与输出 | ✅ multipart 创建、后端状态更新、workspace upload、明确失败 |
| IEU-S4 | Web 改为 Helixflow 单端调用 | ✅ 能力列表和执行只请求 `/api/workspaces/...`；右侧结果保持不变 |
| IEU-S5 | 回归验证 | ✅ focused Rust/Web tests、build、diff check；未触发付费生成 |

## 设计复核优化（2026-09-03）

| ID | 任务 | Done When |
| --- | --- | --- |
| IEU-O1 | 服务端拥有终态 | ✅ POST 返回 202；后台执行；GET 可轮询；Web 只关联 result node |
| IEU-O2 | 确定性源图保护 | ✅ inpaint 回贴所有非透明像素；outpaint 保留源节点但不做接缝硬回贴 |
| IEU-O3 | 保持画布比例 | ✅ GPT 使用 16 对齐且满足像素预算的尺寸；Nano 传原生比例与分辨率参数 |
| IEU-O4 | 五项 UI 统一 | ✅ 工具栏与属性面板均可选择扩图、擦除、抠图、超分、增强 |
| IEU-O5 | 模型与费用评估 | 固定测试集比较质量、保真、耗时和费用后再调整默认模型；本轮不伪造估价 |

本轮实施 O1-O4。O5 需要新的真实付费对比测试，不从单次成功样本推导“最佳模型”。

## 生产优化验收记录（2026-09-03）

- production Atlas 固定为 `https://api.atlascloud.ai/v1`，隔离数据目录与端口运行当前代码；
  未使用 dev API。
- 创建 inpaint/outpaint 均在约 5ms 内返回 HTTP 202 queued；后台随后保存 running、真实
  Provider task ID 和服务端终态。
- inpaint 使用 `google/nano-banana-2/edit`，输出保持 308×307；遮罩外逐像素比较
  `outside_changed=0`，遮罩内 18,696 个像素变化，视觉上猫被移除且坐垫和窗景补全自然。
- GPT outpaint 首次 1024×624 被生产 Provider 明确拒绝为低于最小像素预算。尺寸算法改为
  保持比例、16 对齐并至少满足 1024×1024 像素预算后，1312×800 Provider 任务成功，
  Helixflow 输出为目标画布 500×307。
- 用同一生产 Provider 原始输出比较了硬回贴、单侧 feather、双侧 feather 与完整输出。
  硬回贴及 feather 都会在重建图与原图之间留下结构接缝，最终移除 outpaint 硬回贴；
  Provider 完整输出视觉连续性最好。
- 新验收输入、响应、Provider 原始输出、Helixflow 输出和视觉对比保存在
  `/tmp/helixflow-prod-opt.52lxVG/`。

## 验证记录

- `cargo test -p helixflow-gateway --locked`：通过，真实付费测试保持 ignored。
- `cargo test -p helixflow-store --locked`：通过。
- 图片路由 fake Atlas 端到端测试：通过，覆盖上传、生成、下载、workspace upload 与任务溯源。
- 图片工具前端 focused tests 与 `npm run build`：通过。
- repo 全量 Web tests 仍有一个与本功能无关的既有文案断言失败。
- repo 全量 Server tests 仍有既有 Provider/catalog 断言漂移，且两个 hanging-request 用例不结束；本功能 focused test 通过。

## 真实 Provider 验收记录（2026-09-02）

- 仅使用 production Atlas `https://api.atlascloud.ai/v1`，在隔离数据目录和独立端口通过 Helixflow 正式图片任务 API 提交真实猫照片。
- 扩图：`openai/gpt-image-2/edit`，HTTP 200，输出 1024×1024，视觉上完成室内、窗边与花园延展。
- 擦除：`google/nano-banana-2/edit`，HTTP 200；追加明确猫主体蒙版后，猫被移除且毛毯、窗框和花园自然补全。
- 抠图：`youchuan/v8.2/remove-background`，HTTP 200，输出保持 308×307；alpha mask 仅保留猫主体，白底合成检查通过。
- 超分：`tencent/image/upscaler`，HTTP 200，输出严格从 308×307 增至 616×614。
- 增强：`atlascloud/photo-cleanup`，HTTP 200，输出保持 308×307。
- 六条实际任务（含第二次擦除语义验收）均保存真实 Provider task ID、workspace upload、result node，并终结为 `succeeded`。
- Chrome 实际页面验证通过：选中真实图片后五项工具均可点击；扩图显示可拖拽编辑框和模型/质量/尺寸/提示词，擦除显示笔刷/框选和生成面板。
- 生产验收输入、响应、输出、alpha mask 和隔离数据库保存在 `/tmp/helixflow-real-image-test.PIsjb4/`；未修改默认 Helixflow 数据目录。

## 实施边界

- 不修改独立 `media-processor` 仓库。
- 不增加兼容代理、双写或旧协议 fallback。
- 不新增通用媒体服务或第二套 Provider 配置。
- 不在本轮迁移 `input.image`。
