use agent_dashboard::{llm, UiLanguage};

fn main() {
    // 加载 .env（cwd + exe 旁 + target/release）
    let _ = dotenvy::dotenv();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let _ = dotenvy::from_path(dir.join(".env"));
            let _ = dotenvy::from_path(dir.join("..").join(".env"));
        }
    }
    let args: Vec<String> = std::env::args().collect();
    let tui_file = args.get(1).expect("usage: test_llm <tui_file>");
    let content = std::fs::read_to_string(tui_file).unwrap_or_default();
    let tui = if let Some(s) = content.find("=== TUI 截取") {
        let after = &content[s + "=== TUI 截取".len()..];
        if let Some(e) = after.find("=== 模型输出 ===") {
            after[..e].trim().to_string()
        } else {
            after.trim().to_string()
        }
    } else {
        content
    };
    println!(
        "DEEPSEEK_API_KEY 是否设置: {}",
        std::env::var("DEEPSEEK_API_KEY").is_ok()
    );
    println!(
        "TUI 样本（前200字）:\n{}\n",
        tui.chars().take(200).collect::<String>()
    );
    match llm::summarize(&tui, UiLanguage::ZhCn) {
        Ok(s) => println!("摘要: {}", s),
        Err(e) => println!("失败: {}", e),
    }
}
