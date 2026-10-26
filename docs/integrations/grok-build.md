# Grok Build 配置接入约定

核实日期：2026-10-09（P03），2026-10-16、2026-10-23 按安装版本重新核实。本机验证平台为 macOS Apple Silicon。

## 产品与安装身份

[官方介绍](https://docs.x.ai/build/overview)与 [xai-org/grok-build](https://github.com/xai-org/grok-build)
确认 Grok Build 是 xAI 的编码 Agent，支持 TUI、headless 与 ACP。本机命令是
`grok`，`grok --help` 标题为 Grok Build TUI；`grok --version` 返回
`grok 1.0.67 (2fe064390d07)`。本机入口是 `~/.grok/bin/grok` 符号链接，指向
`~/.grok/downloads/grok-1.0.67-macos-aarch64`，`bin/agent` 指向同一文件；`downloads/`
仍保留 `grok-1.0.62-…`、无版本号的 `grok-macos-aarch64` 等旧文件，因此适配器只能使用解析后的
目标，不能假定目录里只有一个二进制。

Grok 启动时把随版本发布的手册解包到 `~/.grok/docs/user-guide/`，并写入
`~/.grok/.metadata_version`（本机内容为 `1.0.67`）。这两份产物与配置根同目录，是**与安装
版本绑定**的合约证据：适配器可以据此判断本机字段是否与被核实的格式一致，而不必依赖在线文档。
`~/.grok/CHANGELOG.md` 同样按版本记录变更。
官方源码许可证为 Apache-2.0；本次仅记录接入规则，没有将源码加入 MIT 应用。

官方安装页面提供 macOS/Linux/WSL 和 Windows 安装入口，本机只验证 macOS。
公开 GitHub 树查询未找到本机短提交 `01a8e1c591c0`；main 源码只能作为当前机制的
补充证据，不能声称与该二进制完全一致。下面注明哪些行为已由本机隔离检查确认。

## 文件与配置优先级

[Settings](https://docs.x.ai/build/settings)使用 `~/.grok/config.toml`，Windows
默认 `%USERPROFILE%\.grok\config.toml`。`GROK_HOME` 覆盖配置、认证、资源、会话
和日志的根目录。探测时使用选定目标的实际目录与环境，不固化用户路径。

1.0.60 随版本发布的 `05-configuration.md` 把优先级列成七层，由高到低是：CLI 参数 →
环境变量 → `requirements.toml` / MDM → `GROK_CONFIG` / `GROK_CONFIG_PATH` 覆盖层 →
`config.toml` → `managed_config.toml` → 内置默认。配置文件层自低到高深合并，所以
requirements 与 MDM 同时钳制 `config.toml` 和覆盖层。`[models].default` 与
`[models].allowed_models` 属于可被上层 `pin` 的字段，因此仅修改用户文件不能证明生效。
覆盖层是新出现的一层：ACP 客户端启动 `grok agent stdio` 时注入 JSON/TOML 覆盖，且只允许
`models`、`features`、缩窄的 `toolset` 与 `shell_environment_policy` 过滤器字段。
vibemate 不写环境变量、也不使用该覆盖层，但预览必须把它算作可能覆盖自身写入的来源。
[企业部署说明](https://docs.x.ai/build/enterprise)的公开分层描述与上面一致；具体机器的生效来源必须检查，不能只修改用户文件就认定成功。

项目 `.grok/config.toml` 只贡献 MCP、plugins 和 permission，不承载完整用户设置
或自定义模型；其余段落只从 `~/.grok/config.toml` 加载。[MCP 文档](https://docs.x.ai/build/features/mcp-servers)说明项目目录
向仓库根目录遍历，同名项目 Server 完整替换用户条目。不能照搬 Pi 的只覆盖
`enabled` 合并语义。项目 trust 与管理策略可能进一步限制资源加载。
Grok 自己写入项目配置时会把末段符号链接换成普通文件，以免后续保存跟随链接越出仓库；
这是 Grok 的写入策略，不代表用户级配置的写入规则。

Grok 还发现 Claude/Cursor 兼容资源；原生 config.toml 优先于兼容 MCP 定义。
通过来源信息区分自己管理的条目、兼容来源、项目覆盖与策略固定项，不替用户关闭
整个兼容层。`inspect --json` 是发现报告，不是连接成功证明。

## P24.a 参数核实更新（2026-10-20，2026-10-23 复核）

本机现为 `grok 1.0.67 (2fe064390d07)`，`.metadata_version` 同为 `1.0.67`；
1.0.62 与 1.0.67 的随版本文档都将 `supports_reasoning_effort` 和单值
`reasoning_effort` 标为弃用，推荐 `reasoning_efforts`。1.0.67 二进制内嵌的默认模型目录
给出数组项 `value/label/description/default`；适配器只写 `value/label/default`。
在线简表仍列旧字段，不能据此新增弃用字段写入。下文 1.0.60 的努力档位说明保留为旧证据；
逐字段状态见 [P24.a 逐字段矩阵](model-parameter-mapping.md)。隔离 macOS 原生检查已确认 Chat/Responses 的菜单、列表默认档和会话内切换；
本地 stub 已核实 Chat 的 `reasoning_effort` 与 Responses 的 `reasoning.effort`；
隔离 TUI 已确认切档后的下一请求和新会话读取持久选择；Grok 自身将选择保存到全局
`[models].default_reasoning_effort`，不是每 alias 默认。真实 Provider 接受仍未核实。

## P24 模型注入与预览

预览显示上下文来自服务商模型库，以及写入、保留或原生默认状态；只显示已解析整数，
未知原生形状只说明保留，完整详情重新构建并比较状态。图像能力只有在中央明确声明时才
映射到已核实的 `model.<alias>.vision` 布尔字段；未知时保留原值。

模型定义只在服务商模块维护。Grok 已消费中央上下文、明确声明的图像能力和非空档位列表：
`reasoning_efforts` 只在原生字段缺失、中央 `Listed` 非空、推理支持为 supported、安装参考
含该字段且未被 `supports_reasoning_effort = false` 禁用时写入。每项写 `value/label`，中央
默认档在列表内时写 `default = true`；标签是英文展示文本。已有数组保留且不解析显示，
`any`、空列表与未知支持不展开。不写旧布尔/单值字段，也不改全局
`[models].default_reasoning_effort` 或 `[models].default`。隔离 macOS 的 Chat/Responses 会话已确认菜单展示、列表默认档和运行中
`/effort` 选择更新；本地 stub 另确认两个 backend 的努力值字段及列表默认档。
这仍不证明真实 Provider 接受该档位。

不提供 temperature/top_p、请求输出或推理 token 数编辑器，不将 `max_completion_tokens`
自动等同于独立模型输出能力。保留用户原有参数、全局 `[models]`、默认模型与其它 alias。
模型级工具能力没有已核实字段，因此不写工具声明；这不表示模型不能调用工具。

## 模型与认证

[配置参考](https://docs.x.ai/build/settings/reference)声明自定义 `[model.<alias>]`：

| 作用         | 字段                                                                                 |
| ------------ | ------------------------------------------------------------------------------------ |
| 标识         | alias 是 Grok 内部配置身份；`model` 是发送给 Provider 的模型 ID                      |
| 协议/地址    | `base_url`、`api_backend`：`chat_completions`、`responses`、`messages`               |
| 认证         | `env_key` 指向环境变量；`api_key` 是目标私有文件中的明文值                           |
| 原生模型属性 | `vision` 表示图像输入；仅中央明确声明时写入，未知时保留现值                          |
| 容量元数据   | `context_window`；不能当作请求参数透传                                               |
| 用户请求值   | temperature/top_p/max_completion_tokens 与未知 effort 保持原样；不提供额外调参编辑器 |
| 路由扩展     | `extra_headers`、超时、重试、工具调用流式设置；逐项核实支持范围                      |

`[models].default` 选择新会话默认模型，环境选择与 CLI `-m` 可改变实际选择，
恢复的会话和运行中的 TUI 也不能仅凭默认值判断。官方支持 `/model` 切换；
外部配置修改后的重读/重启与当前会话有效状态要单独验证。**vibemate 不写
`[models].default`，只读取它来展示当前模型**：它是用户与 Grok 自身的选择，Grok 的默认模型选择器会写这个字段。
适配器只拥有 `[model.<alias>]` 条目；写入后模型在 Grok 中可选，但不会自动成为新会话默认。
Agent 列表读取 `[models].default` **当前指向的 alias**：如果该 alias 有
`grok_model_binding` 归属记录且 `[model.<alias>].model` 与记录一致，显示对应服务商与模型；
否则显示「自有配置」。vibemate 只读这个字段来判断当前模型，从不写入。1.0.60 的
`05-configuration.md` 在 web 搜索策略一节写明「配置在会话开始时读取，改动不在会话中生效」，
`[ui] screen_mode` 与 `pager.toml` 也标注需要重启；但模型段落没有独立的生效声明，
适配器仍按「写入不等于已生效」提示，重启后由维护者确认。

认证顺序按官方 enterprise 文档为模型 `api_key` → 模型 `env_key` → 活动会话
Token → `XAI_API_KEY`。支持浏览器/设备认证与外部认证命令，但 vibemate 不新增
任意脚本执行 API，不执行导入的认证命令。现有登录不等于某个 BYOK 端点已获授权。

1.0.60 的 `11-custom-models.md` 补充了一条对本适配器很关键的规则：当模型自己设置了
`api_key` 或 `env_key`，但值为空或其变量未设置时，Grok **不会**退回会话 Token，也
**不会**把 `XAI_API_KEY` 发给模型自己声明的 `base_url`（除非该地址是 xAI 主机或匹配的
`[endpoints]` 地址）；请求因此不带任何密钥，日志说明缺了什么。也就是说写了 `api_key`
就完全接管该条目的认证，失败不会静默回落到已有登录。

`env_key` 在参考里允许写成字符串或字符串数组（第一个已设置且非空的变量获胜）。适配器
读取时要同时接受两种形状，不能假定一定是字符串。

### 共享 provider 块（1.0.60 新增）

`11-custom-models.md` 与 `26-config-reference.md` 描述 `[model_providers.<name>]`：模型用
`model_provider = "<name>"` 指向它，可继承 `base_url`、`api_backend`、`max_request_bytes`、
`extra_headers`、`query_params` 与 `env_http_headers`；指向的表无法解析时整份配置被拒绝。
参考页**没有**把 `api_key` 或 `env_key` 列为可继承字段，`[models]` 的全局默认也明确排除
`api_key`。因此不能把「一处存放密钥」的设想建立在这个块上：随模型条目存放仍是唯一有文档
依据的做法。适配器遇到用户自己写的 `model_provider` 引用时不得假定该块不含凭据，也不
主动创建这种块。

## 认证落点与别名归属（2026-10-16 核实）

参考表把 `model.<id>.api_key` 标为 user 层字段，并写着「优先使用 `env_key`；不要把内联
API key 放进共享仓库」。也就是说用户级 `config.toml` 里的 `api_key` **有效**，但密钥是
明文落盘：预览必须显示存放位置、脱敏差异与私有备份，写入沿用 0600。Grok 只对
`auth.json` 与 `mcp_credentials.json` 声明 owner-only 权限（`02-authentication.md`），
`config.toml` 的权限要求仍未由官方说明；本机该文件实测已是 0600，但这只是当前机器的状态，
不能当作 Grok 的强制要求。

**密钥实际放进哪个请求头仍未核实。** 官方对 Anthropic `messages` 后端的示例是把
`x-api-key` 写进 `extra_headers`，并说这些头逐字随每次请求发送；示例里没有同时设置
`api_key`。文档没有一句话说明后端为 `messages` 时 `api_key` 会被当作 `x-api-key` 还是
`Authorization: Bearer`。在这一条核实之前，适配器对 Anthropic Messages 协议的模型返回
`protocol_unsupported`，不猜测头的写法；`chat_completions` 与 `responses` 两个 OpenAI
协议按 `env_key`/`api_key` 即 provider API key 的说明映射。

维护者本机六个 `[model.<alias>]` 条目全部使用 `env_key`，`api_backend` 只出现
`chat_completions`（四个）与 `responses`（两个），没有 `messages` 条目；原先三个写错的
`chat_comptetions` 已不在本机配置中。这说明该 Agent 的实际用法落在前两类协议上，也说明
客户端是否会因为拼错的值报错仍未观察过——适配器必须自己校验值域，不能依赖客户端反馈。

别名是配置身份，因此需要区分 vibemate 拥有的条目与用户手写条目：只有私有存储里有归属记录的
`[model.<alias>]` 才能被本适配器改写，同名但无记录的条目一律拒绝接管。

### 适配器写入的 `name` 与别名（2026-10-19 修订）

vibemate 拥有的 `[model.<alias>]` 条目写入时：

- **`name` 字段**固定为 `(vibemate-<服务商名>) <模型 ID>`：括号内 `vibemate` 固定，服务商显示名称转为连字符标记（与配置键同一形状），括号后一个空格接模型 ID 原文。例如 `Command Code GOAT` + `deepseek/deepseek-v4.1-flash` → `name = "(vibemate-Command-Code-GOAT) deepseek/deepseek-v4.1-flash"`；斜杠、大写和点号原样保留，不使用用户模型别名或 `upstream_name`。连续空白折叠，控制字符与不可见格式字符（含 bidi 覆盖）移除；模型 ID 过长时截断并以 `…` 结尾，整段不超过 160 个字符。努力档位属于原生模型选项列表，不拼接到显示名称；该列表按上文边界注入，原生菜单验证不代表 Provider 已接受。
- **格式处理**：连续空白折叠为一个空格（`name` 始终单行）；控制字符与不可见格式字符（含 bidi 覆盖）被移除，避免两个不同名称看起来相同。组合文本超过 160 个字符时截断模型 ID 部分并以 `…` 结尾；模型 ID 本身仍是身份，`name` 只是显示文本。
- **配置别名（表名）**仍由 `choose_alias` 决定：优先复用本根下已记录的归属；否则按 `vibemate-{provider}-{model}` 等候选选取空闲名。表名是配置身份，保持稳定；**不会**接管同名但无归属记录的用户手写表。
- **碰撞拒绝**：若计划写入的 API `model` 字段值（即 `model_id`）与当前 `config.toml` 里某个已有 `[model.<alias>]` 表名**完全相同**，预览以 `TargetConflict` / `target_conflict` 拒绝，不静默改写。典型场景：用户已手写 `[model.deepseek-flash]`，而 vibemate 又要写 `model = "deepseek-flash"`。
- **再应用**：对已归属的别名再次预览/写入时，会把该条目的 `name` 更新为上述格式（仅 vibemate 拥有的条目）。

`name` 与 alias 故意分开：alias 是配置身份，参与 `disabled_models`/`allowed_models` 与归属键；`name` 只用于 Grok 模型选择器中的显示。共享实现为 `shared::agent_model_display_name`，Pi 的模型 `name` 字段使用同一规则。该格式在 2026-10-19 由维护者定为最终形态。

环境引用依赖独立启动 Grok 的终端环境；桌面设置不能自动传播到已有终端。
中央 OS 凭据引用也不能直接充当 `env_key` 值。若目标需要私有明文配置，应明确
预览、权限、脱敏与私有备份策略。认证文件（如 `auth.json`、`mcp_credentials.json`）
未在本次读取；未验证本机权限或 Windows ACL，不能承诺固定权限。

### 努力档位与凭据写入约定（2026-10-13，2026-10-23 更新）

`[models].default_reasoning_effort` 给出默认模型的默认档位；
`supports_reasoning_effort` 与 `reasoning_effort` 已在当前随版本参考中标为弃用，不再新增写入。
现行可选档位写成数组表
`[[model.<alias>.reasoning_efforts]]`，每项一个 `value` 和 `label`，其中一项可标 `default = true`。
1.0.67 内嵌的默认模型目录还带 `description`，但适配器不写该字段。参考页没有列全可选值；
维护者配置里出现的是 `medium`、`high`、`xhigh`、`max`。适配器只在原生字段缺失时写入中央
已声明的非空列表，已有数组保持原样；单项默认只在中央默认档也出现在列表内时标记。

**凭据注入走配置文件，不写环境变量**：适配 Grok Build 时把密钥写入其私有配置的 `api_key`。认证顺序
是 `api_key` → `env_key` → 活动会话 Token → `XAI_API_KEY`，所以写入 `api_key` 会**接管**已有的
`env_key`——不是补充。适配时保留原有 `env_key` 行（不是 vibemate 拥有的字段），界面显示生效来源已
变为 `api_key`，并明示明文落盘与私有备份。维护者会在适配时自行清理本机环境变量，vibemate 不管理
环境变量。

凭据按模型条目存放：同一把密钥会分布在每个 `[model.<alias>]` 上，链接 N 个模型就是 N 份，轮换要改
N 处。P22/P23 首批按「只写选中条目」实现：一次预览只写选中的那一个 `[model.<alias>]`，
不改 `[models].default`，其它条目保持原样，因此同一 Provider 下多个模型不会被静默改写；写全部
相关条目的扩展留待维护者提出。`api_key` 在用户级 `config.toml` 里有效已由 1.0.60 参考表核实，Grok Build 对
该文件权限有无强制要求仍未核实。

维护者配置里 `api_backend` 有三个条目写成 `chat_comptetions`（官方值域是 `chat_completions`）。
客户端是否校验、这些条目是否因此失效未核实；适配器不能依赖客户端报错来发现写错的值。

## P33 MCP 契约复核与当前边界

本轮实际入口已指向 `grok-1.0.70-macos-aarch64`，临时 HOME/GROK_HOME/cwd
中的 `--version` 返回 `grok 1.0.70 (f50904eaee6c)`；真实根的
`.metadata_version` 仍为 `1.0.67`。因此不能把真实根中随版本文档当作
1.0.70 已解包的文档，也没有为了刷新它而启动真实会话。

依据为 [官方 MCP 文档源码](https://github.com/xai-org/grok-build/blob/main/crates/codegen/xai-grok-pager/docs/user-guide/07-mcp-servers.md)、
[官方字段参考源码](https://github.com/xai-org/grok-build/blob/main/crates/codegen/xai-grok-pager/docs/user-guide/26-config-reference.md)
（通过 Context7 读取）和本机仍标记为 1.0.67 的同名手册。核实结果：

- 原生表为 `mcp_servers`，不是 Pi 的 `mcpServers`。stdio 使用
  `command/args/env/cwd`，HTTP 使用 `url/headers`；两者支持 `enabled`。
  在拒绝全部网络的 macOS sandbox 中，用 1.0.70 的
  `mcp add --transport http vibemate-probe https://example.invalid/mcp`
  只写临时根，实际产生 `url` 与 `enabled = true`，没有 `type` 字段。
  这验证 CLI 生成的配置形状，不验证连接或 Streamable HTTP 请求。
- `disabled_mcp_servers` 是独立用户级关闭列表；原生启停可能同时更改它和条目的
  `enabled`。只预览条目不能声称已启用，也不应自动清除此列表。
- 工具目录键是 `server__tool`：服务名须以 ASCII 字母或 `_` 开头，随后只含
  ASCII 字母、数字、`_`、`-`；嵌入 `__` 或末尾 `_` 会产生歧义，工具会被跳过。
  Grok 保留连字符，不套用 Pi 的连字符／下划线归一化规则。
- Grok 在 `command/args/url/env/headers` 中扩展 `${VAR}` 和默认值引用。
  第一片只读 preflight 拒绝公开 stdio 字段的 `${`，不把中央字面值改成动态引用。
  cwd 相对路径及前导 `~/` 的语义未核实，先明确拒绝，不猜测展开或悄悄丢弃。
- 项目、兼容来源、策略与 Agent frontmatter 仍可改变最终生效条目；只读 proposal
  不执行这些发现，也不代表写入、有效或连接成功。

后续只读预览已接入 Rust 目标发现、严格 IPC 与中英文摘要／详情。目标固定为
发现规则选出的 `GROK_HOME|~/.grok/config.toml`；调用方只传中央 identity/revision，
不传路径或文档。复用共享有界读取器，现有符号链接只报告、不替换；详情明确标为
拟新增 TOML 表的 JSON 表示，不冒充完整配置或写入审批。

本轮验证没有启动 MCP 服务器、推理或 doctor，没有访问外部服务、读取真实配置／凭据
或修改真实 Grok 文件。浏览器合成 IPC 检查不代表真实 WebView IPC 或原生 Grok 生效；
完整写入闭环、原生连接与跨平台验收仍待 P33 后续切片。

## Skill 与 MCP 启停

[Skill 文档](https://docs.x.ai/build/features/skills-plugins-marketplaces)支持含
`SKILL.md` 的目录：用户 `.grok/skills/`、沿项目目录发现的 `.grok/skills/`、
启用插件与 `[skills].paths`，还有 `.agents`/Claude/Cursor 兼容来源。
目录链接在本机隔离检查中可发现，其他平台链接与复制同步保持待验。
vibemate 只写入用户级 `<GROK_HOME|~/.grok>/skills/<name>/`（见
[skills.md](../skills.md)）；不写项目路径或兼容来源目录。

`[skills].disabled` 按名称发现但不激活资源；本机 `inspect` 确认停用的 Skill
仍被报告并带 `disabled: true`。管理名称冲突与来源，不能误停同名用户资源。
不改共享 Skill 的 frontmatter 来关闭单个 Agent，不在扫描时运行其脚本。

MCP 原生使用 `[mcp_servers.<name>]`，stdio 为 `command/args/env/cwd`，远端为
`url/headers/bearer_token_env_var`，支持 `enabled`、启动和工具超时。
`${VAR}` 与默认值引用在加载时扩展；不存在的环境值和敏感输出应安全处理。
官方 MCP 页面重点说明 HTTP，字段参考还提到 SSE；第一阶段先按已确认 stdio/HTTP
实现，不凭这一表述承诺旧 SSE 的运行兼容性。

本机有 `grok mcp enable/disable`。隔离操作已确认启停持久化且保留 command args
和无关 UI 设置。TUI `/mcps` 提供开关及 `r` 刷新；当前会话连接与工具撤销效果
没有验证。HTTP OAuth 由 Agent 处理，不能将其 Token 放进中央普通导出。

**本机版本差异：** `1.0.50` 的 `inspect` 列出启用的原生 MCP，却省略关闭的原生
MCP；兼容来源还可能以 disabled 状态列出。因此适配器必须结合配置中的 enabled
与发现报告，不能把缺席当作删除、解析失败或不存在，也不能认为报告格式与 main
源码永远相同。

## 适配方向与未实现范围

| 关系                  | 可用原生方向                                                                       | 实现前还需验证                                                                |
| --------------------- | ---------------------------------------------------------------------------------- | ----------------------------------------------------------------------------- |
| Provider/Model → Grok | 用户级 model alias 与协议；`disabled_models` 移除目录模型；不写 `[models].default` | 模型身份恢复、实际请求字段与生效会话；hidden_models 仅隐藏，仍可通过 CLI 使用 |
| Skill → Grok          | 用户级 `skills/<name>/` 部署（symlink/copy）或撤销自己的部署；也可 skills.disabled | 名称/来源冲突、项目兼容覆盖、运行时刷新                                       |
| MCP → Grok            | per-server enabled，原生 CLI/TUI 开关                                              | 原生/项目/兼容来源合并、策略限制、连接及关闭效果                              |

没有发现第一阶段必须加入本地代理或启动器的证据；三个原生协议入口能作为适配
起点。每个 Provider/模型的工具、推理、图像与输出参数仍需 P24 请求级验证。
不据此声称六个 Provider/Agent 组合已经接通。凭据落点未核实之前，Anthropic Messages
协议按不支持处理，只有 `chat_completions` 与 `responses` 可以进入预览。

## 脱敏示例

下面是格式与发现检查用的用户级 TOML，不是已生效业务配置：

```toml
[model.vibemate-deepseek]
model = "deepseek-flash"
base_url = "https://api.deepseek.com"
name = "(vibemate-DeepSeek) deepseek-flash"
env_key = "VIBEMATE_DEEPSEEK_API_KEY"
api_backend = "chat_completions"

[models]
disabled_models = ["vibemate-deepseek"]

[skills]
disabled = ["vibemate-smoke"]

[mcp_servers.vibemate_smoke]
command = "vibemate-must-not-execute"
args = ["--preserve"]
enabled = false
```

不存在的 MCP 命令仅用于证明配置发现不要求执行服务器；不是安装建议。
密钥值不在示例中。输出字段和推理参数后续需要显式映射到 Provider 协议。

## 本次验证

- 官网、发行仓库、`grok --version`、主帮助与 inspect/MCP 子命令帮助核对完成。
- 临时 `GROK_HOME` 与临时 cwd 的 inspect 成功；关闭自动更新与 Claude/Cursor 兼容功能。兼容定义仍可能被发现并标记 disabled，不能将关闭扫描视为完全不读取元数据。
- 自定义模型格式未出现解析警告，但 inspect 不验证模型请求或保证列出所有模型字段。
- symlink Skill 被发现并标记 disabled；原生 MCP enabled=true 被列出，false 被省略。
- 临时原生 disable/enable 保留 args 与无关 UI 设置；临时文件全部清理。
- TOML 示例解析、`pnpm run format:check`；仅文档改动不重跑原生构建。
- 未运行登录、推理、MCP doctor 或 Agent 会话，未输出真实凭据，未修改用户原有配置。
- 运行中刷新、协议实际调用、Windows/Linux、文件权限与政策覆盖留待后续验证。

## 2026-10-16 复核（P22 前置）

- `grok --version` 为 `grok 1.0.60 (01a8e1c591c0) [stable]`；`~/.grok/.metadata_version` 同为
  `1.0.60`；`~/.grok/bin/grok` 与 `bin/agent` 符号链接指向 `downloads/grok-1.0.60-macos-aarch64`，
  `downloads/` 另存 `grok-1.0.52-…` 与无版本号的 `grok-macos-aarch64`。
- 逐条核对随版本解包的 `05-configuration.md`、`11-custom-models.md`、`26-config-reference.md`、
  `02-authentication.md`：七层优先级与 `GROK_CONFIG` 覆盖层、`model.<id>.api_key` 的 user 层
  有效性、密钥为空时不回落、`env_key` 可为字符串数组、`[model_providers.<id>]` 可继承的字段
  集合（不含凭据）与 `api_backend` 值域。
- 只读取字段名与值形状核对本机 `~/.grok/config.toml`，值全部脱敏：六个用户别名条目均用
  `env_key`，`api_backend` 为 `chat_completions`（4）与 `responses`（2），文件权限实测 0600。
- `grok --help` 子命令清单核对：`inspect`、`doctor`、`models`、`mcp`、`setup`、`version` 等存在；
  `doctor` 的帮助文字只声明检查终端、剪贴板、颜色与输入支持，未声明校验模型配置。
- 本轮只做读取与 `--help`/`--version`，未启动会话、未发起网络请求、未写入或修改任何 Grok 文件，
  未读取 `auth.json` 与 `mcp_credentials.json`。
- 仍未核实：`api_key` 在各 `api_backend` 下实际使用的请求头、`config.toml` 权限是否被强制、
  模型段落的运行时重读语义、`requirements.toml`/MDM 在本机的存在与影响。

## 2026-10-23 复核（P24.d）

- `grok --version` 为 `grok 1.0.67 (2fe064390d07)`；`~/.grok/.metadata_version` 同为 `1.0.67`；
  `~/.grok/bin/grok` 指向 `downloads/grok-1.0.67-macos-aarch64`，`downloads/` 仍保留 1.0.62。
- 1.0.62 与 1.0.67 的 `26-config-reference.md` 都列出 `model.<id>.reasoning_efforts`
  （`array of tables`）；1.0.67 二进制内嵌的默认模型目录显示每项为
  `value`、`label`、`description` 和可选 `default`。适配器只写 `value/label/default`。
- 写入边界：仅当原生 `reasoning_efforts` 缺失、中央 `Listed` 非空、推理支持为 supported、
  安装参考含该字段且 `supports_reasoning_effort` 未显式为 false 时，写入已声明档位；
  中央默认档同时出现在列表内时标 `default = true`。已有数组、旧布尔/单值字段、全局
  `[models]` 与其他 alias 不改。`any`、空列表和未知支持不展开。
- 独立最大输出仍无模型级字段；`max_completion_tokens` 是请求字段，适配器不写。
  工具调用没有已核实模型级字段，适配器不写工具声明。
- Rust 666 项通过、4 项忽略（含 3 项真实网络检查）；前端 1129 项通过；`check:frontend`、
  fmt/Clippy 与锁定 no-bundle 桌面构建通过。临时 `GROK_HOME` 中让 Grok 1.0.67 读取同一数组表写法，
  `grok models` 正常列出条目且无解析警告；这不证明原生菜单或会话切换。
- 此次字段复核未启动 Grok 会话、未发送请求。后续原生菜单检查见下节；各 backend 出站载荷
  与真实 Provider 接受结果仍未验证。

## P24.d 隔离原生菜单与切换验证

使用同一 `grok 1.0.67 (2fe064390d07)`，分别对 `chat_completions` 与 `responses` 启动
临时 PTY 会话。`HOME`、`GROK_HOME` 和 cwd 均指向临时目录；环境只含基础终端变量、
关闭更新的开关与合成密钥变量。关闭 leader、更新、遥测、trace 上传和兼容资源，
并传入 `--no-subagents --disable-web-search --no-alt-screen`。
macOS `sandbox-exec` 拒绝全部网络访问及真实 Grok/Claude/Cursor 目录读取，
仅允许读取已安装 Grok 二进制及下载目录；未读取真实认证或配置文件。

复验样本：模型 alias/API ID 均为 `vibemate-effort-probe`，`base_url` 为
`http://127.0.0.1:9/v1`，`env_key` 指向合成变量，窗口为 32768。
临时 `[models].default` 指向该 alias（只为选择测试模型，vibemate 适配器仍不写全局默认）。
声明两项 `reasoning_efforts`：`high / Probe High / default = true` 和 `max / Probe Max`。
不设置旧 effort 字段或全局默认 effort。两种 backend 各通过以下五项检查：

1. 启动状态显示 `Vibemate effort probe (high)`，确认列表默认档被选用。
2. 输入 `/effort`，菜单显示 `Probe High (active)` 和 `Probe Max`。
3. 下箭头、回车选 `max`，TUI 确认 `Switched to Vibemate effort probe (max effort)`。
4. 再次打开 `/effort`，显示 `Probe Max (active)`。
5. 选择 `Probe High`，TUI 确认切回 `high effort`。

共 10 项断言通过；每次退出后清理临时配置与会话文件。未提交模型提示词、执行工具或发送
Provider 请求。这里只验证合成模型的原生菜单与会话状态，不验证三家 Provider 的合法档位、
请求接受结果或 Windows/Linux 行为；backend 字段与原生持久化检查见下文。

## P24.d 本地 backend 载荷验证

同一 Grok 1.0.67 在临时 HOME/GROK_HOME/cwd 中，以合成密钥连接仅绑定
`127.0.0.1` 的 HTTP stub。macOS sandbox 只允许回环网络，拒绝真实配置读取；
关闭 leader、更新、遥测和兼容资源，传入 `--no-subagents --disable-web-search --max-turns 1`。
headless 样本使用 `-m vibemate-probe --verbatim -p` 和无敏感信息的合成提示词，
不连接三家 Provider，不执行工具。

模型 API ID 为 `vibemate-synthetic-model`，列表声明 `high`（默认）与 `max`。
两种 backend 分别检查未传 CLI effort、显式 `high` 与显式 `max`，共 6 个样本通过：

| backend            | 实际路径               | 模型请求 effort 字段                                | 未传 CLI effort |
| ------------------ | ---------------------- | --------------------------------------------------- | --------------- |
| `chat_completions` | `/v1/chat/completions` | `reasoning_effort: high/max`                        | 列表默认 `high` |
| `responses`        | `/v1/responses`        | `reasoning: { effort: high/max, summary: concise }` | 列表默认 `high` |

stub 在记录 JSON 后故意返回 HTTP 400，Grok 退出码 1 是样本预期，不是推理测试失败或成功。
样本还捕获到目标请求前两个 `model=grok-4.6` 的辅助请求；只对确切合成 API ID 的请求
断言模型字段，不能把后台请求误当成用户选择。它们均发往同一本地 stub，没有外部网络。
此次只核实出站形状，不能替代运行中切档后真实请求、Provider 对该模型档位的接受、
工具/图像调用、恢复后使用或跨平台验收。

## P24.d 运行中下一请求与重启选择

沿用上节回环 stub 与隔离 sandbox，分别启动 Chat/Responses 原生 TUI。每种 backend：
先提交合成提示词，确认目标 API ID 的请求采用列表默认 `high`；通过 `/effort` 菜单切到
`max` 后再提交合成提示词，确认下一请求确实发送 `max`。两个 backend 均通过，
不是只检查菜单文字。stub 仍故意返回 400，不执行推理或工具。

随后检查临时 `config.toml` 并重启为新会话：Grok 自己写入
`[models].default_reasoning_effort = "max"`，没有写模型级旧 `reasoning_effort`；
新会话启动采用 `max`，而不再采用模型列表中的 `high` 默认。两种 backend 均确认。
因此原生 `/effort` 的保存有全局范围，不能描述为 vibemate Provider/alias 隔离的默认档。
vibemate 继续保留该全局值、不写入它；声明列表默认不能保证覆盖用户已保存的全局选择。

此次确认运行中切档对下一请求生效及原生全局持久化，仍未证明真实 Provider 的模型接受。
临时配置与会话在进程退出后删除，真实配置未改动。
