//! A conversation written the way each model family was trained to read it. Docufy formats the
//! prompt itself (the host only encodes it), so the format is tested here, without a model. The
//! tokenizer adds the begin-of-text token itself where a family has one.

use super::catalog::Family;
use crate::chat::{ChatRequest, Role};

/// `req` as one prompt, ending where the model's answer begins.
pub fn prompt(family: Family, req: &ChatRequest) -> String {
    let system = req.system.trim();
    let mut out = String::new();
    match family {
        Family::Phi3 => {
            if !system.is_empty() {
                out.push_str(&format!("<|system|>{system}<|end|>"));
            }
            for m in &req.messages {
                let tag = if m.role == Role::User { "user" } else { "assistant" };
                out.push_str(&format!("<|{tag}|>{}<|end|>", m.text));
            }
            out.push_str("<|assistant|>");
        }
        Family::Qwen2 => {
            let system = if system.is_empty() { "You are a helpful assistant." } else { system };
            out.push_str(&format!("<|im_start|>system\n{system}<|im_end|>\n"));
            for m in &req.messages {
                let tag = if m.role == Role::User { "user" } else { "assistant" };
                out.push_str(&format!("<|im_start|>{tag}\n{}<|im_end|>\n", m.text));
            }
            out.push_str("<|im_start|>assistant\n");
        }
        Family::Mistral => {
            // Mistral has no system turn: the instructions go before the last user message.
            let last_user = req.messages.iter().rposition(|m| m.role == Role::User);
            for (i, m) in req.messages.iter().enumerate() {
                if m.role == Role::User {
                    let text = if Some(i) == last_user && !system.is_empty() { format!("{system}\n\n{}", m.text) } else { m.text.clone() };
                    out.push_str(&format!("[INST] {text} [/INST]"));
                } else {
                    out.push_str(&format!(" {}</s>", m.text));
                }
            }
        }
        Family::Llama3 => {
            if !system.is_empty() {
                out.push_str(&format!("<|start_header_id|>system<|end_header_id|>\n\n{system}<|eot_id|>"));
            }
            for m in &req.messages {
                let tag = if m.role == Role::User { "user" } else { "assistant" };
                out.push_str(&format!("<|start_header_id|>{tag}<|end_header_id|>\n\n{}<|eot_id|>", m.text));
            }
            out.push_str("<|start_header_id|>assistant<|end_header_id|>\n\n");
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::Message;

    fn req() -> ChatRequest {
        ChatRequest {
            system: "Antworte kurz.".into(),
            messages: vec![Message::user("Hallo"), Message::assistant("Hallo!"), Message::user("Wie geht's?")],
            max_tokens: 100,
        }
    }

    #[test]
    fn each_family_gets_its_own_format() {
        assert_eq!(
            prompt(Family::Phi3, &req()),
            "<|system|>Antworte kurz.<|end|><|user|>Hallo<|end|><|assistant|>Hallo!<|end|><|user|>Wie geht's?<|end|><|assistant|>"
        );
        assert_eq!(
            prompt(Family::Qwen2, &req()),
            "<|im_start|>system\nAntworte kurz.<|im_end|>\n<|im_start|>user\nHallo<|im_end|>\n<|im_start|>assistant\nHallo!<|im_end|>\n<|im_start|>user\nWie geht's?<|im_end|>\n<|im_start|>assistant\n"
        );
        assert_eq!(prompt(Family::Mistral, &req()), "[INST] Hallo [/INST] Hallo!</s>[INST] Antworte kurz.\n\nWie geht's? [/INST]");
        assert_eq!(
            prompt(Family::Llama3, &req()),
            "<|start_header_id|>system<|end_header_id|>\n\nAntworte kurz.<|eot_id|><|start_header_id|>user<|end_header_id|>\n\nHallo<|eot_id|><|start_header_id|>assistant<|end_header_id|>\n\nHallo!<|eot_id|><|start_header_id|>user<|end_header_id|>\n\nWie geht's?<|eot_id|><|start_header_id|>assistant<|end_header_id|>\n\n"
        );
    }

    #[test]
    fn a_missing_system_prompt_leaves_no_empty_turn() {
        let r = ChatRequest { system: String::new(), messages: vec![Message::user("Hi")], max_tokens: 10 };
        assert_eq!(prompt(Family::Phi3, &r), "<|user|>Hi<|end|><|assistant|>");
        assert_eq!(prompt(Family::Mistral, &r), "[INST] Hi [/INST]");
        assert!(prompt(Family::Qwen2, &r).starts_with("<|im_start|>system\nYou are a helpful assistant."));
    }
}
