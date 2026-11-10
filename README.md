# 真题学习 · 本地开发版

Tauri + React 界面，Rust 业务核心与 SQLite。使用成熟 PdfCraft 引擎处理原卷，独立目录开发，不覆盖旧「试卷清洗」项目。

## 在这台 Mac 启动

双击 `启动桌面学习软件.command`；网页入口为 `启动学习软件.command`，地址 <http://127.0.0.1:4317>。新版运行不需要 Python，也不启动旧版 Node 服务。

数据位于 `data/`，可用 `STUDY_DATA_DIR` 指定独立目录。原旧版完整工具仍保留在 `legacy-agent/`，需要时单独运行 `启动旧版兼容工具.command`，入口 <http://127.0.0.1:4318>。旧工具保留历史 Python 实现，不是新版运行依赖。

本地源码已比云端版本 5 更新，不能用旧云端包覆盖。迁移盘点、原数据保护与实际验证见 [本地功能对照](本地迁移与功能对照.txt)。本轮修改前的可恢复备份位置记录在 `verification/native-pdf-backup.json`。

## 云端审查与版本管理

源码已建立独立私有 GitHub 仓库；私人 `data/`、凭据、旧快照与构建产物不提交。2026-11-10 云端审查、修复与验证边界见 [审查报告](docs/cloud-review-2026-11-10.md)。本机现有学习数据库和源码未被云端修复覆盖。

## 当前能力

- 已迁入物理/经济合计 6,981 条原题、542 份 QP/MS 原卷及原标签；保留向量快照。
- 组合高级筛选、原算法中英语义检索、持久化试卷篮、按权重组卷、选答规则、移除/换题/撤销与学生卷/评分卷导出。
- 章节主列表按考纲自然顺序，右侧独立排名；276 个考点对照正式考纲显示中英文及原文页码。
- 定义库：物理 33 个定义/71 次已确认考查，经济 28 个/41 次；支持章节、年份、双语背诵版、逐字评分来源及去重统计。属于已核对样本，未声称全库定义均已识别；见 [来源与口径](docs/definition-library.md)。
- 原 8 项 Agent HTTP 工具全部由 Rust 实现，支持 `{tool,arguments}` 和 `{name,args}`。
- 上传、预检、任务领取、提交、独立复核、异常队列及受校验入库；可选的应用专属 SiWC 登录、Luna 图片连接测试和按页识别预览已实现，真实账户尚未授权或验证。
- 笔记、自评、继续学习与小蓝互动；跨桌面端口重启仍保留。

题库卡片直接显示 192 DPI 原卷截图，按可见区域延迟加载；唯一题号定位时显示该题区域，位置不确定时保留完整原页并提示核对，可展开翻页查看共用材料。不把未审核的整页碎片冒充完整题干。全库题干/图表/页码尚未逐题准确拆分与复核；已有数量不代表全部识别准确。

题面展示的备份与核验见 `verification/question-preview-backup.json` 和 `verification/question-preview-final.json`。订阅接入实现、用户授权步骤及账户实测缺项见 [Luna 接入说明](docs/luna-subscription-assessment.md)，本轮备份/验收见 `verification/luna-siwc-backup.json` 和 `verification/luna-siwc-final.json`。定义库最终来源、39项界面/27项Rust测试及数据保护汇总见 `verification/definition-final.json`。尚未授权或实测接通真实模型。

## PDF 运行依赖

详见 [原生 PDF 运行说明](docs/native-pdf-runtime.md)。解析、渲染、红删、裁页、抽页、矢量拼装和展平交给 PdfCraft；Rust 实现题目分组、边界策略、版面规划、ZIP、JSON Schema 与来源门槛。生成标题和中文笔记使用成熟 printpdf 与 OFL 静态字体。

原卷文字层不足时，使用已安装的 Tesseract 仅定位导出边界，内容仍来自原矢量页。这不是题目 AI 识别模型，也不自动发布转写。无法定位则明确失败，不输出猜测裁切。

Python 只保留为开发测试、一次性迁移/字体资产准备、独立比对和旧版工具的依赖。新版启动器和 Rust 服务没有 Python 子进程调用。

## 开发与验证

已安装项目本地 Rust 工具链、Node/npm、官方 PdfCraft CLI 与本机 Tesseract。首次构建需这些开发依赖；启动不自动安装系统组件或修改安全设置。

```sh
./scripts/run-local.sh
./scripts/desktop-dev.sh
```

Rust 测试包含来源篡改、角色/页码/坐标、视觉证据哈希、旧渲染版本、自我复核和旋转页门槛。HTTP 回归使用隔离临时库；SiWC/模型链路使用本机合成提供方，真实账户尚未授权。开发用独立 PDF 检查器仍用 Python/PyMuPDF，比对结果不引入软件运行依赖。

```sh
./scripts/test-all.sh
```

真实卷导出与本机窗口证据位于 `verification/`。`Study Local.app` 仍是本地开发产物，签名、公证和正式安装分发尚未完成。原生打包脚本仅分阶段准备资源，不意味着已交付可分发安装包。
