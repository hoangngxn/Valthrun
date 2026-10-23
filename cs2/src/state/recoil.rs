use anyhow::{
    anyhow,
    Context,
};
use cs2_schema_provider::{
    try_resolve_offset,
    OffsetInfo,
};
use raw_struct::{
    FromMemoryView,
    MemoryView,
};

use crate::StateCS2Memory;

pub struct AimPunchSample {
    pub angle: [f32; 2],
    pub source: &'static str,
}

fn offset(class_name: &'static str, member: &'static str) -> Option<u64> {
    try_resolve_offset(&OffsetInfo {
        default_value: 0,
        module: "client.dll",
        class_name,
        member,
    })
}

/// Read a measured recoil state using the running build's schema, never fixed offsets.
/// Service base angles are timestamped engine state, not an exact future shot prediction.
pub fn read_aim_punch(
    memory: &StateCS2Memory,
    pawn_address: u64,
) -> anyhow::Result<AimPunchSample> {
    read_sample(memory.view(), pawn_address, offset)
}

fn read_sample(
    memory: &dyn MemoryView,
    pawn_address: u64,
    lookup: impl Fn(&'static str, &'static str) -> Option<u64>,
) -> anyhow::Result<AimPunchSample> {
    let (address, source) =
        if let Some(services_offset) = lookup("C_CSPlayerPawn", "m_pAimPunchServices") {
            let services = u64::read_object(memory, pawn_address + services_offset)
                .map_err(|error| anyhow!(error))?;
            anyhow::ensure!(services != 0, "aim punch services unavailable");
            let angle_offset = lookup("CCSPlayer_AimPunchServices", "m_predictableBaseAngle")
                .context("aim punch services angle unavailable in this schema")?;
            (services + angle_offset, "Live service base angle")
        } else {
            let angle_offset = lookup("C_CSPlayerPawn", "m_aimPunchAngle")
                .context("no supported live recoil field in this schema")?;
            (pawn_address + angle_offset, "Live pawn punch angle")
        };
    // QAngle is three floats. The generated legacy schema describes four;
    // reading only the actual vector avoids including a neighboring field.
    let angle = <[f32; 3]>::read_object(memory, address).map_err(|error| anyhow!(error))?;
    anyhow::ensure!(
        angle.iter().all(|value| value.is_finite()),
        "invalid live recoil sample"
    );
    Ok(AimPunchSample {
        angle: [angle[0], angle[1]],
        source,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn modern_layout(_: &str, member: &str) -> Option<u64> {
        match member {
            "m_pAimPunchServices" => Some(8),
            "m_predictableBaseAngle" => Some(16),
            // Must not fall back here if a modern service pointer is null.
            "m_aimPunchAngle" => Some(80),
            _ => None,
        }
    }

    #[test]
    fn modern_layout_follows_pointer_using_resolved_offsets() {
        let mut bytes = [0u8; 128];
        bytes[8..16].copy_from_slice(&32u64.to_ne_bytes());
        bytes[48..52].copy_from_slice(&(-2.0f32).to_ne_bytes());
        bytes[52..56].copy_from_slice(&1.5f32.to_ne_bytes());
        let sample = read_sample(&bytes, 0, modern_layout).unwrap();
        assert_eq!(sample.angle, [-2.0, 1.5]);
        assert_eq!(sample.source, "Live service base angle");
    }

    #[test]
    fn legacy_layout_reads_exactly_three_floats() {
        // Exactly twelve bytes: a four-float read would fail.
        let bytes = [-3.0f32, 0.5, 0.0];
        let sample = read_sample(&bytes, 0, |_, member| {
            (member == "m_aimPunchAngle").then_some(0)
        })
        .unwrap();
        assert_eq!(sample.angle, [-3.0, 0.5]);
    }

    #[test]
    fn unavailable_or_invalid_samples_fail_without_guessing() {
        assert!(read_sample(&[0u8; 128], 0, modern_layout).is_err());
        assert!(read_sample(&[0u8; 128], 0, |_, _| None).is_err());
        assert!(read_sample(&[f32::NAN, 0.0, 0.0], 0, |_, member| {
            (member == "m_aimPunchAngle").then_some(0)
        })
        .is_err());
    }
}
