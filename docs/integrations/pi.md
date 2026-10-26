# Pi 配置接入约定

最新核实：2026-10-22，当前托管安装为 Pi 1.4.0；本机平台为 macOS Apple Silicon。P02 的 1.1.0 与 1.3.0 历史证据保留在下文，配置写入适配以本节的现行规则为准。

## 1.4.0 复核（2026-10-22）

本机托管安装已升级至 1.4.0（2026-10-21 发布；1.3.0 发布目录仍保留）。按安装包、CHANGELOG 与源码复核：

- 版本门禁接受 1.4.0；`models.json` 与 `settings.json` 的结构和所用字段与 1.3.0 相同，`thinkingLevelMap` 的 `$defs` 引用未变。
- schema 补充了字段说明和显式默认值：`contextWindow` 128000、`maxTokens` 16384、`reasoning` false、`input` `["text"]`、`authHeader` false。这些默认值在 1.3.0 的 `provider-composer.js` 中已生效，不是 1.4.0 的新行为。
- `thinkingLevelMap` 的 partial/null 规则写入了 schema：`null` 排除该档，未映射的 `off` 到 `high` 仍可用，`xhigh`/`max` 只有显式字符串映射才可用。1.4.0 的 `getSupportedThinkingLevels` 与 1.3.0 相同，P24.c 的注入规则无需修改。
- 1.4.0 起 compaction 与分支摘要使用模型的 `maxTokens` 作为输出上限，并按上下文收缩；`reserveTokens` 只决定何时开始压缩。该变化不新增配置字段，但说明 `maxTokens` 会同时影响摘要请求。
- 新增 `docs/models-configuration.md` 作为 `models.json` 的逐字段参考。内置 provider 的目录更新（如 `MiniMax-M3.1-Flash`、Xiaomi 默认模型、xAI `xhigh`）不改变 vibemate 自定义 provider 条目。
- 本次未启动 Pi 会话、未发送 Provider 请求，也未在真实 Tauri 窗口重新验收；以上结论来自安装包、schema、CHANGELOG 与源码对照。

## P24.a 参数核实更新（2026-10-20）

该次核对时本机为 1.3.0；1.4.0 的复核见上节。已核对模型/settings schema、启动档位选择与 OpenAI 协议请求构建：
现有适配器写入声明的 `contextWindow/maxTokens/reasoning/input` 及每模型默认思考档位，并注入已核实的档位映射。
OpenAI sampling 对象在动态上下文限制之后合并，持久化输出键可能绕过该限制；因此它们不会作为本轮 Agent 注入选项。Messages 不支持这些 sampling 对象。
档位可选集与 partial map/null 规则已由本机 1.3.0 源码核实（见下文 P24.c 首批），1.4.0 的同一实现见上节；具体 Provider 的档位值域仍需补验，见 [P24.a 逐字段矩阵](model-parameter-mapping.md)。

## P24.c 首批：每模型思考档位（2026-10-20）

可选集首次以 1.3.0 安装包核对；1.4.0 的同一实现见上节，规则未变。`dist/bundle/chunks/bedrock-converse-stream.js` 中的
`getSupportedThinkingLevels` 与 `clampThinkingLevel` 原文抽出后在 Node 中运行，结果如下：

- `reasoning` 不为 `true` 时只接受 `off`。
- `reasoning: true` 时接受 `off/minimal/low/medium/high`；`thinkingLevelMap` 中某档为 `null` 则排除该档。
- `xhigh` 与 `max` 必须在 `thinkingLevelMap` 中有字符串映射才会被接受。
- 请求不被接受的档位会被 `clampThinkingLevel` 静默降到邻近档（例如 `max` 降为 `high`），所以 vibemate 拒绝保存，而不是依赖这一降级。

保存与启动顺序：`settings.json` 的 `modelThinkingLevels["<provider>/<modelId>"]`，值为
`off…max` 枚举（`settings.schema.json`）。`dist/core/sdk.js` 中新会话在 `defaultThinkingLevel`
之前读取该模型键；`agent-session.js` 切换模型时同样优先读取它。已恢复且带档位记录的会话
沿用会话自己的档位，因此不能保证运行中的会话立即使用新值。

vibemate 的写入规则：

- 中央模型必须有效声明推理支持（覆盖声明优先，否则用目录结果），并列出该档位。目录的 `none`
  对应 Pi 的 `off`，其余名称相同；`any`、未声明与空列表不展开。
