# Codex 配置接入约定

核实日期：2026-10-29。对应 P41 写入适配器；本机验证平台为 macOS Apple Silicon。本页记录当前工作区代码的行为（含审查项 S1–S6 的修复）。

## 发行身份与证据

本机 `codex --version` 返回 `codex-cli 0.165.0`，入口为 `/opt/homebrew/bin/codex`。
对应源码 tag 为 [`rust-v0.165.0`](https://github.com/openai/codex/tree/rust-v0.165.0)
（commit `360319d8`）。下文源码结论固定在这个 tag；官方文档按 2026-10-29 的线上版读取：

- [Configuration reference](https://developers.openai.com/codex/config-reference)
- [Advanced configuration](https://developers.openai.com/codex/config-advanced)
- [Skills](https://developers.openai.com/codex/skills)

影响适配器设计的几条事实：

- Codex 0.165.0 的 `wire_api` 只剩 `"responses"`，写 `"chat"` 会在加载配置时报错。所以一个服务商能否接入，只看它有没有 OpenAI Responses 兼容端点。
- 顶层 `profile = "..."` 与 `[profiles.*]` 是旧机制；0.165.0 遇到顶层 `profile` 会报 `legacy profile ... is no longer supported`。新机制是 `$CODEX_HOME/<name>.config.toml` 加 `--profile <name>`。
- `openai`、`amazon-bedrock`、`amazon-bedrock-runtime`、`gcp`、`ollama`、`lmstudio` 是保留的 provider ID，不能用作自定义表名。
- Codex 自己也会改写 `config.toml`（`/model` 写 `model`/`model_reasoning_effort`，`codex mcp add/remove` 整体替换 `[mcp_servers]`），并且会丢掉未建模的键。因此归属只记在 vibemate 的 SQLite 里，不依赖文件里的注释或自定义键。

## 配置文件与优先级

| 位置                                             | 生效范围           | vibemate 是否写入 |
| ------------------------------------------------ | ------------------ | ----------------- |
| `/etc/codex/config.toml`、MDM/云端托管层         | 系统或组织         | 否                |
| `$CODEX_HOME/config.toml`（未设置时 `~/.codex`） | 本机用户           | 是（唯一目标）    |
| `$CODEX_HOME/<name>.config.toml` + `--profile`   | 显式选用的 profile | 否                |
| 受信项目的 `.codex/config.toml`                  | 项目会话           | 否                |
| 命令行 `-c`、会话内 `/model` 等运行时覆盖        | 单次运行           | 否                |

后者覆盖前者。项目级 `.codex/config.toml` 本身也会忽略 `model_provider`、`model_providers`、`profile(s)` 等键，所以 provider 只能来自用户级配置。所有 Agent 的 MCP 与 Skill 注入只做用户级，vibemate 不写任何项目级文件。

配置根的解析与发现一致：`CODEX_HOME` 非空时必须已存在且是目录，按规范化后的路径使用；为空或未设置时使用 `~/.codex`。vibemate 从不创建 `$CODEX_HOME`。目录缺失、`config.toml` 是符号链接、只读、超过大小上限或不可读时返回 `target_unavailable`，不写任何内容。`config.toml` 不存在但目录存在时，按新文件创建。

所有写入都走共享的可恢复写入路径（[configuration.md](../configuration.md)）：预览绑定文件摘要，确认时在跨实例 `.config-lock` 下重建并比对，变化即 `plan_stale`；先写私有备份与 journal，再同目录临时文件 + 原子 rename。私有写入把 Unix 权限设为 0600。TOML 通过 `toml_edit` 修改，只动受管的表或键，注释、键序和其它字节保持不变。

## 模型配置

### 写入的字段

一次「添加 Provider」（`deploy`）只写一张表：

```toml
[model_providers.vibemate-deepseek]
name = "vibemate-DeepSeek"
base_url = "https://api.deepseek.com"
wire_api = "responses"
experimental_bearer_token = "<明文密钥>"
```

- 表名 `vibemate-<slug>`：slug 取 Provider 显示名的 ASCII 小写字母和数字，其它字符变为 `-`，最长 40 个字符；取不出时退回服务商类别名。同一配置根下若该 slug 已归另一个 Provider，追加 Provider ID 的前 6 个十六进制字符；仍冲突则返回 `name_conflict`。
- `name` 为 `vibemate-<Provider 显示名>`。
- `base_url` 只取 Responses 路由：模板没有 Responses 协议时返回 `protocol_unsupported`；Provider 已是 Responses 协议时沿用其地址；使用模板地址时换成模板的 Responses 地址；自定义地址无法核实时返回 `base_url_unverified`。
- OpenRouter 另写 `supports_websockets = false`。
- 密钥见下文「认证与凭据」。

### 永不写入的字段

vibemate 不写顶层 `profile`、`[profiles.*]`、`model_catalog_json`，也不在 provider 表里写 `env_key`、`auth`、`requires_openai_auth`。`model_reasoning_summary`、`model_verbosity`、上下文窗口等全局元数据也不写。

由于 Codex 没有逐模型的显示名字段，vibemate 界面里的 `(vibemate-<Provider>) <model ID>` 显示名不会出现在 Codex 中；`vibemate-` 前缀只体现在 provider 表名和 `name` 上。

### 可写入的模型

只有 Responses 可用的模型能被设为默认：

| 服务商       | 规则                                                                        |
| ------------ | --------------------------------------------------------------------------- |
| DeepSeek     | 首批只开放 `deepseek-flash` 与 `deepseek-v4-pro`                            |
| Command Code | 模型的 `supported_endpoints` 必须显式包含 `/responses`；Claude 系模型不可用 |
| OpenRouter   | 允许，除非模型的端点列表明确不含 Responses                                  |

其它情况返回 `model_unsupported`。设为默认时必须选定一个模型。

### 设为 Codex 默认

Agent 列表的「写入」对话框里，表单「添加 Provider」带一个可选的「设为 Codex 默认」复选框。provider 表尚未写入或中央 Provider 已更新时先生成 `deploy` 步骤，勾选后再生成 `set_default` 步骤，界面按「第 n 步，共 m 步」逐步审阅确认。Agent 详情的 History 段列出受管 provider，提供撤销与清除默认，以及变更历史。`set_default` 要求该 provider 表已经写入（否则 `not_deployed`），写顶层：

- `model = "<模型 ID>"`
- `model_provider = "vibemate-<slug>"`
- `model_reasoning_effort`：只有中央模型声明了默认推理档位时才随「设为默认」写入（`max`/`xhigh`/`high`/`medium`/`low`/`minimal`/`none`）；没有声明时不碰用户原有值，既不删除也不改写。归属里记录本次是否写了档位（`writes_effort`，schema v38）。若被替换的上一个 vibemate 默认写过档位、而新模型没有声明，则把档位还原为 vibemate 首次写入前的值。

首次设为默认时，vibemate 记录写入前的 `model`/`model_provider`/`model_reasoning_effort`（`previous`）。预览提示只有 `plaintext_credential`、`profile_overrides_default` 与 `release_only` 三种。之后：

- `clear_default` 只恢复 vibemate 写过的键：`model`、`model_provider`，以及仅在 `writes_effort` 时的 `model_reasoning_effort`；前提是这些键仍与 vibemate 写入的完全一致。vibemate 没写过的档位不参与比对，也不会被恢复或删除。
- 若用户已在 Codex 里用 `/model` 或手工改过、删掉了默认，`clear_default` 只释放归属（`release_only` 提示），不改文件，以免覆盖用户的选择或把已删除的值写回。
- 默认值不可读时拒绝。
- 某个 provider 仍是默认时，撤销该 provider（`revoke`）返回 `default_in_use`，需先清除默认。

文件里存在顶层 `profile` 时，预览给出 `profile_overrides_default` 提示，状态里标 `profileActive`；vibemate 从不修改 profile，也不替用户修复。

### Agent 列表摘要

Agent 列表读取实时的顶层 `model_provider` 与 `model`。只有默认归属仍为 `written` 且它指向的 provider 表也仍为 `written` 时，才显示 `Provider · Model`；默认被改动、缺失或没有默认时显示「自有配置」；文件不可读时显示「暂无配置摘要」。

## MCP 配置

写入同一个 `$CODEX_HOME/config.toml` 的 `[mcp_servers.<name>]`。Codex 按字段推断传输方式，没有 `type` 键，也没有 SSE。

| 传输            | 写入的键                                   | 说明                                                                                                            |
| --------------- | ------------------------------------------ | --------------------------------------------------------------------------------------------------------------- |
| stdio           | `command`、`args`、`env`、`cwd`、`enabled` | `cwd` 只接受绝对路径；相对路径和 `~/` 返回 `path_semantics_unverified`                                          |
| Streamable HTTP | `url`、`http_headers`、`enabled`           | 中央定义里的 `Authorization` 作为普通 header 写入 `http_headers.Authorization`（Codex 拒绝内联 `bearer_token`） |

- 不写 `bearer_token`、`bearer_token_env_var`、`env_vars`、`env_http_headers`、`http_headers_helper`、`auth`/`oauth`、`type`，也不写超时或工具过滤字段。
- 含 `${` 的值返回 `dynamic_value_unsupported`。env 与 header 的值只在确认应用时由 Rust 从 OS 凭据库读取，以明文写入文件；预览和历史只显示占位符。
- 预览、历史摘要与历史全文还会遮罩公开字段里夹带的凭据：URL 类字段的敏感查询参数（如 `api_key`、`token`、`access_token`、`key`、`sig`、`signature`、`X-Amz-Credential`）与 `user:password@` 中的密码，以及 `args` 里 `--api-key`/`--token` 等凭据类参数的值、`Bearer <值>`、`Authorization: …` 之类的 header 参数和凭据类 `NAME=value`。provider 表的 `base_url` 同样遮罩。写入文件的内容不受影响。
- 服务名必须匹配 `[A-Za-z0-9_\-:@/.]+` 且不超过 64 个字符，否则返回 `unsupported_server_name`；名称不做规范化，保证 `codex mcp` 能管理同一条目。
- 动作：部署/更新、启用、停用（直接映射 `enabled`）、撤销，以及从历史恢复**该条目**（可能带回旧明文，确认前提示）。
- 无归属的同名条目返回 `name_conflict`，不接管。
- 条目摘要按语义计算：内联表与点分表、省略 `enabled = true` 都视为未变化。条目里出现 vibemate 不管理的键（例如 `enabled_tools`）时列为 `unmanagedKeys` 并视为外部修改；`enabled` 不是布尔值时同样视为 `modified`。
- 替换已有条目时保留该表原有的前置注释和在文件中的位置。`mcp_servers = { ... }` 整体写成内联表时，部署、启用、停用与恢复在预览阶段就返回 `invalid_config`，不生成审批；撤销仍能从内联父表里删除条目，不会出现「报告成功但条目仍在」。
- 管理入口在 MCP 模块的详情页 Codex 行；Agent 详情的 MCP 段对 Codex 只读。仍有注册或未完成变更时，中央定义删除返回 `mcp_in_use`。

## Skill 配置

目标为 `$CODEX_HOME/skills/<name>/`，根目录与模型配置相同。Codex 源码把这个位置标为「已弃用但为兼容保留」，仍会加载；官方推荐的 `~/.agents/skills` 是跨 Agent 共享目录，vibemate 不写入，以保持按 Agent 部署的语义。项目内 `.agents/skills` 同样不写。

- 优先 symlink，不可用或用户在「高级」里选择「始终复制文件」时用 copy-sync。Codex 会跟随符号链接读取 `SKILL.md`。
- 所有权记在 `skill_deployment`（`agent_id = codex`，schema v36）。
- `~/.agents/skills/<name>` 已存在时，预览返回 `sharedDuplicatePath` 并提示：Codex 会同时加载两个目录、同名 Skill 分别列出、不合并。这只是警告，不阻止注入。
- `SKILL.md` 的 `name`（≤64）与 `description`（非空）在导入中央库时已校验，Codex 部署不另做检查。
- 注入复制、重新同步暂存/替换、库更新后的刷新与现场 symlink 探测的临时条目都在真实 `skills/` 目录同级的 `.vibemate-staging/` 中（通常是 `$CODEX_HOME/.vibemate-staging`），不在 `skills/` 内，Codex 扫描不到。Unix 上新建的 staging 目录为 0700；重新同步备份在 `<应用数据>/skill-deploy-backups/codex/<skill ID>/<ms>-<op>/`，各级目录 0700、文件 0600。
- 注入、移除、重新同步与恢复的通用流程，以及启动清理、单实例锁、部署锁与 `read_failed` 的语义，见 [skills.md](../skills.md#deploy-to-agents)。

## 认证与凭据

首版把 Provider 密钥以明文写入 provider 表的 `experimental_bearer_token`，与 DeepSeek 官方 Codex 指南的写法一致，也与 Grok `api_key`、Claude Code env 的「按设计明文」先例一致。

- 预览遮罩密钥，并给出 `plaintext_credential` 提示。
- 密钥只在确认应用时、持凭据锁的情况下读取一次；错误不带任何诊断内容。
- 写入后 `config.toml` 为 0600；私有备份和版本副本也是 0600（目录 0700）。
- `experimental_bearer_token` 是实验字段，Codex 源码注释也建议改用 `env_key`；后续版本可能改名，需在版本复核时跟踪。
- 改用 `auth.command` 从 OS 凭据库读取密钥（文件里不留明文）是后续增强，当前未实现。

应用成功后结果带 `restart_required`：需要开启新的 Codex 会话才能生效。

## 归属与撞名

归属都在 SQLite（schema v36/v37）：`codex_provider_binding`、`codex_default_binding`、`codex_settings_change`（模型设置的持久意图，v37 增加 `model_id`），以及 `codex_mcp_deployment`、`codex_mcp_change`。每个配置根同一时刻只允许一个未完成变更；模型设置与 MCP 各有独立 journal，任一未完成时另一方的写入也被阻止。

以下情况一律拒绝，不覆盖：

- provider 表名属于保留 ID，或同名表没有 vibemate 归属：`name_conflict`。
- MCP 同名条目没有归属：`name_conflict`。
- `skills/<name>` 已被 vibemate 未管理的内容占用：界面显示「已被占用」。

Provider 删除在仍有 Codex provider 归属、默认归属或未完成变更时被阻止；模型删除在 Codex 默认仍指向它时被阻止。两者的影响列表都单列「Codex 绑定」，此时删除按钮不可用。引用列表里的 Codex 行不能直接清除，只能在 Codex 设置里撤销或清除默认，或通过中断恢复释放。

预览与应用的修订约定：`deploy`/`set_default` 绑定中央 Provider 的当前修订；`revoke`/`clear_default` 绑定归属里记录的修订，因此中央 Provider 已删除后仍可撤销。

## 用户手动修改后的行为

| 用户的改动                                                                                  | vibemate 的处理                                                                                                                                                          |
| ------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| 预览后、确认前改了 `config.toml`（含 Codex `/model`、`codex mcp`）                          | 摘要不符，`plan_stale`，需要重新预览                                                                                                                                     |
| 改了受管 provider 表的任意键（含添加 `env_key`/`auth`）                                     | 按语义摘要判为外部修改：`ownership_conflict`，界面显示「自有配置」，不覆盖                                                                                               |
| 删除了受管 provider 表                                                                      | 视为缺失；再次添加可在同一表名下重建                                                                                                                                     |
| 用 `/model` 改了默认模型                                                                    | 默认归属变为 modified；列表显示「自有配置」；`clear_default` 只释放归属                                                                                                  |
| 改了受管 MCP 条目（含手动 `enabled = false`、增加 `enabled_tools`、`enabled` 改成非布尔值） | 外部修改，该行锁定，Rust 对所有动作返回 `ownership_conflict`（`enabled` 无法解析时为 `invalid_config`）；手动停用显示「在 Codex 中手动停用」。用户改回原值后才能继续管理 |
| 修改了 copy 方式放置的 Skill 文件，或把受管链接改指别处                                     | 显示「需要同步」；重新同步前对话框说明会覆盖改动并给出备份位置，确认后先备份再用库版本替换；移除则保留文件、只停止管理                                                   |

## 中断与恢复

确认应用前先写 `prepared` 意图；应用中断后，Overview 的 RecoveryHub 和 Codex 设置/MCP 面板显示中断核对入口。核对只比对写入前/预期摘要并修复归属记录，**从不重放写入**。模型设置的历史按时间倒序只读展示已完成的变更；MCP 历史可以恢复所选条目。

## 已定决策

维护者于 2026-10-29 批准的 10 条决定（见 [todo P41](../plans/todo.md)），逐条对应当前工作区代码：

| #   | 决定                                                                                  | 实际行为                                                                                                                                                                                                                                                                                 | 代码依据                                                                                                                                                                                                                           |
| --- | ------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 1   | P41 作为独立任务                                                                      | Codex 有自己的模型设置适配器、MCP 适配器与 SQLite 表，不挂在其它 Agent 的写入路径下；Skill 复用共享部署模块，`agent_id = codex`                                                                                                                                                          | `src-tauri/src/agents/codex.rs`（模块注释 `Codex write adapter (P41)`）、`src-tauri/src/mcp/codex_deployment/`、`codex_provider_binding`/`codex_default_binding`/`codex_settings_change`/`codex_mcp_deployment`/`codex_mcp_change` |
| 2   | 密钥首版以明文写入 `experimental_bearer_token`，文件 0600；钥匙串 `auth.command` 后续 | 密钥只在确认应用时从 OS 凭据库读取，以明文写入 provider 表的 `experimental_bearer_token`；预览遮罩并给出 `plaintext_credential` 提示；经共享私有写入器写成 0600。`auth.command` 未实现                                                                                                   | `agents/codex/plan.rs`（`TOKEN_FIELD = "experimental_bearer_token"`）、`agents/codex/service.rs`（`apply_private_files_locked`）；代码中没有 `auth.command`                                                                        |
| 3   | 只写服务商表；可选「设为默认」写顶层 `model` + `model_provider`                       | 只写 `[model_providers.vibemate-<slug>]`（`name`、`base_url`、`wire_api = "responses"`、密钥，OpenRouter 另加 `supports_websockets = false`）；勾选「设为 Codex 默认」时另生成 `set_default` 步骤写顶层 `model` 与 `model_provider`；从不写 `profile`、`[profiles.*]`、`env_key`、`auth` | `agents/codex/plan.rs`（provider 表字段、`write_defaults`）、`agents/codex.rs` 模块注释                                                                                                                                            |
| 4   | 不写 `model_catalog_json`                                                             | 任何动作都不写该键，因此 Codex 里可能缺少模型元数据（见已知限制）                                                                                                                                                                                                                        | `agents/codex.rs` 模块注释；写入路径中没有该键                                                                                                                                                                                     |
| 5   | 推理档位只随「设为默认」写，不删用户原值                                              | 只有中央模型声明默认档位时才写 `model_reasoning_effort`，归属记 `writes_effort`（schema v38）；未声明时不碰用户原值；`clear_default` 只恢复 vibemate 写过的键                                                                                                                            | `agents/codex/plan.rs`（`reasoning_effort`、`owned_defaults`、`write_defaults`）、`agents/codex/types.rs`（`DefaultOwner.writes_effort`）                                                                                          |
| 6   | Skill 放 `$CODEX_HOME/skills`                                                         | 目标为 `$CODEX_HOME/skills/<name>/`（`CODEX_HOME` 非空时使用该目录，须已存在；未设置或为空时用 `~/.codex`）；不写 `~/.agents/skills`，同名时只给 `sharedDuplicatePath` 警告                                                                                                              | `skills/deploy/mod.rs`（`SkillDeployAgent::Codex => codex_config_root`、`shared_duplicate`）、`agents/discovery.rs`（`codex_config_root`）                                                                                         |
| 7   | HTTP MCP 的 Bearer 写 `http_headers.Authorization`                                    | 中央 `Authorization` header 作为普通 header 写入 `http_headers`；从不写 `bearer_token`、`bearer_token_env_var`、`env_http_headers`                                                                                                                                                       | `mcp/codex_deployment/plan.rs`（HTTP 分支写 `url`/`http_headers`/`enabled`）                                                                                                                                                       |
| 8   | `config.toml` 为软链接时拒绝写入                                                      | 模型设置与 MCP 读取目标时发现符号链接即返回 `target_unavailable`，不跟随、不写                                                                                                                                                                                                           | `agents/codex/plan.rs` `read`、`mcp/codex_deployment/plan.rs` 读取函数（`if target.symlink { return Err(Error::TargetUnavailable) }`）                                                                                             |
| 9   | DeepSeek 首批 `deepseek-flash`、`deepseek-v4-pro`                                     | DeepSeek 只有这两个模型 ID 可写入，其它返回 `model_unsupported`                                                                                                                                                                                                                          | `agents/codex/plan.rs`（`DEEPSEEK_RESPONSES_MODELS`）                                                                                                                                                                              |
| 10  | 不做实际联调，维护者自测                                                              | 这是流程决定，代码里没有对应实现；自动化检查只用单元测试和前端测试，不连接真实 Provider 或 Codex。todo 中维护者在 `codex-cli 0.165.0` 上的验收项未勾选                                                                                                                                   | 无代码依据；见 [todo P41](../plans/todo.md) 的维护者验收项                                                                                                                                                                         |

## 已知限制

- 未做真实联调：Command Code、DeepSeek、OpenRouter 与 Codex 的工具调用、流式事件和无状态（`store`/`previous_response_id`）兼容性均未由代理实测，由维护者验收。
- Command Code 与 OpenRouter 模型在 Codex 中可能缺少模型元数据（例如出现 fallback metadata 提示），因为 vibemate 不写 `model_catalog_json`；使用 bearer token 时 OpenRouter 是否仍会刷新模型目录未验证。
- `$CODEX_HOME/skills` 已被 Codex 标记为弃用；若将来移除需迁移。
- `mcp_servers = { ... }` 整体内联时不会被改写成标准表，需要用户自行改成 `[mcp_servers.<name>]` 形式后再部署。
- 凭据遮罩按键名和参数形状识别：名字不像凭据的参数（例如 `-p <值>`）或自定义位置参数里的密钥不会被识别。
- `$CODEX_HOME/skills` 是指向其它位置（例如 dotfiles 仓库）的链接时，`.vibemate-staging` 会出现在链接目标的同级目录；异常退出后的残留会在下次启动或下次注入时清理。
- Windows/Linux 原生路径与权限行为未验证。
