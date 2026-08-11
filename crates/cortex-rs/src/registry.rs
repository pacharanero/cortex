// SPDX-FileCopyrightText: 2026 Dr Marcus Baw
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Typed registry for the operational Cortex Control protobuf messages.
//!
//! The trailer assigns tags 1 through 70 to generated protobuf structs. This
//! module records that structural relationship without assigning operational
//! semantics to any message. Decoding is explicit and opt-in; the session hot
//! path continues to retain raw protobuf bodies.
//!
//! @see spec/120-proto-schema/spec.md [FR-14]
//! @see spec/120-proto-schema/design.md [DES-REGISTRY]

use crate::proto::cortex_message_type::Enum as MessageType;

/// One entry in the compile-time operational message registry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RegistryEntry {
    /// The generated trailer-tag enum variant.
    pub message_type: MessageType,
    /// The stable protobuf enum name, preserving schema capitalization.
    pub name: &'static str,
    /// The generated Rust protobuf struct name.
    pub rust_type_name: &'static str,
}

/// Failure to resolve or decode a trailer-tagged message.
#[derive(Debug, thiserror::Error)]
pub enum RegistryError {
    /// The tag is a schema boundary marker rather than an operational message.
    #[error("message type {0} is a sentinel, not an operational message")]
    Sentinel(u16),
    /// The tag is not present in this version of the recovered schema.
    #[error("message type {0} is not registered")]
    Unknown(u16),
    /// The registered protobuf body was malformed for its generated struct.
    #[error("message type {message_type} protobuf decode failed: {source}")]
    Decode {
        /// The original numeric trailer tag.
        message_type: u16,
        /// The protobuf decoder failure.
        #[source]
        source: prost::DecodeError,
    },
}

impl RegistryError {
    /// Return the original numeric trailer tag without coercing unknown values
    /// to `Undefined`.
    #[must_use]
    pub const fn message_type(&self) -> u16 {
        match self {
            Self::Sentinel(message_type)
            | Self::Unknown(message_type)
            | Self::Decode { message_type, .. } => *message_type,
        }
    }
}

macro_rules! operational_messages {
    ($(($variant:ident, $message:ident, $name:literal)),+ $(,)?) => {
        /// A protobuf body decoded according to its registered trailer tag.
        #[derive(Debug, Clone, PartialEq)]
        pub enum DecodedMessage {
            $(
                #[doc = concat!("A protobuf body tagged as `", $name, "`.")]
                $variant(Box<crate::proto::$message>),
            )+
        }

        impl DecodedMessage {
            /// Return the generated message type corresponding to this body.
            #[must_use]
            pub const fn message_type(&self) -> MessageType {
                match self {
                    $(Self::$variant(_) => MessageType::$variant,)+
                }
            }
        }

        /// Every concrete operational message in the recovered schema, in
        /// numeric trailer-tag order.
        pub const REGISTRY: &[RegistryEntry] = &[
            $(RegistryEntry {
                message_type: MessageType::$variant,
                name: $name,
                rust_type_name: stringify!($message),
            },)+
        ];

        fn registered_entry(message_type: MessageType) -> Option<RegistryEntry> {
            match message_type {
                $(MessageType::$variant => Some(RegistryEntry {
                    message_type: MessageType::$variant,
                    name: $name,
                    rust_type_name: stringify!($message),
                }),)+
                MessageType::Undefined | MessageType::NumberOfMessageTypes => None,
            }
        }

        fn decode_known(
            message_type: MessageType,
            body: &[u8],
        ) -> Result<DecodedMessage, prost::DecodeError> {
            use prost::Message as _;

            match message_type {
                $(MessageType::$variant => crate::proto::$message::decode(body)
                    .map(Box::new)
                    .map(DecodedMessage::$variant),)+
                MessageType::Undefined | MessageType::NumberOfMessageTypes => unreachable!(
                    "sentinels are rejected before registered protobuf decode"
                ),
            }
        }
    };
}