- 基础可选集是已声明的 `off/minimal/low/medium/high`。官方 Chat 端点上的 DeepSeek
  `deepseek-flash`、`deepseek-v4-pro` 额外提供已声明的 `xhigh/max`：仅在该键不存在时补
  `xhigh→high` 或 `max→max` 映射。官方 OpenRouter Chat 则为任何明确列出相应档位的模型补
  `xhigh→xhigh/max→max`，由网关按该模型能力处理。所有已声明的扩展档位都会注入，与是否选择某个默认档位无关，
  因而不必先选中该档才能在 Pi 内切换。已有用户映射（包括 null）不覆盖，其他绑定不自动生成厂商映射；非官方地址、Responses/Messages
  不复用这两条 Chat 规则。
  DeepSeek 当前官方 Chat schema 明确 `xhigh` 兼容为 `high`，不由档位名自行推断。
- Rust 在合并后的 `models.json` 定义上再次核对：`reasoning` 必须是布尔；`thinkingLevelMap`
  可以缺失、为 `null` 或为对象，对象条目只能是字符串或 `null`。其它形状直接拒绝并返回既有的
  `target_conflict`。档位不被声明或 Pi 不接受时返回 `thinking_level_unavailable`。两种情况都不生成计划，
  用户自己写的映射也保持不变。
- 选择「不覆盖」时不改动 `modelThinkingLevels`。选定档位时只改该模型键，其它模型键与全局
  `defaultThinkingLevel` 保持不变；改名迁移沿用既有的 `modelThinkingLevels` 处理。
- 预览、详情与应用使用同一选择。选择随计划作为审批 token 返回；应用时用该 token 重新构建计划，
  并继续比较文件、Provider 与模型 token。
- 计划卡片的「Pi 模型设置」逐项列出中央值、来源与本次计划的处理。上下文窗口与最大输出来自
  模型库，库中不记录它是目录还是人工填写，分别写入 `contextWindow` 与 `maxTokens`；`maxTokens`
  在界面中说明为模型上限，不是每次请求的输出限制。推理与图片输入来自用户声明（覆盖目录）或目录证据，
  分别写入 `reasoning` 与 `input`。思考档位来自本次选择，写入 `modelThinkingLevels`。
- 未声明的项不写入。文件已有值时保持不变并显示该值，否则说明沿用 Pi 默认值。每一行都由写入同一分支的
  Rust 代码生成，界面只翻译，不重算。

边界：

- 证明的是 Pi 的配置语义与启动读取顺序，不证明某个 Provider 对该档位的实际请求值。
  DeepSeek Chat 官方 schema 允许 `minimal→low`、`medium/xhigh→high`，并接受 `low/high/max`；
  Pi 的基础档位透传依赖这些兼容规则。OpenRouter Chat 使用网关努力值，不等同于直连上游值。
  Command Code 与其它协议的厂商范围仍需按模型核实。
- 没有「清除」动作，只能选择「不覆盖」保留原值或选择一个档位。删除中央模型或服务商不会重写该键：
  `modelThinkingLevels` 是 Pi 的启动偏好，不是仍存活的绑定；P35 的删除约束只阻止仍被 Pi 模型条目或
  Grok alias 使用的中央记录。对应 Provider/model 被删除后，残留键不会再被 Pi 采用，也不会影响其它模型；
  需要清理时可在 Pi 的 `settings.json` 中修改，或从配置历史恢复一个更早的 `settings.json`。vibemate 的中央删除本身
  只改 SQLite 与凭据库，不写任何 Agent 文件。
- Pi 模型定义没有工具调用字段，因此不写工具声明；这不代表模型不能调用工具。
- 不提供 temperature/top_p、请求输出或推理 token 数编辑器，保留用户原有请求参数。
  模型最大输出仍映射为 `maxTokens`。源码显示 sampling 输出键在动态上下文限制后合并，
  这不能作为另设输出能力字段的依据。
- 注入映射检查安装 schema 的 `thinkingLevelMap` 字段；不能仅靠版本号假定支持。
- 隔离原生会话的菜单、启动默认档和运行中选择已验证，见下节；真实请求是否使用该厂商值仍需维护者确认。

## P24.c 隔离原生菜单与切换验证

