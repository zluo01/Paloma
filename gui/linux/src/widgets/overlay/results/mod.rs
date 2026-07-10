mod chat;
mod search;
mod sessions;

pub(super) const CSS: &str = include_str!("style.css");

pub(super) use chat::ChatView;
pub(super) use search::SearchView;
pub(super) use sessions::SessionsView;

fn step_index(current: usize, delta: i32, len: usize) -> Option<usize> {
    if len == 0 {
        None
    } else {
        Some((current as i32 + delta).clamp(0, len as i32 - 1) as usize)
    }
}

#[cfg(test)]
mod tests {
    use super::step_index;

    #[test]
    fn step_index_returns_none_when_empty() {
        assert_eq!(step_index(0, 1, 0), None);
    }

    #[test]
    fn step_index_moves_within_bounds() {
        assert_eq!(step_index(1, 1, 4), Some(2));
        assert_eq!(step_index(2, -1, 4), Some(1));
    }

    #[test]
    fn step_index_clamps_at_edges() {
        assert_eq!(step_index(0, -1, 4), Some(0));
        assert_eq!(step_index(3, 1, 4), Some(3));
    }
}
