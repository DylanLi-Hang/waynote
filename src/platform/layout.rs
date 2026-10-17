//! Presentation state for the single board. Old per-note geometry is ignored.
use super::{paths::Paths, store::atomic_write};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct BoardState {
    pub width: i32,
    pub height: i32,
    pub scroll: f64,
    pub active: Option<String>,
}
impl Default for BoardState {
    fn default() -> Self {
        Self {
            width: 920,
            height: 680,
            scroll: 0.0,
            active: None,
        }
    }
}
pub fn load(paths: &Paths) -> BoardState {
    std::fs::read_to_string(paths.layout_file())
        .ok()
        .and_then(|s| toml::from_str(&s).ok())
        .unwrap_or_default()
}
pub fn save(paths: &Paths, state: &BoardState) -> std::io::Result<()> {
    let path = paths.layout_file();
    std::fs::create_dir_all(path.parent().unwrap())?;
    let text = toml::to_string(state).map_err(std::io::Error::other)?;
    atomic_write(&path, text.as_bytes())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn old_geometry_is_ignored_without_preventing_board_defaults() {
        let state: BoardState =
            toml::from_str("[notes.old.geometry]\nx=10\ny=20\nw=280\nh=220").unwrap();
        assert_eq!(state.width, 920);
        assert!(state.active.is_none());
    }
    #[test]
    fn board_state_roundtrips() {
        let state = BoardState {
            width: 1100,
            height: 700,
            scroll: 180.0,
            active: Some("id".into()),
        };
        let loaded: BoardState = toml::from_str(&toml::to_string(&state).unwrap()).unwrap();
        assert_eq!(loaded.active, state.active);
        assert_eq!(loaded.scroll, 180.0);
        assert_eq!((loaded.width, loaded.height), (1100, 700));
    }
}