在已安装 Pi 1.4.0 中启动临时 PTY 会话。`HOME`、`PI_CODING_AGENT_DIR` 与 cwd 均为
临时目录；直接使用已安装的 Node 与 Pi CLI 入口，环境不继承 Provider 凭据。
开启 `--offline`，关闭遥测、扩展、MCP、Skill、提示模板、上下文文件、主题发现与工具，
并用 `--no-approve` 忽略项目资源。macOS sandbox 拒绝全部网络及真实 Pi/Agents/Claude
配置读取，只允许读取 Pi 1.4.0 安装目录及其父目录的路径元数据。

两个自定义 Provider 均使用 API ID `deepseek-flash`、`openai-completions`、合成密钥与
`http://127.0.0.1:9/v1`。模型声明 `reasoning: true`，窗口 32768、最大输出 8192，
兼容格式为 `deepseek`。A 的 map 为 `xhigh→high/max→max`，B 的对应键均为 `null`。
`settings.json` 保存 A 的启动默认档 `xhigh`、B 的 `medium`，全局默认为 `low`。
该地址仅供无网络原生配置检查，不是适配器新增的厂商映射范围。

11 项断言通过：

- 启动采用 A 的 `xhigh`，优先于全局 `low`；菜单显示活动 `xhigh` 和可选 `max`。
- `/thinking max` 成功，重新打开菜单确认活动档为 `max`。
- `/model vibemate-probe-b/deepseek-flash` 切到同 ID 的 B 后，活动档为其保存的 `medium`。
  菜单不含 `xhigh/max`，直接输入 `/thinking max` 返回不可用错误，未静默接受。
- 切回 A 后重新采用保存的 `xhigh`，不沿用离开前临时选择的 `max`。
- 会话内选择未改写两个 `modelThinkingLevels` 值、全局 `defaultThinkingLevel` 或模型文件字节。
  Pi 启动会保存其它设置，因此不声称整个 `settings.json` 字节不变。

复验时，输入 `/thinking` 查看菜单；输入带参数的命令时，先确认自动补全，再提交命令。
检查完成后终止临时进程并清理目录。未提交模型提示词、调用工具或发送 Provider 请求。
这证明原生菜单与会话选择，不证明 `xhigh→high/max→max` 的出站载荷、Provider 接受、
维护者 Tauri 预览/应用/恢复验收或 Windows/Linux 行为。

## P24.c OpenRouter 与 DeepSeek Pro 映射证据

