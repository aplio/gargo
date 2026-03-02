use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use gargo::command::registry::CommandRegistry;
use gargo::config::Config;
use gargo::input::action::{Action, AppAction, IntegrationAction};
use gargo::input::chord::KeyState;
use gargo::syntax::language::LanguageRegistry;
use gargo::ui::framework::component::EventResult;
use gargo::ui::framework::compositor::Compositor;

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

#[test]
fn down_then_enter_applies_next_hover_candidate() {
    let registry = CommandRegistry::new();
    let lang_registry = LanguageRegistry::new();
    let config = Config::default();
    let key_state = KeyState::Normal;

    let mut compositor = Compositor::new();
    compositor.set_markdown_link_hover_candidates(vec![
        "alpha.md".to_string(),
        "beta.md".to_string(),
        "gamma.md".to_string(),
    ]);

    let move_result = compositor.handle_key(
        key(KeyCode::Down),
        &registry,
        &lang_registry,
        &config,
        &key_state,
    );
    assert!(matches!(move_result, EventResult::Consumed));

    let apply_result = compositor.handle_key(
        key(KeyCode::Enter),
        &registry,
        &lang_registry,
        &config,
        &key_state,
    );
    assert_eq!(
        apply_result,
        EventResult::Action(Action::App(AppAction::Integration(
            IntegrationAction::ApplyMarkdownLinkCompletion {
                candidate: "beta.md".to_string(),
            },
        )))
    );
}

#[test]
fn up_wraps_and_enter_applies_last_hover_candidate() {
    let registry = CommandRegistry::new();
    let lang_registry = LanguageRegistry::new();
    let config = Config::default();
    let key_state = KeyState::Normal;

    let mut compositor = Compositor::new();
    compositor.set_markdown_link_hover_candidates(vec![
        "alpha.md".to_string(),
        "beta.md".to_string(),
        "gamma.md".to_string(),
    ]);

    let move_result = compositor.handle_key(
        key(KeyCode::Up),
        &registry,
        &lang_registry,
        &config,
        &key_state,
    );
    assert!(matches!(move_result, EventResult::Consumed));

    let apply_result = compositor.handle_key(
        key(KeyCode::Enter),
        &registry,
        &lang_registry,
        &config,
        &key_state,
    );
    assert_eq!(
        apply_result,
        EventResult::Action(Action::App(AppAction::Integration(
            IntegrationAction::ApplyMarkdownLinkCompletion {
                candidate: "gamma.md".to_string(),
            },
        )))
    );
}
