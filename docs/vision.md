# SyncMind: The Vision & Architecture Blueprint

> "不要让 AI 等待你的提问，让 AI 活在你的上下文里。"

## 1. The Problem (我们为什么要做这个？)

在当前的 AI 研发流中，大模型（如 Claude, GPT）虽然拥有海量的通用世界知识，但它们**严重缺乏“你”的本地知识**。 作为一个全栈开发者或研究人员，你每天都在产生高价值的数字资产（代码片段、架构图草稿、阅读过的论文、过去的填坑笔记）。但当你使用 AI 代码助手时，这些资产如同“信息孤岛”。 同时，由于这些数据通常包含商业机密或未发表的研究，**将它们传给中心化云端（如 Notion AI 或 ChatGPT 记忆库）存在极大的隐私风险。**

## 2. Product Vision (产品愿景)

**SyncMind 不是一个笔记软件，也不是一个聊天的 UI。** 它是一个**隐私优先、完全离线的“主动式本地上下文引擎” (Proactive Local Context Engine)。**

它的终极形态是：

1. **隐形运作：** 像操作系统的守护进程一样，静默地为你全本地的碎片化资产建立语义索引。
2. **主动响应：** 当你在 IDE 中敲击代码，或在终端使用 Agent 时，它能通过 MCP 协议自动将历史相关的代码或笔记作为 Context 注入，无需你手动搜索。
3. **无处不在：** 移动端极速捕获灵感，桌面端提供全键盘唤醒的极客命令台，Web 端提供上帝视角的知识图谱可视化。

## 3. Core Architecture (全局架构蓝图)

SyncMind 采用 **"Headless-First (无头优先)"** 的架构模式，核心计算与 UI 展现彻底解耦。

### 3.1 The Brain (核心计算引擎层)

- **定位：** 整个系统的心脏，纯本地离线运行。
- **技术栈：** Rust (高并发、低内存占用)。
- **职责：**
  - **Data Pipeline:** 监听本地文件树变化 -> 提取文本 -> 语义分块 (Chunking) -> 调用本地大模型 (Ollama) 生成高维向量。
  - **Storage:** 维护关系型元数据 (SQLite) 和本地向量库 (LanceDB / Local Qdrant)。
  - **MCP Provider:** 向外部（如 Claude Code、Cursor）提供标准的 Model Context Protocol 接口。

### 3.2 The Spine (云端网关层 - 进阶阶段)

- **定位：** 跨端数据同步与私有化部署的中枢。
- **技术栈：** Go (Go-Zero / Hertz) + Redis + PostgreSQL。
- **职责：** 处理移动端传来的语音/图像素材，进行异步分析，并与桌面端引擎进行端到端的加密数据同步 (E2EE)。

### 3.3 The Nerve Endings (多端触角)

所有 UI 客户端都被视为核心引擎的“消费者 (Consumers)”。

1. **Desktop (Tauri + SolidJS/React):**
   - **形态：** 类似 Raycast 的全局搜索台 (Command Palette)。
   - **特性：** IPC 直接调用 Rust 核心能力，毫秒级响应，几乎零内存负担。
2. **Web Dashboard (Next.js / Vue):**
   - **形态：** 赛博朋克风的管理后台。
   - **特性：** 展示 3D 知识图谱关联，提供 RAG 实验室（调参面板）。
3. **Mobile App (React Native / Expo):**
   - **形态：** 极简灵感捕获器。
   - **特性：** 打开即刻语音录入或文档扫描，一键流转回本地数字大脑。

## 4. Engineering Directives (工程第一性原理)

在向 codebase 提交任何代码之前，必须遵守以下四条铁律：

1. **Privacy is Absolute (绝对隐私)：** 所有用户的原始文本、代码和生成的向量数据，必须 100% 留存在用户本地物理机上。任何核心 RAG 检索逻辑都不允许依赖公网 API。
2. **Docs as Code & Spec-Driven (规范驱动开发)：** API 文档、需求规格不应该存在于代码仓库之外。所有的功能开发必须先在 `docs/prd/` 下定义好输入输出 Schema，AI Agent 和人类开发者必须基于 Spec 进行开发。
3. **Frugal Resource Usage (对系统资源极其克制)：** 作为常驻后台的程序，不允许为了开发效率而引入臃肿的运行时（如 Electron）。核心进程空闲时内存占用必须 < 100MB，这也是核心层强制采用 Rust 的原因。
4. **Decouple Data from UI (数据与视图解耦)：** 永远优先实现 API 或 MCP 接口。如果脱离了 UI 面板程序就无法运行，说明架构设计是失败的。

## 5. Development Roadmap (演进路线)

- **Phase 1: Headless MCP Core (当前聚焦)**
  - 目标：跑通从 Markdown/Code 到向量库的数据管道，暴露 MCP `search_knowledge` 协议，能被 Claude Code 在终端直接调用。

- **Phase 2: The Command Palette (桌面端赋能)**
  - 目标：套上 Tauri 壳，实现桌面端快捷键唤醒的秒级语义搜索面板。

