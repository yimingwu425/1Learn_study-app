# 识别 Agent 接入协议 v1

基础接口保持供应商中立的本机 pull 协议，不自动调用模型或生成密钥。现已另有可选的应用专属 SiWC/Luna 适配，真实授权尚待用户本人完成；完整合同见 agent-intake.md，授权和按页预览流程见 luna-subscription-assessment.md。

## 顺序

1. 上传PDF到 POST `/api/imports`，得到 recognition_job；也可 POST `/api/recognition/jobs`，body `{import_ids:[...]}` 将同一组原件组成任务。相同组合幂等，不重复创建。
2. GET `/api/recognition/jobs/{id}/input` 获取原文件URL、哈希、页数、文件名提示、JSON Schema位置。
3. POST `.../claim`：`{agent_run_id, role:"extractor", agent_name}`。任务从awaiting_agent变为recognizing。
4. POST `.../proposal`：schema规定的完整结果。含stem_text/answer_text、各自来源区域和图表。schema合格后进入awaiting_verification，并计算每题的精确proposal_sha256。
5. 独立Agent运行 POST `.../claim`，role=verifier、不同run_id。再对照原PDF核查同一摘要的内容。该步骤由Agent做，不是要求用户逐题填写。
6. POST `.../verification`：`{agent_run_id,source_verifications:[{source_id,role,verified_exam_key}],verified_questions:[{exam_key,question_no,proposal_sha256,checks}],visual_verifications:[]}`。
7. 程序来源检查通过的项目自动写入同一题库，未解决项目返回exceptions；不覆盖已有题目。

复核checks必须包含 boundaries_complete、answer_pair_correct、figures_complete、subparts_complete、transcription_complete。检查记录绑定完整结果摘要，修改正文后必须重新复核。

## 视觉证据

当前使用 Rust/PdfCraft，render_version 为 pdfcraft-region-v2。旧版图像证据保留但需重新渲染/独立复核；不混用引擎哈希。

对于文本层乱码/扫描页，POST `.../region`，body `{source_id,page,bbox}`，获取原图URL、image_sha256、render_version。生成图像只存本机，未发给模型。

visual_verifications使用agent-intake.md定义的结构。后端只接收当前已领取verifier run的记录，并现场重渲染比对原图和转写摘要；提取Agent不能在proposal中自塞“验证成功”。版权删减不能通过复核强行补造。

## 状态与UI

awaiting_agent → recognizing → awaiting_verification → verifying → completed / needs_attention。

GET `/api/recognition/jobs` 返回任务卡片数据。GET `/api/recognition/status` 的 provider_configured 取决于本应用当前账户的实际订阅授权；recognition_model_connected 只有在该账户完成过有效请求后才为真，ai_accuracy 仍为 null。active_registration 是不含令牌的注册标识；Luna 模型查询、图片测试和预览请求必须传 registration_id，账户已切换时拒绝发送，前端也会清除旧的发送确认。没有识别模型运行时不展示“正在AI识别”。

## 可选开发适配器（不属于软件运行依赖）

`python3 tools/agent_import_client.py --job ID prepare --dest /tmp/study-agent-input` 下载任务原件到指定本地目录并核对哈希，生成job-input.json，不调用模型。

其claim/submit/verify子命令对应上述接口。只支持localhost/127.0.0.1，非浏览器写请求带X-Study-Client标识。run_id用于任务归属和独立步骤约束，不是认证密钥；目前基于同一OS用户的本机信任边界。若未来接远程Agent，须另行设计经授权的身份认证和传输，不应把当前本机接口公开到网络。

## 已验证与待做

使用合成PDF及模拟Agent结果跑通提交→独立复核→自动入库，包括不同run限制、防重复完成、图片区域和来源摘要。此测试证明接口和规则执行，不证明任何AI的识别准确率。

4份真实样本已通过上传、保留原件、待Agent状态及无模型不发布检查。预检会标明文本层乱码或缺失、needs_visual，但不会调用OCR。真实样本覆盖乱码文本层、旋转MS、共享材料和版权删减；尚未运行真实识别模型。待真实runner接入后补充任务租约、自动重试、按题修订及例外恢复策略。当前已完成任务不得直接覆写，避免丢失来源和已入库内容。
