#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct EffectSelection {
    clip_id: Option<i32>,
    selected: Vec<usize>,
    current: Option<usize>,
    drag_source: Option<usize>,
    drag_target: Option<usize>,
    scroll_target: Option<usize>,
}

impl EffectSelection {
    pub fn reconcile(&mut self, clip_id: i32, length: usize) {
        if self.clip_id != Some(clip_id) {
            self.clip_id = Some(clip_id);
            self.clear();
            return;
        }
        self.selected.retain(|index| *index < length);
        if self.current.is_some_and(|index| index >= length) {
            self.current = self.selected.last().copied();
        }
        if self.drag_source.is_some_and(|index| index >= length)
            || self.drag_target.is_some_and(|index| index >= length)
        {
            self.cancel_drag();
        }
    }

    pub fn click(&mut self, index: usize, control: bool, shift: bool) {
        if shift {
            let anchor = self.current.unwrap_or(0);
            let (start, end) = if anchor <= index {
                (anchor, index)
            } else {
                (index, anchor)
            };
            self.selected = (start..=end).collect();
        } else if control {
            if let Some(position) = self.selected.iter().position(|item| *item == index) {
                self.selected.remove(position);
            } else {
                self.selected.push(index);
            }
        } else {
            self.selected.clear();
            self.selected.push(index);
        }
        self.current = Some(index);
        self.scroll_target = Some(index);
    }

    pub fn context_click(&mut self, index: usize) {
        if !self.is_selected(index) {
            self.selected.clear();
            self.selected.push(index);
            self.current = Some(index);
        }
    }

    pub fn is_selected(&self, index: usize) -> bool {
        self.selected.contains(&index)
    }

    pub fn current(&self) -> Option<usize> {
        self.current
    }

    pub fn action_targets(&self, index: usize) -> Vec<usize> {
        if self.is_selected(index) && self.selected.len() > 1 {
            self.selected.clone()
        } else {
            vec![index]
        }
    }

    pub fn deletion_targets(&self) -> Vec<usize> {
        if self.selected.is_empty() {
            self.current.into_iter().collect()
        } else {
            self.selected.clone()
        }
    }

    pub fn begin_drag(&mut self, source: usize) {
        self.drag_source = Some(source);
        self.drag_target = None;
    }

    pub fn set_drag_target(&mut self, target: usize) {
        if self.drag_source != Some(target) {
            self.drag_target = Some(target);
        }
    }

    pub fn drag_target(&self) -> Option<usize> {
        self.drag_target
    }

    pub fn is_dragging(&self) -> bool {
        self.drag_source.is_some()
    }

    pub fn scroll_target(&self) -> Option<usize> {
        self.scroll_target
    }

    pub fn clear_scroll_target(&mut self) {
        self.scroll_target = None;
    }

    pub fn finish_drag(&mut self, audio: bool) -> Option<(Vec<usize>, usize)> {
        let source = self.drag_source.take()?;
        let target = self.drag_target.take()?;
        let indices = if !audio && self.selected.len() > 1 && self.is_selected(source) {
            self.selected.clone()
        } else {
            vec![source]
        };
        (target != source).then_some((indices, target))
    }

    pub fn cancel_drag(&mut self) {
        self.drag_source = None;
        self.drag_target = None;
    }

    pub fn apply_permutation(&mut self, permutation: &[usize]) {
        let mut inverse = vec![usize::MAX; permutation.len()];
        for (new_index, old_index) in permutation.iter().copied().enumerate() {
            if let Some(slot) = inverse.get_mut(old_index) {
                *slot = new_index;
            }
        }
        self.selected = self
            .selected
            .iter()
            .filter_map(|index| inverse.get(*index).copied())
            .filter(|index| *index != usize::MAX)
            .collect();
        self.current = self
            .current
            .and_then(|index| inverse.get(index).copied())
            .filter(|index| *index != usize::MAX);
        self.scroll_target = self
            .scroll_target
            .and_then(|index| inverse.get(index).copied())
            .filter(|index| *index != usize::MAX);
    }

    pub fn apply_removals(&mut self, removed: &[usize]) {
        let mut removed = removed.to_vec();
        removed.sort_unstable();
        removed.dedup();
        let remap = |index: usize| {
            removed
                .binary_search(&index)
                .err()
                .map(|position| index.saturating_sub(position))
        };
        self.selected = self
            .selected
            .iter()
            .filter_map(|index| remap(*index))
            .collect();
        self.current = self.current.and_then(remap);
        self.scroll_target = self.scroll_target.and_then(remap);
        self.cancel_drag();
    }

    pub fn clear(&mut self) {
        self.selected.clear();
        self.current = None;
        self.scroll_target = None;
        self.cancel_drag();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn click_modifiers_match_the_qt_sidebar() {
        let mut selection = EffectSelection::default();
        selection.reconcile(7, 6);
        selection.click(2, false, false);
        selection.click(4, true, false);
        assert_eq!(selection.selected, [2, 4]);
        assert_eq!(selection.current(), Some(4));

        selection.click(1, false, true);
        assert_eq!(selection.selected, [1, 2, 3, 4]);
        assert_eq!(selection.current(), Some(1));

        selection.context_click(3);
        assert_eq!(selection.selected, [1, 2, 3, 4]);
        selection.context_click(5);
        assert_eq!(selection.selected, [5]);
        assert_eq!(selection.current(), Some(5));
    }

    #[test]
    fn multi_drag_preserves_selected_objects_after_reorder() {
        let mut selection = EffectSelection::default();
        selection.reconcile(7, 6);
        selection.click(2, false, false);
        selection.click(4, true, false);
        selection.begin_drag(2);
        selection.set_drag_target(5);
        assert_eq!(selection.finish_drag(false), Some((vec![2, 4], 5)));

        let permutation = [0, 1, 3, 2, 4, 5];
        selection.apply_permutation(&permutation);
        assert_eq!(selection.selected, [3, 4]);
        assert_eq!(selection.current(), Some(4));
    }

    #[test]
    fn audio_drag_moves_only_the_grabbed_plugin() {
        let mut selection = EffectSelection::default();
        selection.reconcile(9, 4);
        selection.click(1, false, false);
        selection.click(2, true, false);
        selection.begin_drag(1);
        selection.set_drag_target(3);
        assert_eq!(selection.finish_drag(true), Some((vec![1], 3)));
    }

    #[test]
    fn direct_removal_preserves_selection_by_object_identity() {
        let mut selection = EffectSelection::default();
        selection.reconcile(9, 5);
        selection.click(1, false, false);
        selection.click(4, true, false);
        selection.apply_removals(&[2]);
        assert_eq!(selection.selected, [1, 3]);
        assert_eq!(selection.current(), Some(3));

        selection.apply_removals(&[1]);
        assert_eq!(selection.selected, [2]);
        assert_eq!(selection.current(), Some(2));
    }
}