- **Phase 3: The Omnipresent Brain (跨端与图谱化)**
  - 目标：搭建 Go 网关，实现移动端捕获 -> 云端中转 -> 桌面端消费的闭环，并上线 Web 端知识图谱。

  - # SyncMind: The Vision & Architecture Blueprint

    > "不要让 AI 等待你的提问，让 AI 活在你的上下文里。"

    ## 1. The Problem (我们为什么要做这个？)

    在当前的 AI 研发流中，大模型（如 Claude, GPT）虽然拥有海量的通用世界知识，但它们**严重缺乏“你”的本地知识**。 作为一个全栈开发者或研究人员，你每天都在产生高价值的数字资产（代码片段、架构图草稿、阅读过的论文、过去的填坑笔记）。但当你使用 AI 代码助手时，这些资产如同“信息孤岛”。 同时，由于这些数据通常包含商业机密或未发表的研究，**将它们传给中心化云端（如 Notion AI 或 ChatGPT 记忆库）存在极大的隐私风险。**

    ## 2. Product Vision (产品愿景)

    **SyncMind 不是一个笔记软件，也不是一个聊天的 UI。** 它是一个**隐私优先、完全离线的“主动式本地上下文引擎” (Proactive Local Context Engine)。**

    它的终极形态是：
    1. **隐形运作：** 像操作系统的守护进程一样，静默地为你全本地的碎片化资产建立语义索引。
    2. **主动响应：** 当你在 IDE 中敲击代码，或在终端使用 Agent 时，它能通过 MCP 协议自动将历史相关的代码或笔记作为 Context 注入，无需你手动搜索。
    3. **无处不在：** 移动端极速捕获灵感，桌面端提供全键盘唤醒的极客命令台，Web 端提供上帝视角的知识图谱可视化。

    ## 3. Core Architecture (全局架构蓝图)

    SyncMind 采用 **"Headless-First (无头优先)"** 的架构模式，核心计算与 UI 展现彻底解耦。

    ### 3.1 The Brain (核心计算引擎层)
    - **定位：** 整个系统的心脏，纯本地离线运行。
    - **技术栈：** Rust (高并发、低内存占用)。
    - **职责：**
      - **Data Pipeline:** 监听本地文件树变化 -> 提取文本 -> 语义分块 (Chunking) -> 调用本地大模型 (Ollama) 生成高维向量。
      - **Storage:** 维护关系型元数据 (SQLite) 和本地向量库 (LanceDB / Local Qdrant)。
      - **MCP Provider:** 向外部（如 Claude Code、Cursor）提供标准的 Model Context Protocol 接口。

    ### 3.2 The Spine (云端网关层 - 进阶阶段)
    - **定位：** 跨端数据同步与私有化部署的中枢。
    - **技术栈：** Go (Go-Zero / Hertz) + Redis + PostgreSQL。
    - **职责：** 处理移动端传来的语音/图像素材，进行异步分析，并与桌面端引擎进行端到端的加密数据同步 (E2EE)。

    ### 3.3 The Nerve Endings (多端触角)

    所有 UI 客户端都被视为核心引擎的“消费者 (Consumers)”。
    1. **Desktop (Tauri + SolidJS/React):**
       - **形态：** 类似 Raycast 的全局搜索台 (Command Palette)。
       - **特性：** IPC 直接调用 Rust 核心能力，毫秒级响应，几乎零内存负担。
    2. **Web Dashboard (Next.js / Vue):**
       - **形态：** 赛博朋克风的管理后台。
       - **特性：** 展示 3D 知识图谱关联，提供 RAG 实验室（调参面板）。
    3. **Mobile App (React Native / Expo):**
       - **形态：** 极简灵感捕获器。
       - **特性：** 打开即刻语音录入或文档扫描，一键流转回本地数字大脑。

    ## 4. Engineering Directives (工程第一性原理)

    在向 codebase 提交任何代码之前，必须遵守以下四条铁律：
    1. **Privacy is Absolute (绝对隐私)：** 所有用户的原始文本、代码和生成的向量数据，必须 100% 留存在用户本地物理机上。任何核心 RAG 检索逻辑都不允许依赖公网 API。
    2. **Docs as Code & Spec-Driven (规范驱动开发)：** API 文档、需求规格不应该存在于代码仓库之外。所有的功能开发必须先在 `docs/prd/` 下定义好输入输出 Schema，AI Agent 和人类开发者必须基于 Spec 进行开发。
    3. **Frugal Resource Usage (对系统资源极其克制)：** 作为常驻后台的程序，不允许为了开发效率而引入臃肿的运行时（如 Electron）。核心进程空闲时内存占用必须 < 100MB，这也是核心层强制采用 Rust 的原因。
    4. **Decouple Data from UI (数据与视图解耦)：** 永远优先实现 API 或 MCP 接口。如果脱离了 UI 面板程序就无法运行，说明架构设计是失败的。

    ## 5. Development Roadmap (演进路线)
    - **Phase 1: Headless MCP Core**
      - 目标：跑通从 Markdown/Code 到向量库的数据管道，暴露 MCP `search_knowledge` 协议，能被 Claude Code 在终端直接调用。
    - **Phase 2: The Command Palette (桌面端赋能)**
      - 目标：套上 Tauri 壳，实现桌面端快捷键唤醒的秒级语义搜索面板。
    - **Phase 3: The Omnipresent Brain (跨端与图谱化)**
      - 目标：搭建 Go 网关，实现移动端捕获 -> 云端中转 -> 桌面端消费的闭环，并上线 Web 端知识图谱。