operational_messages!(
    (Grid, GridMessage, "Grid"),
    (SetlistPosition, SetlistPositionMessage, "SetlistPosition"),
    (IoSettings, IoSettingsMessage, "IOSettings"),
    (File, FileMessage, "File"),
    (IoMeter, IoMeterMessage, "IOMeter"),
    (Tuner, TunerMessage, "Tuner"),
    (Diagnostics, DiagnosticsMessage, "Diagnostics"),
    (MidiSettings, MidiSettingsMessage, "MIDISettings"),
    (GeneralSettings, GeneralSettingsMessage, "GeneralSettings"),
    (Version, VersionMessage, "Version"),
    (
        ProductionAutomationMode,
        ProductionAutomationModeMessage,
        "ProductionAutomationMode"
    ),
    (GridMove, GridMoveMessage, "GridMove"),
    (Scene, SceneMessage, "Scene"),
    (Mode, ModeMessage, "Mode"),
    (RecallPreset, RecallPresetMessage, "RecallPreset"),
    (
        EnableCaptureOut,
        EnableCaptureOutMessage,
        "EnableCaptureOut"
    ),
    (MasterVolume, MasterVolumeMessage, "MasterVolume"),
    (CloudLogin, CloudLoginMessage, "CloudLogin"),
    (
        DefaultParameters,
        DefaultParametersMessage,
        "DefaultParameters"
    ),
    (
        RecentsFavorites,
        RecentsFavoritesMessage,
        "RecentsFavorites"
    ),
    (UndoRedo, UndoRedoMessage, "UndoRedo"),
    (SceneCopy, SceneCopyMessage, "SceneCopy"),
    (SceneLabel, SceneLabelMessage, "SceneLabel"),
    (ShowGigView, ShowGigViewMessage, "ShowGigView"),
    (Screenshot, ScreenshotMessage, "Screenshot"),
    (CpuLoad, CpuLoadMessage, "CPULoad"),
    (ShowTuner, ShowTunerMessage, "ShowTuner"),
    (Looper, LooperMessage, "Looper"),
    (ProductForward, ProductForwardMessage, "ProductForward"),
    (BackupsForward, BackupsForwardMessage, "BackupsForward"),
    (LogsForward, LogsForwardMessage, "LogsForward"),
    (KeepAlive, KeepAliveMessage, "KeepAlive"),
    (GlobalTempo, GlobalTempoMessage, "GlobalTempo"),
    (PresetDirty, PresetDirtyMessage, "PresetDirty"),
    (ModuleStats, ModuleStatsMessage, "ModuleStats"),
    (NeuralCapture, NeuralCaptureMessage, "NeuralCapture"),
    (GridModelMeter, GridModelMeterMessage, "GridModelMeter"),
    (GlobalEq, GlobalEqMessage, "GlobalEQ"),
    (RecentSearches, RecentSearchesMessage, "RecentSearches"),
    (LocalBackup, LocalBackupMessage, "LocalBackup"),
    (CloudBackup, CloudBackupMessage, "CloudBackup"),
    (
        CompilerInhibitedModules,
        CompilerInhibitedModulesMessage,
        "CompilerInhibitedModules"
    ),
    (SystemTimeSync, SystemTimeSyncMessage, "SystemTimeSync"),
    (Logs, LogsMessage, "Logs"),
    (
        ProcessDownloadsQueue,
        ProcessDownloadsQueueMessage,
        "ProcessDownloadsQueue"
    ),
    (CloudProduct, CloudProductMessage, "CloudProduct"),
    (Confirmation, ConfirmationMessage, "Confirmation"),
    (SceneColor, SceneColorMessage, "SceneColor"),
    (Connection, ConnectionMessage, "Connection"),
    (NewModels, NewModelsMessage, "NewModels"),
    (ModelRepo, ModelRepoMessage, "ModelRepo"),
    (
        ResetCommsBuffers,
        ResetCommsBuffersMessage,
        "ResetCommsBuffers"
    ),
    (
        SuspendConnection,
        SuspendConnectionMessage,
        "SuspendConnection"
    ),
    (PinnedModels, PinnedModelsMessage, "PinnedModels"),
    (GigViewButton, GigViewButtonMessage, "GigViewButton"),
    (GenericError, GenericErrorMessage, "GenericError"),
    (BulkOperation, BulkOperationMessage, "BulkOperation"),
    (License, LicenseMessage, "License"),
    (PresetSpeedTest, PresetSpeedTestMessage, "PresetSpeedTest"),
    (Updater, UpdaterMessage, "Updater"),
    (UpdaterForward, UpdaterForwardMessage, "UpdaterForward"),
    (GainCalibration, GainCalibrationMessage, "GainCalibration"),
    (NeuralCapture2, NeuralCapture2Message, "NeuralCapture2"),
    (Serialization, SerializationMessage, "Serialization"),
    (TestFarm, TestFarmMessage, "TestFarm"),
    (ProductionTest, ProductionTestMessage, "ProductionTest"),
    (
        LoadAutomatedTestPreset,
        LoadAutomatedTestPresetMessage,
        "LoadAutomatedTestPreset"
    ),
    (
        SetTestPresetInputOutputPorts,
        SetTestPresetInputOutputPortsMessage,
        "SetTestPresetInputOutputPorts"
    ),
    (
        SetTestPresetSplitMixPoints,
        SetTestPresetSplitMixPointsMessage,
        "SetTestPresetSplitMixPoints"
    ),
    (
        GenerateTestPreset,
        GenerateTestPresetMessage,
        "GenerateTestPreset"
    ),
);

