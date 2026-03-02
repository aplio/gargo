use crate::input::action::CoreAction;

pub struct DotRecorder {
    recording: Option<Vec<CoreAction>>,
    last_edit: Option<Vec<CoreAction>>,
    replaying: bool,
}

impl Default for DotRecorder {
    fn default() -> Self {
        Self::new()
    }
}

impl DotRecorder {
    pub fn new() -> Self {
        Self {
            recording: None,
            last_edit: None,
            replaying: false,
        }
    }

    pub fn begin_insert_session(&mut self, entry: CoreAction) {
        if self.replaying {
            return;
        }
        self.recording = Some(vec![entry]);
    }

    pub fn record(&mut self, action: &CoreAction) {
        if self.replaying {
            return;
        }
        if let Some(ref mut actions) = self.recording {
            actions.push(action.clone());
        }
    }

    pub fn finalize_insert_session(&mut self, exit: CoreAction) {
        if self.replaying {
            return;
        }
        if let Some(mut actions) = self.recording.take() {
            actions.push(exit);
            self.last_edit = Some(actions);
        }
    }

    pub fn record_single_shot(&mut self, action: CoreAction) {
        if self.replaying {
            return;
        }
        self.recording = None;
        self.last_edit = Some(vec![action]);
    }

    pub fn last_edit(&self) -> Option<Vec<CoreAction>> {
        self.last_edit.clone()
    }

    pub fn enter_replay(&mut self) -> bool {
        if self.last_edit.is_none() {
            return false;
        }
        self.replaying = true;
        true
    }

    pub fn exit_replay(&mut self) {
        self.replaying = false;
    }

    pub fn is_replaying(&self) -> bool {
        self.replaying
    }

    pub fn is_recording_insert(&self) -> bool {
        self.recording.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::mode::Mode;

    #[test]
    fn single_shot_records_immediately() {
        let mut rec = DotRecorder::new();
        rec.record_single_shot(CoreAction::DeleteSelection);
        assert_eq!(rec.last_edit(), Some(vec![CoreAction::DeleteSelection]));
    }

    #[test]
    fn insert_session_records_full_sequence() {
        let mut rec = DotRecorder::new();
        rec.begin_insert_session(CoreAction::ChangeMode(Mode::Insert));
        rec.record(&CoreAction::InsertChar('h'));
        rec.record(&CoreAction::InsertChar('i'));
        rec.finalize_insert_session(CoreAction::ChangeMode(Mode::Normal));

        let expected = vec![
            CoreAction::ChangeMode(Mode::Insert),
            CoreAction::InsertChar('h'),
            CoreAction::InsertChar('i'),
            CoreAction::ChangeMode(Mode::Normal),
        ];
        assert_eq!(rec.last_edit(), Some(expected));
    }

    #[test]
    fn replaying_suppresses_recording() {
        let mut rec = DotRecorder::new();
        rec.record_single_shot(CoreAction::Paste);
        assert!(rec.enter_replay());

        rec.record_single_shot(CoreAction::Indent);
        rec.begin_insert_session(CoreAction::ChangeMode(Mode::Insert));
        rec.record(&CoreAction::InsertChar('x'));
        rec.finalize_insert_session(CoreAction::ChangeMode(Mode::Normal));

        rec.exit_replay();
        assert_eq!(rec.last_edit(), Some(vec![CoreAction::Paste]));
    }

    #[test]
    fn enter_replay_returns_false_when_empty() {
        let mut rec = DotRecorder::new();
        assert!(!rec.enter_replay());
    }

    #[test]
    fn single_shot_overwrites_previous_edit() {
        let mut rec = DotRecorder::new();
        rec.record_single_shot(CoreAction::Paste);
        rec.record_single_shot(CoreAction::Indent);
        assert_eq!(rec.last_edit(), Some(vec![CoreAction::Indent]));
    }

    #[test]
    fn insert_session_overwrites_previous_single_shot() {
        let mut rec = DotRecorder::new();
        rec.record_single_shot(CoreAction::Paste);

        rec.begin_insert_session(CoreAction::ChangeMode(Mode::Insert));
        rec.record(&CoreAction::InsertChar('a'));
        rec.finalize_insert_session(CoreAction::ChangeMode(Mode::Normal));

        let expected = vec![
            CoreAction::ChangeMode(Mode::Insert),
            CoreAction::InsertChar('a'),
            CoreAction::ChangeMode(Mode::Normal),
        ];
        assert_eq!(rec.last_edit(), Some(expected));
    }

    #[test]
    fn partial_insert_session_discarded_by_single_shot() {
        let mut rec = DotRecorder::new();
        rec.begin_insert_session(CoreAction::ChangeMode(Mode::Insert));
        rec.record(&CoreAction::InsertChar('x'));

        rec.record_single_shot(CoreAction::DeleteSelection);

        assert!(!rec.is_recording_insert());
        assert_eq!(rec.last_edit(), Some(vec![CoreAction::DeleteSelection]));
    }
}
