use super::*;

impl Editor {
    pub fn current_jump_location(&self) -> JumpLocation {
        let doc = self.active_buffer();
        JumpLocation {
            doc_id: doc.id,
            file_path: doc.file_path.clone(),
            cursor: doc.cursors[0],
            line: doc.cursor_line(),
            char_col: doc.cursor_col(),
        }
    }

    pub fn push_jump_location(&mut self, location: JumpLocation) {
        if let Some(idx) = self.jump_list_index
            && idx + 1 < self.jump_list.len()
        {
            self.jump_list.truncate(idx + 1);
        }

        if self
            .jump_list
            .last()
            .is_some_and(|last| Self::locations_equivalent(last, &location))
        {
            self.jump_list_index = Some(self.jump_list.len().saturating_sub(1));
            return;
        }

        self.jump_list.push(location);
        self.trim_jump_list_front();
        self.jump_list_index = Some(self.jump_list.len().saturating_sub(1));
    }

    pub fn record_jump_transition(&mut self, before: JumpLocation, after: JumpLocation) {
        if Self::locations_equivalent(&before, &after) {
            return;
        }
        self.push_jump_location(before);
        self.push_jump_location(after);
    }

    fn jump_to_location(&mut self, location: &JumpLocation) -> bool {
        if self
            .documents
            .iter()
            .position(|d| d.id == location.doc_id)
            .map(|idx| {
                self.active_index = idx;
                true
            })
            .unwrap_or(false)
        {
            let len = self.active_buffer().rope.len_chars();
            self.active_buffer_mut().cursors = vec![location.cursor.min(len)];
            return true;
        }

        if let Some(path) = &location.file_path {
            self.open_file(&path.to_string_lossy());
            let len = self.active_buffer().rope.len_chars();
            self.active_buffer_mut().cursors = vec![location.cursor.min(len)];
            return true;
        }
        false
    }

    pub fn jump_list_entries(&self) -> &[JumpLocation] {
        &self.jump_list
    }

    pub fn jump_list_index(&self) -> Option<usize> {
        self.jump_list_index
    }

    pub fn jump_to_list_index(&mut self, index: usize) -> Result<(), String> {
        if index >= self.jump_list.len() {
            return Err("Invalid jump location".to_string());
        }
        let location = self.jump_list[index].clone();
        if self.jump_to_location(&location) {
            self.jump_list_index = Some(index);
            Ok(())
        } else {
            Err("Jump location is no longer available".to_string())
        }
    }

    pub fn jump_older(&mut self) -> Result<(), String> {
        let Some(idx) = self.jump_list_index else {
            return Err("Jumplist is empty".to_string());
        };
        if idx == 0 {
            return Err("Already at oldest jump".to_string());
        }
        self.jump_to_list_index(idx - 1)
    }

    pub fn jump_newer(&mut self) -> Result<(), String> {
        let Some(idx) = self.jump_list_index else {
            return Err("Jumplist is empty".to_string());
        };
        if idx + 1 >= self.jump_list.len() {
            return Err("Already at newest jump".to_string());
        }
        self.jump_to_list_index(idx + 1)
    }

    fn locations_equivalent(a: &JumpLocation, b: &JumpLocation) -> bool {
        if let (Some(pa), Some(pb)) = (&a.file_path, &b.file_path) {
            return pa == pb && a.cursor == b.cursor;
        }
        a.doc_id == b.doc_id && a.cursor == b.cursor
    }

    fn trim_jump_list_front(&mut self) {
        if self.jump_list.len() <= Self::MAX_JUMP_LIST_LEN {
            return;
        }
        let drop_count = self.jump_list.len() - Self::MAX_JUMP_LIST_LEN;
        self.jump_list.drain(0..drop_count);
        self.jump_list_index = self
            .jump_list_index
            .map(|idx| idx.saturating_sub(drop_count))
            .or_else(|| {
                if self.jump_list.is_empty() {
                    None
                } else {
                    Some(self.jump_list.len() - 1)
                }
            });
    }
}