针对新映射复核 [DeepSeek Chat schema](https://api-docs.deepseek.com/api/create-chat-completion/)：
模型枚举同时列出 `deepseek-flash` 与 `deepseek-v4-pro`；努力值为 `none/low/high/max`，
兼容 `minimal→low` 与 `medium/xhigh→high`。因此 Pro 使用同一已核实 Chat 映射，
不按字符串前缀匹配其它 DeepSeek ID。

[OpenRouter reasoning](https://openrouter.ai/docs/guides/best-practices/reasoning-tokens)
与官方请求 schema 明确接受 `xhigh/max`；具体可用值仍取中央模型的明确列表。
Pi 1.4.0 的 `pi-ai/dist/api/openai-completions.js` 在 `thinkingFormat=openrouter` 时
把 `thinkingLevelMap` 值写入嵌套 `reasoning.effort`，不是顶层 DeepSeek 字段。
使用已安装 `streamSimple` 的 `onPayload` 回调在网络发送前截获并中断，分别确认
DeepSeek 的 `xhigh→high/max→max` 和 OpenRouter 的 `xhigh→xhigh/max→max`；
4 个样本通过，替换后的 fetch 调用计数为 0，不是 Provider 调用成功证明。

Rust 回归覆盖缺省选用值下的全列表注入、显式选用任一扩展档、用户 map/null 与请求参数保留、
过期审批、双文件字节恢复、精确 ID/官方 base/协议边界及不展开 `any`。
前端只在同一绑定条件下提供声明档位，默认仍需显式选择，不能因目录声明而覆盖原生设置。

## Pi 1.x 模型配置

重新核对 [官方模型文档](https://pi.dev/docs/latest/models)、[配置文档](https://pi.dev/docs/latest/configuration) 与已安装 1.2.0、1.3.0、1.4.0 包中的 `model-config.js`、`provider-composer.js`、`resolve-config-value.js`、`settings-manager.js` 和 `model-resolver.js`。

- 用户目录使用桌面进程的 `PI_CODING_AGENT_DIR`，未设置时为 `~/.pi/agent`；无效覆盖不回退。配置操作验证包入口或官方托管安装布局，并读取当前激活版本；稳定 1.x（1.2 起）检查包内 `models.schema.json`、`settings.schema.json` 的所用字段与兼容选项，不因小版本号变化直接拒绝。预览不启动 Pi。
- 每个服务商实例的配置键使用 `vibemate-<可读配置名称>`；空格和分隔符转为连字符，同名冲突才加 ID 短后缀。配置键是 Provider 身份，`/model` 列表按该键分组并显示它。
- 模型条目的 `name` 固定为 `(vibemate-<服务商名>) <模型 ID>`（例如 `(vibemate-Command-Code-GOAT) deepseek/deepseek-v4.1-flash`），与 Grok Build 共用 `shared::agent_model_display_name`：括号内 `vibemate` 固定、服务商显示名称转为连字符标记，括号后一个空格接模型 ID 原文；连续空白折叠、不可见字符移除、超长截断并以 `…` 结尾（整段不超过 160 个字符）。用户模型别名和 `upstream_name` 不再写入该字段。单独设置 Provider `name` 不能替代配置键。
- 已保存模型的 `resolvedProtocol` 分别映射 `openai-completions`、`openai-responses` 和 `anthropic-messages`。需要不同官方地址时按已核实的模板选择；自定义地址不猜测另一协议的路径。DeepSeek/OpenRouter Chat 的兼容字段仅用于各自官方地址。
- `auth.json` 高于 `models.json.apiKey`。预览只读其结构和摘要；有同名凭据则拒绝应用，不执行其中的凭据命令，不修改凭据文件。应用重新检查其摘要。
- 应用时从 OS 凭据存储读取密钥，写入模型定义。Pi 会解释 `!command` 与 `$VAR`，因此按 1.2.0 起沿用至 1.4.0 的 `$$`/`$!` 规则转义，保证按原文使用密钥；预览的旧密钥和新密钥均为掩码。
- 两个文件通过共享写入日志、私有备份、摘要检查、同目录临时文件和原子替换写入；多文件不是单次原子事务，后续文件失败时补偿已写文件。Unix 替换后为 0600，避免在原本 0644 的模型文件中暴露新密钥。历史恢复按 Agent 类型隔离，并检查应用后的手工改动。
- 新建会话读取默认选择。项目设置、CLI 参数和恢复会话可能覆盖它；未知元数据沿用 Pi 默认值。每模型默认思考档位与可切换映射见上文 P24.c；不添加请求调参控件。当前解析接受 JSON 对象；含 JSON 注释等不可解析内容会拒绝写入。

验证与验收状态以 [todo.md](../plans/todo.md) 为准；写入成功不代表真实推理或其他平台已经通过。

## P02 发行身份与证据（1.1.0 历史）

P02 核实时，`pi --version` 返回 `1.1.0`。命令由 `~/.local/bin/pi` 链接到托管安装的
`~/.pi/agent/bin/pi`，再转发到 `install/releases/1.1.0/node_modules/.bin/pi`。
安装包 metadata 确认包名为 `@earendil-works/pi-coding-agent`、版本 `1.1.0`，
官方仓库为 [earendil-works/pi](https://github.com/earendil-works/pi)，许可证 MIT。
不能继续按旧的 `@mariozechner` 包名假定安装布局。

证据包含官网与该安装包的 `docs/`、`dist/core/model-config.js`、
`dist/core/auth-storage.js`、`dist/core/skills.js`、`dist/extensions/mcp/config.js`。
官网 latest 页面可能变化；适配器要记录实际发行版本，不能把本机验证扩展为全部版本。

## 配置位置与优先级

[官方配置说明](https://pi.dev/docs/latest/configuration)使用默认 agent 目录
`~/.pi/agent`，支持 `PI_CODING_AGENT_DIR` 和 SDK `agentDir` 覆盖。
探测时以运行环境与选定目录为准，不把默认路径当作唯一位置。

| 文件或资源     | 用户级                                     | 项目级                               | 优先级/生效                                                    |
| -------------- | ------------------------------------------ | ------------------------------------ | -------------------------------------------------------------- |
| 设置           | `<agent-dir>/settings.json`                | `<cwd>/.pi/settings.json`            | 受信任项目覆盖普通用户设置，资源数组组合；部分设置只允许用户级 |
| Provider/Model | `<agent-dir>/models.json`                  | 当前官方目录表未声明项目 models.json | 不自行创建项目模型文件；`/model` 重新加载目录                  |
| 凭据           | `<agent-dir>/auth.json`                    | 无对应项目凭据文件                   | 使用原生认证顺序；不能向项目文件写真实密钥                     |
| Skill          | `<agent-dir>/skills/`、`~/.agents/skills/` | `.pi/skills/`、`.agents/skills/`     | 项目资源受 trust 约束；同名 Skill 首个发现者保留并报告冲突     |
| MCP            | `<agent-dir>/mcp.json`                     | `<cwd>/.pi/mcp.json`                 | 受信任项目同名条目覆盖用户级；也可只覆盖 enabled/exposure      |

项目 `.agents/skills/` 会沿父目录发现，遇仓库根目录停止；自定义资源路径另有
settings 规则。[Settings](https://pi.dev/docs/latest/settings)说明用户资源相对
agent 目录解析，项目资源相对 `.pi` 目录解析，支持绝对路径和 `~`。
CLI 选择与恢复的会话可能改变当前模型，不能仅凭 `defaultProvider/defaultModel`
认定正在运行的会话已经使用新模型。运行时确认留给后续 Agent 验收。

### Agent 列表中的当前模型（2026-10-19）

Pi 的 `/model` 切换是**会话级**的：它写入 session JSONL（`model_change` 事件和 assistant
消息的 `provider`/`model` 字段），通常不更新 `settings.json` 的
`defaultProvider`/`defaultModel`。因此 Agent 列表按以下顺序判断当前模型：

1. 在 `<agent-dir>/sessions/` 下找最近修改的 `*.jsonl` 会话文件，只读取其尾部（最多
   512 KiB）；从后往前取最后一条 `model_change` 或 assistant 消息，只保留
   `provider`/`model` 两个字段，其余会话内容立即丢弃，不跨 IPC、不写日志。
2. 该 provider key 只有在 `pi_provider_binding` 中仍有归属、且对应 Provider/model
   仍保存在中央库时，才显示为「服务商 · 模型」；否则显示「自有配置」。
3. 没有可用的 session 元数据时，回退到 `settings.json` 的启动默认值。

多个 Pi 会话并存时，“最近修改的文件”只代表最近一次使用，不证明某个正在运行的进程当前
使用哪个模型；运行中切换后的最终确认仍由维护者完成。

## Provider、模型与独立启动认证

[模型文档](https://pi.dev/docs/latest/models)支持兼容端点的 `baseUrl`、`api`、
`apiKey`、`headers`、`models` 与 `modelOverrides`。模型字段包括 `id/name`、
`reasoning/input`、`contextWindow/maxTokens`、成本、采样参数与兼容设置。
`maxTokens` 描述模型上限，不能直接等同于每次请求想要的输出长度；1.4.0 起它也是 compaction 与分支摘要的输出上限。

首条 DeepSeek 路径使用 `openai-completions`。安装 schema 明确允许
`compat.maxTokensField = max_tokens`、`compat.thinkingFormat = deepseek`；
OpenRouter 可使用自己的 thinking 格式和 routing 设置。Claude 使用
`anthropic-messages` 路由的映射需按具体模型核对。schema 接受字段不代表已经
验证请求效果；P01 的 Provider 字段还必须与 Pi 实际发送的请求对应。

推理使用 `thinkingLevelMap` 与 Pi 的 `off/minimal/low/medium/high/xhigh/max`
级别；不把努力级别强行转换为 token 数量。采样顺序为模型默认、所选 Pi 推理级别
覆盖、请求级参数，后者优先。兼容开关只针对已确认的差异使用。

推理档位在 Pi 里分成两处，观察维护者本机配置可得（2026-10-13）：

| 位置            | 字段                                                                   | 作用                                           |
| --------------- | ---------------------------------------------------------------------- | ---------------------------------------------- |
| 模型定义        | `reasoning`（布尔）、`thinkingLevelMap`（Pi 档位 → 厂商值）            | 声明该模型是否推理、Pi 档位怎么落到厂商字段    |
| 模型定义        | `samplingParamsByThinkingLevel`                                        | 按 Pi 档位覆盖采样参数，用 Pi 档位键而非厂商值 |
| `settings.json` | `defaultThinkingLevel`、`modelThinkingLevels`（键是 `provider/model`） | 保存**每个模型选定的档位**                     |

也就是说 Pi 自己就把「能力声明」与「选定值」分开存：前者在模型定义，后者在 settings。
`thinkingLevelMap` 允许只映射部分档位：`null` 排除该档，未映射的 `off` 到 `high` 仍可用，
`xhigh`/`max` 只有显式字符串映射才可用。

认证优先级由官方 models 文档与 [Providers](https://pi.dev/docs/latest/providers)
说明：运行时 `--api-key` → `auth.json` → `models.json` 的 `apiKey` → Provider
环境变量/云端凭据。扩展可定义不同认证。`/logout` 仅移除存储凭据，不能解除其他
来源，因此不能当作通用停用操作。

`apiKey` 可写环境变量引用或文字值，也支持 `!command`。vibemate 仅管理经过验证的
结构化配置，不生成任意凭据执行命令，也不在导入/预览时执行既有命令。
桌面进程设置的环境变量不会自动传播给用户已有的终端；选择环境变量方式时必须
确认用户独立启动 Pi 的实际来源，缺少变量不能显示可用。

Pi 的 `auth.json` 是明文凭据存储，而非 vibemate 的 OS 凭据引用。安装源码创建
凭据文件时使用 `0600`、新建父目录时使用 `0700`；已有文件权限与 ACL 不自动更改。
需要明确区分中央 OS 凭据、目标 Agent 私有凭据及其备份，不声称外部 Agent 能直接
读取 vibemate 引用。Pi 1.2.0 的 OS 凭据注入已实现，目标文件明文限制见上文。

## Skill 部署与关闭

[Skills](https://pi.dev/docs/latest/skills)采用含 `SKILL.md` 的目录，frontmatter
提供名称和描述，按任务需要加载内容。安装版目录扫描会跟随目录 symlink；本机
临时 Skill 的 symlink 发现已验证，Windows 链接权限与 Linux 行为仍待原生验证。

Settings 的资源数组支持 `!pattern`、`+path`、`-path`。Skill 停用可通过精确排除
或撤销 vibemate 管理的链接/复制实现；必须保留用户资源和中央 Skill 内容。
`enableSkillCommands: false` 只控制命令发现，手动输入仍可使用；
`disable-model-invocation` 只排除自动选择，也不能代表完全关闭。

文件改变后按官方要求 `/reload`。运行中的会话是否重载要单独确认，重新加载前
不能把磁盘上的停用操作当作当前会话已撤销全部既有上下文。

## MCP 原生能力与关闭

[Pi MCP 文档](https://pi.dev/docs/latest/mcp)与安装 `1.1.0` 的实现一致：
原生支持 stdio 和 streamable HTTP，拒绝旧 SSE。stdio 配置是单个 executable 的
`command` 加 `args/env/cwd`，不是可任意执行的 shell 字符串；HTTP 使用
`url/headers`，可选 OAuth。OAuth 凭据保存到私有 `mcp-auth.json`，本次未读取。

每个 Server 支持 `enabled: false`，定义保留但不连接。项目同名完整定义替换全局
定义；仅含 `enabled/exposure/toolExposure` 的项目条目合并这些状态并保留命令、
环境与认证配置。未获项目 trust 时忽略项目配置。

外部修改后需要 `/reload`；Pi `/mcp` 可在会话里切换并保存状态。启动 Pi 或运行
`pi mcp list` 都可能连接 enabled Server，因此本次没有执行它们。
预览只用纯配置解析，不能用会连接服务器的命令当成无副作用验证。

安装了接管 `/mcp` 的第三方扩展时，会话可能不再使用内置 `mcp.json`。此外，
`-builtin:mcp`、`--no-mcp`、`--no-extensions` 等会影响内置能力。
需要检测有效配置/扩展来源；仅看到 `mcp.json` 不能保证 Agent 已加载其中的 Server。

## 开关映射约定

| 首页关系            | Pi 适配方向                                                            | 当前可确认边界                                                                     |
| ------------------- | ---------------------------------------------------------------------- | ---------------------------------------------------------------------------------- |
| Provider/Model → Pi | 管理自己的 Provider/模型声明和默认选择；停用时撤销受管注入并恢复原配置 | schema 没有通用 enabled 字段；不能借 enabledModels 的启动/循环筛选声称彻底禁止使用 |
| Skill → Pi          | 精确排除或撤销自己的链接/复制，之后重载                                | 不修改共享 Skill 文档来停用某个 Agent，不清理用户自有资源                          |
| MCP → Pi            | 原生 enabled；识别全局/项目覆盖与内置实现是否被替换                    | 停用定义可保留；当前会话的连接状态仍需确认                                         |

模型配置的 Pi 1.2.0 实现见上文。Skill 向 `<agent-dir>/skills/<name>/` 的部署（symlink
优先、用户显式 copy-sync、所有权记录与仅撤销托管目标）已实现，见 [skills.md](../skills.md)。
Pi 1.4.0 的用户级 MCP 管理已实现预览、审批应用、启停、撤销和条目历史恢复，
见 [mcp.md](../mcp.md)。中央定义的启停不自动改写 Pi；需要独立审阅原生启停变更。
不推断当前会话已连接，也不把多个文件操作描述为一个原子事务。

## 脱敏格式示例

以下只用于 schema 核实，不是已生效配置。环境变量引用保持原文，不解析凭据。

```json
{
    "providers": {
        "vibemate-deepseek": {
            "baseUrl": "https://api.deepseek.com",
            "api": "openai-completions",
            "apiKey": "${VIBEMATE_DEEPSEEK_API_KEY}",
            "models": [
                {
                    "id": "deepseek-flash",
                    "input": ["text", "image"],
                    "reasoning": true,
                    "compat": {
                        "maxTokensField": "max_tokens",
                        "thinkingFormat": "deepseek"
                    }
                }
            ]
        }
    }
}
```

项目 MCP 停用覆盖示例，要求用户级已经存在同名 Server：

```json
{
    "mcpServers": {
        "vibemate-smoke": { "enabled": false }
    }
}
```

## P02 历史验证与限制（1.1.0）

- `pi --version`、托管 launcher、安装 metadata 与本机附带文档/源码核对完成。
- 临时目录调用 `ModelConfig.load`：接受上述模型配置，未解析环境变量引用。
- 临时 MCP 调用 `loadMcpConfig`：受信任项目 enabled=false 保留全局 command/env；未受信任项目覆盖被忽略。
- 临时 `loadSkillsFromDir`：symlink 指向的 Skill 成功发现。所有临时文件随后清理。
- `pnpm run format:check` 与 JSON 示例解析检查通过。
- 未启动 Agent 会话、MCP Server、Skill 脚本或推理请求；未读写真实凭据/配置。
- Windows/Linux、有效会话重载、实际协议请求、复制同步与 OS 凭据注入留待后续任务验证。

## Pi 1.2.0 本次验证

- `check:frontend`、Rust fmt/Clippy/全量测试与锁定 macOS no-bundle 构建通过。
- 独立应用标识和临时 `PI_CODING_AGENT_DIR` 下，Tauri 实测预览、应用、两份原文件备份、重启后历史读取、预览后修改导致恢复拒绝，以及恢复后字节与原文件一致；测试合成凭据已删除。
- 调用已安装 Pi 1.2.0 的 `ModelConfig` / `ModelRuntime`，禁用目录联网，确认生成的模型 ID、原生 API、地址、上下文信息和合成认证可读取，并核对密钥转义与 Unix 0600 权限。
- 浏览器预览明确显示无法读写配置及历史；Chrome 中检查中文显示和键盘焦点。真实 Provider 消息请求、维护者界面验收及 Windows/Linux 行为仍单独确认。

## 1.3.0 升级与名称修复验证

本机已升级至 1.3.0；该版 `model-config.js` 和 `resolve-config-value.js` 与安装的 1.2.0 相同，所用模型/默认选择 schema 字段一致。新版原生兼容入口在隔离目录中通过了当前 1.3.0 安装检查；真实 Tauri 窗口成功生成旧 UUID 到可读名称的双文件迁移预览，含模型范围与推理档位引用。

同一份由 Rust 适配器生成、从 UUID 条目迁移的临时配置分别交给已安装的 1.2.0 与 1.3.0 `ModelRuntime` 离线加载，并用各自的 `ModelSelectorComponent` 渲染：服务商徽标显示可读键，旧 UUID 条目已移除，额外模型和合成认证可读取。1.3.0 的源码确认选择器会显示所选模型的 `Model Name: <name>` 并把它计入搜索文本，因此模型 `name` 采用上方现行格式；该显示未单独做原生窗口验收。回滚后再次预览仍正确识别别名。没有启动 Agent 会话或发送 Provider 请求。

维护者于 2026-10-16 确认 Pi 1.3.0 升级后的配置预览与可读 Provider 名称验收通过。真实 Provider 消息请求及 Windows/Linux 行为仍待单独确认。
