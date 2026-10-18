//! Fleet kernel: enriched chat window for `egui` desktop apps.
//!
//! Drop-in reuse: copy this file **verbatim** together with `browser.rs`
//! into any repo's app `src/`, add `mod richchat;`, and construct one
//! [`RichChat`] per chat page. Only dependencies are `eframe::egui` plus
//! the sibling `browser` theme helpers — zero app state, zero network.
//! (Repconv SOP Phase 1 kernel list.)
//!
//! What the module owns: message history, streaming reveal buffer, input
//! box, example-prompt chips, persona picker, clear/export actions, and
//! theme-aware bubble rendering. What the app owns: the LLM dispatch —
//! [`RichChat::show`] returns a [`ChatIntent::Send`] carrying the prompt
//! plus the composed system prompt (repo skill preprompt + persona), the
//! app runs its `LlmChat` command, and feeds text back via [`push_chunk`].
//! Progressive reveal makes full replies read as streaming; true token
//! SSE is an `llm.rs` follow-up, not faked here.

use crate::browser::{amber, faint, ink};
use eframe::egui::{self, Color32, CornerRadius, Margin, RichText, Stroke};

/// Selectable chat persona. `system` is appended after the repo skill
/// preprompt when composing the effective system prompt.
#[derive(Debug, Clone)]
pub struct ChatPersona {
    /// Stable id (e.g. `"librarian"`).
    pub id: String,
    /// Display label in the picker.
    pub label: String,
    /// Persona instructions, repo-domain specific.
    pub system: String,
}

impl ChatPersona {
    #[must_use]
    pub fn new(id: &str, label: &str, system: &str) -> Self {
        Self {
            id: id.to_string(),
            label: label.to_string(),
            system: system.to_string(),
        }
    }
}

/// One rendered transcript entry.
#[derive(Debug, Clone)]
pub struct ChatMessage {
    pub from_user: bool,
    pub text: String,
}

/// Action the app must handle after [`RichChat::show`].
#[derive(Debug, Clone)]
pub enum ChatIntent {
    /// Nothing to do this frame.
    Idle,
    /// Dispatch this prompt to the LLM with the composed system prompt,
    /// then feed the reply back via [`RichChat::push_chunk`].
    Send { prompt: String, system: String },
    /// History was cleared (app may note it in the status line).
    Cleared,
    /// Transcript markdown was copied to the OS clipboard.
    Exported,
}

/// Enriched, repo-specific chat window.
pub struct RichChat {
    skill_name: String,
    skill_preprompt: String,
    personas: Vec<ChatPersona>,
    persona_idx: usize,
    examples: Vec<String>,
    messages: Vec<ChatMessage>,
    input: String,
    /// Full pending reply; revealed progressively for the streaming feel.
    streaming: String,
    revealed_chars: usize,
    status: String,
}

/// Characters revealed per frame while streaming.
const REVEAL_STEP: usize = 24;

impl RichChat {
    /// Build a chat window. `skill_preprompt` is the repo skill text that
    /// makes the chat domain-specific; it prefixes every system prompt.
    /// Falls back to a single Generic persona when `personas` is empty.
    #[must_use]
    pub fn new(
        skill_name: &str,
        skill_preprompt: &str,
        personas: Vec<ChatPersona>,
        examples: Vec<String>,
    ) -> Self {
        let personas = if personas.is_empty() {
            vec![ChatPersona::new(
                "generic",
                "Assistant",
                "Answer helpfully and concisely.",
            )]
        } else {
            personas
        };
        Self {
            skill_name: skill_name.to_string(),
            skill_preprompt: skill_preprompt.to_string(),
            personas,
            persona_idx: 0,
            examples,
            messages: Vec::new(),
            input: String::new(),
            streaming: String::new(),
            revealed_chars: 0,
            status: String::new(),
        }
    }

    /// Currently selected persona.
    #[must_use]
    pub fn persona(&self) -> &ChatPersona {
        self.personas
            .get(self.persona_idx)
            .unwrap_or(&self.personas[0])
    }

