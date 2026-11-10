# Luna 与 ChatGPT 订阅接入（2026-11-09）

应用自己的 SiWC 登录、账户管理、Luna 连接测试和分批真卷识别预览已经实现；真实账户仍未授权。不能把模拟测试、公共模型页或 Codex 登录状态当作账户 Luna 权限证明。

## 适用条件与官方依据

[官方概览](https://developers.openai.com/siwc/token-sharing-open-source) 明确覆盖开源和本地托管应用。目前本项目是只监听本机 loopback 的 Tauri/Rust 应用，因此本机使用存在文档支持的接入路径。付费或远程托管应用另有申请路径；本项目尚未确立公开发布许可证，不因此擅自公开源码或试卷。账户、工作区政策和实际额度必须在授权后查询并调用验证。

[注册与登录](https://developers.openai.com/siwc/token-sharing-open-source/sign-in) 的动态注册无需 client secret 或 partner API Key。本应用用自己的真实名称“真题学习”、稳定 UUID host ID、一次性 state/nonce、PKCE S256 和 `127.0.0.1` 回调；授权码只在 Rust 后端交换。ID token 校验官方 JWKS 的 RS256 签名、issuer、audience、expiry、nonce 和主体。返回账户的 client ID/身份不匹配时拒绝替换原凭据；拒绝、过期和重放回调不交换凭据。`invalid_grant` 后保留已颁发的注册 ID，用新的授权流程恢复。

[账户与会话](https://developers.openai.com/siwc/token-sharing-open-source/profiles-and-sessions)：每个注册的账户身份、client ID 和令牌分开保存；相同邮箱的不同工作区可区分。应用目录为 `data/siwc/`，权限 0700，凭据文件 0600，原子写入；跨进程文件锁防止桌面/网页实例同时刷新同一 rotating token。刷新沿用实际 scope；退出时尝试官方 revocation endpoint，并清除本机访问、续期和身份令牌。远程撤销无法确认时明确提示去 ChatGPT 设置断开。保留账户注册映射及 host ID，不复制到另一台设备冒充同一 host。启动/读取状态不会创建该目录或发起登录。

## 用户怎样完成实际授权与验证

1. 打开本地软件的“导入题目”。阅读“让 Luna 帮你整理原卷”的范围说明。
2. 由用户勾选确认，点击 **Continue with ChatGPT**；软件打开系统浏览器，用户本人在 OpenAI 官方页面登录并确认“真题学习”的授权。
3. 请求范围为身份 `openid profile email`，以及订阅使用和续期 `offline_access resource.invoke chatgpt.tokens.use.direct`。授权后，本应用会独立保存可续期登录，供重启与后续请求使用；不获得 ChatGPT 聊天读取权限，不读取 Codex 凭据。不要向聊天发送任何令牌。
4. 回到软件，点击“查询账户模型权限”。这里实际调用同一账户 token 的公开 `/v1/models`，不使用 Codex 缓存目录；没有 `gpt-6-luna` 就停止，不静默换模型。
5. 列表包含 Luna 后，点击“测试 Luna 图片连接”。仅发送应用自己生成的数字“31”测试图片，会使用少量订阅额度；结构化结果正确且收到完整成功事件才标记连接测试完成。这不代表真题识别准确率。
6. 真卷识别另行选择原件页码，每批总计 1–6 页，并明确确认发送这些页到 OpenAI。结果保留为候选预览；可提交至原有协议的独立复核队列，不会单凭一次模型输出直接入库。

## 识别接口与请求限制

[模型与推理](https://developers.openai.com/siwc/token-sharing-open-source/models-and-inference) 要求用本应用授权 token 请求公开 Responses API。已实现同账户即时模型查询、`gpt-6-luna` 图片输入、严格结构化输出及 SSE 终态检查；只有 `response.completed` 可成功。流中额度错误、未完成、拒绝、断流、超量输出均失败，不自动重试消耗额度。跨窗口模型请求锁防止重复并发。

[预览限制](https://developers.openai.com/siwc/token-sharing-open-source/preview-limitations)：请求固定 `store:false`、`stream:true`，上下文放在 input 数组；不使用 max_output_tokens、temperature、previous_response_id、Files 上传或不支持的 hosted tools。PDF 仍在本机由成熟 PdfCraft 渲染；发送的是用户选定页的图片与来源元数据。

严格模型 Schema 根据现有提案协议生成：所有对象禁止额外字段、全部属性必填，去除 uniqueItems 等未依赖的关键词；本机原 Schema 仍执行去重和约束。未能完整识别时可返回空提案和说明。来源 ID、所选页码、边界及原件哈希要通过本机检查。提交提案后仅进入 awaiting_verification，独立 verifier/视觉证据/原文匹配及原有入库门槛继续有效。当前没有实现由 Luna 自动执行独立 verifier 或整卷分批汇总的调度；现有外部 Agent 复核接口保留。

主要接口：GET `/api/siwc/status`；POST `/api/siwc/login/start`、`login/open`、`login/cancel`、`accounts/select`、`logout`、`welcome`、`models`、`test`；GET `/auth/openai`、`/auth/callback`；POST `/api/recognition/jobs/{id}/luna/preview`，GET `/api/recognition/jobs/{id}/luna/runs/{run}`，POST `.../{run}/submit`。

`models`、`test` 的 JSON 请求必须包含状态返回的 `registration_id`；真卷 `luna/preview` 也必须包含此字段。后端在查询模型和发送图片前核对所选账户，未登录返回 401，跨窗口切换后陈旧账户返回 409，不向替换后的账户静默发送已勾选的试卷。界面在账户变化时清空模型列表、旧预览及发送同意，需重新确认。

## 本轮实测与尚缺项

已用仅本机的合成 OAuth/模型服务验证签名、身份、拒绝/重放/过期、scope、PKCE、模型列表、流式失败、刷新竞争、登录恢复及连接测试。还用真实电路题 PDF 的临时副本验证了渲染→模型请求→候选保存→提交独立复核的接口链路；测试提供方输出明确标为合成结果，不用于题库或准确率。

真实 OpenAI 仅只读获取了公开 OIDC discovery 元数据，核对 issuer/JWKS/撤销端点；未发起真实注册、登录或模型推理，未发送试卷，未读取 Codex auth.json/Keychain，未创建实际凭据或持久授权。具体待办是用户本人完成上述官方授权，然后实际查询账户模型并完成测试；真实 Schema 接受情况、额度、Luna 对真卷的准确率均尚未验证。

最终代码验证为 Rust 27 项、前端 39 项通过，包含陈旧账户不能发起模型或试卷请求、账户切换后清除发送同意。实际 Mac 桌面已核对登录范围、默认未选的同意框及禁用登录按钮；末次构建后的桌面复查因 Mac 锁屏待完成，没有尝试解锁。定义库实际浏览器截图及最终来源回归见 `verification/definition-final.json`。
