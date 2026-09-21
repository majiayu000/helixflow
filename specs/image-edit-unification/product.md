# Helixflow 图片编辑能力 · Product Spec

## 决策

Helixflow 自己提供扩图、擦除、抠图、超分和增强，不再要求用户另外启动
`media-processor`。实际生成仍由工作区选择的模型 Provider 完成；“自包含”指任务协议、
图片预处理、状态、输出落盘和错误处理全部属于 Helixflow，而不是脱离外部模型服务。

`media-processor` 原本承担四件事：

1. 把产品操作翻译成供应商模型请求。
2. 对扩图和擦除准备透明区域，对输出做尺寸和 alpha 校验。
3. 上传输入、等待远端任务、下载输出。
4. 独立保存队列状态并向 Helixflow 暴露 HTTP Job API。

前三项是 Helixflow 图片能力的必要组成，迁入 Helixflow；第四项会造成第二套任务真相和
第二个本地故障点，删除。独立服务只在多产品共享、需要独立扩缩容或资源隔离时才值得
保留，当前产品不满足这些条件。

## 2026-09-03 设计复核

当前实现是已通过生产 Provider 验收的 V1，不等于质量与架构已经最优。五种用户意图、
Helixflow 单一任务真相、保留源图并创建派生结果这些方向继续保留；以下问题必须和
“模型已经成功返回图片”分开判断：

1. 默认 prompt 只是安全基线，不能保证遮罩外像素绝对不变。
2. 擦除的遮罩外像素由确定性合成保护。扩图不能把完整原图硬贴回 Provider 结果，否则
   在两幅图的色彩或几何不一致时必然产生接缝；扩图保留源节点不变，派生结果使用
   Provider 的完整生成图。
3. 供应商尺寸参数必须保持编辑画布比例，不能映射到少数会改变构图的固定尺寸桶。
4. HTTP 请求、浏览器生命周期和 Provider 任务生命周期必须解耦；服务端拥有终态。
5. 默认模型只证明了可用，未经过质量、耗时和费用的系统对比，不能宣称最优。
6. 工具栏与属性面板必须暴露同一组五种能力。

默认 prompt 由两部分组成：Helixflow 固定的操作约束，以及可选的用户创作要求。固定约束
不能被用户文本覆盖，但 prompt 仍只是语义引导，不承担像素级不变量。

```text
outpaint constraint:
Extend the image into transparent canvas regions only. Treat opaque pixels as the
locked visual reference. Continue boundary lines, texture, perspective, lighting,
depth of field and grain naturally. Do not introduce a new focal subject unless requested.

inpaint constraint:
Replace only the transparent masked region. Reconstruct it from surrounding visual
context. Keep geometry, lighting, texture and perspective continuous. Preserve all
content outside the mask.
```

用户输入单独作为 `User request: ...` 追加。是否调整现有默认文案，应和各编辑模型的固定
测试集 A/B 一起决定，不以“文案更长”作为优化标准。

## 第一性原则

系统只保留三种产品对象：

1. **图片资产**：源图和派生结果。
2. **编辑意图**：用户提交的边界、蒙版、提示词、模型和质量。
3. **图片任务**：Helixflow 保存的执行状态、Provider、模型、输出和错误。

供应商模型路径不是节点类型，内部任务协议也不是用户术语。快捷编辑保留源图，并在右侧
创建结果图片；显式可复用工作流才显示 Operator 节点。

## 能力模型

| 用户能力 | Intent | 必要输入 | 默认模型族 | 输出约束 |
| --- | --- | --- | --- | --- |
| 扩图 | `outpaint` | 源图、四边、prompt、profile | edit | 新画布包含原图和补全部分 |
| 擦除 | `inpaint` | 带透明蒙版的源图、prompt、profile | edit | 透明区域被补全 |
| 抠图 | `cutout` | 源图 | remove-background | 同尺寸且同时存在透明背景与前景 |
| 超分 | `upscale` | 源图、2×/4× | upscaler | 尺寸严格按倍数增加 |
| 增强 | `enhance` | 源图 | photo-cleanup | 同尺寸画质修复 |

`image.edit` 继续表示显式工作流中的自由形式参考图编辑，不作为五个快捷操作的万能协议。

## 体验

1. 选中图片后，从工具栏进入扩图框、擦除蒙版或一键能力。
2. 模型列表来自 Helixflow 当前工作区的图片能力端点。
3. 仅支持尺寸/质量的 profile 显示对应选项。
4. 确认后由 Helixflow 创建 durable 图片任务并调用工作区 Provider。
5. 创建请求立即返回任务；前端读取任务状态，不持有长时间 Provider HTTP 请求。
6. 成功输出由 Helixflow 落到 workspace upload 并写入 succeeded，前端只负责把结果资产
   放到源图右侧并关联 result node。
7. 任一阶段失败都保存明确错误，不换模型、不调用旧服务、不静默降级。

## 状态与溯源

```text
queued -> running -> succeeded
                  -> failed
                  -> interrupted
```

任务保存 source/result node、intent、profile、Provider、模型、远端 Provider task ID、输出
upload、错误和时间。Helixflow 是唯一状态真相；浏览器和外部本地服务都不能写 running、
succeeded、failed 或 interrupted，只能在 succeeded 后关联画布 result node。

## 非目标

- 不修改独立的 media-processor 仓库。
- 不保留 `/media-processor-api` 兼容代理或双路执行。
- 不把本次工作扩展成通用视频/音频处理平台。
- 不在本次迁移 `input.image` 为 `image.asset`。
- 不根据模型名伪造价格；Provider 无可靠金额时明确显示费用未知。
- 本轮不增加图片任务重启恢复、远端取消或新的费用估算配置面；这些需要持久化原始输入、
  参数和 Provider 恢复契约后单独实现。

## 验收标准

- [x] 只启动 Helixflow 前后端即可读取五种能力并提交任务。
- [x] 代码和运行时不再请求 `127.0.0.1:5578` 或 `/media-processor-api`。
- [x] 五种操作均在 Helixflow 后端完成输入准备、Provider 调用、输出校验和落盘。
- [x] 前端不再上传处理结果，结果 upload 由后端返回。
- [x] Provider 未配置、profile 不支持、参数无效和输出无效均明确失败。
- [x] 源图不变，成功结果仍在右侧创建图片卡。
- [x] workspace state 可恢复最近图片任务和真实 Provider/model provenance。
- [x] focused tests、Rust check/tests、Web build 与 `git diff --check` 通过。
- [x] 服务端异步执行并独立写入图片任务终态，浏览器只关联 result node。
- [x] 擦除输出确定性恢复遮罩外像素；扩图不使用会产生接缝的硬回贴。
- [x] 编辑模型使用保持画布比例的供应商原生尺寸参数。
- [x] 工具栏和属性面板均覆盖五种能力。
