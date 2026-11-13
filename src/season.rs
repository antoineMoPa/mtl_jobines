//! Explicit session seasons shared by the palette, simulation and renderer.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Season {
    #[default]
    Summer,
    Winter,
}
impl Season {
    pub fn from_command(id: &str) -> Option<Self> {
        match id {
            "season.winter" => Some(Self::Winter),
            "season.summer" => Some(Self::Summer),
            _ => None,
        }
    }
    pub fn snow_cover(self) -> f32 {
        match self {
            Self::Summer => 0.,
            Self::Winter => 1.,
        }
    }
}
