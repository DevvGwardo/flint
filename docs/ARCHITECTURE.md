# Flint Architecture & Internals

Flint is a high-performance native macOS desktop application for coding agents, engineered in Rust using the [GPUI](https://github.com/zed-industries/zed) GPU-accelerated UI framework. It supports both a native, autonomous agent loop and external agents adhering to the [Agent Client Protocol (ACP)](https://agentclientprotocol.com).

---

## 1. System Overview

Flint divides responsibilities across four specialized crates:

| Crate | Responsibility |
| --- | --- |
| [`crates/flint-app`](../crates/flint-app) | GPUI desktop application, windowing, dockable panel layout, event dispatch, session tabs, and command palette. |
| [`crates/flint-agent`](../crates/flint-agent) | Autonomous agent engine: OpenAI-compatible wire client, tool execution, context budgeting, subagents, and Harness Guard rules. |
| [`crates/flint-acp`](../crates/flint-acp) | ACP client adapter managing child processes for Claude Code, Codex, and Droid over standard I/O JSON-RPC. |
| [`crates/flint-term`](../crates/flint-term) | Integrated pseudo-terminal (PTY) dock, ANSI terminal emulation, scrollback, command execution tabs, and I/O streaming. |

<p align="center">
  <img src="../assets/architecture.jpg" alt="Flint System Architecture" width="100%" />
</p>

<details>
  <summary><b>View Mermaid architecture topology</b></summary>

```mermaid
%%{init: {
  'theme': 'base',
  'themeVariables': {
    'darkMode': true,
    'primaryColor': '#1b1b20',
    'primaryTextColor': '#e7e7ea',
    'primaryBorderColor': '#ff8a3d',
    'lineColor': '#ff8a3d',
    'secondaryColor': '#141418',
    'tertiaryColor': '#0e0e10',
    'mainBkg': '#141418',
    'nodeBorder': '#ff8a3d',
    'clusterBkg': '#141418',
    'clusterBorder': '#222228',
    'defaultLinkColor': '#ff8a3d',
    'titleColor': '#e7e7ea',
    'edgeLabelBackground': '#1b1b20'
  }
}}%%
graph TD
    classDef default fill:#141418,stroke:#222228,stroke-width:1.5px,color:#e7e7ea;
    classDef ember fill:#1b1b20,stroke:#ff8a3d,stroke-width:2px,color:#e7e7ea;
    classDef cyan fill:#141418,stroke:#00e5ff,stroke-width:2px,color:#e7e7ea;

    subgraph UI ["User Interface Layer (crates/flint-app)"]
        Window["GPUI Window & Metal Pipeline"]
        Composer["Composer (Input, Mentions, Slash Commands)"]
        Sidebar["Session Sidebar (Grouping & Management)"]
        Panels["Dockable Panels (Layout Engine)"]
        Changes["Diffs & Changes Inspector"]
        Palette["Command Palette"]
    end

    subgraph Term ["Terminal Dock (crates/flint-term)"]
        PTY["PTY Manager (portable-pty)"]
        Tabs["Shell & Command Execution Tabs"]
    end

    subgraph Engine ["Agent Execution Core (crates/flint-agent)"]
        SessionMgr["Session Engine & State Persistence"]
        ContextMgr["Context Window & Token Budgeting"]
        Tools["Tool Registry (sh, read, write, edit, grep, spawn_agent)"]
        Guard["Harness Guard (Loop Detect, Watchdog, Tests, Repair)"]
        SubagentOrch["Subagent Parallel Coordinator (up to 4)"]
        Approval["Approval Engine (Auto-run vs. Ask)"]
    end

    subgraph ACPBridge ["ACP Protocol Gateway (crates/flint-acp)"]
        ACPClient["ACP Client (JSON-RPC over stdio)"]
    end

    subgraph External ["External Endpoints & CLI Agents"]
        OpenAI["OpenAI / OpenRouter / DeepSeek / Ollama"]
        ClaudeCode["Claude Code CLI"]
        CodexCLI["Codex CLI"]
        DroidCLI["Droid (Factory)"]
        JEV["Typesafe JEV Judge"]
    end

    Window --> Panels
    Panels --> Composer
    Panels --> Sidebar
    Panels --> Changes
    Panels --> Term

    Composer --> SessionMgr
    SessionMgr --> ContextMgr
    ContextMgr --> Guard
    Guard --> Tools
    Tools --> Approval
    Approval --> PTY

    SessionMgr -.-> SubagentOrch
    SubagentOrch --> Tools

    SessionMgr -.-> ACPBridge
    ACPBridge --> ClaudeCode
    ACPBridge --> CodexCLI
    ACPBridge --> DroidCLI

    ContextMgr <--> OpenAI
    Guard -.-> JEV

    class Window,SessionMgr,Guard,SubagentOrch,ACPClient ember;
    class ClaudeCode,CodexCLI,DroidCLI,OpenAI cyan;
```
</details>

---

## 2. Turn Execution Lifecycle & Harness Guard

Every conversational turn in Flint's native engine passes through context window assembly, token budget trimming, model streaming, and the multi-stage **Harness Guard** pipeline.

<p align="center">
  <img src="../assets/turn_lifecycle.jpg" alt="Turn Execution Lifecycle" width="100%" />
</p>

<details>
  <summary><b>View Mermaid sequence flow</b></summary>

```mermaid
%%{init: {
  'theme': 'base',
  'themeVariables': {
    'darkMode': true,
    'actorBkg': '#1b1b20',
    'actorBorder': '#ff8a3d',
    'actorTextColor': '#e7e7ea',
    'actorLineColor': '#ff8a3d',
    'signalColor': '#ff8a3d',
    'signalTextColor': '#e7e7ea',
    'labelBoxBkgColor': '#1b1b20',
    'labelBoxBorderColor': '#ff8a3d',
    'labelTextColor': '#e7e7ea',
    'loopTextColor': '#e7e7ea',
    'noteBkgColor': '#141418',
    'noteTextColor': '#e7e7ea',
    'noteBorderColor': '#222228',
    'activationBkgColor': '#222228',
    'activationBorderColor': '#ff8a3d',
    'sequenceNumberColor': '#0e0e10'
  }
}}%%
sequenceDiagram
    autonumber
    actor User
    participant App as GPUI App / Composer
    participant Engine as Flint Agent Engine
    participant Guard as Harness Guard Pipeline
    participant LLM as Model Endpoint
    participant Tools as Tool Execution & PTY
    participant Judge as Typesafe JEV Judge (Optional)

    User->>App: Submits prompt (+ optional images, @files)
    App->>Engine: Initiate Turn (session_id, workspace_path)
    Engine->>Engine: Assemble context & trim token budget
    Engine->>LLM: Stream Chat Completions request
    LLM-->>Engine: Stream delta chunks (text + tool_calls)
    Engine->>App: Render live transcript tokens

    alt Tool Call Generated
        Engine->>Guard: Inspect proposed tool call
        Guard->>Guard: Check for repetitive tool loops
        Guard->>Guard: Watchdog check (turns editing nothing)
        Guard->>Guard: Validate/repair malformed arguments
        
        opt JEV Judge Configured
            Guard->>Judge: Verify loop / termination suspicion
            Judge-->>Guard: Confirmation / Dismissal
        end

        alt Requires Approval
            Engine->>App: Prompt user for approval (y / a / n)
            User->>App: Approve action
            App->>Engine: Granted
        end

        Engine->>Tools: Execute tool (sh, edit, write, etc.)
        Tools-->>App: Stream terminal output & diff cards
        Tools-->>Engine: Tool result content
        Engine->>Engine: Append tool result to context
        Engine->>LLM: Next turn completion
    else Completion Finished
        Guard->>Guard: Test-before-done check (were tests run?)
        Engine->>App: Final turn response & stats update
    end
```
</details>

<p align="center">
  <img src="../assets/harness_guard.jpg" alt="Harness Guard Pipeline" width="100%" />
</p>

<details>
  <summary><b>View Mermaid guard logic tree</b></summary>

```mermaid
%%{init: {
  'theme': 'base',
  'themeVariables': {
    'darkMode': true,
    'primaryColor': '#1b1b20',
    'primaryTextColor': '#e7e7ea',
    'primaryBorderColor': '#ff8a3d',
    'lineColor': '#ff8a3d',
    'secondaryColor': '#141418',
    'tertiaryColor': '#0e0e10',
    'mainBkg': '#141418',
    'nodeBorder': '#ff8a3d',
    'clusterBkg': '#141418',
    'clusterBorder': '#222228',
    'defaultLinkColor': '#ff8a3d',
    'titleColor': '#e7e7ea',
    'edgeLabelBackground': '#1b1b20'
  }
}}%%
flowchart TD
    classDef default fill:#141418,stroke:#222228,stroke-width:1.5px,color:#e7e7ea;
    classDef ember fill:#1b1b20,stroke:#ff8a3d,stroke-width:2px,color:#e7e7ea;
    classDef green fill:#141418,stroke:#4cc38a,stroke-width:2px,color:#e7e7ea;
    classDef red fill:#141418,stroke:#f2555a,stroke-width:2px,color:#e7e7ea;
    classDef cyan fill:#141418,stroke:#00e5ff,stroke-width:2px,color:#e7e7ea;

    Start["Incoming Model Response"] --> CheckTool{"Contains Tool Calls?"}
    
    CheckTool -->|Yes| ArgCheck{"Malformed Tool Call Syntax?"}
    ArgCheck -->|Yes| Repair["Tool-Call Repair Pass"]
    ArgCheck -->|No| LoopCheck{"Repetitive Action Loop Detected?"}
    Repair --> LoopCheck

    LoopCheck -->|Detected| LoopAction{"Exceeds Threshold?"}
    LoopAction -->|Yes| TriggerLoop["Harness Guard: Break Infinite Loop"]
    LoopAction -->|No| WatchdogCheck{"Watchdog: Turns Editing Nothing?"}
    LoopCheck -->|No| WatchdogCheck

    WatchdogCheck -->|Warning| TriggerWatchdog["Inject Guidance Prompt"]
    WatchdogCheck -->|OK| ExecTool["Execute Tool within Sandbox"]

    TriggerLoop --> JEVCheck{"JEV Judge Enabled?"}
    TriggerWatchdog --> JEVCheck
    JEVCheck -->|Yes| ConsultJEV["Consult Typesafe JEV API"]
    ConsultJEV --> ActionConfirm{"Judge Confirms?"}
    ActionConfirm -->|Yes| HaltTurn["Halt / Re-prompt Agent"]
    ActionConfirm -->|No| ExecTool
    JEVCheck -->|No| HaltTurn

    CheckTool -->|No| TestCheck{"Marked as Done & Changes Made?"}
    TestCheck -->|Yes| VerifyTests{"Were Verification Tests Executed?"}
    VerifyTests -->|No| TestReminder["Guard: Enforce Test-Before-Done Reminder"]
    VerifyTests -->|Yes| Complete["Turn Concluded Successfully"]
    TestCheck -->|No| Complete

    class Start,CheckTool,ArgCheck,LoopCheck,LoopAction,WatchdogCheck,JEVCheck,TestCheck,VerifyTests,ActionConfirm ember;
    class ExecTool,Complete green;
    class TriggerLoop,TriggerWatchdog,HaltTurn,TestReminder red;
    class Repair,ConsultJEV cyan;
```
</details>

---

## 3. Subagent Parallel Delegation

Flint allows the native agent to spin off up to four independent, concurrent child sessions using the `spawn_agent` tool.

<p align="center">
  <img src="../assets/subagents_workflow.jpg" alt="Subagents Workflow" width="100%" />
</p>

### Delegation Characteristics:
- **Isolated Contexts:** Each child has its own independent context window and token budget, preventing memory contamination.
- **Shared Workspace:** Children operate within the same workspace root; independent subagent operations can target different files concurrently.
- **Unified Approvals:** Children inherit the parent's approval policy (`auto` vs `ask`).
- **Resumability:** Each child run produces a durable `session_id`. The parent can invoke follow-ups on the same subagent session later.
- **Recursion Guard:** Child subagents are restricted from spawning grandchildren, preventing unbounded agent explosion.

<details>
  <summary><b>View Mermaid sequence flow</b></summary>

```mermaid
%%{init: {
  'theme': 'base',
  'themeVariables': {
    'darkMode': true,
    'actorBkg': '#1b1b20',
    'actorBorder': '#ff8a3d',
    'actorTextColor': '#e7e7ea',
    'actorLineColor': '#ff8a3d',
    'signalColor': '#ff8a3d',
    'signalTextColor': '#e7e7ea',
    'labelBoxBkgColor': '#1b1b20',
    'labelBoxBorderColor': '#ff8a3d',
    'labelTextColor': '#e7e7ea',
    'loopTextColor': '#e7e7ea',
    'noteBkgColor': '#141418',
    'noteTextColor': '#e7e7ea',
    'noteBorderColor': '#222228',
    'activationBkgColor': '#222228',
    'activationBorderColor': '#ff8a3d',
    'sequenceNumberColor': '#0e0e10'
  }
}}%%
sequenceDiagram
    autonumber
    participant Parent as Parent Agent Session
    participant Orch as Subagent Coordinator
    participant Sub1 as Subagent A (Backend)
    participant Sub2 as Subagent B (Frontend)
    participant Disk as Workspace Filesystem

    Parent->>Orch: spawn_agent(task: "Refactor API routing", model: "gpt-4.1-mini")
    Parent->>Orch: spawn_agent(task: "Update UI component tests", model: "claude-sonnet-4.5")
    
    par Parallel Subagent Execution
        Orch->>Sub1: Launch isolated conversation
        Sub1->>Disk: Read/edit crates/flint-agent/src/...
        Sub1-->>Orch: Task completed (final summary + session_id_1)
    and
        Orch->>Sub2: Launch isolated conversation
        Sub2->>Disk: Read/edit crates/flint-app/src/...
        Sub2-->>Orch: Task completed (final summary + session_id_2)
    end

    Orch-->>Parent: Consolidated results [session_id_1, session_id_2]
    Parent->>Parent: Synthesize final result & present diffs to user
```
</details>

---

## 4. Agent Client Protocol (ACP) Integration

Flint acts as an ACP host, allowing Claude Code, Codex, and Droid to be launched and driven directly from the same native desktop UI without altering their native authentication or configuration workflows.

<p align="center">
  <img src="../assets/acp_diagram.jpg" alt="Agent Client Protocol Architecture" width="100%" />
</p>

<details>
  <summary><b>View Mermaid protocol flow</b></summary>

```mermaid
%%{init: {
  'theme': 'base',
  'themeVariables': {
    'darkMode': true,
    'primaryColor': '#1b1b20',
    'primaryTextColor': '#e7e7ea',
    'primaryBorderColor': '#ff8a3d',
    'lineColor': '#ff8a3d',
    'secondaryColor': '#141418',
    'tertiaryColor': '#0e0e10',
    'mainBkg': '#141418',
    'nodeBorder': '#ff8a3d',
    'clusterBkg': '#141418',
    'clusterBorder': '#222228',
    'defaultLinkColor': '#ff8a3d',
    'titleColor': '#e7e7ea',
    'edgeLabelBackground': '#1b1b20'
  }
}}%%
flowchart LR
    classDef default fill:#141418,stroke:#222228,stroke-width:1.5px,color:#e7e7ea;
    classDef ember fill:#1b1b20,stroke:#ff8a3d,stroke-width:2px,color:#e7e7ea;
    classDef cyan fill:#141418,stroke:#00e5ff,stroke-width:2px,color:#e7e7ea;
    classDef surface fill:#141418,stroke:#222228,stroke-width:1.5px,color:#9a9aa4;

    subgraph FlintHost ["Flint Host Application"]
        UI["GPUI View Model"]
        ACPDriver["crates/flint-acp Gateway"]
    end

    subgraph ExternalAgents ["Local CLI Agents (JSON-RPC over stdio)"]
        Claude["@agentclientprotocol/claude-agent-acp"]
        Codex["@agentclientprotocol/codex-acp"]
        Droid["droid exec --output-format acp"]
    end

    subgraph Auth ["Credentials & Subscriptions"]
        ClaudeAuth["Anthropic Subscription / CLI Login"]
        CodexAuth["OpenAI Subscription / CLI Login"]
        DroidAuth["Factory Account / Droid Login"]
    end

    UI <--> ACPDriver
    ACPDriver <-->|stdio JSON-RPC| Claude
    ACPDriver <-->|stdio JSON-RPC| Codex
    ACPDriver <-->|stdio JSON-RPC| Droid

    Claude --- ClaudeAuth
    Codex --- CodexAuth
    Droid --- DroidAuth

    class UI,ACPDriver ember;
    class Claude,Codex,Droid cyan;
    class ClaudeAuth,CodexAuth,DroidAuth surface;
```
</details>
