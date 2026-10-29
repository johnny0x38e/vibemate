# vibemate 第一阶段执行清单

这里是唯一任务状态来源。已完成任务只保留一行摘要；实现过程、旧文件清单和逐次测试计数从 Git 历史、CI 与 `CHANGELOG.md` 查阅。

行为参考文档见 [`docs/README.md`](../README.md)：Agent 与发现 [agents](../agents.md)、配置写入与历史 [configuration](../configuration.md)、Skill [skills](../skills.md)、MCP [mcp](../mcp.md)、模型 [models](../models.md)、前端 [frontend](../frontend.md)。

## 当前位置

1. Claude Code、Pi、Grok Build 三个写入适配器的预览、应用、完整分页历史、任意版本恢复、中断核对、历史删除均已交付；H2 已完成，H3 后端与界面已合入本机。
2. Agent 列表当前模型展示已合入本机（维护者 2026-10-19 检查通过）：Claude Code 读取 `env.ANTHROPIC_MODEL`/`model`，Pi 读取最新 session 的模型元数据，Grok Build 读取 `[models].default`；只有仍归属 vibemate 的 Provider/model 才显示服务商与模型，否则显示「自有配置」，未发现或不可读显示「暂无配置摘要」。Grok 写入不再修改 `[models].default`。
3. 六 Agent 固定目录、只读发现、显式与自动版本读取已交付并通过验收（2026-10-13）。
4. 维护者已确认 H1/H3 桌面目视验收通过；H3 已完成，H1 的 Windows/Linux 原生分页验证仍随 P37 跟踪。
5. P24.c 首批（每模型思考档位，以及 Pi 计划中的模型设置行）已在本机完成代码与自动化检查，待维护者桌面验收；本机 Pi 已升级至 1.4.0，2026-10-22 复核所用字段与档位规则未变。
6. P26 已交付：Git 固定修订与已核实的 `skills@1.7.4` npx 安装均在隔离目录获取并导入，失败不改变已有库；来源页可补记获取方式。
7. P27–P30 已交付：Pi/Grok Skill 部署、更新预览与应用（keep/replace、部署刷新）；Claude Code Skill 部署代码与自动化检查已补齐（schema v31），维护者桌面验收待做。
8. P24.d 首批已合入本机：Grok Build 只在原生字段缺失时写入已声明的 `reasoning_efforts` 列表与列表内默认档；已有数组、旧字段和全局默认保持不变；隔离 macOS Chat/Responses 已确认原生菜单、列表默认档与运行中选择更新，本地 stub 已核实 backend 字段/默认档及运行中切档后的下一请求；Grok 自身将选用值全局持久化并供新会话读取，真实 Provider 接受仍待验证。
9. skills.sh 市场浏览/安装、Git revision 检查与 P30 应用更新已交付。Skill 方向参考 [skills-marketplace-reference](skills-marketplace-reference.md)；不引入 preset/工作区。
10. P35 非 MCP／Grok 引用与 P32/P33 MCP 写入、P34 Skill 恢复与 Overview `RecoveryHub`（schema v34）均已合入本机；统一入口只发现并跳转到对应模块，不重放写入。维护者决定桌面／真实连接验收后置；见 [phase-1](phase-1.md)、[skill-recovery](skill-recovery.md)。
11. P24 可写代码已收口：未核实绑定不生成厂商 map／不猜测 Claude 容量通道；真实 Provider 接受与维护者 Tauri 验收仍后置。P35 代码侧引用约束已复核收口（2026-10-26）：Pi 保存档位按惰性偏好保留，中央删除不重写 Agent 文件。P36 代理侧浏览器检查已完成：Providers/Models、Agents、Skills、MCP、Settings、Overview 均检查 720×560 与窄宽度下的长内容、分页、错误态和键盘焦点，并修复列表挤压、英文单复数与 Agent 摘要截断。实际 Tauri 与维护者桌面验收待做；桌面与 P37–P39 发布前专项靠后。
12. 2026-10-28 已推送：技能市场改为二级页（卡片、分页、加载即显示描述、详情弹窗，修复 TLS crypto provider 导致的一直加载）；MCP 列表只保留编辑／启停／删除与详情图标，Pi/Grok 注册、历史、刷新挪到详情页 Agent 行；所有弹窗遮罩覆盖整个窗口含标题栏。代码与 CI 通过，维护者桌面验收待做。

## 文档整理（2026-10-19）

- [x] 文档拆分为现行参考页（[`docs/README.md`](../README.md)）与剩余计划；已交付设计文档及其过程记录从 `docs/plans/` 移除，任务状态只保留在本文件。
- [x] 移除被分页历史取代的「最近十条」列表命令（`claude_code_operations`、`pi_operations`、`grok_operations`）、对应域函数/类型与前端封装；相关测试改为断言根限定分页历史。（Rust 572 通过、1 忽略；前端 1046 通过。）

## 执行约定

