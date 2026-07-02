pub enum ChatIntent {
    ChatOnly,
    WorkflowChange,
}

pub fn classify(text: &str) -> ChatIntent {
    let normalized = text.trim().to_lowercase();
    if normalized.is_empty() || is_greeting(&normalized) {
        return ChatIntent::ChatOnly;
    }
    if workflow_keywords()
        .iter()
        .any(|keyword| normalized.contains(keyword))
    {
        ChatIntent::WorkflowChange
    } else {
        ChatIntent::ChatOnly
    }
}

fn is_greeting(text: &str) -> bool {
    matches!(
        text,
        "你好" | "你好啊" | "hi" | "hello" | "hey" | "哈喽" | "嗨"
    )
}

fn workflow_keywords() -> &'static [&'static str] {
    &[
        "workflow",
        "工作流",
        "node",
        "节点",
        "graph",
        "画布",
        "comfy",
        "comfyui",
        "prompt",
        "提示词",
        "seed",
        "controlnet",
        "参数",
        "连接",
        "运行",
        "queue",
        "生成",
        "文生图",
        "图像",
        "图片",
        "视频",
        "创建",
        "新建",
        "做一个",
        "做个",
        "修改",
        "改成",
        "加",
        "删除",
        "修复",
        "报错",
    ]
}

#[cfg(test)]
mod tests {
    use super::{ChatIntent, classify};

    #[test]
    fn creation_requests_are_workflow_changes() {
        assert!(matches!(
            classify("帮我创建一个文生图工作流"),
            ChatIntent::WorkflowChange
        ));
        assert!(matches!(
            classify("新建一个 comfyui 图像流程"),
            ChatIntent::WorkflowChange
        ));
        assert!(matches!(
            classify("做个视频生成"),
            ChatIntent::WorkflowChange
        ));
    }

    #[test]
    fn greetings_stay_chat_only() {
        assert!(matches!(classify("你好啊"), ChatIntent::ChatOnly));
    }
}
