//! Board palette colors: core `Option<LabelColor>` ⇄ wire `v1::LabelColor` (since 1.8).
//! "No color" is `LABEL_COLOR_UNSPECIFIED`, and values from a newer peer that this side
//! doesn't know read as no color.

use urushi_core::LabelColor;

use crate::v1;

/// Core color → wire enum value.
pub fn color_to_proto(color: Option<LabelColor>) -> i32 {
    let wire = match color {
        None => v1::LabelColor::Unspecified,
        Some(LabelColor::Red) => v1::LabelColor::Red,
        Some(LabelColor::Orange) => v1::LabelColor::Orange,
        Some(LabelColor::Yellow) => v1::LabelColor::Yellow,
        Some(LabelColor::Green) => v1::LabelColor::Green,
        Some(LabelColor::Teal) => v1::LabelColor::Teal,
        Some(LabelColor::Blue) => v1::LabelColor::Blue,
        Some(LabelColor::Purple) => v1::LabelColor::Purple,
        Some(LabelColor::Pink) => v1::LabelColor::Pink,
        Some(LabelColor::Gray) => v1::LabelColor::Gray,
    };
    wire.into()
}

/// Wire enum value → core color; unknown values are no color.
pub fn color_from_proto(value: i32) -> Option<LabelColor> {
    match v1::LabelColor::try_from(value).ok()? {
        v1::LabelColor::Unspecified => None,
        v1::LabelColor::Red => Some(LabelColor::Red),
        v1::LabelColor::Orange => Some(LabelColor::Orange),
        v1::LabelColor::Yellow => Some(LabelColor::Yellow),
        v1::LabelColor::Green => Some(LabelColor::Green),
        v1::LabelColor::Teal => Some(LabelColor::Teal),
        v1::LabelColor::Blue => Some(LabelColor::Blue),
        v1::LabelColor::Purple => Some(LabelColor::Purple),
        v1::LabelColor::Pink => Some(LabelColor::Pink),
        v1::LabelColor::Gray => Some(LabelColor::Gray),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_color_round_trips_and_unknown_values_are_no_color() {
        for c in LabelColor::ALL {
            assert_eq!(color_from_proto(color_to_proto(Some(c))), Some(c));
        }
        assert_eq!(color_to_proto(None), 0);
        assert_eq!(color_from_proto(0), None);
        assert_eq!(color_from_proto(999), None);
    }
}