- 按依赖顺序实施，一次一个可检查行为；不使用未授权子代理。
- P24 只做现有服务商模型定义到 Agent 的适配注入，区分默认档位与原生可切换映射；不新增请求调参编辑器。实现和测试代码补齐后集中检查，不以原生字段存在就宣称支持。
- 复用已有接入调研，不因 Agent 小版本更新全面重查或阻塞实现；遇到已知弃用、明确冲突或验证失败，再针对性核实（维护者 2026-10-20 决定）。
- 不主动推送远端；仅在维护者当次明确指示时执行 push（2026-10-13 约定）。
- UI 变更同时补中英文和可访问名称；保持既有严格检查，不用 suppression 绕过失败。
- 外观与桌面／真实连接验收后置（维护者 2026-10-26）；功能代码与自动化检查优先。原生行为、真实凭据和跨平台验证单独记录。
- Verification 使用 `AGENTS.md` 的适用命令；Rust 过滤测试必须实际运行非零用例。
- 勾选表示该项已验证；代码完成不等于真实安装、推理或其他平台验收完成。
- 任务完成后删除详细执行过程，保留仍未解决的限制；未来路径是建议，不提前创建空壳。
- 所有 Agent 的 MCP 与 Skill 注入暂时只做用户级：不写项目级、Local 作用域或项目目录（维护者 2026-10-29）。
- 所有配置写入/恢复预览统一采用两层展示（字段摘要 + 详情弹窗），见 [configuration.md](../configuration.md#preview-presentation)。

## 已交付摘要

| 任务                   | 已交付内容                                                                           | 现行说明                                                                                   |
| ---------------------- | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ |
| P00                    | pnpm 与锁文件迁移                                                                    | 根目录 `AGENTS.md`                                                                         |
| P01–P03 / C03          | Provider 与 Pi/Grok 官方接入证据                                                     | [integrations](../integrations)，实施前重新核实版本                                        |
| P04–P06 / C06          | UI 行为测试、私有 SQLite、OS 凭据接口                                                | [frontend](../frontend.md)、[architecture](../architecture.md)                             |
| P07–P09 / C09          | 中英文资源、偏好、启动与界面接线                                                     | [frontend](../frontend.md)                                                                 |
| I02–I15                | 桌面壳、基础外观、共享表单、Base UI 下拉与概览五卡片                                 | [frontend](../frontend.md)                                                                 |
| P10–P12 / C12          | 服务商与密钥统一保存，模型获取/选择/搜索和手动模型                                   | [models](../models.md)                                                                     |
| P13 / P14              | 并入 P12                                                                             | 最小推理检查仍在 P38，不因合并而标为通过                                                   |
| P15 / C15              | 按模型协议路由与三家多协议模板                                                       | [integrations/providers](../integrations/providers.md)                                     |
| P16 / C16 / P40        | 能力解析与来源、用户声明、别名编辑、上下文/输出/档位                                 | [models](../models.md)、[provider 证据](../integrations/providers.md)                      |
| P17.a–P17.t            | 六 Agent 只读发现、手动路径、显式/自动版本读取与官方标志                             | [agents](../agents.md)                                                                     |
| P18–P21 / C21          | Claude Code 预览、写入、回滚与历史                                                   | [configuration](../configuration.md)                                                       |
| PI1                    | Pi 预览、双文件备份、应用与历史                                                      | [Pi 契约](../integrations/pi.md)                                                           |
| P22–P23                | Grok Build 预览、应用与历史恢复                                                      | [Grok 契约](../integrations/grok-build.md)                                                 |
| 当前模型展示           | Claude Code/Pi/Grok Build 列表当前模型读取与归属判定；Grok 不改写 `[models].default` | [agents](../agents.md)、[Pi](../integrations/pi.md)、[Grok](../integrations/grok-build.md) |
| H1 分页与界面          | 快照、核对/解决、未引用副本管理、根限定版本分页、当前版本独立加载并置顶              | [configuration](../configuration.md)                                                       |
| H2                     | 独立完整版本选择、严格 IPC、字段差异预览、并发保护                                   | [configuration](../configuration.md)                                                       |
| H3 后端与界面          | 持久删除状态、独占副本删除、严格 IPC、确认对话框                                     | [configuration](../configuration.md)                                                       |
| BR0–BR5                | 命令、Provider/MCP、Models/Fetch 与共享校验拆分                                      | [backend-modularity](backend-modularity.md)                                                |
| P25 / P31              | 中央 Skill 库与本地导入/删除；中央 MCP 定义与删除                                    | [skills](../skills.md)、[mcp](../mcp.md)                                                   |
| P26                    | Git 固定修订与已核实 npx 安装器；隔离获取、命令确认、来源/获取方式记录               | [skills](../skills.md)                                                                     |
| P27                    | 向 Pi 部署 Skill：预览、symlink/copy-sync、所有权、禁用、Agent 只读槽                | [skills](../skills.md)、[Pi](../integrations/pi.md)                                        |
| P28                    | 向 Grok Build 部署 Skill：同路径约定、独立所有权与撤销，schema v29                   | [skills](../skills.md)、[Grok](../integrations/grok-build.md)                              |
| Claude Code Skill 部署 | 第三个部署目标：配置根、symlink/copy、独立所有权与撤销，schema v31                   | [skills](../skills.md)、[Claude Code](../integrations/claude-code.md)                      |
| P29                    | 预览 Skill 更新：按已记录 Git/npx 获取、比对摘要与路径差异、命名 keep/replace 策略   | [skills](../skills.md)                                                                     |
| P30                    | 应用 Skill 更新：摘要令牌复核、keep/replace、中央库替换与部署刷新                    | [skills](../skills.md)                                                                     |
| 市场安装               | skills.sh 二级页卡片/分页/描述预取与详情弹窗；GitHub `HEAD` 安装入中央库             | [skills](../skills.md)、[marketplace 参考](skills-marketplace-reference.md)                |
| Revision 检查          | Git `ls-remote` 对比已记录修订并存储状态；列表提示 update_available；npx 不走此路    | [skills](../skills.md)                                                                     |

P15 的真实连接记录来自维护者确认，不代表代理重新执行，也不补写未提供的模型、协议或套餐细节。

## 已交付功能验收

- [x] 2026-10-13 维护者验收通过：C06（存储/凭据审阅）、I16（系统风主题浅/深色）、P17（六家 CLI、授权版本读取、原生运行与外观）、P17.t（品牌条款核对）、P25（原生技能导入）、P31（MCP 凭据库与原生外观）。
- [x] 2026-10-14 维护者验收通过：P16/P40 模型编辑对话框与真实目录刷新；P18/P20 Claude Code 原生预览与应用；P31 中央 MCP 删除；P25 技能删除。
- [x] 2026-10-16 维护者验收通过：P21（Claude Code 原生恢复）、PI1（Pi 1.3.0 预览与可读 Provider 名称）、P22/P23（Grok Build 配置预览、应用与历史恢复，本机 `grok 1.0.60`）。
- [x] 概览 v1 五张统计卡与侧栏跳转（维护者 2026-10-14）。
- [ ] 平台专项：本阶段略过；发布前按 P37–P39 验证 Windows/Linux 原生行为、真实 OS 凭据与专门读屏。

**已知 flaky：** `App.test.tsx`「a save shows an app-level notification that outlives the jump back and page changes」偶发 focus 落在 `document.body`（编辑页卸载与 `refreshList` 重挂 IconButton 的竞态）；重跑通常通过。已核对 Notifications 关闭按钮在 `role=status` 外；暂无不改产品行为的稳妥修复。

## 后续任务

### Task H1: 保留每次改写快照并浏览完整历史

**状态：** 后端、界面与维护者桌面目视验收已完成；Windows/Linux 原生分页验证仍随 P37 跟踪，不视为已通过。

已交付：schema v20 独立逐文件快照、只读核对、schema v21 显式解决、未引用私有副本管理、配置根限定只读分页、只读分页 IPC、前端严格边界、分页历史界面、备份可用性、当前配置置顶。设计与边界见 [configuration.md](../configuration.md#history)。

**Acceptance criteria:**

- [x] 所有实际改写路径（含恢复与补偿）先持久化改写前字节/不存在状态；失败不得继续写入。
- [x] 持久记录手动恢复与原版本、补偿关系；旧日志保持原样且不被误重放。
- [x] 三个已支持 Agent 的界面可访问全部配置版本并分页；恢复/补偿保留在审计日志但不增加版本行；当前版本按完整写入摘要核对并独立置顶，外部修改不误标当前。
- [x] 维护者桌面验收版本分页、置顶、选择可用性与核对入口。
- [ ] Windows/Linux 原生分页验证随 P37。历史行删除界面另在 H3 跟踪。

**Verification:** Rust 分页/核对/并发/重启测试、前端边界测试与隔离 macOS Tauri 运行已通过；平台与桌面目视项见上。

**Dependencies:** P21,PI1,P23。

### Task H2: 从界面恢复任意历史快照

**状态：** 已完成（2026-10-19）。隔离 macOS Tauri 与浏览器人工检查由代理执行，Windows/Linux 仍按发布前专项验证。

**Acceptance criteria:**

- [x] 每个应用版本拥有独立完整结果快照（含 Pi 未变化文件），选择时恢复该版本写入后的内容；每次恢复仍先保存审计/恢复快照，但不新增可见版本。
- [x] 展开以字段差异和「使用此配置」为主，保留并发保护、严格 IPC、多文件/失败重试及简短旧密钥/新会话提示；不重复备份/操作流水说明。
- [x] 缺失/损坏快照和不可证明的旧状态阻止恢复；三 Agent 模型配置复用同一呈现。

**Dependencies:** H1 的独立快照、核对保护及配置根限定历史。

### Task H3: 从界面删除历史记录及备份

**状态：** 已完成；维护者已确认桌面目视验收通过，Windows/Linux 原生验证仍随 P37 跟踪。

**Description:** 提供用户主动清理入口；删除不可恢复，但不修改当前 Agent 配置或模型归属。

- [x] 后端首批：schema v23 持久删除状态与独占备份目标；严格 IPC `delete_configuration_history_entry`；10 项 Rust 与边界测试。
- [x] 删除界面本机合入：压缩历史行、`HistoryDeleteAction` 确认对话框、活动/未解决行禁用、残留副本重试与双语文案。

**Acceptance criteria:**

- [x] 每条可删除版本（含最早/中间/曾恢复版本）提供二次确认，说明仅删除本版本快照、保留审计/恢复数据及不可撤销；不自动过期。
- [x] 已完成的隐藏恢复批次不阻止版本删除；保留活动/未解决保护和别名所有权，删除一条不破坏其它版本，删除当前版本不写 Agent 文件。
- [x] 删除文件和数据库记录有持久状态、幂等重试及界面错误反馈；重试在删副本前重新核验保护，中断后可继续处理残留。
- [x] 维护者桌面目视验收删除界面。

**Verification:** 当前完整 Rust 592 项通过、1 项忽略，Clippy 与完整前端检查通过。三 Agent 独立快照、迁移/重启、删除重试保护有回归；隔离 macOS Tauri 使用合成凭据与临时 Claude Code 根，23 项断言覆盖 A/B/C 切换、分页外当前项、中间/曾恢复/当前版本删除且文件不变，测试凭据已清理。中英浅深色、键盘和 720×560 浏览器目视由代理完成；维护者已确认桌面验收通过；Windows/Linux 原生验证仍待完成。

**Dependencies:** H1,H2。

### Task P24.a: 核实模型字段与 Agent 参数映射

**状态：** 已有证据供 P24.b–d 按字段实施；未核实项继续记录，不作全部字段的前置阻塞。维护者 2026-10-20 决定复用先前调研，遇到实际冲突或已知弃用再针对性查证。[逐字段证据矩阵](../integrations/model-parameter-mapping.md) 记录本机 Claude Code 2.1.304、Pi 1.4.0、Grok Build 1.0.67 与剩余缺口；2026-10-22 按 Pi 1.4.0 复核，2026-10-23 按 Grok 1.0.67 复核 `reasoning_efforts` 数组项，整项验收暂不勾选。

**Description:** 建立三个 Agent 消费现有服务商模型定义的逐字段矩阵，要求见 [model-parameters.md](model-parameters.md)。不新增第二套模型定义或请求调参编辑器。

**Acceptance criteria:**

- [ ] 复用官方文档与安装版本证据，核对上下文、最大输出能力、图像、工具、推理支持/可切换档位/默认档的原生映射。请求调参字段不是新增编辑目标。
- [x] 记录每个 Agent/协议的目标字段、优先级、来源、限制及 supported/unsupported/unverified；区分 Provider API ID 与 Agent alias。
- [x] 给出各 Agent 最小可交付字段与拒绝原因，不猜测 effort 与 token 数量转换或未知档位。

**Verification:** 复核文档与安装 schema/随版本手册；不读取或输出真实凭据，不启动推理。

**Dependencies:** P16,P23。**Files:** `docs/integrations/`、`docs/plans/model-parameters.md`。

### Tasks P24.b–P24.d: 按 Agent 注入现有模型定义

P24.b = Claude Code；P24.c = Pi；P24.d = Grok Build。服务商模型是唯一来源；重点是原生 Agent 能使用哪些已有属性，不再扩展模型定义或请求调参页面。

- [x] **P24.b 已交付：** 单一模型注入时，Claude Code 的会话模型与 class alias 均指向同一已选模型；已核实 DeepSeek `deepseek-flash` 上下文校正、中央声明列表内的显式默认 effort、独立记忆起始偏好。env 非原生每模型隔离，不代表所有自定义模型可在内部切换。
- [x] **P24.b 未核实通道：** 其它 Claude 模型容量／能力映射保持 unverified，不猜测写入；不得以请求输出编辑器替代独立最大输出能力。真实验收仍待维护者。
- [x] **P24.c 已交付：** 上下文、最大输出、推理/图片能力、每模型默认思考档位与来源/状态预览。服务商默认档位目前仅显示、不自动选用；用户可从已声明的可映射档位选默认值。「不覆盖」保留原值。
- [x] **P24.c 本轮纠偏：** 官方 DeepSeek Chat `deepseek-flash` 的所有已声明 `xhigh/max` 独立注入映射，不再只写选中默认档的映射；保留用户 map/null 与原有请求参数。实现与回归检查通过；代理隔离 Pi 原生菜单与会话切换检查通过，维护者 Tauri 预览/应用/恢复及真实请求验收仍待执行。
- [x] **P24.c 原生菜单与选择：** Pi 1.4.0 隔离 macOS 会话通过 11 项断言：扩展档位显示与 `max` 选择、每模型启动默认档优先于全局值、同 ID 跨 Provider 默认隔离、用户 null 排除与拒绝不可用档、切回模型重新采用保存档位；会话选择不改保存档位或模型文件。合成密钥、临时目录、禁网 sandbox，无 Provider 请求。步骤与边界见 [Pi 契约](../integrations/pi.md#p24c-隔离原生菜单与切换验证)。
- [x] **P24.c 厂商映射补充：** 官方 DeepSeek Chat 的精确 `deepseek-v4-pro` 使用 `xhigh→high/max→max`；官方 OpenRouter Chat 对明确列表注入 `xhigh→xhigh/max→max`，独立于默认选择并保留用户 map/null。4 个安装 SDK 发送前载荷样本确认 DeepSeek 顶层与 OpenRouter 嵌套努力字段，fetch 调用为 0；Rust 回归覆盖过期审批、双文件恢复与协议/base/ID 边界，前端覆盖选项与预览参数。
- [x] **P24.c 未核实绑定：** 其它绑定不生成厂商 map（Rust 回归覆盖）；真实 Provider 接受与维护者 Tauri 预览/应用/恢复验收仍待完成。模型/服务商删除不重写 Pi 的 `modelThinkingLevels`；P35 复核确认它是惰性偏好而非存活绑定，残留键可在 Pi 或配置历史中清理。
- [x] **P24.d 首批已交付：** 非空中央 `Listed` 档位在原生 `reasoning_efforts` 缺失、推理支持为 supported、安装参考含该字段且未被 `supports_reasoning_effort = false` 禁用时，写入 `value/label`；列表内中央默认档标 `default = true`。已有数组、旧布尔/单值字段、全局 `[models]` 与其他 alias 保持不变；`any`、空列表和未知支持不展开。图像能力继续映射到 `vision`。
- [x] **P24.d 原生菜单与选择：** Grok 1.0.67 的隔离 macOS Chat/Responses 会话均显示注入列表、启动选用列表默认档，并通过 `/effort` 完成 `high → max → high`；重新打开菜单确认活动档位。10 项断言通过，使用合成密钥、临时 HOME/GROK_HOME/cwd，sandbox 拒绝网络及真实配置读取；无模型提示词或 Provider 请求。复验步骤见 [Grok 契约](../integrations/grok-build.md#p24d-隔离原生菜单与切换验证)。
- [x] **P24.d backend 载荷：** 隔离 macOS 的两个 backend 分别以列表默认、显式 high/max 发送合成样本到回环 HTTP stub，6 项通过；Chat 输出 `reasoning_effort`，Responses 输出 `reasoning.effort`。stub 故意返回 400 后终止，不联系真实 Provider、不代表推理成功；辅助请求与目标 API ID 分别核对。
- [x] **P24.d 运行中请求与持久化：** 两种 backend 的隔离 TUI 均确认 `/effort high→max` 后下一合成请求发送 max；Grok 自身写入全局 `default_reasoning_effort=max`，重启新会话优先采用它而非模型列表默认。vibemate 不写该全局字段，不声称每 alias 持久隔离。
- [x] **P24.d 代码收口：** 独立最大输出与工具能力已确认没有已核实模型级字段，不写入。真实 Provider 接受与维护者桌面验收仍后置；本地样本不等于完整真实组合验收。

**Verification:** Rust 666 项通过、4 项忽略；前端 1129 项通过；`check:frontend`、fmt/Clippy 与锁定 no-bundle 桌面构建通过；临时 `GROK_HOME` 中的数组表配置被 Grok 1.0.67 正常读取。该批实现检查未启动 Grok 会话或发送 Provider 请求；后续隔离 macOS 原生菜单与切换检查见上。随后 6 个回环 stub 载荷样本及 4 个 Pi SDK 发送前样本通过，均无真实 Provider 请求；真实接受与 Windows/Linux 未验证。本轮完整检查通过：前端 1132 项，Rust 671 项、4 忽略，fmt、Clippy 与锁定 no-bundle 桌面构建通过。另已确认两 backend 的运行中下一请求与原生全局持久化。

**Acceptance criteria（每个 Agent 分别满足）:**

- [ ] 已定义的模型属性按原生能力注入；缺少配置字段与模型不支持该能力分别说明，不新增多余模型字段。
- [ ] 支持原生切换的 Agent 注入已核实列表/映射，且与默认选择分离；仅默认通道的绑定从中央声明中选择一个值，不猜字段、token 换算或 `any` 的枚举展开。
- [ ] Agent 页面仅选择模型与必要默认值、查看来源/映射状态和注入差异；不提供 temperature/top_p 或请求输出编辑器。
- [ ] Rust/IPC/中英文、过期保护、同名账户/alias 隔离、未拥有字段保留和历史恢复均验证。

**Verification:** 全部实现和测试代码准备好后集中运行完整前端/Rust检查与锁定桌面构建；维护者自行桌面验收，真实调用与跨平台另记。

**Dependencies:** P24.a 的逐字段证据、H1/H2/H3。**Files:** 对应 Agent 适配器、IPC、配置预览与接入文档。

### 本轮纠偏：模型注入，不是请求调参

**状态:** 已撤回偏离目标的新增请求调参表单、IPC 设置结构及 Rust 写入,并修正 Pi 映射与默认值耦合。本轮 `check:frontend` 通过(56 文件、1127 项测试)，Rust fmt/Clippy 与测试通过（659 项、4 忽略），锁定依赖 no-bundle 桌面构建通过。测试数因撤回请求编辑器及其专用测试而减少;新增映射回归保留。P24 整项仍未完成。

- 不修改服务商模型 schema，保留原有容量/能力/档位定义；撤回 temperature/top_p、请求输出编辑及其错误码和仅相关测试。
- Pi 映射改为注入所有已声明且已核实的扩展档，不以选中的默认值限制可切换集合；未知绑定不生成厂商值，用户 map/null 和请求参数不接管。
- Grok 按中央明确声明把图像能力注入 native `vision`，未知时保留原值并在预览显示来源/处理。effort 数组/default 随后已按 P24.d 交付，原生菜单与会话选择验证见上；真实请求仍待验收。
- Claude 的会话模型与 class alias 按单一模型注入设计指向同一选定模型；其默认选择从中央已声明列表取值，不要求每个 alias 单独选择。
- 桌面/浏览器由维护者验收：依次预览→详情→应用→历史恢复；检查中央能力值、默认值与可切换映射分别正确，用户未拥有字段和其他 alias 不变，恢复到原字节。真实推理与 Windows/Linux 未验收，代理不新建浏览器空间。

### Task P24: 验证 Provider 与 Agent 组合

**Description:** 在 P24.a–P24.d 实现后，检查三家 Provider × 三个 Agent 的实际兼容矩阵与切换行为，不承诺不存在的协议转换。

**Acceptance criteria:**

- [x] 每个组合有 supported/unsupported/unverified 状态及原因、版本与证据；九种组合见 [兼容矩阵](../integrations/compatibility.md#p24-九种组合的状态)，配置通道状态与真实 Provider 结果分列，不把 unverified 当通过。
- [ ] 切换账户或模型保持稳定绑定，并正确提示项目/环境变量覆盖。
- [ ] 回归工具调用、推理/图像参数等已支持能力，未支持项不被静默丢弃。

独立最小推理检查已按维护者决定取消，不实现检查按钮或真实 Provider 测试请求；真实调用由维护者在注入 Agent 后验证。本轮只使用合成密钥的 SDK 发送前截获与回环 HTTP stub；验收步骤见 [维护者交接](model-parameters.md#maintainer-acceptance-handoff)。

**当前完成障碍：** 可写注入代码与「未核实不写入」边界已收口并通过回归；P24.b 其它 Claude ID 容量/能力通道仍为有原因的 unverified。P24 整项勾选仍需维护者完成真实 Provider/Agent 组合及 Tauri 预览→详情→应用→恢复验收，不能由代理的隔离样本替代。维护者决定桌面／真实连接验收后置；不再发起真实请求。

**Verification:** 运行映射样本与 UI 切换测试、完整项目检查；真实用例仅使用许可的测试凭据。

**Dependencies:** P12,P15,P16,P20,P23,P24.b,P24.c,P24.d。**Files:** `docs/integrations/compatibility.md`、`src/features/agents/`。

### Checkpoint C24（P22–P24）

- [ ] H1–H3、P24.a–P24.d 与 P24 均已勾选；最小写入验收不等于历史管理与参数映射完成。

## 阶段 F：Skill

部署目标顺序与配置适配一致（Claude Code、Pi、Grok Build）。中央库行为见 [skills.md](../skills.md)。

### Task P26: 从已确认来源安装 Skill

**状态：** 已完成（2026-10-20）。下一步为 P27。

**Description:** 在隔离目录获取 GitHub 或经核实的安装器结果，再加入中央 Skill 库。

**Acceptance criteria:**

- [x] Git 来源固定 revision；npx 来源记录准确安装器/版本/命令和退出状态，执行前显示命令。
- [x] 不执行 Skill 内容中的命令；校验路径遍历、失效来源和结果格式。
- [x] 失败不破坏已有库；成功记录来源及可更新方式，不声称未知来源可更新。本地导入的技能在来源页补齐地址与获取方式后写入同一字段，供 P29 使用。

**Verification:** Rust 630 项通过、4 项忽略（含 3 项真实网络检查），前端 1112 项通过，`check:frontend`、fmt/Clippy、锁定桌面构建与 720×560/520×640 浏览器检查通过；真实 `vercel-labs/agent-skills` 的 Git 固定提交与 `skills@1.7.4` npx 安装均成功，缺失提交失败后库保持为空。私库、Windows/Linux 原生行为和维护者桌面目视未验证。

**Dependencies:** P25（已完成）。

**Files:** `src-tauri/src/skills/install.rs`、`desktop/skills.ts`、`SkillLibrary.tsx` 与测试；获取方式字段需一次迁移。

### Task P27: 向 Pi 部署 Skill

**状态：** 已完成（代码与自动化检查）。真实 Pi `/reload`、维护者桌面目视与 Windows/Linux 原生验证仍未完成。

**Acceptance criteria:**

- [x] 预览新增/冲突与目标路径，symlink 优先；不可用时让用户选择 copy-sync。
- [x] 保留同名用户内容，记录所有权；禁用只撤销自己管理的目标。
- [x] 报告目标已同步的 revision，失败可恢复，不把部分部署显示为整体成功。
- [x] 在 Agent 详情汇总区添加 Skill 只读槽位（部署状态 + 跳转技能模块）。

**Verification:** Rust 658 项通过、4 项忽略（含 9 项部署：symlink/copy、foreign 冲突、owned 撤销、过期审批、`skill_in_use`）；fmt/Clippy `-D warnings` 与 `check:frontend`（1123 项 UI 测试）通过。真实 Pi `/reload`、维护者桌面目视与 Windows/Linux 仍随发布前专项。

**Dependencies:** P02,P19,P25。

### Task P28: 向 Grok Build 部署 Skill

**状态：** 已完成（代码与自动化检查）。真实 Grok 可发现性、维护者桌面目视与 Windows/Linux 原生验证仍未完成。

**Acceptance criteria:**

- [x] 仅对已确认支持的格式部署（用户级 `GROK_HOME|~/.grok/skills/<name>/` 与 `SKILL.md`）；未知能力明确为待决，不创建无效目录。
- [x] 正确处理目标冲突、symlink 权限与 copy-sync，并记录部署 revision（schema v29）。
- [x] 撤销、重试和部分失败不会影响其他 Agent 或用户 Skill。

**Verification:** Rust 678 项通过、4 项忽略（含 7 项部署：Pi symlink、Grok copy、双 Agent 独立部署/撤销、foreign 冲突、过期审批、`skill_in_use`）；fmt/Clippy `-D warnings` 与 `check:frontend`（1132 项 UI 测试）通过。真实 Grok 发现、维护者桌面目视与 Windows/Linux 仍随发布前专项。

**Dependencies:** P03,P27。

### Task P29: 预览 Skill 更新

**状态：** 已完成（代码与自动化检查）。真实网络获取与维护者桌面目视仍未完成；应用更新见 P30。

**Acceptance criteria:**

- [x] 只对来源明确的 Skill 检查更新，Git/npx 使用已记录的获取方式和版本。
- [x] 比较原 revision、中央本地修改与候选内容，显示差异与可选处理策略。
- [x] 更新预览不写中央或 Agent 目标；超时/失效/版本变化有明确错误。

**Verification:** Rust 685 项通过、4 项忽略（含 7 项更新预览：无 fetch method、current/local/upstream/diverged、过期修订、超时）；fmt/Clippy `-D warnings` 与 `check:frontend`（1132 项 UI 测试）通过。真实网络与桌面目视仍随发布前专项。

**Dependencies:** P26,P27,P28。

### Task P30: 应用 Skill 更新

**状态：** 已完成（代码与自动化检查）。真实网络获取、桌面目视与 Windows/Linux 原生验证仍随发布前专项。

**Acceptance criteria:**

- [x] 应用前重新检查内容摘要，不能覆盖未处理的本地修改或过期计划。
- [x] 中央与链接/复制目标按操作日志更新，显示逐目标状态；部分失败可恢复。
- [x] 失败和回滚保留来源/revision/所有权一致性，不删除其他 Agent 内容。

**Verification:** Rust apply/keep/replace/stale/strategy 与部署刷新测试；fmt/Clippy/`check:frontend` 随交付跑通。真实网络与桌面目视仍待维护者。

**Dependencies:** P19,P29。

### Checkpoint C30（P25–P30）

- [x] P25–P30 均已勾选（桌面/跨平台专项仍随 P37 等跟踪）。

## 阶段 G：MCP

中央 MCP 行为见 [mcp.md](../mcp.md)。

### Task P32: 向 Pi 注入 MCP 配置

**状态：** 已完成 Pi 配置管理闭环及隔离服务器发现／停用验收：中央 revision／完整文件审批复核、受管条目所有权、部署／更新、原生启停、撤销、所选历史条目恢复、显式中断核对、跨目录引用删除保护及 Agent 只读汇总。凭据只在 Rust 部署／更新时读取，保留其它条目和顶层字段；不覆盖外来或被外部修改的受管条目，不自动重放。Pi 1.4.0 原生 stdio/Streamable HTTP 已核实，无桥接。实现边界见 [mcp-deployment](mcp-deployment.md)。经用户明确授权，真实 Pi 1.4.0 CLI 对 stdio 与本机 loopback Streamable HTTP 的发现／停用／恢复／撤销验收已通过；Grok P33 和 MCP 整体交付仍未完成。

**Verification：** 19 项 Rust 生命周期测试覆盖两种传输、权限／脱敏、条目恢复、备份损坏／借用拒绝、并发锁、审批防重放和三个中断窗口；全量 Rust 724 通过、4 忽略，fmt/Clippy、`check:frontend`（1211 项 UI 测试）与锁定 no-bundle macOS 构建通过。真实 macOS Tauri WebView／IPC 在唯一应用标识、临时 HOME／应用数据／Pi 根下完成 9 项配置生命周期检查，未使用 OS 凭据字段。Pi 1.4.0 纯解析器验证 6 份实际结果副本及 14 项配置／字面值断言。浏览器隔离 mock 检查中英浅／深色、720×560 长名称／路径、详情键盘／逐层 Escape 与焦点返回、恢复审阅；mock 不冒充真实 IPC。另 9 项真实 Pi CLI 检查验证 stdio/HTTP 启用时发现固定工具、停用后启动次数／HTTP 初始化次数不增加、恢复后重新连接、撤销后不再列出。CLI 使用干净环境、隔离 HOME／工作目录及 offline／禁用版本检查；未启动 Agent 推理会话、读取真实凭据或请求外部服务。临时服务器全部退出，无关条目及顶层字段保留。前端 chunk 大于 500 kB 的既有警告仍在；Windows/Linux 与 OS 凭据部署仍需独立验收。

**Acceptance criteria:**

- [x] 原生支持时按实际格式预览并应用，保留无关字段和已有 MCP 条目。
- [x] 凭据在 Rust 中解析，差异脱敏，写入/撤销/恢复复用安全机制。
- [x] 无原生支持时明确标记为不支持、不实现桥接（维护者决定 2026-10-13）。
- [x] 在 Agent 详情汇总区添加 MCP 只读槽位（部署状态 + 跳转 MCP 模块）。

**Verification:** Rust 映射/恢复测试及部署 UI 测试通过；真实 macOS Tauri 配置闭环与 Pi 1.4.0 的 stdio／loopback HTTP 无敏感测试服务器发现／停用／恢复／撤销均通过。此结果不代表真实第三方服务、Provider 请求或 Windows/Linux 验收。

**Dependencies:** P02,P19,P21,P31（已完成）。

### Task P33: 向 Grok Build 注入 MCP 配置

**状态：** 配置管理闭环已合入本机（代码与自动化检查）：schema v33 所有权／意图、审批预览、deploy/update/enable/disable/revoke/restore、中断核对、中央删除引用与 Agent 只读汇总。Rust 写入仅改 `mcp_servers.<name>`，保留模型表与 `disabled_mcp_servers`；凭据只在 deploy/update 读取并拒绝 `${…}`；精确服务名（无 Pi 命名空间归一化）。9 项 Rust 生命周期测试与完整前端检查通过。真实 Grok 连接、桌面 Tauri WebView／IPC、OS 凭据部署与 Windows/Linux 仍待验收。设计见 [mcp-deployment](mcp-deployment.md)。

**Verification：** 9 项 Grok MCP 生命周期测试（stdio/HTTP、脱敏、连字符独立性、过期计划、foreign/modified、删除引用、条目恢复、中断放弃、动态密钥拒绝）；全量 Rust 测试、fmt、Clippy `-D warnings` 与完整 `check:frontend`（1260 项 UI 测试）通过。未启动 MCP／推理／Provider；未改真实 Grok 配置。

**Acceptance criteria:**

- [x] 原生支持时独立映射传输与参数，不能照搬其他适配器配置或丢弃不支持字段。
- [x] 预览、应用、撤销与恢复保持原配置、所有权和凭据脱敏。
- [x] 无原生支持时明确标记为不支持、不实现额外桥接（维护者决定 2026-10-13）。
- [ ] 维护者桌面／真实 Grok 连接与跨平台验收（随发布前专项）。

**Verification:** Rust 与部署 UI 回归测试已跑通；真实 Grok Build 可用性与恢复结果仍待维护者。

**Dependencies:** P03,P23,P31。

### Task P33.b: 向 Claude Code 注入 MCP 配置（仅用户级）

**状态：** 代码与自动化检查已在本机完成（2026-10-29，schema 35，未提交）：后端、MCP 详情页 Claude Code 行、Agent 详情只读槽、删除引用与恢复中心。用户级无单服务器开关，启停标为不支持；含 `cwd` 的 stdio 定义拒绝。维护者桌面验收待做。只写 `~/.claude.json` 顶层 `mcpServers`；不写 `projects.<路径>.mcpServers`（Local）、项目根 `.mcp.json` 或托管 `managed-mcp.json`。接入证据见 [Claude Code 契约](../integrations/claude-code.md#mcp-配置)。

**Description:** 参照 P32/P33 的闭环，为 Claude Code 提供 MCP 注册：审阅预览、部署/更新、启停（若原生支持，否则明确不支持）、撤销、所选历史条目恢复、中断核对、中央删除引用保护与 Agent 只读汇总；前端在 MCP 详情页增加 Claude Code 行。

**Acceptance criteria:**

- [x] 只改 `~/.claude.json` 顶层 `mcpServers.<name>`，`~/.claude.json` 里其它键（含 `projects`、会话/账户状态）逐字节保留；写入原子、带备份与并发/过期复核。
- [x] 按 Claude Code 原生格式独立映射 stdio 与 Streamable HTTP（及其需要的 type 字段），不照搬 Pi/Grok 映射；不支持的字段明确拒绝而非丢弃。
- [x] 精确名称所有权，拒绝覆盖外来或被外部修改的条目；凭据只在 Rust 部署/更新时读取，预览与历史脱敏。
- [x] 中央删除、Agent 详情 MCP 槽与恢复中心包含 Claude Code 引用；中英文与可访问名称齐全。
- [ ] 维护者桌面与真实 Claude Code 连接验收（随发布前专项）。

**Verification:** Rust 生命周期测试覆盖两种传输、脱敏、无关键保留、过期计划、foreign/modified、条目恢复与中断；完整 fmt、Clippy `-D warnings`、Rust 测试与 `check:frontend` 通过。

**Dependencies:** P18–P21,P31,P32,P33。

### Checkpoint C33（P31–P33）

- [ ] P31–P33 代码已交付；P33 维护者桌面／真实连接与跨平台仍未勾选；P33.b（Claude Code 用户级 MCP）代码已完成，维护者验收待做。

## 阶段 H：维护与验收

### Task P34: 恢复未完成操作

**状态：** Pi／Grok MCP 的持久意图与显式解决已由 P32／P33 交付；Skill 部署中断恢复（schema v34、`SkillRecoveryPanel`）与 Overview `RecoveryHub`（汇总 Agent／Skill／MCP 并仅导航）已合入本机；维护者桌面验收仍后置，设计见 [skill-recovery](skill-recovery.md)。

**Description:** 在启动时发现上次中断的 Agent 配置或 Skill/MCP 部署操作，提供明确的恢复入口，不静默重放敏感操作。Agent 配置的中断核对与显式解决已交付（见 H1）；P32／P33 已补 Pi／Grok MCP 的持久意图与不重放的显式解决；Skill 切片已补 journal／分类／Finish／Dismiss；Overview 汇总入口只发现并跳转。

**Acceptance criteria:**

- [x] Skill：显示未完成部署的脱敏状态、受影响目标与 Finish／Dismiss；不把部分成功显示为全部完成；不静默重放。
- [x] 统一入口汇总 Agent／Skill／MCP 未完成操作；各模块恢复前核对目标变化，冲突时保留当前内容；Finish／Dismiss／MCP 确认不静默重放且可重试。
- [ ] 恢复失败仍可继续处理；维护者桌面验收通过。

**Verification:** Skill 中断/冲突/重复 Finish 与面板行为、`listRecoverySummary`／`RecoveryHub` 聚合与导航测试已覆盖；临时配置重启与维护者桌面验收仍待做。

**Dependencies:** P21,P23,P30,P33。

### Task P35: 约束被引用配置的删除

**状态：** 代码与自动化检查已收口（2026-10-26）：Provider/Model 的 Pi/Grok 引用查询、删除阻止和失效绑定清除；Skill 部署与未完成 journal 的删除阻止；Pi/Grok MCP 跨目录注册与未完成意图的引用展示及事务内删除拒绝。维护者桌面验收仍待做，不计为已验收。

**Description:** 完善 Provider/Model/Skill/MCP 的删除行为，防止已有 Agent 引用突然失效。中央删除只改 vibemate 存储与凭据库，不重写 Agent 配置；仍存活的绑定或受管部署必须先通过对应模块解除。Pi 的 `modelThinkingLevels` 是模型启动偏好，不是绑定，删除后保留为惰性键。

**Acceptance criteria:**

- [x] 展示被引用关系，禁止静默级联删除或自动重写全部 Agent 配置。
- [x] 用户选择先解除绑定/撤销受管部署，再按对应机制清理；失败保留可恢复状态。
- [x] 凭据清理与数据库删除遵守补偿约定；用户文件和共享凭据不误删。

**Verification:** 引用查询、活动/失效/不可读绑定、事务内拒绝、失效清除、删除补偿与失败重试均有 Rust 回归；Provider/Model/Skill/MCP 删除确认及引用列表有前端行为测试。Rust 回归新增断言：清除失效 Pi 绑定并删除 Provider 后，`settings.json` 的保存档位字节不变。本轮完整检查：Rust 767 项通过、4 项忽略，fmt/Clippy `-D warnings`、`check:frontend`（66 文件、1272 项）与锁定 no-bundle 桌面构建通过。维护者桌面验收仍未执行。

**Dependencies:** P21,P23,P30,P33。

### Task P36: 检查前端实际可用性

**状态：** 代理侧浏览器检查已完成（2026-10-26）；实际 Tauri 交互与维护者桌面验收待做，验收项暂不整体勾选。

**页面切片：**

- [x] Providers/Models：720×560 中英文、520×640 窄宽度下的长名称、长模型 ID、加载更多、列表错误态和模型列表错误态可操作且反馈明确。
- [x] Provider 删除确认初始焦点在「取消」，Escape 关闭后焦点返回行操作；创建页进入时标题聚焦，Tab 可达保存、页签与表单。
- [x] Agents：720/520 中英文列表、长 Provider/model 摘要、详情与「安装与路径」页、发现失败态；修复窄窗口英文摘要被状态栏挤压。
- [x] Skills：列表、长描述/来源、详情、部署预览、恢复面板、删除确认与焦点返回。
- [x] MCP：12 行长列表、长名称/路径、520 宽度下操作按钮换行、编辑表单与长参数/凭据行、删除确认、Pi 注册管理弹窗。
- [x] Settings：常规/关于页、方向键切换页签、720/520 下主题色板换行、长日志路径。
- [x] Overview：五张计数卡与 RecoveryHub 多行汇总、720/520 下的堆叠与跳转按钮。
- [x] 修复窄列表下标签组把名称列压到 0 宽、行高被撑到数千像素的问题：列表自身按可用宽度把协议/模型数标签移到下一行。
- [x] 修复英文「1 added models」：改为单数/复数资源键，中文保持一个计数形式。
- [x] 修复 Agent 列表状态栏过宽导致的英文摘要截断：状态栏收窄，`Own configuration` 与 `No configuration summary` 在 720×560 完整显示。

本轮使用临时 Vite 页注入合成 IPC 数据检查；临时页、截图与测试数据已删除，未读取真实凭据或 Agent 配置。`ProvidersView.test.tsx` 新增单数文案回归。加载骨架、空列表、分页边界与缩放主要由既有行为测试覆盖，仍需实际 Tauri 桌面复核。

**Description:** 用真实业务页面检查长内容、键盘、分页与错误状态，修复有证据的问题。

**Acceptance criteria:**

- [ ] 中英文在 720×560、缩放和长 URL/模型 ID 下可操作，列表加载/空/失败/分页有明确反馈。
- [ ] 表单与确认界面键盘可达、焦点恢复正确，状态不只靠颜色；符合严格样式规则。
- [ ] 仅修复发现的问题；跨多个 feature 时先按页面拆子任务。

**Verification:** 代理侧用临时 Vite 页与合成 IPC 数据检查全部页面的 720×560/520×640 中英文长内容、分页、错误态与键盘焦点；临时页、截图与测试数据已删除。完整 `check:frontend`（66 文件、1272 项）、Rust 767 项通过/4 项忽略、fmt/Clippy `-D warnings` 与锁定 no-bundle 桌面构建通过。实际 Tauri 交互、缩放与维护者桌面验收仍待做，因此验收项暂不整体勾选。

**Dependencies:** P34,P35。

### Task P37: 验证三平台系统行为

**Acceptance criteria:**

- [ ] 原生验证路径/大小写、权限、替换、链接/复制和失败恢复，不用交叉编译冒充平台证据。
- [ ] macOS/Windows/Linux 的凭据不可用路径可测，真实 OS 密钥库的交互验证单独记录。
- [ ] 检查四发布目标的依赖与运行条件，CI 结果和未完成手工项有清楚状态。

**Verification:** 完整前端与 Rust 检查及本机原生构建；核对三个原生 CI 结果；人工验证各 OS 的权限/密钥库/安装条件。

**Dependencies:** P20,P23,P30,P33,P35。

### Task P38: 完成真实接入验收

**Acceptance criteria:**

- [ ] 三 Provider/三 Agent 每个受支持组合有版本和实际结果，不把 unsupported/unverified 当通过。
- [ ] 验证原生启动、模型参数、回滚、Skill 来源更新与 MCP；记录未实现的桥接缺口。
- [ ] 真实请求在允许范围内，使用临时或可恢复配置，不将密钥或敏感输出放入记录。

**Verification:** 按 documented matrix 人工完成真实 Agent/Provider 验收；运行完整项目检查。

**Dependencies:** P24,P30,P32,P33,P34,P37。

### Task P39: 准备首个草稿 Release

**Acceptance criteria:**

- [ ] 版本、锁文件和 CHANGELOG 条目一致；更新说明不包含未实现功能或未通过的平台承诺。
- [ ] 在维护者授权创建远端/标签后运行四目标草稿构建，检查资产与安装启动结果。
- [ ] 签名/公证未配置时明确记录限制，维护者检查后决定是否发布，不默认启用应用更新。

**Verification:** `pnpm run check:frontend`、完整 Rust 检查和发布说明预览；检查四个真实 Actions job 与安装包。

**Dependencies:** P36,P37,P38。

### Checkpoint C39（P37–P39）

- [ ] P37–P39 均已勾选；可打第一阶段 release 标签。

## 后续后端与产品扩展

BR0–BR5 已完成；现行边界见 [backend-modularity.md](backend-modularity.md)。以下不是六 Agent 发现的额外前置，维护者已决定先不排期：

- [ ] **BR6：Catalog/Storage/Credentials。** 各厂商解析局部化；迁移注册顺序和已发布 SQL 保持原样；按需要分离 OS store 实现和各业务补偿策略。
- [ ] **BR7：外部服务适配。** GitHub 客户端/账号支持备份目标操作；Vercel 用途待确定。
- [ ] **BR8：配置备份恢复设计。** 版本化非敏感快照、GitHub 目标/路径、远端修订冲突、凭据重新关联与恢复差异。
- [ ] **BR9：MCP/Skill 来源安装设计。** 首个实际来源、固定 revision、安装计划/执行/更新/卸载、受管路径和恢复。路线 A 已确认：更新能力以「来源是否明确」为条件。
- [ ] **BR10：API 转发设计。** 首条同协议转发、gateway 生命周期、路由、协议适配、流式/工具调用/取消/错误验收。
- [ ] **BR11：Agent 安装设计。** 首个 Agent 的安装/升级/卸载契约、目标平台/架构与现有安装来源；安装完成后只检测，不自动启动或应用配置。
