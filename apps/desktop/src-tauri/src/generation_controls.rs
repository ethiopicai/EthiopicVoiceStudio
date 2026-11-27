//! Per-take controls; capability checks happen before model loading.
use serde::{Deserialize, Serialize};
#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Options {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sentence_pause: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub speed: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub num_steps: Option<u32>,
}
fn invalid(value: Option<f64>, low: f64, high: f64) -> bool {
    value.is_some_and(|n| !n.is_finite() || !(low..=high).contains(&n))
}
impl Options {
    pub fn validate_values(&self) -> Result<(), String> {
        if invalid(self.temperature, 0., 1.) {
            return Err("Choose a temperature between 0 and 1.".into());
        }
        if invalid(self.sentence_pause, 0., 2.) {
            return Err("Choose a sentence pause between 0 and 2 seconds.".into());
        }
        if invalid(self.speed, 0.5, 2.) {
            return Err("Choose a speaking speed between 0.5× and 2×.".into());
        }
        if invalid(self.duration, 0.5, 600.) {
            return Err("Choose a target duration between 0.5 and 600 seconds.".into());
        }
        if self.num_steps.is_some_and(|n| ![16, 32, 64].contains(&n)) {
            return Err("Choose 16, 32 or 64 generation steps.".into());
        }
        if self.duration.is_some() && (self.speed.is_some() || self.sentence_pause.is_some()) {
            return Err(
                "Use target duration or speed and sentence pauses, rather than both.".into(),
            );
        }
        Ok(())
    }
    pub fn validate(&self, small_cpu: bool) -> Result<(), String> {
        self.validate_values()?;
        if small_cpu {
            if invalid(self.temperature, 0.1, 1.) {
                return Err("Choose a temperature between 0.1 and 1.".into());
            }
            if self.speed.is_some() || self.duration.is_some() || self.num_steps.is_some() {
                return Err("Speed, target duration and generation steps are unavailable with these CPU speech tools.".into());
            }
        }
        Ok(())
    }
}
pub fn validate_seed(seed: u64) -> Result<(), String> {
    if seed > u32::MAX as u64 {
        Err("Choose a whole-number seed from 0 to 4294967295.".into())
    } else {
        Ok(())
    }
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Capabilities {
    temperature: bool,
    sentence_pause: bool,
    speed: bool,
    duration: bool,
    num_steps: bool,
    seed: bool,
    temperature_min: f64,
    default_temperature: Option<f64>,
    default_sentence_pause: Option<f64>,
    default_speed: f64,
    default_num_steps: Option<u32>,
}
pub fn capabilities_with_steps(small_cpu: bool, steps: u32) -> Capabilities {
    Capabilities {
        temperature: true,
        sentence_pause: true,
        speed: !small_cpu,
        duration: !small_cpu,
        num_steps: !small_cpu,
        seed: true,
        temperature_min: if small_cpu { 0.1 } else { 0. },
        default_temperature: Some(if small_cpu { 0.3 } else { 0. }),
        default_sentence_pause: Some(0.2),
        default_speed: 1.,
        default_num_steps: (!small_cpu).then_some(steps),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn invalid_and_incompatible_controls_are_rejected_before_loading() {
        for json in [
            r#"{"temperature":true}"#,
            r#"{"speed":"1"}"#,
            r#"{"numSteps":8}"#,
            r#"{"speed":0}"#,
            r#"{"duration":601}"#,
            r#"{"duration":2,"speed":1}"#,
            r#"{"duration":2,"sentencePause":0}"#,
        ] {
            assert!(
                serde_json::from_str::<Options>(json).map_or(true, |v| v.validate(false).is_err()),
                "{json}"
            );
        }
        let gpu: Options = serde_json::from_str(
            r#"{"temperature":0,"speed":0.9,"numSteps":32,"sentencePause":0.5}"#,
        )
        .unwrap();
        gpu.validate(false).unwrap();
        assert!(gpu.validate(true).is_err());
        let target: Options =
            serde_json::from_str(r#"{"temperature":0.2,"duration":4,"numSteps":16}"#).unwrap();
        target.validate(false).unwrap();
        let cpu: Options =
            serde_json::from_str(r#"{"temperature":0.4,"sentencePause":0}"#).unwrap();
        cpu.validate(true).unwrap();
        for value in [f64::NAN, f64::INFINITY, -0.1, 1.1] {
            assert!(Options {
                temperature: Some(value),
                ..Default::default()
            }
            .validate(false)
            .is_err());
        }
        assert!(validate_seed(u32::MAX as u64).is_ok());
        assert!(validate_seed(u32::MAX as u64 + 1).is_err());
    }
    #[test]
    fn capabilities_describe_real_cpu_and_gpu_controls() {
        let cpu = capabilities_with_steps(true, 32);
        assert!(
            cpu.temperature && cpu.sentence_pause && !cpu.speed && !cpu.duration && !cpu.num_steps
        );
        assert_eq!(cpu.default_temperature, Some(0.3));
        assert_eq!(cpu.temperature_min, 0.1);
        let gpu = capabilities_with_steps(false, 16);
        assert!(
            gpu.temperature && gpu.speed && gpu.duration && gpu.num_steps && gpu.sentence_pause
        );
        assert_eq!(gpu.default_temperature, Some(0.));
        assert_eq!(gpu.default_num_steps, Some(16));
    }
}