    /// System prompt sent with every message: skill preprompt + persona.
    #[must_use]
    pub fn effective_system(&self) -> String {
        let persona = self.persona();
        format!(
            "{}\n\nPersona: {}\n{}",
            self.skill_preprompt, persona.label, persona.system
        )
    }

    /// Feed LLM text (a chunk or a full reply). Revealed progressively.
    pub fn push_chunk(&mut self, chunk: &str) {
        self.streaming.push_str(chunk);
    }

    /// Drop the pending reply without committing it.
    pub fn cancel_stream(&mut self) {
        self.streaming.clear();
        self.revealed_chars = 0;
        self.status = "Reply discarded.".to_string();
    }

    /// True while a reply is still being revealed.
    #[must_use]
    pub fn is_streaming(&self) -> bool {
        self.revealed_chars < self.streaming.chars().count()
    }

    /// Markdown transcript of the visible history (user/assistant turns).
    #[must_use]
    pub fn transcript_markdown(&self) -> String {
        let mut out = format!(
            "# {} chat transcript (persona: {})\n\n",
            self.skill_name,
            self.persona().id
        );
        for message in &self.messages {
            let role = if message.from_user {
                "User"
            } else {
                "Assistant"
            };
            out.push_str(&format!("## {role}\n\n{}\n\n", message.text));
        }
        out
    }

    /// Render the window. Returns at most one intent per frame.
    pub fn show(&mut self, ui: &mut egui::Ui) -> ChatIntent {
        let mut intent = ChatIntent::Idle;

        // Persona picker + actions row.
        ui.horizontal_wrapped(|ui| {
            ui.label("Persona:");
            let current = self.persona().label.clone();
            egui::ComboBox::from_id_salt("richchat_persona")
                .selected_text(current)
                .show_ui(ui, |ui| {
                    for (idx, persona) in self.personas.iter().enumerate() {
                        ui.selectable_value(&mut self.persona_idx, idx, &persona.label);
                    }
                });
            ui.separator();
            if ui.button("Clear").clicked() {
                self.messages.clear();
                self.streaming.clear();
                self.revealed_chars = 0;
                self.status = "Chat history cleared.".to_string();
                intent = ChatIntent::Cleared;
            }
            if ui.button("Export").clicked() {
                ui.ctx().copy_text(self.transcript_markdown());
                self.status = "Transcript copied to clipboard.".to_string();
                intent = ChatIntent::Exported;
            }
            if self.is_streaming() && ui.button("Stop").clicked() {
                self.cancel_stream();
            }
        });

        ui.add_space(6.0);

        // Transcript.
        egui::ScrollArea::vertical()
            .max_height(380.0)
            .stick_to_bottom(true)
            .show(ui, |ui| {
                for message in &self.messages {
                    self.bubble(ui, message.from_user, &message.text);
                    ui.add_space(6.0);
                }
                if !self.streaming.is_empty() {
                    let total = self.streaming.chars().count();
                    self.revealed_chars = (self.revealed_chars + REVEAL_STEP).min(total);
                    let shown: String = self.streaming.chars().take(self.revealed_chars).collect();
                    let mut pending = shown;
                    if self.revealed_chars < total {
                        pending.push('…');
                        ui.ctx().request_repaint();
                    }
                    self.bubble(ui, false, &pending);
                    if self.revealed_chars >= total {
                        let done = std::mem::take(&mut self.streaming);
                        self.revealed_chars = 0;
                        self.messages.push(ChatMessage {
                            from_user: false,
                            text: done,
                        });
                    }
                    ui.add_space(6.0);
                }
            });

        ui.add_space(6.0);

        // Example-prompt chips.
        if !self.examples.is_empty() {
            ui.horizontal_wrapped(|ui| {
                ui.label("Try:");
                let mut chosen: Option<String> = None;
                for example in &self.examples {
                    if ui.small_button(example).clicked() {
                        chosen = Some(example.clone());
                    }
                }
                if let Some(text) = chosen {
                    self.input = text;
                }
            });
            ui.add_space(4.0);
        }

        // Input row. Enter sends, like every chat app.
        ui.horizontal(|ui| {
            let resp = ui.text_edit_singleline(&mut self.input);
            let enter = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            let mut send = ui.button("Send").clicked() || enter;
            if send && self.input.trim().is_empty() {
                send = false;
            }
            if send {
                let prompt = std::mem::take(&mut self.input);
                let system = self.effective_system();
                self.messages.push(ChatMessage {
                    from_user: true,
                    text: prompt.clone(),
                });
                intent = ChatIntent::Send { prompt, system };
            }
        });

        if !self.status.is_empty() {
            ui.label(RichText::new(&self.status).size(11.0).color(faint(ui)));
        }
        intent
    }

