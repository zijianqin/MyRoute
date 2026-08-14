use serde::{Deserialize, Serialize};

use crate::{Error, Result};

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct DrivingPreferences {
    pub highway_penalty: f64,
    pub minor_road_penalty: f64,
    pub traffic_signal_penalty_s: f64,
    pub left_turn_penalty_s: f64,
    #[serde(default)]
    pub turn_penalty_s: f64,
    #[serde(default)]
    pub u_turn_penalty_s: f64,
    pub max_extra_time_ratio: Option<f64>,
}

impl DrivingPreferences {
    pub const fn fastest() -> Self {
        Self {
            highway_penalty: 0.0,
            minor_road_penalty: 0.0,
            traffic_signal_penalty_s: 0.0,
            left_turn_penalty_s: 0.0,
            turn_penalty_s: 0.0,
            u_turn_penalty_s: 0.0,
            max_extra_time_ratio: None,
        }
    }

    pub fn validate(&self) -> Result<()> {
        validate_value("highway_penalty", self.highway_penalty)?;
        validate_value("minor_road_penalty", self.minor_road_penalty)?;
        validate_value("traffic_signal_penalty_s", self.traffic_signal_penalty_s)?;
        validate_value("left_turn_penalty_s", self.left_turn_penalty_s)?;
        validate_value("turn_penalty_s", self.turn_penalty_s)?;
        validate_value("u_turn_penalty_s", self.u_turn_penalty_s)?;
        if let Some(value) = self.max_extra_time_ratio {
            validate_value("max_extra_time_ratio", value)?;
        }
        Ok(())
    }

    pub fn scaled(self, scale: f64) -> Result<Self> {
        validate_value("penalty_scale", scale)?;
        let scaled = Self {
            highway_penalty: self.highway_penalty * scale,
            minor_road_penalty: self.minor_road_penalty * scale,
            traffic_signal_penalty_s: self.traffic_signal_penalty_s * scale,
            left_turn_penalty_s: self.left_turn_penalty_s * scale,
            turn_penalty_s: self.turn_penalty_s * scale,
            u_turn_penalty_s: self.u_turn_penalty_s * scale,
            max_extra_time_ratio: self.max_extra_time_ratio,
        };
        scaled.validate()?;
        Ok(scaled)
    }
}

impl Default for DrivingPreferences {
    fn default() -> Self {
        Self::fastest()
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DrivingProfile {
    pub name: String,
    #[serde(flatten)]
    pub preferences: DrivingPreferences,
}

impl DrivingProfile {
    pub fn from_toml(input: &str) -> Result<Self> {
        let profile: Self = toml::from_str(input)?;
        profile.preferences.validate()?;
        Ok(profile)
    }
}

fn validate_value(field: &'static str, value: f64) -> Result<()> {
    if value.is_finite() && value >= 0.0 {
        Ok(())
    } else {
        Err(Error::InvalidPreference { field, value })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_flat_profile_toml() {
        let profile = DrivingProfile::from_toml(
            r#"
                name = "chill"
                highway_penalty = 0.2
                minor_road_penalty = 0.25
                traffic_signal_penalty_s = 10
                left_turn_penalty_s = 15
                max_extra_time_ratio = 0.15
            "#,
        )
        .unwrap();
        assert_eq!(profile.name, "chill");
        assert_eq!(profile.preferences.left_turn_penalty_s, 15.0);
        assert_eq!(profile.preferences.turn_penalty_s, 0.0);
        assert_eq!(profile.preferences.u_turn_penalty_s, 0.0);
    }

    #[test]
    fn rejects_negative_and_nonfinite_preferences() {
        let mut preferences = DrivingPreferences::fastest();
        preferences.highway_penalty = -0.1;
        assert!(matches!(
            preferences.validate(),
            Err(Error::InvalidPreference {
                field: "highway_penalty",
                ..
            })
        ));
        preferences.highway_penalty = f64::INFINITY;
        assert!(preferences.validate().is_err());
    }

    #[test]
    fn scaling_preserves_time_limit() {
        let preferences = DrivingPreferences {
            highway_penalty: 0.8,
            turn_penalty_s: 4.0,
            u_turn_penalty_s: 120.0,
            max_extra_time_ratio: Some(0.1),
            ..DrivingPreferences::fastest()
        };
        let scaled = preferences.scaled(0.25).unwrap();
        assert_eq!(scaled.highway_penalty, 0.2);
        assert_eq!(scaled.turn_penalty_s, 1.0);
        assert_eq!(scaled.u_turn_penalty_s, 30.0);
        assert_eq!(scaled.max_extra_time_ratio, Some(0.1));
    }
}