/// Return the registered protobuf name for an operational numeric trailer tag.
///
/// Sentinels and unknown future tags return `None`. The caller retains the raw
/// numeric tag, so an unknown value is never represented as `Undefined`.
#[must_use]
pub fn registered_name(message_type: u16) -> Option<&'static str> {
    let message_type = MessageType::try_from(i32::from(message_type)).ok()?;
    registered_entry(message_type).map(|entry| entry.name)
}

/// Decode a protobuf body using its operational numeric trailer tag.
///
/// # Errors
///
/// Returns [`RegistryError::Sentinel`] for tags 0 and 71,
/// [`RegistryError::Unknown`] for a tag absent from this schema, or
/// [`RegistryError::Decode`] for a malformed registered body. Every error
/// retains the original numeric tag.
pub fn decode_registered(message_type: u16, body: &[u8]) -> Result<DecodedMessage, RegistryError> {
    let Ok(known) = MessageType::try_from(i32::from(message_type)) else {
        return Err(RegistryError::Unknown(message_type));
    };
    if registered_entry(known).is_none() {
        return Err(RegistryError::Sentinel(message_type));
    }
    decode_known(known, body).map_err(|source| RegistryError::Decode {
        message_type,
        source,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_contains_all_seventy_operational_types_in_order() {
        assert_eq!(REGISTRY.len(), 70);
        for (index, entry) in REGISTRY.iter().enumerate() {
            let tag = u16::try_from(index + 1).unwrap();
            assert_eq!(entry.message_type as u16, tag, "registry index {index}");
            assert_eq!(
                MessageType::try_from(i32::from(tag)),
                Ok(entry.message_type)
            );
            assert_eq!(registered_name(tag), Some(entry.name));
            assert_eq!(entry.name, entry.message_type.as_str_name());
            assert!(entry.rust_type_name.ends_with("Message"));
        }
    }

    #[test]
    fn every_registered_empty_message_decodes_and_round_trips_its_type() {
        for entry in REGISTRY {
            let tag = entry.message_type as u16;
            let decoded = decode_registered(tag, &[]).unwrap_or_else(|error| {
                panic!("empty {} ({tag}) did not decode: {error}", entry.name)
            });
            assert_eq!(decoded.message_type(), entry.message_type, "{}", entry.name);
        }
    }

    #[test]
    fn sentinels_are_not_operational_messages() {
        for tag in [
            MessageType::Undefined as u16,
            MessageType::NumberOfMessageTypes as u16,
        ] {
            assert_eq!(registered_name(tag), None);
            let error = decode_registered(tag, &[]).unwrap_err();
            assert!(matches!(error, RegistryError::Sentinel(value) if value == tag));
            assert_eq!(error.message_type(), tag);
        }
    }

    #[test]
    fn unknown_future_tags_retain_their_numeric_value() {
        for tag in [72, 9999, u16::MAX] {
            assert_eq!(registered_name(tag), None);
            let error = decode_registered(tag, &[]).unwrap_err();
            assert!(matches!(error, RegistryError::Unknown(value) if value == tag));
            assert_eq!(error.message_type(), tag);
        }
    }

    #[test]
    fn malformed_registered_body_reports_its_numeric_type() {
        let tag = MessageType::Version as u16;
        let error = decode_registered(tag, &[0x0a, 0x02, 0x01]).unwrap_err();
        assert!(matches!(error, RegistryError::Decode { message_type, .. } if message_type == tag));
        assert_eq!(error.message_type(), tag);
    }
}
