//! Lanceur des agents CLI et sélecteur de la barre d'onglets.

use grove_core::{AgentProvider, TabKind, WorkspaceKind};
use gpui::{AnyElement, Context, FocusHandle, KeyDownEvent, Window, div, prelude::*, px};

use crate::grove_view::GroveView;
use crate::theme;
use crate::ui::{Icon, icon, list_row, popover};
use crate::worktrees::Notice;

pub(crate) struct AgentPicker {
    focus: FocusHandle,
    selected: usize,
}

pub(crate) fn agent_icon(provider: AgentProvider) -> Icon {
    match provider {
        AgentProvider::Claude => Icon::Claude,
        _ => Icon::Terminal,
    }
}

impl GroveView {
    pub(crate) fn open_agent_tab(&mut self, provider: AgentProvider, window: &mut Window, cx: &mut Context<Self>) {
        self.open_agent_session(provider, None, window, cx);
    }

    pub(crate) fn resume_claude_tab(&mut self, session_id: String, window: &mut Window, cx: &mut Context<Self>) {
        self.open_agent_session(AgentProvider::Claude, Some(session_id), window, cx);
    }

    fn open_agent_session(
        &mut self,
        provider: AgentProvider,
        resume: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(key) = self.state.active_workspace else { return };
        self.leave_settings(cx);
        let Some(workspace) = self.state.workspace_mut(key) else { return };
        if workspace.kind == WorkspaceKind::Direct
            && workspace.tabs.iter().any(|tab| matches!(tab.kind, TabKind::Agent { .. } | TabKind::Claude { .. }))
        {
            self.notice = Some(Notice {
                workspace: key.1,
                text: "Plusieurs agents utilisent le même dossier et peuvent modifier les mêmes fichiers.".into(),
                suggest_worktree: true,
            });
        }
        let tab = match resume {
            Some(session_id) => workspace.add_claude_tab(Some(session_id), self.config.claude.view),
            None => workspace.add_agent_tab(provider, self.config.claude.view),
        };
        self.claude.history = None;
        self.activate_tab(key, tab, window, cx);
    }

    pub(crate) fn open_agent_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let focus = cx.focus_handle();
        window.focus(&focus);
        let selected = AgentProvider::ALL
            .iter()
            .position(|&provider| provider == self.config.agents.default_provider)
            .unwrap_or_default();
        self.agent_picker = Some(AgentPicker { focus, selected });
        cx.notify();
    }

    fn close_agent_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.agent_picker = None;
        self.focus_active(window, cx);
        cx.notify();
    }

    fn agent_picker_key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let Some(picker) = &mut self.agent_picker else { return };
        let count = AgentProvider::ALL.len();
        match event.keystroke.key.as_str() {
            "up" => picker.selected = (picker.selected + count - 1) % count,
            "down" => picker.selected = (picker.selected + 1) % count,
            "enter" => {
                let provider = AgentProvider::ALL[picker.selected];
                self.open_agent_tab(provider, window, cx);
            }
            "escape" => self.close_agent_picker(window, cx),
            _ => return,
        }
        cx.stop_propagation();
        cx.notify();
    }

    pub(crate) fn render_agent_picker(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let picker = self.agent_picker.as_ref()?;
        let default = self.config.agents.default_provider;
        let list = AgentProvider::ALL.into_iter().enumerate().map(|(index, provider)| {
            list_row(provider.command(), index == picker.selected)
                .h(px(30.0))
                .child(icon(agent_icon(provider), theme::text_muted()))
                .child(div().flex_1().child(provider.name()))
                .children((provider == default).then(|| {
                    div().text_xs().text_color(theme::text_muted()).child("Par défaut")
                }))
                .on_click(cx.listener(move |this, _, window, cx| this.open_agent_tab(provider, window, cx)))
        });
        Some(
            popover("agent-picker", &picker.focus, px(260.0))
                .relative()
                .top_0()
                .left_0()
                .on_key_down(cx.listener(Self::agent_picker_key_down))
                .on_mouse_down_out(cx.listener(|this, _, window, cx| {
                    this.popover_dismissed = true;
                    this.close_agent_picker(window, cx);
                }))
                .child(div().p_1().children(list))
                .into_any_element(),
        )
    }
}
