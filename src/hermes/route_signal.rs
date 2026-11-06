use super::{routing::Navigation, Hermes, RouteStatus};
use glam::Vec2;

// Map units are 100 metres. Accumulating heading change also catches
// turns authored as several short curve segments.
const SIGNAL_DISTANCE: f32 = 0.4;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RouteSignal {
    #[default]
    Off,
    Left,
    Right,
}

impl Navigation {
    fn signal(&self) -> RouteSignal {
        if self.status != RouteStatus::Active {
            return RouteSignal::Off;
        }
        let mut heading = None;
        let mut distance = 0.;
        for pair in self.points.windows(2) {
            let delta = Vec2::from_array(pair[1]) - Vec2::from_array(pair[0]);
            let length = delta.length();
            if length < 0.0001 {
                continue;
            }
            if distance > SIGNAL_DISTANCE {
                break;
            }
            let first = *heading.get_or_insert(delta);
            let angle = first.perp_dot(delta).atan2(first.dot(delta));
            if angle.abs() >= std::f32::consts::FRAC_PI_6 {
                return if angle > 0. {
                    RouteSignal::Left
                } else {
                    RouteSignal::Right
                };
            }
            distance += length;
        }
        RouteSignal::Off
    }
}

impl Hermes {
    pub fn route_signal(&self) -> RouteSignal {
        self.navigation.signal()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signals_nearby_turns_and_clears_for_other_route_states() {
        let mut navigation = Navigation::default();
        navigation.status = RouteStatus::Active;
        for (points, expected) in [
            (vec![[0., 0.], [0.2, 0.], [0.2, 1.]], RouteSignal::Left),
            (vec![[0., 0.], [0.2, 0.], [0.2, -1.]], RouteSignal::Right),
            (vec![[0., 0.], [0.5, 0.], [0.5, 1.]], RouteSignal::Off),
            (vec![[0., 0.], [0.2, 0.], [1., 0.]], RouteSignal::Off),
            (
                vec![[0., 0.], [0., 0.], [0.2, 0.], [0.2, 1.]],
                RouteSignal::Left,
            ),
        ] {
            navigation.points = points;
            assert_eq!(navigation.signal(), expected);
        }
        for status in [
            RouteStatus::Inactive,
            RouteStatus::Arrived,
            RouteStatus::Unreachable,
        ] {
            navigation.status = status;
            assert_eq!(navigation.signal(), RouteSignal::Off);
        }
        navigation.clear();
        assert_eq!(navigation.signal(), RouteSignal::Off);
    }
}