    fn bubble(&self, ui: &mut egui::Ui, from_user: bool, text: &str) {
        let is_dark = ui.visuals().dark_mode;
        let bg = if is_dark {
            if from_user {
                Color32::from_rgb(26, 36, 54)
            } else {
                Color32::from_rgb(33, 46, 68)
            }
        } else if from_user {
            Color32::from_rgb(226, 232, 240)
        } else {
            Color32::WHITE
        };
        let edge = if from_user {
            if is_dark {
                Color32::from_rgb(51, 65, 85)
            } else {
                Color32::from_rgb(148, 163, 184)
            }
        } else {
            amber(ui)
        };
        egui::Frame::NONE
            .fill(bg)
            .stroke(Stroke::new(1.5, edge))
            .inner_margin(Margin::same(8))
            .corner_radius(CornerRadius::same(6))
            .show(ui, |ui| {
                ui.label(
                    RichText::new(if from_user { "You" } else { "Assistant" })
                        .strong()
                        .color(if from_user {
                            Color32::from_rgb(14, 165, 233)
                        } else {
                            amber(ui)
                        }),
                );
                ui.add_space(2.0);
                ui.label(RichText::new(text).color(ink(ui)));
            });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chat() -> RichChat {
        RichChat::new(
            "plex-skill",
            "You are the Plex library assistant.",
            vec![
                ChatPersona::new("librarian", "Librarian", "Keep answers shelf-ready."),
                ChatPersona::new("critic", "Critic", "Judge everything."),
            ],
            vec!["What should I watch?".to_string()],
        )
    }

    #[test]
    fn system_prompt_combines_skill_and_persona() {
        let chat = chat();
        let system = chat.effective_system();
        assert!(system.contains("Plex library assistant"));
        assert!(system.contains("Librarian"));
    }

    #[test]
    fn empty_personas_fall_back_to_generic() {
        let chat = RichChat::new("x", "pre", Vec::new(), Vec::new());
        assert_eq!(chat.persona().id, "generic");
    }

    #[test]
    fn stream_reveal_commits_full_reply() {
        let mut chat = chat();
        chat.push_chunk("hel");
        chat.push_chunk("lo");
        assert!(chat.is_streaming());
        // Drain the reveal buffer the way frames would.
        while chat.is_streaming() {
            let total = chat.streaming.chars().count();
            chat.revealed_chars = (chat.revealed_chars + REVEAL_STEP).min(total);
            if chat.revealed_chars >= total {
                let done = std::mem::take(&mut chat.streaming);
                chat.revealed_chars = 0;
                chat.messages.push(ChatMessage {
                    from_user: false,
                    text: done,
                });
            }
        }
        assert_eq!(chat.messages.len(), 1);
        assert_eq!(chat.messages[0].text, "hello");
    }

    #[test]
    fn cancel_discards_without_committing() {
        let mut chat = chat();
        chat.push_chunk("partial");
        chat.cancel_stream();
        assert!(!chat.is_streaming());
        assert!(chat.messages.is_empty());
    }

    #[test]
    fn transcript_uses_markdown_roles() {
        let mut chat = chat();
        chat.messages.push(ChatMessage {
            from_user: true,
            text: "hi".to_string(),
        });
        chat.messages.push(ChatMessage {
            from_user: false,
            text: "hello".to_string(),
        });
        let md = chat.transcript_markdown();
        assert!(md.contains("## User"));
        assert!(md.contains("## Assistant"));
        assert!(md.contains("plex-skill"));
    }
}
