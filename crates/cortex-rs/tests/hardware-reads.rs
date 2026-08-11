// SPDX-FileCopyrightText: 2026 Dr Marcus Baw
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Hardware checks for wider client reads and explicitly reversible mutations.

use std::sync::{Arc, Barrier};
use std::time::Duration;

use cortex_rs::client::USER_SETLIST;
use cortex_rs::proto::{
    BinaryPreset, GeneralSettingsMessage, GlobalEqMessage, MidiMessageInfo, ModeMessage, Model,
    NeuralCaptureMessage, Param, TunerMessage, general_settings_message, global_eq_message,
    mode_message, param, param_value, tuner_message,
};
use cortex_rs::{
    CAPTURE_FILE_NAME_PARAM, Catalog, DEFAULT_CAPTURE_MODEL, DeviceKind, ExpressionBypassMode,
    ExpressionPedal, FIRST_IR_LOADER_MODEL, Footswitch, FootswitchModeSlot, GeneralSettingsPatch,
    GlobalBypassPatch, GlobalBypassState, GlobalEqBandPatch, GlobalEqFilter, GlobalEqOutputPatch,
    InputPort, InputPortPatch, Instrument, LibraryEntry, MasterVolumeAssignment,
    MasterVolumeAssignmentPatch, MetronomeRouting, MetronomeSound, MidiOut, MidiSource,
    OutputPairingPatch, OutputPort, OutputPortPatch, ParameterInput, ParameterKind,
    ParameterTarget, ParameterWrite, QuadCortex, RecallConsent, Row, SaveConfirmation, SavePolicy,
    SceneBypassBehavior, ScratchOverride, ScratchRange, TempoParameter, TempoSubdivision,
    TimeSignature, TunerInput, UsbPortPatch, Value, midi_out, preset_load_midi_out,
};

#[test]
#[ignore = "requires an operator to tap New Neural Capture on a real Quad Cortex"]
fn observe_capture_dialog_message_version() -> cortex_rs::Result<()> {
    let session = Arc::new(cortex_rs::Session::open(DeviceKind::QuadCortex)?);
    session.connect(Duration::from_secs(10), Duration::from_secs(1))?;
    let barrier = Arc::new(Barrier::new(3));

    let v1_session = session.clone();
    let v1_barrier = barrier.clone();
    let v1 = std::thread::spawn(move || {
        v1_session.collect(
            cortex_rs::proto::cortex_message_type::Enum::NeuralCapture,
            || {
                v1_barrier.wait();
                Ok(())
            },
            Duration::from_secs(60),
            |_| true,
        )
    });
    let v2_session = session.clone();
    let v2_barrier = barrier.clone();
    let v2 = std::thread::spawn(move || {
        v2_session.collect(
            cortex_rs::proto::cortex_message_type::Enum::NeuralCapture2,
            || {
                v2_barrier.wait();
                Ok(())
            },
            Duration::from_secs(60),
            |_| true,
        )
    });

    barrier.wait();
    eprintln!("[WAITING] Tap New Neural Capture once and stop on its first screen");
    let v1 = v1
        .join()
        .map_err(|_| cortex_rs::Error::Session("v1 observer panicked".into()))??;
    let v2 = v2
        .join()
        .map_err(|_| cortex_rs::Error::Session("v2 observer panicked".into()))??;
    session.close();

    let v1_decoded = v1
        .iter()
        .filter_map(|message| {
            prost::Message::decode(message.body.as_ref())
                .ok()
                .map(|message: cortex_rs::proto::NeuralCaptureMessage| message)
        })
        .collect::<Vec<_>>();
    for (index, message) in v1_decoded.iter().enumerate() {
        let try_dialog = message.try_to_show_dialog.as_ref().map(|value| {
            let cortex_rs::proto::neural_capture_message::TryToShowDialog::TryToShowDialog(value) =
                value;
            *value
        });
        let show_dialog = message.show_dialog.as_ref().map(|value| {
            let cortex_rs::proto::neural_capture_message::ShowDialog::ShowDialog(value) = value;
            *value
        });
        let state = message.state.as_ref().map(|value| {
            let cortex_rs::proto::neural_capture_message::State::State(value) = value;
            *value
        });
        let progress = message.progress.as_ref().map(|value| {
            let cortex_rs::proto::neural_capture_message::Progress::Progress(value) = value;
            *value
        });
        eprintln!(
            "[V1 {index}] action={} request_id={} try_dialog={try_dialog:?} fail_reason={} show_dialog={show_dialog:?} state={state:?} progress={progress:?} parameters={} save_info={} error={} model_ab={}",
            message.action,
            u8::from(message.request_id.is_some()),
            u8::from(message.show_dialog_fail_reason.is_some()),
            message.parameters.len(),
            u8::from(message.save_info.is_some()),
            u8::from(message.error_id.is_some()),
            u8::from(message.model_ab.is_some())
        );
    }
    let v1_dialog_requests = v1_decoded
        .iter()
        .filter(|message| {
            matches!(
                message.try_to_show_dialog.as_ref(),
                Some(
                    cortex_rs::proto::neural_capture_message::TryToShowDialog::TryToShowDialog(
                        true
                    )
                )
            )
        })
        .count();
    let v2_dialog_requests = v2
        .iter()
        .filter_map(|message| {
            prost::Message::decode(message.body.as_ref())
                .ok()
                .map(|message: cortex_rs::proto::NeuralCapture2Message| message)
        })
        .filter(|message| message.open_dialog.is_some())
        .count();
    eprintln!(
        "[OBSERVED] v1 messages={}, v1 dialog requests={v1_dialog_requests}, v2 messages={}, v2 dialog requests={v2_dialog_requests}",
        v1.len(),
        v2.len()
    );
    if v1.is_empty() && v2.is_empty() {
        return Err(cortex_rs::Error::NotFound(
            "no NeuralCapture or NeuralCapture2 message was observed".into(),
        ));
    }
    Ok(())
}

#[derive(Clone, Copy)]
enum RowControl {
    Splitter,
    Mixer,
    LaneOutput,
    InputGate,
}

impl RowControl {
    const fn catalog_model(self) -> u32 {
        match self {
            Self::Splitter => 10_004,
            Self::Mixer => 11_000,
            Self::LaneOutput => 23_000,
            Self::InputGate => 28_000,
        }
    }

    const fn label(self) -> &'static str {
        match self {
            Self::Splitter => "splitter",
            Self::Mixer => "mixer",
            Self::LaneOutput => "lane output",
            Self::InputGate => "input gate",
        }
    }
}

fn control_model(chain: &cortex_rs::proto::Chain, control: RowControl) -> Option<&Model> {
    match control {
        RowControl::Splitter => chain.combined_splitter.first().or(chain.splitter.first()),
        RowControl::Mixer => chain.mixer.first(),
        RowControl::LaneOutput => chain.output_control.first(),
        RowControl::InputGate => chain.input_control.first(),
    }
}

fn parameter_index(parameter: &Param, position: usize) -> Option<u32> {
    parameter.index.as_ref().map_or_else(
        || u32::try_from(position).ok(),
        |index| {
            let param::Index::Index(index) = index;
            Some(*index)
        },
    )
}

fn active_float(parameter: &Param, scene: usize) -> Option<f32> {
    let position = if parameter.param_values.len() > scene {
        scene
    } else {
        0
    };
    match parameter.param_values.get(position)?.value {
        Some(param_value::Value::FloatValue(value)) => Some(value),
        _ => None,
    }
}

fn changed_value(value: f32) -> f32 {
    if (value - 0.2).abs() > 0.05 { 0.2 } else { 0.8 }
}

fn file_ops_failure(message: impl Into<String>) -> cortex_rs::Error {
    cortex_rs::Error::Session(message.into())
}

fn same_audio_state(left: &BinaryPreset, right: &BinaryPreset) -> bool {
    left.chains == right.chains
        && left.bypass == right.bypass
        && left.tempo_program_data == right.tempo_program_data
        && left.scene_tempo == right.scene_tempo
        && left.scene_labels == right.scene_labels
        && left.scene_colors == right.scene_colors
}

fn midi_info_matches(actual: &MidiMessageInfo, expected: MidiOut) -> bool {
    actual.r#type == expected.message_type as u32
        && actual.channel == expected.channel
        && actual.param1 == expected.param1
        && actual.param2 == expected.param2
        && actual.param3 == expected.param3
}

fn verify_stored_midi(
    preset: &BinaryPreset,
    expected: &[(MidiSource, MidiOut)],
    expected_load: MidiOut,
) -> cortex_rs::Result<()> {
    let decoded = midi_out(preset)?;
    if decoded.len() != expected.len()
        || expected.iter().any(|(source, message)| {
            decoded
                .get(source)
                .is_none_or(|messages| messages.as_slice() != std::slice::from_ref(message))
        })
    {
        return Err(file_ops_failure(
            "stored preset did not contain the exact source MIDI messages",
        ));
    }
    if preset_load_midi_out(preset)? != vec![expected_load] {
        return Err(file_ops_failure(
            "stored preset did not contain the exact preset-load MIDI message",
        ));
    }

    if preset.midi_messages_general_v2.len() != 120 {
        return Err(file_ops_failure(
            "stored preset did not expose the 10x12 source MIDI layout",
        ));
    }
    for source in 0..10_usize {
        for offset in 0..12_usize {
            let actual = &preset.midi_messages_general_v2[source * 12 + offset];
            let wanted = expected
                .iter()
                .find(|(candidate, _)| *candidate as usize == source)
                .map(|(_, message)| *message);
            if offset == 0 {
                if wanted.is_some_and(|message| !midi_info_matches(actual, message))
                    || wanted.is_none() && actual != &MidiMessageInfo::default()
                {
                    return Err(file_ops_failure(
                        "stored source MIDI message occupied an unexpected slot",
                    ));
                }
            } else if actual != &MidiMessageInfo::default() {
                return Err(file_ops_failure(
                    "stored source MIDI layout retained an unexpected extra message",
                ));
            }
        }
    }

    // Some firmware exposes the ten-slot legacy first-message mirror and some
    // omits it. When present, require the exact source-indexed mirror.
    if !preset.midi_messages_general.is_empty() {
        if preset.midi_messages_general.len() != 10 {
            return Err(file_ops_failure(
                "legacy source MIDI mirror had an unexpected length",
            ));
        }
        for (source, actual) in preset.midi_messages_general.iter().enumerate() {
            let wanted = expected
                .iter()
                .find(|(candidate, _)| *candidate as usize == source)
                .map(|(_, message)| *message);
            if wanted.is_some_and(|message| !midi_info_matches(actual, message))
                || wanted.is_none() && actual != &MidiMessageInfo::default()
            {
                return Err(file_ops_failure(
                    "legacy source MIDI mirror did not match the first source messages",
                ));
            }
        }
    }
    Ok(())
}

fn tempo_value(preset: &BinaryPreset, index: u32) -> Option<f32> {
    let model = preset.tempo_program_data.iter().find(|model| {
        matches!(
            model.hash,
            Some(cortex_rs::proto::model::Hash::Hash(
                cortex_rs::grid::TEMPO_CONTROL
            ))
        )
    })?;
    model
        .params
        .iter()
        .enumerate()
        .find(|(position, parameter)| parameter_index(parameter, *position) == Some(index))
        .and_then(|(_, parameter)| active_float(parameter, 0))
}

fn verify_tempo_value(
    qc: &QuadCortex,
    parameter: TempoParameter,
    target: f32,
    timeout: Duration,
) -> cortex_rs::Result<BinaryPreset> {
    let preset = qc.read_current_preset(timeout)?;
    let actual = tempo_value(&preset, parameter.index()).ok_or_else(|| {
        cortex_rs::Error::NotFound("targeted tempo parameter is absent on read-back".into())
    })?;
    if (actual - target).abs() > 0.000_1 {
        return Err(cortex_rs::Error::Session(
            "fresh live read did not contain the targeted tempo value".into(),
        ));
    }
    Ok(preset)
}

fn different_option(value: f32, count: u32) -> u32 {
    if value.abs() > 0.000_1 { 0 } else { count - 1 }
}

fn model_at(preset: &BinaryPreset, row: Row, column: u32) -> Option<&Model> {
    preset
        .chains
        .get(row.wire() as usize)?
        .models
        .get(column as usize)
}

fn model_id(model: &Model) -> Option<u32> {
    match model.hash {
        Some(cortex_rs::proto::model::Hash::Hash(id)) if id != 0 => Some(id),
        _ => None,
    }
}

fn compatible_or_replaceable_cell(
    preset: &BinaryPreset,
    compatible: impl Fn(u32) -> bool,
) -> cortex_rs::Result<(Row, u32, bool)> {
    let mut replacement = None;
    for (row, chain) in preset.chains.iter().enumerate() {
        for column in 0..8 {
            let model = chain.models.get(column);
            let id = model.and_then(model_id);
            let row = Row::try_from_wire(u32::try_from(row).map_err(|_| {
                cortex_rs::Error::InvalidRow("fixture row does not fit on the wire".into())
            })?)?;
            let column = u32::try_from(column).map_err(|_| {
                cortex_rs::Error::InvalidParameter("fixture column does not fit on the wire".into())
            })?;
            if id.is_some_and(&compatible) {
                return Ok((row, column, true));
            }
            if id.is_some() && (replacement.is_none() || id == Some(12_044)) {
                replacement = Some((row, column, false));
            }
        }
    }
    replacement.ok_or_else(|| {
        cortex_rs::Error::NotFound(
            "USER 6A has neither a compatible block nor a replaceable occupied cell".into(),
        )
    })
}

fn string_parameter(model: &Model, wanted: u32) -> Option<&str> {
    model
        .params
        .iter()
        .enumerate()
        .find(|(position, parameter)| parameter_index(parameter, *position) == Some(wanted))
        .and_then(|(_, parameter)| parameter.param_values.first())
        .and_then(|value| match value.value.as_ref() {
            Some(param_value::Value::StringValue(value)) => Some(value.as_str()),
            _ => None,
        })
}

fn float_parameter(model: &Model, wanted: u32, active_scene: usize) -> Option<f32> {
    model
        .params
        .iter()
        .enumerate()
        .find(|(position, parameter)| parameter_index(parameter, *position) == Some(wanted))
        .and_then(|(_, parameter)| active_float(parameter, active_scene))
}

fn expression_candidate(
    preset: &BinaryPreset,
    catalog: &Catalog,
) -> cortex_rs::Result<(Row, u32, u32)> {
    preset
        .chains
        .iter()
        .enumerate()
        .find_map(|(row, chain)| {
            chain.models.iter().enumerate().find_map(|(column, model)| {
                let Some(cortex_rs::proto::model::Hash::Hash(model_id)) = model.hash else {
                    return None;
                };
                let specification = catalog.get(model_id)?;
                model
                    .params
                    .iter()
                    .enumerate()
                    .find_map(|(position, parameter)| {
                        let index = parameter_index(parameter, position)?;
                        let parameter_specification =
                            specification.parameters.get(index as usize)?;
                        if parameter_specification.kind.is_read_only()
                            || matches!(
                                parameter_specification.kind,
                                ParameterKind::Str | ParameterKind::Empty | ParameterKind::Unknown
                            )
                        {
                            return None;
                        }
                        Some((
                            Row::try_from_wire(u32::try_from(row).ok()?).ok()?,
                            u32::try_from(column).ok()?,
                            index,
                        ))
                    })
            })
        })
        .ok_or_else(|| {
            cortex_rs::Error::NotFound(
                "fixture has no occupied block with a writable numeric parameter".into(),
            )
        })
}

fn exercise_row_control(
    qc: &QuadCortex,
    baseline: &BinaryPreset,
    catalog: &Catalog,
    active_scene: usize,
    control: RowControl,
    timeout: Duration,
) -> cortex_rs::Result<()> {
    let catalog_model = catalog.get(control.catalog_model()).ok_or_else(|| {
        cortex_rs::Error::NotFound(format!(
            "{} subcheck: required catalog control is absent",
            control.label()
        ))
    })?;
    let candidate = baseline
        .chains
        .iter()
        .enumerate()
        .filter(|(row, _)| {
            !matches!(control, RowControl::Splitter | RowControl::Mixer) || row % 2 == 0
        })
        .find_map(|(row, chain)| {
            let model = control_model(chain, control)?;
            model
                .params
                .iter()
                .enumerate()
                .find_map(|(position, parameter)| {
                    let index = parameter_index(parameter, position)?;
                    let specification = catalog_model.parameters.get(index as usize)?;
                    if specification.kind.is_read_only()
                        || matches!(
                            specification.kind,
                            ParameterKind::Str | ParameterKind::Empty | ParameterKind::Unknown
                        )
                    {
                        return None;
                    }
                    let value = active_float(parameter, active_scene)?;
                    Some((u32::try_from(row).ok()?, index, changed_value(value)))
                })
        })
        .ok_or_else(|| {
            cortex_rs::Error::NotFound(format!(
                "{} subcheck: fixture has no writable numeric sub-control",
                control.label()
            ))
        })?;
    let (row, index, target) = candidate;
    let row = Row::try_from_wire(row)?;
    match control {
        RowControl::Splitter => qc.set_splitter_param(row, index, target, None, false)?,
        RowControl::Mixer => qc.set_mixer_param(row, index, target, None, false)?,
        RowControl::LaneOutput => qc.set_lane_output(row, index, target, None, false)?,
        RowControl::InputGate => qc.set_input_gate(row, index, target, None, false)?,
    }

    let after = qc.read_current_preset(timeout)?;
    let parameter = after
        .chains
        .get(row.wire() as usize)
        .and_then(|chain| control_model(chain, control))
        .and_then(|model| {
            model
                .params
                .iter()
                .enumerate()
                .find(|(position, parameter)| parameter_index(parameter, *position) == Some(index))
                .map(|(_, parameter)| parameter)
        })
        .ok_or_else(|| {
            cortex_rs::Error::NotFound(format!(
                "{} subcheck: targeted control disappeared on read-back",
                control.label()
            ))
        })?;
    let actual = active_float(parameter, active_scene).ok_or_else(|| {
        cortex_rs::Error::NotFound(format!(
            "{} subcheck: targeted value is absent on read-back",
            control.label()
        ))
    })?;
    if (actual - target).abs() > 0.000_1 {
        return Err(cortex_rs::Error::Session(format!(
            "{} subcheck: fresh live read did not contain the targeted value",
            control.label()
        )));
    }
    Ok(())
}

fn exercise_split_mute(
    qc: &QuadCortex,
    baseline: &BinaryPreset,
    timeout: Duration,
) -> cortex_rs::Result<()> {
    let (row, current) = baseline
        .chains
        .iter()
        .enumerate()
        .filter(|(row, _)| row % 2 == 0)
        .find_map(|(row, chain)| {
            if control_model(chain, RowControl::Splitter).is_none()
                && control_model(chain, RowControl::Mixer).is_none()
            {
                return None;
            }
            Some((u32::try_from(row).ok()?, chain.mix_bypass.first()?.bypass))
        })
        .ok_or_else(|| {
            cortex_rs::Error::NotFound(
                "split mute subcheck: fixture has no split path with readable mute state".into(),
            )
        })?;
    let target = !current;
    let row = Row::try_from_wire(row)?;
    qc.set_split_mute(row, target)?;
    let after = qc.read_current_preset(timeout)?;
    let read_back = after
        .chains
        .get(row.wire() as usize)
        .ok_or_else(|| {
            cortex_rs::Error::NotFound(
                "split mute subcheck: targeted row disappeared on read-back".into(),
            )
        })?
        .mix_bypass
        .as_slice();
    if read_back.len() != 8 || read_back.iter().any(|value| value.bypass != target) {
        return Err(cortex_rs::Error::Session(
            "split mute subcheck: fresh live read did not show the target in all eight scenes"
                .into(),
        ));
    }
    Ok(())
}

#[test]
#[ignore = "creates and unconditionally deletes uniquely fictional USER setlists on a real Quad Cortex; USER 6A is the only existing preset recalled"]
fn prot_006_10_file_operations_converge_and_cleanup() -> cortex_rs::Result<()> {
    let qc = QuadCortex::connect(
        DeviceKind::QuadCortex,
        Duration::from_secs(10),
        Duration::from_secs(1),
    )?;
    let timeout = Duration::from_secs(60);
    let suffix = std::process::id();
    let source_name = format!("cortex-prot00610-{suffix}-source");
    let duplicate_name = format!("cortex-prot00610-{suffix}-duplicate");
    let generated_names = [source_name.as_str(), duplicate_name.as_str()];
    let mut created = Vec::new();

    let operation = (|| -> cortex_rs::Result<()> {
        let existing = qc.list_folders(Duration::from_secs(20))?;
        for name in generated_names {
            let key = format!("{}/{name}", cortex_rs::client::USER_SETLIST_ROOT);
            if existing.iter().any(|folder| folder.key == key) {
                return Err(file_ops_failure(format!(
                    "generated temporary setlist {key} already exists; aborting before mutation"
                )));
            }
        }

        let source_folder = qc.create_setlist(&source_name, timeout)?;
        let created_source_name = source_folder
            .key
            .strip_prefix(&format!("{}/", cortex_rs::client::USER_SETLIST_ROOT))
            .ok_or_else(|| file_ops_failure("created source escaped the USER setlist root"))?
            .to_string();
        created.push(created_source_name);
        let expected_source = format!("{}/{}", cortex_rs::client::USER_SETLIST_ROOT, source_name);
        if source_folder.key != expected_source {
            return Err(file_ops_failure(format!(
                "device created the temporary source under unexpected key {}",
                source_folder.key
            )));
        }
        // A healthy subscription can finish Incomplete when its initial grid
        // seed is absent. A targeted side-effect-free live read repairs that
        // baseline; a stream gap remains Invalidated and still fails closed.
        qc.read_current_preset(timeout)?;

        let policy = SavePolicy::new(
            source_folder.key.clone(),
            vec![ScratchRange::new("1A", "1B")?],
        )?;
        let preparation = qc.prepare_save_before_editing(
            &policy,
            &source_folder.key,
            "1A",
            ScratchOverride::ScratchOnly,
            RecallConsent::DiscardWorkingCopy,
            timeout,
        )?;
        qc.recall_preset(USER_SETLIST, "6A", false, timeout)?;
        qc.save_prepared(
            &policy,
            preparation,
            SaveConfirmation::explicit(true)?,
            Some("Fictional Bass Source"),
            Instrument::Bass,
            timeout,
        )?;
        qc.copy_preset(
            &policy,
            &source_folder.key,
            "1A",
            &source_folder.key,
            "1B",
            Some("Fictional Vocal Copy"),
            Instrument::Vocal,
            RecallConsent::DiscardWorkingCopy,
            timeout,
        )?;

        let source_entries = qc.list_presets(&source_folder.key, timeout, false)?;
        if source_entries
            .iter()
            .find(|entry| entry.index == 0)
            .and_then(|entry| entry.instrument)
            != Some(Instrument::Bass)
            || source_entries
                .iter()
                .find(|entry| entry.index == 1)
                .and_then(|entry| entry.instrument)
                != Some(Instrument::Vocal)
        {
            return Err(file_ops_failure(
                "fresh source listing did not preserve discriminating Bass/Vocal tags",
            ));
        }

        let duplicate = qc.duplicate_setlist(
            &source_name,
            &duplicate_name,
            Some(2),
            cortex_rs::RecallConsent::DiscardWorkingCopy,
            timeout,
        )?;
        created.push(duplicate_name.clone());
        if !duplicate.complete() {
            return Err(file_ops_failure(duplicate.failure.unwrap_or_else(|| {
                "duplicate returned incomplete progress".into()
            })));
        }
        let duplicate_entries = qc.list_presets(&duplicate.destination.key, timeout, false)?;
        if duplicate_entries
            .iter()
            .find(|entry| entry.index == 0)
            .and_then(|entry| entry.instrument)
            != Some(Instrument::Bass)
            || duplicate_entries
                .iter()
                .find(|entry| entry.index == 1)
                .and_then(|entry| entry.instrument)
                != Some(Instrument::Vocal)
        {
            return Err(file_ops_failure(
                "fresh duplicate listing did not preserve discriminating Bass/Vocal tags",
            ));
        }

        let source_audio = qc.read_preset(&source_folder.key, "1A", false, timeout)?;
        let duplicate_audio = qc.read_preset(&duplicate.destination.key, "1A", false, timeout)?;
        if !same_audio_state(&source_audio, &duplicate_audio) {
            return Err(file_ops_failure(
                "recalled duplicate did not preserve the source audio-state fields",
            ));
        }
        Ok(())
    })();

    let mut cleanup_failures = Vec::new();
    for name in created.iter().rev() {
        if let Err(error) = qc.delete_setlist(name, timeout) {
            cleanup_failures.push(format!("deleting {name}: {error}"));
        }
    }
    // A create can land without returning. Inspect both generated names and
    // attempt deletion even when no creation receipt reached `created`.
    match qc.list_folders(Duration::from_secs(20)) {
        Ok(folders) => {
            let prefix = format!("cortex-prot00610-{suffix}-");
            for name in folders.iter().filter_map(|folder| {
                folder
                    .key
                    .strip_prefix(&format!("{}/", cortex_rs::client::USER_SETLIST_ROOT))
                    .filter(|name| name.starts_with(&prefix))
            }) {
                if !created.iter().any(|created_name| created_name == name)
                    && let Err(error) = qc.delete_setlist(name, timeout)
                {
                    cleanup_failures.push(format!("deleting unconfirmed {name}: {error}"));
                }
            }
        }
        Err(error) => cleanup_failures.push(format!(
            "enumerating generated names after mutation: {error}"
        )),
    }
    let restore = qc.recall_preset(USER_SETLIST, "6A", false, timeout);
    qc.disconnect();
    let cleanup = if cleanup_failures.is_empty() {
        Ok(())
    } else {
        Err(file_ops_failure(format!(
            "temporary setlist cleanup failed: {}",
            cleanup_failures.join("; ")
        )))
    };

    match (operation, cleanup, restore) {
        (Ok(()), Ok(()), Ok(())) => Ok(()),
        (operation, cleanup, restore) => Err(file_ops_failure(format!(
            "operation: {}; cleanup: {}; final USER 6A recall: {}",
            operation
                .err()
                .map_or_else(|| "ok".into(), |error| error.to_string()),
            cleanup
                .err()
                .map_or_else(|| "ok".into(), |error| error.to_string()),
            restore
                .err()
                .map_or_else(|| "ok".into(), |error| error.to_string())
        ))),
    }
}

#[test]
#[ignore = "creates and unconditionally deletes one uniquely fictional USER setlist; outputs must remain disconnected because recall emits preset-load MIDI"]
fn prot_006_9_midi_persists_in_saved_preset_and_cleanup() -> cortex_rs::Result<()> {
    let qc = QuadCortex::connect(
        DeviceKind::QuadCortex,
        Duration::from_secs(10),
        Duration::from_secs(1),
    )?;
    let timeout = Duration::from_secs(60);
    let generated_name = format!("cortex-prot0069-midi-{}", uuid::Uuid::new_v4().simple());
    let generated_prefix = generated_name.clone();
    let mut cleanup_authorized = false;

    let operation = (|| -> cortex_rs::Result<()> {
        let existing = qc.list_folders(Duration::from_secs(20))?;
        let root = format!("{}/", cortex_rs::client::USER_SETLIST_ROOT);
        if existing.iter().any(|folder| {
            folder
                .key
                .strip_prefix(&root)
                .is_some_and(|name| name.starts_with(&generated_prefix))
        }) {
            return Err(file_ops_failure(
                "generated temporary MIDI setlist already exists; aborting before mutation",
            ));
        }
        // Once a complete preflight proves the unique prefix unused, any later
        // matching setlist belongs to this operation even if CREATE lands
        // without returning its ownership receipt.
        cleanup_authorized = true;

        let folder = qc.create_setlist(&generated_name, timeout)?;
        let expected_key = format!(
            "{}/{}",
            cortex_rs::client::USER_SETLIST_ROOT,
            generated_name
        );
        if folder.key != expected_key {
            return Err(file_ops_failure(
                "temporary MIDI setlist was created under an unexpected key",
            ));
        }

        if qc.state_cache().status().phase != cortex_rs::CachePhase::Live {
            qc.read_current_preset(timeout)?;
        }
        if qc.state_cache().status().phase != cortex_rs::CachePhase::Live {
            return Err(file_ops_failure(
                "live subscribed baseline was not established before save preparation",
            ));
        }

        let policy = SavePolicy::new(folder.key.clone(), vec![ScratchRange::new("1A", "1A")?])?;
        let preparation = qc.prepare_save_before_editing(
            &policy,
            &folder.key,
            "1A",
            ScratchOverride::ScratchOnly,
            RecallConsent::DiscardWorkingCopy,
            timeout,
        )?;
        if !preparation.view().target_was_empty {
            return Err(file_ops_failure(
                "temporary MIDI destination 1A was not empty before editing",
            ));
        }

        qc.recall_preset(USER_SETLIST, "6A", false, timeout)?;
        for source in [
            MidiSource::FootswitchA,
            MidiSource::FootswitchB,
            MidiSource::FootswitchC,
            MidiSource::FootswitchD,
            MidiSource::FootswitchE,
            MidiSource::FootswitchF,
            MidiSource::FootswitchG,
            MidiSource::FootswitchH,
            MidiSource::Expression1,
            MidiSource::Expression2,
        ] {
            qc.set_midi_out(source, &[])?;
        }

        let expected = [
            (MidiSource::FootswitchA, MidiOut::cc(1, 1, 1)?),
            (MidiSource::FootswitchB, MidiOut::cc_toggle(2, 2, 0, 2)?),
            (MidiSource::FootswitchC, MidiOut::pc(3, 3, 0, 0)?),
            (MidiSource::Expression1, MidiOut::expression_cc(4, 4, 0, 3)?),
        ];
        for (source, message) in expected {
            qc.set_midi_out(source, &[message])?;
        }
        let preset_load = MidiOut::cc(5, 5, 1)?;
        qc.set_preset_load_midi_out(&[preset_load])?;

        qc.save_prepared(
            &policy,
            preparation,
            SaveConfirmation::explicit(true)?,
            Some("Fictional MIDI Check"),
            Instrument::Other,
            timeout,
        )?;

        // Recalling the stored preset may emit its preset-load MIDI message.
        // This ignored test is authorized only with every output disconnected.
        qc.recall_preset(&folder.key, "1A", false, timeout)?;
        let stored = qc.read_current_preset(timeout)?;
        verify_stored_midi(&stored, &expected, preset_load)
    })();

    let mut cleanup_failures = Vec::new();
    if cleanup_authorized {
        let root = format!("{}/", cortex_rs::client::USER_SETLIST_ROOT);
        // Allow one full delete poll plus two independent directory scans.
        let cleanup_deadline = std::time::Instant::now() + timeout.saturating_mul(3);
        let mut absent_observations = 0;
        while std::time::Instant::now() < cleanup_deadline && absent_observations < 2 {
            if let Ok(folders) = qc.list_folders(Duration::from_secs(20)) {
                if !folders.iter().any(|folder| folder.key == USER_SETLIST) {
                    continue;
                }
                let leftovers = folders
                    .iter()
                    .filter_map(|folder| folder.key.strip_prefix(&root))
                    .filter(|name| name.starts_with(&generated_prefix))
                    .map(str::to_string)
                    .collect::<Vec<_>>();
                if leftovers.is_empty() {
                    absent_observations += 1;
                } else {
                    absent_observations = 0;
                    for name in leftovers {
                        let _ = qc.delete_setlist(&name, timeout);
                    }
                }
            }
        }
        if absent_observations < 2 {
            cleanup_failures.push("generated MIDI setlist absence did not converge".to_string());
        }
    }
    let restore = qc.recall_preset(USER_SETLIST, "6A", false, timeout);
    qc.disconnect();

    match (operation, cleanup_failures.is_empty(), restore) {
        (Ok(()), true, Ok(())) => Ok(()),
        (operation, cleanup_ok, restore) => Err(file_ops_failure(format!(
            "MIDI verification: {}; cleanup: {}; final USER 6A recall: {}",
            operation
                .err()
                .map_or_else(|| "ok".into(), |error| error.to_string()),
            if cleanup_ok { "ok" } else { "failed" },
            restore
                .err()
                .map_or_else(|| "ok".into(), |error| error.to_string())
        ))),
    }
}

#[test]
#[ignore = "requires an exclusively available real Quad Cortex"]
fn wider_state_reads_answer_without_exposing_device_data() {
    let qc = QuadCortex::connect(
        DeviceKind::QuadCortex,
        Duration::from_secs(10),
        Duration::from_secs(1),
    )
    .expect("connect handshake");
    let timeout = Duration::from_secs(30);
    let mut failures = Vec::new();

    macro_rules! read {
        ($name:literal, $operation:expr, $summary:expr) => {
            match $operation {
                Ok(value) => {
                    eprintln!(concat!("[PASS] ", $name, ": {}"), $summary(&value));
                    Some(value)
                }
                Err(error) => {
                    eprintln!(concat!("[FAIL] ", $name, ": {}"), error);
                    failures.push(format!(concat!($name, ": {}"), error));
                    None
                }
            }
        };
    }

    let captures = read!("captures", qc.captures(timeout), |items: &Vec<
        cortex_rs::LibraryEntry,
    >| format!(
        "{} entries",
        items.len()
    ));
    let captures_again = read!("captures repeat", qc.captures(timeout), |items: &Vec<
        cortex_rs::LibraryEntry,
    >| format!(
        "{} entries",
        items.len()
    ));
    if let (Some(first), Some(second)) = (&captures, &captures_again) {
        if first != second {
            failures.push("repeated capture listings differed".into());
        }
    }

    let _ = read!("IR library", qc.list_irs(None, timeout), |items: &Vec<
        cortex_rs::LibraryEntry,
    >| format!(
        "{} loadable entries",
        items.len()
    ));
    let _ = read!(
        "user IR folder",
        qc.list_irs(Some("2_q"), timeout),
        |items: &Vec<cortex_rs::LibraryEntry>| format!("{} loadable entries", items.len())
    );
    let _ = read!(
        "recents",
        qc.recents(timeout),
        |state: &cortex_rs::proto::RecentsFavoritesMessage| format!(
            "{} entries",
            state.items.len()
        )
    );
    let _ = read!("favorites", qc.favorites(timeout, 3), |items: &Vec<
        cortex_rs::proto::RecentsFavoritesItem,
    >| format!(
        "{} entries",
        items.len()
    ));
    let _ = read!("pinned models", qc.pinned_models(timeout), |items: &Vec<
        u32,
    >| format!(
        "{} ids",
        items.len()
    ));
    let _ = read!(
        "master volume",
        qc.master_volume(timeout),
        |_: &cortex_rs::proto::MasterVolumeMessage| "volume present".to_string()
    );
    let _ = read!(
        "looper",
        qc.looper(timeout),
        |_: &cortex_rs::proto::LooperMessage| "status present".to_string()
    );
    let _ = read!(
        "tuner",
        qc.tuner(timeout),
        |_: &cortex_rs::proto::TunerMessage| "input and reference settings present".to_string()
    );
    let _ = read!(
        "I/O settings",
        qc.io_settings(timeout),
        |message: &cortex_rs::proto::IoSettingsMessage| {
            let Some(cortex_rs::proto::io_settings_message::Settings::Settings(settings)) =
                message.settings.as_ref()
            else {
                return "settings absent".to_string();
            };
            format!(
                "{} input and {} output ports; USB, MIDI and pairings present: {}",
                settings.in_port.len(),
                settings.out_port.len(),
                settings.usb_port.is_some()
                    && settings.midi_port.is_some()
                    && message.xlr1_2_linked.is_some()
                    && message.out3_4_linked.is_some()
            )
        }
    );
    let _ = read!(
        "general settings",
        qc.settings(timeout),
        |_: &cortex_rs::proto::GeneralSettingsMessage| "scene bypass policy present".to_string()
    );
    let _ = read!(
        "global EQ",
        qc.global_eq(timeout),
        |_: &cortex_rs::proto::GlobalEqMessage| "bypass state present".to_string()
    );
    let _ = read!(
        "active mode",
        qc.mode(timeout),
        |_: &cortex_rs::proto::ModeMessage| "mode present".to_string()
    );
    let _ = read!(
        "mode cycle",
        qc.mode_cycle(timeout),
        |modes: &Vec<u32>| format!("{} slots", modes.len())
    );

    qc.disconnect();
    assert!(failures.is_empty(), "{}", failures.join("; "));
}

#[test]
#[ignore = "requires an exclusively available real Quad Cortex and temporarily mutates pins/favorites"]
fn pin_and_favorite_mutations_restore_exact_state() -> cortex_rs::Result<()> {
    let qc = QuadCortex::connect(
        DeviceKind::QuadCortex,
        Duration::from_secs(10),
        Duration::from_secs(1),
    )?;
    let timeout = Duration::from_secs(30);
    let baseline_pins = qc.pinned_models(timeout)?;
    let baseline_favorites = qc.favorites(timeout, 3)?;
    let catalog = Catalog::parse(&qc.fetch_model_repo(timeout)?)?;
    let candidate_model = catalog
        .models()
        .into_iter()
        .map(|model| model.id)
        .find(|id| !baseline_pins.contains(id))
        .ok_or_else(|| {
            cortex_rs::Error::NotFound("no unpinned model available for smoke test".into())
        })?;
    let candidate_favorite = qc
        .recents(timeout)?
        .items
        .into_iter()
        .find(|item| {
            !item.is_factory
                && item.folder_key.starts_with("/media/p4/Presets/")
                && !baseline_favorites.contains(item)
        })
        .ok_or_else(|| {
            cortex_rs::Error::NotFound(
                "no non-favorite user preset entry available in Recents for smoke test".into(),
            )
        })?;

    let mut favorite_may_be_added = false;
    let exercise = (|| -> cortex_rs::Result<()> {
        qc.pin_model(candidate_model)?;
        let after_first_pin = qc.pinned_models(timeout)?;
        if after_first_pin
            .iter()
            .filter(|&&id| id == candidate_model)
            .count()
            != 1
        {
            return Err(cortex_rs::Error::Session(
                "first pin did not append exactly one candidate id".into(),
            ));
        }

        qc.pin_model(candidate_model)?;
        let after_second_pin = qc.pinned_models(timeout)?;
        if after_second_pin
            .iter()
            .filter(|&&id| id == candidate_model)
            .count()
            != 2
        {
            return Err(cortex_rs::Error::Session(
                "second pin did not append a duplicate candidate id".into(),
            ));
        }

        qc.unpin_model(candidate_model)?;
        if qc.pinned_models(timeout)?.contains(&candidate_model) {
            return Err(cortex_rs::Error::Session(
                "unpin did not remove every candidate id".into(),
            ));
        }

        favorite_may_be_added = true;
        qc.add_favorite(&candidate_favorite, timeout)?;
        if !qc.favorites(timeout, 3)?.contains(&candidate_favorite) {
            return Err(cortex_rs::Error::Session(
                "favorite add was not visible in read-back".into(),
            ));
        }
        qc.remove_favorite(&candidate_favorite, timeout)?;
        favorite_may_be_added = false;
        if qc.favorites(timeout, 3)?.contains(&candidate_favorite) {
            return Err(cortex_rs::Error::Session(
                "favorite removal was not visible in read-back".into(),
            ));
        }
        Ok(())
    })();

    let pin_cleanup = (|| -> cortex_rs::Result<()> {
        qc.unpin_model(candidate_model)?;
        for _ in 0..baseline_pins
            .iter()
            .filter(|&&id| id == candidate_model)
            .count()
        {
            qc.pin_model(candidate_model)?;
        }
        Ok(())
    })();
    let favorite_cleanup = if favorite_may_be_added {
        qc.remove_favorite(&candidate_favorite, timeout)
    } else {
        Ok(())
    };
    let final_pins = qc.pinned_models(timeout);
    let final_favorites = qc.favorites(timeout, 3);
    qc.disconnect();

    assert_eq!(
        final_pins?, baseline_pins,
        "pin state was not fully restored"
    );
    assert_eq!(
        final_favorites?, baseline_favorites,
        "favorite state was not fully restored"
    );
    pin_cleanup?;
    favorite_cleanup?;
    exercise?;
    Ok(())
}

#[test]
#[ignore = "requires an exclusively available real Quad Cortex and explicit authorization to mutate USER 6A transiently"]
fn row_level_grid_mutations_read_back_and_recall_cleanup() -> cortex_rs::Result<()> {
    let qc = QuadCortex::connect(
        DeviceKind::QuadCortex,
        Duration::from_secs(10),
        Duration::from_secs(1),
    )?;
    let timeout = Duration::from_secs(30);

    let exercise = (|| -> cortex_rs::Result<()> {
        // USER 6A is the explicitly authorized fixture for this ignored test.
        qc.recall_preset(USER_SETLIST, "6A", false, timeout)?;
        let baseline = qc.read_current_preset(timeout)?;
        let active_scene = usize::try_from(qc.active_scene(timeout)?).map_err(|_| {
            cortex_rs::Error::InvalidScene("active scene does not fit this host".into())
        })?;
        let catalog = Catalog::parse(&qc.fetch_model_repo(timeout)?)?;
        let mut failures = Vec::new();
        let mut only_not_found = true;

        for control in [
            RowControl::Splitter,
            RowControl::Mixer,
            RowControl::LaneOutput,
            RowControl::InputGate,
        ] {
            if let Err(error) =
                exercise_row_control(&qc, &baseline, &catalog, active_scene, control, timeout)
            {
                only_not_found &= matches!(error, cortex_rs::Error::NotFound(_));
                failures.push(error.to_string());
            }
        }
        if let Err(error) = exercise_split_mute(&qc, &baseline, timeout) {
            only_not_found &= matches!(error, cortex_rs::Error::NotFound(_));
            failures.push(error.to_string());
        }

        if failures.is_empty() {
            Ok(())
        } else if only_not_found {
            Err(cortex_rs::Error::NotFound(failures.join("; ")))
        } else {
            Err(cortex_rs::Error::Session(failures.join("; ")))
        }
    })();

    // Recall is the rollback: these operations modify only the working copy.
    let cleanup = qc.recall_preset(USER_SETLIST, "6A", false, timeout);
    qc.disconnect();
    cleanup?;
    exercise
}

#[test]
#[ignore = "requires an exclusively available real Quad Cortex and explicit authorization to mutate USER 6A transiently"]
fn tempo_mutations_read_back_and_recall_cleanup() -> cortex_rs::Result<()> {
    let qc = QuadCortex::connect(
        DeviceKind::QuadCortex,
        Duration::from_secs(10),
        Duration::from_secs(1),
    )?;
    let timeout = Duration::from_secs(30);

    let mut baseline_tempo = None;
    let exercise = (|| -> cortex_rs::Result<()> {
        qc.recall_preset(USER_SETLIST, "6A", false, timeout)?;
        let baseline = qc.read_current_preset(timeout)?;
        baseline_tempo = Some(baseline.tempo_program_data.clone());
        if baseline_tempo.as_ref().is_none_or(Vec::is_empty)
            || !baseline_tempo.as_ref().is_some_and(|models| {
                models.iter().any(|model| {
                    matches!(
                        model.hash,
                        Some(cortex_rs::proto::model::Hash::Hash(
                            cortex_rs::grid::TEMPO_CONTROL
                        ))
                    )
                })
            })
        {
            return Err(cortex_rs::Error::NotFound(
                "fixture has no hash-25000 tempo program data".into(),
            ));
        }

        // Silence playback before changing sound or routing. This extra safety
        // write is restored with every other working-copy edit by final recall.
        qc.set_tempo_param(
            TempoParameter::Mute,
            ParameterInput::Normalised(1.0),
            timeout,
        )?;
        let mut current = verify_tempo_value(&qc, TempoParameter::Mute, 1.0, timeout)?;

        let target = changed_value(
            tempo_value(&current, TempoParameter::Tempo.index()).ok_or_else(|| {
                cortex_rs::Error::NotFound("tempo value is absent from fixture".into())
            })?,
        );
        qc.set_tempo_param(
            TempoParameter::Tempo,
            ParameterInput::Normalised(target),
            timeout,
        )?;
        current = verify_tempo_value(&qc, TempoParameter::Tempo, target, timeout)?;

        let option = different_option(
            tempo_value(&current, TempoParameter::Routing.index()).ok_or_else(|| {
                cortex_rs::Error::NotFound("routing value is absent from fixture".into())
            })?,
            5,
        );
        qc.set_tempo_option(TempoParameter::Routing, option)?;
        let target = option as f32 / 4.0;
        current = verify_tempo_value(&qc, TempoParameter::Routing, target, timeout)?;

        let option = different_option(
            tempo_value(&current, TempoParameter::Subdivisions.index()).ok_or_else(|| {
                cortex_rs::Error::NotFound("subdivision value is absent from fixture".into())
            })?,
            4,
        );
        qc.set_tempo_subdivision(TempoSubdivision::try_from(option)?)?;
        let target = option as f32 / 3.0;
        current = verify_tempo_value(&qc, TempoParameter::Subdivisions, target, timeout)?;

        let option = different_option(
            tempo_value(&current, TempoParameter::Sound.index()).ok_or_else(|| {
                cortex_rs::Error::NotFound("sound value is absent from fixture".into())
            })?,
            6,
        );
        qc.set_metronome_sound(MetronomeSound::try_from(option)?)?;
        let target = option as f32 / 5.0;
        current = verify_tempo_value(&qc, TempoParameter::Sound, target, timeout)?;

        let option = different_option(
            tempo_value(&current, TempoParameter::Routing.index()).ok_or_else(|| {
                cortex_rs::Error::NotFound("routing value disappeared from fixture".into())
            })?,
            5,
        );
        qc.set_metronome_routing(MetronomeRouting::try_from(option)?)?;
        let target = option as f32 / 4.0;
        current = verify_tempo_value(&qc, TempoParameter::Routing, target, timeout)?;

        let option = different_option(
            tempo_value(&current, TempoParameter::TimeSignature.index()).ok_or_else(|| {
                cortex_rs::Error::NotFound("time-signature value is absent from fixture".into())
            })?,
            21,
        );
        qc.set_time_signature(TimeSignature::try_from(option)?)?;
        let target = option as f32 / 20.0;
        // Time-signature writes can rewrite STEPSTATE accent parameters. Verify
        // the requested target, not unrelated tempo-program equality.
        current = verify_tempo_value(&qc, TempoParameter::TimeSignature, target, timeout)?;

        let led = tempo_value(&current, TempoParameter::LedLight.index()).ok_or_else(|| {
            cortex_rs::Error::NotFound("tempo LED value is absent from fixture".into())
        })? < 0.5;
        qc.set_tempo_led(led)?;
        current = verify_tempo_value(
            &qc,
            TempoParameter::LedLight,
            if led { 1.0 } else { 0.0 },
            timeout,
        )?;

        let volume = changed_value(
            tempo_value(&current, TempoParameter::Volume.index()).ok_or_else(|| {
                cortex_rs::Error::NotFound("metronome volume is absent from fixture".into())
            })?,
        );
        qc.set_metronome_volume(volume)?;
        verify_tempo_value(&qc, TempoParameter::Volume, volume, timeout)?;

        Ok(())
    })();

    // Recall is the rollback and runs regardless of which subcheck failed.
    let cleanup = qc.recall_preset(USER_SETLIST, "6A", false, timeout);
    let restored = if cleanup.is_ok() {
        qc.read_current_preset(timeout).and_then(|preset| {
            if baseline_tempo
                .as_ref()
                .is_some_and(|expected| *expected != preset.tempo_program_data)
            {
                Err(cortex_rs::Error::Session(
                    "tempo program data was not fully restored by recall".into(),
                ))
            } else {
                Ok(())
            }
        })
    } else {
        Ok(())
    };
    qc.disconnect();

    cleanup?;
    restored?;
    exercise
}

#[test]
#[ignore = "requires an exclusively available real Quad Cortex and explicit authorization to mutate USER 6A transiently"]
fn stomp_and_expression_mutations_read_back_and_recall_cleanup() -> cortex_rs::Result<()> {
    let qc = QuadCortex::connect(
        DeviceKind::QuadCortex,
        Duration::from_secs(10),
        Duration::from_secs(1),
    )?;
    let timeout = Duration::from_secs(30);

    let exercise = (|| -> cortex_rs::Result<()> {
        qc.recall_preset(USER_SETLIST, "6A", false, timeout)?;
        let baseline = qc.read_current_preset(timeout)?;
        let catalog = Catalog::parse(&qc.fetch_model_repo(timeout)?)?;
        let (row, column, target_parameter) = expression_candidate(&baseline, &catalog)?;

        qc.set_stomp_assignment(row, column, Footswitch::A)?;
        let after = qc.read_current_preset(timeout)?;
        if !after.stomp_mode_assignments.iter().any(|assignment| {
            assignment.row == row.wire()
                && assignment.column == column
                && assignment.stomp_index == Footswitch::A as u32
        }) {
            return Err(cortex_rs::Error::Session(
                "fresh live read did not contain the STOMP assignment".into(),
            ));
        }

        qc.clear_stomp_assignment(row, column)?;
        let after = qc.read_current_preset(timeout)?;
        if after
            .stomp_mode_assignments
            .iter()
            .any(|assignment| assignment.row == row.wire() && assignment.column == column)
        {
            return Err(cortex_rs::Error::Session(
                "fresh live read still contained the cleared STOMP assignment".into(),
            ));
        }

        let momentary = !baseline
            .stomp_is_momentary
            .get(&(Footswitch::H as u32))
            .copied()
            .unwrap_or(false);
        qc.set_stomp_momentary(Footswitch::H, momentary)?;
        let after = qc.read_current_preset(timeout)?;
        if after.stomp_is_momentary.get(&(Footswitch::H as u32)) != Some(&momentary) {
            return Err(cortex_rs::Error::Session(
                "fresh live read did not contain the STOMP momentary value".into(),
            ));
        }

        qc.set_stomp_label(Footswitch::A, "Test Group", false)?;
        let after = qc.read_current_preset(timeout)?;
        if after
            .stomp_labels
            .get(&(Footswitch::A as u32))
            .map(String::as_str)
            != Some("Test Group")
        {
            return Err(cortex_rs::Error::Session(
                "fresh live read did not contain the general STOMP label".into(),
            ));
        }

        qc.set_stomp_label(Footswitch::A, "Test Single", true)?;
        let after = qc.read_current_preset(timeout)?;
        if after
            .single_stomp_labels
            .get(&(Footswitch::A as u32))
            .map(String::as_str)
            != Some("Test Single")
        {
            return Err(cortex_rs::Error::Session(
                "fresh live read did not contain the single-block STOMP label".into(),
            ));
        }

        qc.set_expression(
            row,
            column,
            ParameterTarget::Index(target_parameter),
            ExpressionPedal::Two,
            0.2,
            0.8,
            None,
        )?;
        let after = qc.read_current_preset(timeout)?;
        let parameter = model_at(&after, row, column)
            .and_then(|model| {
                model
                    .params
                    .iter()
                    .enumerate()
                    .find(|(position, parameter)| {
                        parameter_index(parameter, *position) == Some(target_parameter)
                    })
                    .map(|(_, parameter)| parameter)
            })
            .ok_or_else(|| {
                cortex_rs::Error::NotFound(
                    "targeted expression parameter disappeared on read-back".into(),
                )
            })?;
        if parameter.expression != Some(param::Expression::Expression(2))
            || parameter.expression_min != Some(param::ExpressionMin::ExpressionMin(0.2))
            || parameter.expression_max != Some(param::ExpressionMax::ExpressionMax(0.8))
        {
            return Err(cortex_rs::Error::Session(
                "fresh live read did not contain the expression parameter range".into(),
            ));
        }

        for mode in [
            ExpressionBypassMode::Stop,
            ExpressionBypassMode::Switch,
            ExpressionBypassMode::HeelToe,
        ] {
            qc.set_expression_bypass(row, column, ExpressionPedal::One, mode, true, 250, true)?;
            let after = qc.read_current_preset(timeout)?;
            let model = model_at(&after, row, column).ok_or_else(|| {
                cortex_rs::Error::NotFound(
                    "expression-bypass block disappeared on read-back".into(),
                )
            })?;
            let bypass = model.bypass_expression.first();
            let info = model.expression_bypass_info.first();
            if !bypass.is_some_and(|bypass| {
                bypass.expression == 1
                    && bypass.expression_min == 0.0
                    && bypass.expression_max == 1.0
            }) || !info.is_some_and(|info| {
                info.r#type == mode as u32
                    && info.invert
                    && info.delay_ms == 250
                    && info.latch_emulation
            }) {
                return Err(cortex_rs::Error::Session(
                    "fresh live read did not contain the expression bypass mode".into(),
                ));
            }
        }
        Ok(())
    })();

    // Recall is the rollback and runs regardless of which subcheck failed.
    let cleanup = qc.recall_preset(USER_SETLIST, "6A", false, timeout);
    qc.disconnect();
    cleanup?;
    exercise
}

#[test]
#[ignore = "requires an exclusively available real Quad Cortex and explicit authorization to mutate USER 6A transiently"]
fn capture_selection_reads_back_and_recall_cleanup() -> cortex_rs::Result<()> {
    let qc = QuadCortex::connect(
        DeviceKind::QuadCortex,
        Duration::from_secs(10),
        Duration::from_secs(1),
    )?;
    let timeout = Duration::from_secs(30);

    let exercise = (|| -> cortex_rs::Result<()> {
        qc.recall_preset(USER_SETLIST, "6A", false, timeout)?;
        let capture = qc.captures(timeout)?.into_iter().next().ok_or_else(|| {
            cortex_rs::Error::NotFound("capture library returned no selectable entry".into())
        })?;
        let baseline = qc.read_current_preset(timeout)?;
        let active_scene = usize::try_from(qc.active_scene(timeout)?).map_err(|_| {
            cortex_rs::Error::InvalidScene("active scene does not fit this host".into())
        })?;
        let (row, column, compatible) =
            compatible_or_replaceable_cell(&baseline, |id| matches!(id, 14_000 | 14_001))?;
        if !compatible {
            qc.remove_block(row, column)?;
        }
        let follow_up = [ParameterWrite {
            // Upstream hardware evidence identifies index 4 as the capture
            // block's normalized VOLUME, safe for a silent working-copy check.
            index: 4,
            value: Value::Normalised(0.56),
        }];
        qc.set_capture(
            row,
            column,
            &capture,
            (!compatible).then_some(DEFAULT_CAPTURE_MODEL),
            &follow_up,
            timeout,
        )?;

        let after = qc.read_current_preset(timeout)?;
        let model = model_at(&after, row, column).ok_or_else(|| {
            cortex_rs::Error::NotFound("capture block is absent on fresh read-back".into())
        })?;
        let expected_reference = format!("{}{}", capture.key, capture.name);
        if string_parameter(model, CAPTURE_FILE_NAME_PARAM) != Some(expected_reference.as_str()) {
            return Err(cortex_rs::Error::Session(
                "fresh read did not preserve the exact selected capture key/name string".into(),
            ));
        }
        if float_parameter(model, 4, active_scene)
            .is_none_or(|value| (value - 0.56).abs() > 0.000_1)
        {
            return Err(cortex_rs::Error::Session(
                "fresh read did not preserve the post-capture VOLUME parameter".into(),
            ));
        }
        eprintln!(
            "[PASS] capture selection: exact reference and post-selection parameter read back"
        );
        Ok(())
    })();

    // Recall is the rollback and runs regardless of listing, placement, or read-back failure.
    let cleanup = qc.recall_preset(USER_SETLIST, "6A", false, timeout);
    qc.disconnect();
    cleanup?;
    exercise
}

#[test]
#[ignore = "requires an exclusively available real Quad Cortex and an operator tap on New Neural Capture"]
fn capture_dialog_decline_is_graceful_and_session_stays_healthy() -> cortex_rs::Result<()> {
    let qc = QuadCortex::connect(
        DeviceKind::QuadCortex,
        Duration::from_secs(10),
        Duration::from_secs(1),
    )?;

    let exercise = (|| -> cortex_rs::Result<()> {
        let traffic = qc.decline_capture_dialog(
            || {
                eprintln!(
                    "[WAITING] Within 90 seconds, tap New Neural Capture on the Quad Cortex. Do not disconnect or open Cortex Control."
                );
                Ok(())
            },
            Duration::from_secs(90),
            Duration::from_secs(10),
        )?;

        let entered_capture = traffic.iter().any(|message: &NeuralCaptureMessage| {
            message.state == Some(cortex_rs::proto::neural_capture_message::State::State(1))
                || message.progress.is_some()
                || message.toggle_ab_model.is_some()
                || message.model_ab_bypass.is_some()
                || message.model_ab.is_some()
        });
        let scene = qc.active_scene(Duration::from_secs(30))?;
        if entered_capture {
            return Err(cortex_rs::Error::Session(format!(
                "declining the dialog reported capture state/progress/A-B preparation; positive-control active-scene read still returned scene {}",
                scene + 1
            )));
        }
        eprintln!(
            "[PASS] declined with show_dialog=false, observed NeuralCapture traffic for 10 seconds, and active-scene read returned scene {}",
            scene + 1
        );
        Ok(())
    })();

    qc.disconnect();
    eprintln!(
        "[MANUAL] Now that the host is disconnected, open New Neural Capture on the unit, confirm the on-unit wizard opens, then cancel it."
    );
    exercise
}

#[test]
#[ignore = "requires an exclusively available real Quad Cortex, one imported user IR, and explicit authorization to mutate USER 6A transiently"]
fn user_ir_selection_reads_back_and_recall_cleanup() -> cortex_rs::Result<()> {
    let qc = QuadCortex::connect(
        DeviceKind::QuadCortex,
        Duration::from_secs(10),
        Duration::from_secs(1),
    )?;
    let timeout = Duration::from_secs(30);

    let exercise = (|| -> cortex_rs::Result<()> {
        qc.recall_preset(USER_SETLIST, "6A", false, timeout)?;
        let ir: LibraryEntry = qc
            .list_irs(Some("2_q"), timeout)?
            .into_iter()
            .next()
            .ok_or_else(|| {
                cortex_rs::Error::NotFound(
                    "My IRs returned no selectable user entry; import one before this smoke".into(),
                )
            })?;
        let baseline = qc.read_current_preset(timeout)?;
        let (row, column, compatible) =
            compatible_or_replaceable_cell(&baseline, |id| (29_001..=29_008).contains(&id))?;
        if !compatible {
            qc.remove_block(row, column)?;
        }
        qc.set_ir(
            row,
            column,
            &ir,
            0,
            (!compatible).then_some(FIRST_IR_LOADER_MODEL),
            timeout,
        )?;

        let after = qc.read_current_preset(timeout)?;
        let model = model_at(&after, row, column).ok_or_else(|| {
            cortex_rs::Error::NotFound("IR Loader is absent on fresh read-back".into())
        })?;
        if string_parameter(model, 2) != Some(ir.key.as_str())
            || string_parameter(model, 22) != Some(ir.name.as_str())
        {
            return Err(cortex_rs::Error::Session(
                "fresh read did not preserve the exact selected IR key and display name".into(),
            ));
        }
        eprintln!(
            "[PARTIAL] IR selection: exact key/name read back; on-unit absence of the warning icon still requires manual inspection"
        );
        if let Ok(seconds) = std::env::var("CORTEX_VISUAL_PAUSE_SECONDS")
            && let Ok(seconds) = seconds.parse::<u64>()
            && seconds > 0
        {
            eprintln!("[PAUSE] inspect the IR Loader on the unit for {seconds} seconds");
            std::thread::sleep(Duration::from_secs(seconds));
        }
        Ok(())
    })();

    // Recall is the rollback and runs regardless of listing, placement, or read-back failure.
    let cleanup = qc.recall_preset(USER_SETLIST, "6A", false, timeout);
    qc.disconnect();
    cleanup?;
    exercise
}

fn master_assignment(
    settings: &GeneralSettingsMessage,
) -> cortex_rs::Result<MasterVolumeAssignment> {
    let Some(general_settings_message::MasterVolumeAssignment::MasterVolumeAssignment(value)) =
        settings.master_volume_assignment.as_ref()
    else {
        return Err(file_ops_failure(
            "complete settings omitted Master Volume assignment",
        ));
    };
    Ok(MasterVolumeAssignment {
        out12: value.out12,
        out34: value.out34,
        send12: value.send12,
        headphones: value.headphones,
    })
}

fn global_bypass(settings: &GeneralSettingsMessage) -> cortex_rs::Result<GlobalBypassState> {
    let Some(general_settings_message::GlobalBypassCab::GlobalBypassCab(cab)) =
        settings.global_bypass_cab.as_ref()
    else {
        return Err(file_ops_failure(
            "complete settings omitted Cab global bypass",
        ));
    };
    let Some(general_settings_message::GlobalBypassIr::GlobalBypassIr(ir)) =
        settings.global_bypass_ir.as_ref()
    else {
        return Err(file_ops_failure(
            "complete settings omitted IR global bypass",
        ));
    };
    Ok(GlobalBypassState {
        cab: [cab.row1, cab.row2, cab.row3, cab.row4],
        ir: [ir.row1, ir.row2, ir.row3, ir.row4],
    })
}

fn mode_slot(message: &ModeMessage) -> Option<u32> {
    message.mode.as_ref().map(|mode| {
        let mode_message::Mode::Mode(value) = mode;
        *value
    })
}

fn mode_slots(message: &ModeMessage) -> Option<&[u32]> {
    let mode_message::AvailableModes::AvailableModes(available) =
        message.available_modes.as_ref()?;
    Some(&available.modes)
}

fn tuner_values(message: &TunerMessage) -> Option<(i32, f32, bool)> {
    let tuner_message::InputPortId::InputPortId(input) = message.input_port_id.as_ref()?;
    let tuner_message::Frequency::Frequency(reference) = message.frequency.as_ref()?;
    let tuner_message::Mute::Mute(mute) = message.mute.as_ref()?;
    Some((*input, *reference, *mute))
}

fn eq_values(message: &GlobalEqMessage) -> cortex_rs::Result<[f32; 28]> {
    let mut values = [0.0; 28];
    let mut seen = [false; 28];
    for parameter in &message.parameters {
        let index = usize::try_from(parameter.parameter_index)
            .map_err(|_| file_ops_failure("Global EQ returned a negative parameter index"))?;
        if index >= values.len() || seen[index] {
            return Err(file_ops_failure(
                "Global EQ returned invalid or duplicate indices",
            ));
        }
        values[index] = parameter.value;
        seen[index] = true;
    }
    if !seen.into_iter().all(std::convert::identity) {
        return Err(file_ops_failure(
            "Global EQ snapshot was not index-complete",
        ));
    }
    Ok(values)
}

fn eq_bypassed(message: &GlobalEqMessage) -> Option<bool> {
    message.bypassed.as_ref().map(|value| {
        let global_eq_message::Bypassed::Bypassed(value) = value;
        *value
    })
}

fn normalized_settings(mut message: GeneralSettingsMessage) -> GeneralSettingsMessage {
    message.action = 0;
    message.request_id = None;
    message.available_disk_space = None;
    message.total_disk_space = None;
    message.storage_info = None;
    message
}

fn poll_twice<T>(
    mut read: impl FnMut() -> cortex_rs::Result<T>,
    matches: impl Fn(&T) -> bool,
    description: &str,
) -> cortex_rs::Result<T> {
    let mut matched = None;
    for required in 0..2 {
        let mut found = None;
        for _ in 0..10 {
            let value = read()?;
            if matches(&value) {
                found = Some(value);
                break;
            }
            std::thread::sleep(Duration::from_millis(500));
        }
        let value = found.ok_or_else(|| {
            file_ops_failure(format!(
                "{description} did not converge on explicit read {required}"
            ))
        })?;
        matched = Some(value);
    }
    Ok(matched.expect("two successful explicit reads produce a value"))
}

fn tuner_input(value: i32) -> cortex_rs::Result<TunerInput> {
    match value {
        1 => Ok(TunerInput::Input1),
        2 => Ok(TunerInput::Input2),
        3 => Ok(TunerInput::Input12),
        4 => Ok(TunerInput::Return1),
        5 => Ok(TunerInput::Return2),
        8 => Ok(TunerInput::Usb5),
        9 => Ok(TunerInput::Usb6),
        _ => Err(file_ops_failure(
            "baseline tuner input is not in the hardware-established accepted set",
        )),
    }
}

fn eq_filter(value: f32) -> cortex_rs::Result<GlobalEqFilter> {
    if (value - 0.0).abs() < 0.001 {
        Ok(GlobalEqFilter::Peak)
    } else if (value - 0.25).abs() < 0.001 {
        Ok(GlobalEqFilter::HighPass)
    } else if (value - 0.5).abs() < 0.001 {
        Ok(GlobalEqFilter::LowPass)
    } else if (value - 0.75).abs() < 0.001 {
        Ok(GlobalEqFilter::HighShelf)
    } else if (value - 1.0).abs() < 0.001 {
        Ok(GlobalEqFilter::LowShelf)
    } else {
        Err(file_ops_failure(
            "Global EQ returned an unknown filter value",
        ))
    }
}

fn record_cleanup(error: &mut Option<cortex_rs::Error>, result: cortex_rs::Result<()>) {
    if error.is_none() {
        *error = result.err();
    }
}

#[test]
#[ignore = "requires an exclusively available real Quad Cortex and explicit authorization to mutate persistent global settings transiently"]
#[allow(clippy::too_many_lines)]
fn prot_006_12_global_settings_mutate_poll_restore() -> cortex_rs::Result<()> {
    let qc = QuadCortex::connect(
        DeviceKind::QuadCortex,
        Duration::from_secs(10),
        Duration::from_secs(1),
    )?;
    let timeout = Duration::from_secs(10);

    let settings = qc.settings_complete(timeout)?;
    let mode = qc.mode_complete(timeout)?;
    let tuner = qc.tuner_complete(timeout)?;
    let eq = qc.global_eq_complete(timeout)?;
    let assignment = master_assignment(&settings)?;
    let bypass = global_bypass(&settings)?;
    let eq_baseline = eq_values(&eq)?;
    let eq_filters = (0..5)
        .map(|band| eq_filter(eq_baseline[band * 5 + 3]))
        .collect::<cortex_rs::Result<Vec<_>>>()?;
    let eq_was_bypassed = eq_bypassed(&eq)
        .ok_or_else(|| file_ops_failure("complete Global EQ omitted bypass state"))?;
    let mode_baseline = mode_slots(&mode)
        .ok_or_else(|| file_ops_failure("complete Mode omitted cycle"))?
        .iter()
        .copied()
        .map(FootswitchModeSlot::try_from)
        .collect::<cortex_rs::Result<Vec<_>>>()?;
    let active_mode = FootswitchModeSlot::try_from(
        mode_slot(&mode).ok_or_else(|| file_ops_failure("complete Mode omitted active slot"))?,
    )?;
    let (tuner_input_baseline, tuner_reference, tuner_mute) =
        tuner_values(&tuner).ok_or_else(|| file_ops_failure("complete Tuner omitted state"))?;
    let tuner_input_baseline = tuner_input(tuner_input_baseline)?;
    let scene_behavior = settings
        .scene_block_bypass
        .as_ref()
        .map(|value| {
            let general_settings_message::SceneBlockBypass::SceneBlockBypass(value) = value;
            *value
        })
        .ok_or_else(|| file_ops_failure("complete settings omitted scene bypass behavior"))?;
    let scene_behavior = match scene_behavior {
        0 => SceneBypassBehavior::AlwaysOverwrite,
        1 => SceneBypassBehavior::NonStompOverwrite,
        2 => SceneBypassBehavior::NeverOverwrite,
        _ => {
            return Err(file_ops_failure(
                "unknown scene bypass behavior in baseline",
            ));
        }
    };
    let hold_index = settings
        .hold_timing
        .as_ref()
        .map(|value| {
            let general_settings_message::HoldTiming::HoldTiming(value) = value;
            *value
        })
        .ok_or_else(|| file_ops_failure("complete settings omitted HOLD timing"))?;
    let hold_ms = u32::try_from(500 + 100 * hold_index)
        .map_err(|_| file_ops_failure("baseline HOLD timing index is invalid"))?;
    let swap_access = settings
        .swap_tempo_tuner_access
        .as_ref()
        .map(|value| {
            let general_settings_message::SwapTempoTunerAccess::SwapTempoTunerAccess(value) = value;
            *value
        })
        .ok_or_else(|| file_ops_failure("complete settings omitted swap-tempo-tuner setting"))?;

    let exercise = (|| -> cortex_rs::Result<()> {
        let changed_swap = !swap_access;
        qc.update_settings(&GeneralSettingsPatch {
            swap_tempo_tuner_access: Some(changed_swap),
            ..Default::default()
        })?;
        poll_twice(
            || qc.settings_complete(timeout),
            |value| {
                value.swap_tempo_tuner_access.as_ref().is_some_and(|field| {
                matches!(field, general_settings_message::SwapTempoTunerAccess::SwapTempoTunerAccess(actual) if *actual == changed_swap)
            })
            },
            "typed GeneralSettings patch",
        )?;
        qc.update_settings(&GeneralSettingsPatch {
            swap_tempo_tuner_access: Some(swap_access),
            ..Default::default()
        })?;
        poll_twice(
            || qc.settings_complete(timeout),
            |value| normalized_settings(value.clone()) == normalized_settings(settings.clone()),
            "GeneralSettings patch restoration",
        )?;

        let changed_hold = if hold_ms == 1000 { 500 } else { hold_ms + 100 };
        qc.set_hold_timing(changed_hold)?;
        poll_twice(
            || qc.settings_complete(timeout),
            |value| {
                value.hold_timing.as_ref().is_some_and(|field| matches!(field, general_settings_message::HoldTiming::HoldTiming(actual) if *actual == i32::try_from((changed_hold - 500) / 100).unwrap()))
            },
            "HOLD timing",
        )?;
        qc.set_hold_timing(hold_ms)?;
        poll_twice(
            || qc.settings_complete(timeout),
            |value| normalized_settings(value.clone()) == normalized_settings(settings.clone()),
            "HOLD timing restoration",
        )?;

        let changed_scene = match scene_behavior {
            SceneBypassBehavior::AlwaysOverwrite => SceneBypassBehavior::NonStompOverwrite,
            _ => SceneBypassBehavior::AlwaysOverwrite,
        };
        qc.set_scene_bypass_behavior(changed_scene)?;
        poll_twice(
            || qc.settings_complete(timeout),
            |value| {
                value.scene_block_bypass.as_ref().is_some_and(|field| matches!(field, general_settings_message::SceneBlockBypass::SceneBlockBypass(actual) if *actual == changed_scene as i32))
            },
            "scene bypass behavior",
        )?;
        qc.set_scene_bypass_behavior(scene_behavior)?;
        poll_twice(
            || qc.settings_complete(timeout),
            |value| normalized_settings(value.clone()) == normalized_settings(settings.clone()),
            "scene bypass restoration",
        )?;

        qc.set_master_volume_assignment(
            MasterVolumeAssignmentPatch {
                out12: Some(!assignment.out12),
                ..Default::default()
            },
            timeout,
        )?;
        poll_twice(
            || qc.settings_complete(timeout),
            |value| {
                master_assignment(value).is_ok_and(|actual| {
                    actual
                        == MasterVolumeAssignment {
                            out12: !assignment.out12,
                            ..assignment
                        }
                })
            },
            "Master Volume assignment",
        )?;
        qc.restore_master_volume_assignment(assignment)?;
        poll_twice(
            || qc.settings_complete(timeout),
            |value| master_assignment(value).is_ok_and(|actual| actual == assignment),
            "Master Volume restoration",
        )?;

        let mut changed_cab = bypass.cab;
        changed_cab[0] = !changed_cab[0];
        qc.set_global_bypass(
            GlobalBypassPatch {
                cab: Some(changed_cab),
                ir: None,
            },
            timeout,
        )?;
        poll_twice(
            || qc.settings_complete(timeout),
            |value| {
                global_bypass(value).is_ok_and(|actual| {
                    actual
                        == GlobalBypassState {
                            cab: changed_cab,
                            ir: bypass.ir,
                        }
                })
            },
            "Cab global bypass",
        )?;
        qc.restore_global_bypass(bypass)?;
        poll_twice(
            || qc.settings_complete(timeout),
            |value| global_bypass(value).is_ok_and(|actual| actual == bypass),
            "global bypass restoration",
        )?;

        qc.set_global_eq_bypassed(!eq_was_bypassed)?;
        poll_twice(
            || qc.global_eq_complete(timeout),
            |value| eq_bypassed(value) == Some(!eq_was_bypassed),
            "Global EQ bypass",
        )?;
        qc.set_global_eq_bypassed(eq_was_bypassed)?;
        poll_twice(
            || qc.global_eq_complete(timeout),
            |value| eq_bypassed(value) == Some(eq_was_bypassed),
            "Global EQ bypass restoration",
        )?;

        let controls = [
            GlobalEqBandPatch {
                gain: Some(changed_value(eq_baseline[0])),
                ..Default::default()
            },
            GlobalEqBandPatch {
                frequency: Some(changed_value(eq_baseline[1])),
                ..Default::default()
            },
            GlobalEqBandPatch {
                q: Some(changed_value(eq_baseline[2])),
                ..Default::default()
            },
            GlobalEqBandPatch {
                filter_type: Some(if eq_baseline[3] < 0.125 {
                    GlobalEqFilter::LowShelf
                } else {
                    GlobalEqFilter::Peak
                }),
                ..Default::default()
            },
            GlobalEqBandPatch {
                enabled: Some(eq_baseline[4] < 0.5),
                ..Default::default()
            },
        ];
        for (offset, patch) in controls.into_iter().enumerate() {
            qc.set_global_eq(1, patch)?;
            let expected = match offset {
                0 => patch.gain.unwrap(),
                1 => patch.frequency.unwrap(),
                2 => patch.q.unwrap(),
                3 => f32::from(patch.filter_type.unwrap() as u8) / 4.0,
                4 => {
                    if patch.enabled.unwrap() {
                        1.0
                    } else {
                        0.0
                    }
                }
                _ => unreachable!(),
            };
            poll_twice(
                || qc.global_eq_complete(timeout),
                |value| {
                    eq_values(value).is_ok_and(|values| (values[offset] - expected).abs() < 0.0001)
                },
                "Global EQ band control",
            )?;
            let restore = match offset {
                0 => GlobalEqBandPatch {
                    gain: Some(eq_baseline[0]),
                    ..Default::default()
                },
                1 => GlobalEqBandPatch {
                    frequency: Some(eq_baseline[1]),
                    ..Default::default()
                },
                2 => GlobalEqBandPatch {
                    q: Some(eq_baseline[2]),
                    ..Default::default()
                },
                3 => GlobalEqBandPatch {
                    filter_type: Some(eq_filters[0]),
                    ..Default::default()
                },
                4 => GlobalEqBandPatch {
                    enabled: Some(eq_baseline[4] >= 0.5),
                    ..Default::default()
                },
                _ => unreachable!(),
            };
            qc.set_global_eq(1, restore)?;
            poll_twice(
                || qc.global_eq_complete(timeout),
                |value| {
                    eq_values(value)
                        .is_ok_and(|values| (values[offset] - eq_baseline[offset]).abs() < 0.0001)
                },
                "Global EQ band restoration",
            )?;
        }

        for (index, patch, expected) in [
            (
                25,
                GlobalEqOutputPatch {
                    level: Some(changed_value(eq_baseline[25])),
                    ..Default::default()
                },
                changed_value(eq_baseline[25]),
            ),
            (
                26,
                GlobalEqOutputPatch {
                    out12: Some(eq_baseline[26] < 0.5),
                    ..Default::default()
                },
                if eq_baseline[26] < 0.5 { 1.0 } else { 0.0 },
            ),
            (
                27,
                GlobalEqOutputPatch {
                    out34: Some(eq_baseline[27] < 0.5),
                    ..Default::default()
                },
                if eq_baseline[27] < 0.5 { 1.0 } else { 0.0 },
            ),
        ] {
            qc.set_global_eq_output(patch)?;
            poll_twice(
                || qc.global_eq_complete(timeout),
                |value| {
                    eq_values(value).is_ok_and(|values| (values[index] - expected).abs() < 0.0001)
                },
                "Global EQ output control",
            )?;
            let restore = match index {
                25 => GlobalEqOutputPatch {
                    level: Some(eq_baseline[25]),
                    ..Default::default()
                },
                26 => GlobalEqOutputPatch {
                    out12: Some(eq_baseline[26] >= 0.5),
                    ..Default::default()
                },
                27 => GlobalEqOutputPatch {
                    out34: Some(eq_baseline[27] >= 0.5),
                    ..Default::default()
                },
                _ => unreachable!(),
            };
            qc.set_global_eq_output(restore)?;
            poll_twice(
                || qc.global_eq_complete(timeout),
                |value| {
                    eq_values(value)
                        .is_ok_and(|values| (values[index] - eq_baseline[index]).abs() < 0.0001)
                },
                "Global EQ output restoration",
            )?;
        }

        let mut changed_cycle = mode_baseline.clone();
        if changed_cycle.len() > 1 {
            changed_cycle.rotate_left(1);
        } else {
            let alternative = if changed_cycle[0] == FootswitchModeSlot::Preset {
                FootswitchModeSlot::Scene
            } else {
                FootswitchModeSlot::Preset
            };
            changed_cycle.push(alternative);
        }
        qc.set_mode_cycle(&changed_cycle)?;
        poll_twice(
            || qc.mode_complete(timeout),
            |value| {
                mode_slots(value)
                    == Some(
                        changed_cycle
                            .iter()
                            .map(|slot| *slot as u32)
                            .collect::<Vec<_>>()
                            .as_slice(),
                    )
            },
            "mode cycle",
        )?;
        qc.set_mode_cycle(&mode_baseline)?;
        poll_twice(
            || qc.mode_complete(timeout),
            |value| {
                mode_slots(value)
                    == Some(
                        mode_baseline
                            .iter()
                            .map(|slot| *slot as u32)
                            .collect::<Vec<_>>()
                            .as_slice(),
                    )
            },
            "mode-cycle restoration",
        )?;
        if let Some(alternative) = mode_baseline
            .iter()
            .copied()
            .find(|slot| *slot != active_mode)
        {
            qc.set_mode(alternative)?;
            poll_twice(
                || qc.mode_complete(timeout),
                |value| mode_slot(value) == Some(alternative as u32),
                "active mode",
            )?;
            qc.set_mode(active_mode)?;
            poll_twice(
                || qc.mode_complete(timeout),
                |value| mode_slot(value) == Some(active_mode as u32),
                "active-mode restoration",
            )?;
        }

        let changed_input = if tuner_input_baseline == TunerInput::Input1 {
            TunerInput::Input2
        } else {
            TunerInput::Input1
        };
        qc.set_tuner_input(changed_input)?;
        poll_twice(
            || qc.tuner_complete(timeout),
            |value| tuner_values(value).is_some_and(|state| state.0 == changed_input as i32),
            "tuner input",
        )?;
        qc.set_tuner_input(tuner_input_baseline)?;
        poll_twice(
            || qc.tuner_complete(timeout),
            |value| tuner_values(value).is_some_and(|state| state.0 == tuner_input_baseline as i32),
            "tuner-input restoration",
        )?;
        qc.set_tuner_mute(!tuner_mute)?;
        poll_twice(
            || qc.tuner_complete(timeout),
            |value| tuner_values(value).is_some_and(|state| state.2 != tuner_mute),
            "tuner mute",
        )?;
        qc.set_tuner_mute(tuner_mute)?;
        poll_twice(
            || qc.tuner_complete(timeout),
            |value| tuner_values(value).is_some_and(|state| state.2 == tuner_mute),
            "tuner-mute restoration",
        )?;
        let changed_reference = if tuner_reference < 14.0 {
            tuner_reference + 1.0
        } else {
            tuner_reference - 1.0
        };
        qc.set_tuner_reference(changed_reference)?;
        poll_twice(
            || qc.tuner_complete(timeout),
            |value| {
                tuner_values(value).is_some_and(|state| (state.1 - changed_reference).abs() < 0.001)
            },
            "tuner reference",
        )?;
        qc.set_tuner_reference(tuner_reference)?;
        poll_twice(
            || qc.tuner_complete(timeout),
            |value| {
                tuner_values(value).is_some_and(|state| (state.1 - tuner_reference).abs() < 0.001)
            },
            "tuner-reference restoration",
        )?;
        Ok(())
    })();

    // Cleanup is deliberately independent of exercise progress: every complete
    // snapshot is restored even when an earlier mutation or verification failed.
    let mut cleanup_error = None;
    record_cleanup(
        &mut cleanup_error,
        qc.update_settings(&GeneralSettingsPatch {
            swap_tempo_tuner_access: Some(swap_access),
            ..Default::default()
        }),
    );
    record_cleanup(&mut cleanup_error, qc.set_hold_timing(hold_ms));
    record_cleanup(
        &mut cleanup_error,
        qc.set_scene_bypass_behavior(scene_behavior),
    );
    record_cleanup(
        &mut cleanup_error,
        qc.restore_master_volume_assignment(assignment),
    );
    record_cleanup(&mut cleanup_error, qc.restore_global_bypass(bypass));
    record_cleanup(
        &mut cleanup_error,
        qc.set_global_eq_bypassed(eq_was_bypassed),
    );
    for band in 1..=5 {
        let base = usize::from(band - 1) * 5;
        record_cleanup(
            &mut cleanup_error,
            qc.set_global_eq(
                band,
                GlobalEqBandPatch {
                    gain: Some(eq_baseline[base]),
                    frequency: Some(eq_baseline[base + 1]),
                    q: Some(eq_baseline[base + 2]),
                    filter_type: Some(eq_filters[usize::from(band - 1)]),
                    enabled: Some(eq_baseline[base + 4] >= 0.5),
                },
            ),
        );
    }
    record_cleanup(
        &mut cleanup_error,
        qc.set_global_eq_output(GlobalEqOutputPatch {
            level: Some(eq_baseline[25]),
            out12: Some(eq_baseline[26] >= 0.5),
            out34: Some(eq_baseline[27] >= 0.5),
        }),
    );
    record_cleanup(&mut cleanup_error, qc.set_mode_cycle(&mode_baseline));
    record_cleanup(&mut cleanup_error, qc.set_mode(active_mode));
    record_cleanup(&mut cleanup_error, qc.set_tuner_input(tuner_input_baseline));
    record_cleanup(&mut cleanup_error, qc.set_tuner_mute(tuner_mute));
    record_cleanup(&mut cleanup_error, qc.set_tuner_reference(tuner_reference));

    record_cleanup(
        &mut cleanup_error,
        poll_twice(
            || qc.settings_complete(timeout),
            |value| normalized_settings(value.clone()) == normalized_settings(settings.clone()),
            "final GeneralSettings baseline",
        )
        .map(drop),
    );
    record_cleanup(
        &mut cleanup_error,
        poll_twice(
            || qc.global_eq_complete(timeout),
            |value| {
                eq_bypassed(value) == Some(eq_was_bypassed)
                    && eq_values(value).is_ok_and(|values| {
                        values
                            .iter()
                            .zip(eq_baseline)
                            .all(|(actual, expected)| (*actual - expected).abs() < 0.0001)
                    })
            },
            "final Global EQ baseline",
        )
        .map(drop),
    );
    record_cleanup(
        &mut cleanup_error,
        poll_twice(
            || qc.mode_complete(timeout),
            |value| {
                mode_slot(value) == Some(active_mode as u32)
                    && mode_slots(value)
                        == Some(
                            mode_baseline
                                .iter()
                                .map(|slot| *slot as u32)
                                .collect::<Vec<_>>()
                                .as_slice(),
                        )
            },
            "final Mode baseline",
        )
        .map(drop),
    );
    record_cleanup(
        &mut cleanup_error,
        poll_twice(
            || qc.tuner_complete(timeout),
            |value| {
                tuner_values(value).is_some_and(|state| {
                    state.0 == tuner_input_baseline as i32
                        && (state.1 - tuner_reference).abs() < 0.001
                        && state.2 == tuner_mute
                })
            },
            "final Tuner baseline",
        )
        .map(drop),
    );
    qc.disconnect();
    if let Some(error) = cleanup_error {
        return Err(error);
    }
    exercise
}

#[test]
#[ignore = "requires visual confirmation on a real Quad Cortex"]
fn gig_view_and_tuner_visibility_visual_check() -> cortex_rs::Result<()> {
    let qc = QuadCortex::connect(
        DeviceKind::QuadCortex,
        Duration::from_secs(10),
        Duration::from_secs(1),
    )?;
    let pause = std::env::var("CORTEX_VISUAL_PAUSE_SECONDS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(20);

    let exercise = (|| -> cortex_rs::Result<()> {
        qc.set_gig_view(false)?;
        qc.show_tuner(false)?;
        qc.set_gig_view(true)?;
        eprintln!("[PAUSE] confirm Gig View is visible for {pause} seconds");
        std::thread::sleep(Duration::from_secs(pause));
        qc.set_gig_view(false)?;
        qc.show_tuner(true)?;
        eprintln!("[PAUSE] confirm the Tuner is visible for {pause} seconds");
        std::thread::sleep(Duration::from_secs(pause));
        Ok(())
    })();

    let hide_tuner = qc.show_tuner(false);
    let hide_gig_view = qc.set_gig_view(false);
    qc.disconnect();
    hide_tuner?;
    hide_gig_view?;
    exercise
}

#[derive(Debug, Clone, PartialEq)]
struct WritableIoSnapshot {
    inputs: Vec<(InputPort, InputPortPatch)>,
    outputs: Vec<(OutputPort, OutputPortPatch)>,
    usb: [f32; 3],
    midi_thru: f32,
    xlr12_linked: bool,
    out34_linked: bool,
}

fn writable_io_snapshot(
    message: &cortex_rs::proto::IoSettingsMessage,
) -> cortex_rs::Result<WritableIoSnapshot> {
    let Some(cortex_rs::proto::io_settings_message::Settings::Settings(settings)) =
        message.settings.as_ref()
    else {
        return Err(file_ops_failure(
            "complete IOSettings omitted port settings",
        ));
    };
    let mut inputs = settings
        .in_port
        .iter()
        .map(|port| {
            Ok((
                InputPort::try_from(port.input_port_id)?,
                InputPortPatch {
                    level: port.level.map(|field| {
                        let cortex_rs::proto::input_port_settings::Level::Level(value) = field;
                        value
                    }),
                    impedance: port.input_zmode.map(|field| {
                        let cortex_rs::proto::input_port_settings::InputZmode::InputZmode(value) =
                            field;
                        value
                    }),
                    input_type: port.input_type.map(|field| {
                        let cortex_rs::proto::input_port_settings::InputType::InputType(value) =
                            field;
                        value
                    }),
                    ground_lift: port.ground_lift.map(|field| {
                        let cortex_rs::proto::input_port_settings::GroundLift::GroundLift(value) =
                            field;
                        value
                    }),
                },
            ))
        })
        .collect::<cortex_rs::Result<Vec<_>>>()?;
    inputs.sort_by_key(|(port, _)| *port as u32);
    let mut outputs = settings
        .out_port
        .iter()
        .map(|port| {
            Ok((
                OutputPort::try_from(port.output_port_id)?,
                OutputPortPatch {
                    level: port.level.map(|field| {
                        let cortex_rs::proto::output_port_settings::Level::Level(value) = field;
                        value
                    }),
                    ground_lift: port.ground_lift.map(|field| {
                        let cortex_rs::proto::output_port_settings::GroundLift::GroundLift(value) =
                            field;
                        value
                    }),
                    mute: port.mute.map(|field| {
                        let cortex_rs::proto::output_port_settings::Mute::Mute(value) = field;
                        value
                    }),
                },
            ))
        })
        .collect::<cortex_rs::Result<Vec<_>>>()?;
    outputs.sort_by_key(|(port, _)| *port as u32);
    let Some(cortex_rs::proto::port_settings::UsbPort::UsbPort(usb)) = settings.usb_port.as_ref()
    else {
        return Err(file_ops_failure("complete IOSettings omitted USB settings"));
    };
    let cortex_rs::proto::usb_port_settings::Level::Level(usb_level) = usb
        .level
        .ok_or_else(|| file_ops_failure("complete IOSettings omitted USB level"))?;
    let cortex_rs::proto::usb_port_settings::HpSelect::HpSelect(headphone_source) =
        usb.hp_select
            .ok_or_else(|| file_ops_failure("complete IOSettings omitted USB headphone source"))?;
    let cortex_rs::proto::usb_port_settings::DryWet::DryWet(dry_wet) = usb
        .dry_wet
        .ok_or_else(|| file_ops_failure("complete IOSettings omitted USB dry/wet"))?;
    let Some(cortex_rs::proto::port_settings::MidiPort::MidiPort(midi)) =
        settings.midi_port.as_ref()
    else {
        return Err(file_ops_failure(
            "complete IOSettings omitted MIDI settings",
        ));
    };
    let cortex_rs::proto::midi_port_settings::MidiThru::MidiThru(midi_thru) = midi
        .midi_thru
        .ok_or_else(|| file_ops_failure("complete IOSettings omitted MIDI Thru"))?;
    let cortex_rs::proto::io_settings_message::Xlr12Linked::Xlr12Linked(xlr12_linked) = message
        .xlr1_2_linked
        .ok_or_else(|| file_ops_failure("complete IOSettings omitted XLR 1/2 pairing"))?;
    let cortex_rs::proto::io_settings_message::Out34Linked::Out34Linked(out34_linked) = message
        .out3_4_linked
        .ok_or_else(|| file_ops_failure("complete IOSettings omitted Out 3/4 pairing"))?;
    Ok(WritableIoSnapshot {
        inputs,
        outputs,
        usb: [usb_level, headphone_source, dry_wet],
        midi_thru,
        xlr12_linked,
        out34_linked,
    })
}

fn restore_io_snapshot(qc: &QuadCortex, snapshot: &WritableIoSnapshot) -> cortex_rs::Result<()> {
    qc.set_output_pairing(OutputPairingPatch {
        xlr12: Some(snapshot.xlr12_linked),
        out34: Some(snapshot.out34_linked),
    })?;
    for (port, patch) in &snapshot.inputs {
        qc.set_input_port(*port, *patch)?;
    }
    for (port, patch) in &snapshot.outputs {
        qc.set_output_port(*port, *patch)?;
    }
    qc.set_usb_port(UsbPortPatch {
        level: Some(snapshot.usb[0]),
        headphone_source: Some(snapshot.usb[1]),
        dry_wet: Some(snapshot.usb[2]),
    })?;
    qc.set_midi_thru(snapshot.midi_thru >= 0.5)
}

fn io_change_then_restore(
    qc: &QuadCortex,
    baseline: &WritableIoSnapshot,
    changed: &WritableIoSnapshot,
    change: impl FnOnce() -> cortex_rs::Result<()>,
    restore: impl FnOnce() -> cortex_rs::Result<()>,
    timeout: Duration,
    description: &str,
) -> cortex_rs::Result<()> {
    change()?;
    poll_twice(
        || qc.io_settings_complete(timeout),
        |message| writable_io_snapshot(message).is_ok_and(|actual| actual == *changed),
        description,
    )?;
    restore()?;
    poll_twice(
        || qc.io_settings_complete(timeout),
        |message| writable_io_snapshot(message).is_ok_and(|actual| actual == *baseline),
        &format!("{description} restoration"),
    )?;
    Ok(())
}

#[test]
#[ignore = "requires an exclusively available real Quad Cortex; external speakers/amplifiers should be muted/disconnected and no phantom-sensitive path connected"]
#[allow(clippy::too_many_lines)]
fn prot_006_13_io_settings_mutate_poll_restore() -> cortex_rs::Result<()> {
    let qc = QuadCortex::connect(
        DeviceKind::QuadCortex,
        Duration::from_secs(10),
        Duration::from_secs(1),
    )?;
    let timeout = Duration::from_secs(10);
    let baseline_message = qc.io_settings_complete(timeout)?;
    let baseline = writable_io_snapshot(&baseline_message)?;
    let (input_index, (input, input_state)) = baseline
        .inputs
        .iter()
        .copied()
        .enumerate()
        .find(|(_, (port, _))| *port == InputPort::Input1)
        .ok_or_else(|| file_ops_failure("complete IOSettings omitted Input 1"))?;
    let input_values = [
        input_state.level.expect("complete Input 1 has level"),
        input_state
            .impedance
            .expect("complete Input 1 has impedance"),
        input_state
            .input_type
            .expect("complete Input 1 has input type"),
        input_state
            .ground_lift
            .expect("complete Input 1 has ground lift"),
    ];
    let (output_index, (output, output_state)) = baseline
        .outputs
        .iter()
        .copied()
        .enumerate()
        .find(|(_, (port, _))| *port == OutputPort::Xlr1)
        .ok_or_else(|| file_ops_failure("complete IOSettings omitted XLR Output 1"))?;
    let output_values = [
        output_state.level.expect("complete XLR Output 1 has level"),
        output_state
            .ground_lift
            .expect("complete XLR Output 1 has ground lift"),
    ];
    let output_mute = output_state.mute.expect("complete XLR Output 1 has mute");

    // Refuse to begin unless both pairing couples can be restored member by member.
    for required in [
        OutputPort::Xlr1,
        OutputPort::Xlr2,
        OutputPort::Out3,
        OutputPort::Out4,
    ] {
        if !baseline.outputs.iter().any(|(port, _)| *port == required) {
            qc.disconnect();
            return Err(file_ops_failure(format!(
                "complete IOSettings omitted pairing member {required:?}"
            )));
        }
    }

    let exercise = (|| -> cortex_rs::Result<()> {
        for field in 0..4 {
            let mut changed = baseline.clone();
            let changed_value = match field {
                0 | 1 => changed_value(input_values[field]),
                2 => {
                    if input_values[field] < 0.25 {
                        0.5
                    } else {
                        0.0
                    }
                }
                3 => {
                    if input_values[field] < 0.5 {
                        1.0
                    } else {
                        0.0
                    }
                }
                _ => unreachable!(),
            };
            let patch = match field {
                0 => InputPortPatch {
                    level: Some(changed_value),
                    ..Default::default()
                },
                1 => InputPortPatch {
                    impedance: Some(changed_value),
                    ..Default::default()
                },
                2 => InputPortPatch {
                    input_type: Some(changed_value),
                    ..Default::default()
                },
                3 => InputPortPatch {
                    ground_lift: Some(changed_value),
                    ..Default::default()
                },
                _ => unreachable!(),
            };
            match field {
                0 => changed.inputs[input_index].1.level = Some(changed_value),
                1 => changed.inputs[input_index].1.impedance = Some(changed_value),
                2 => changed.inputs[input_index].1.input_type = Some(changed_value),
                3 => changed.inputs[input_index].1.ground_lift = Some(changed_value),
                _ => unreachable!(),
            }
            let restore = InputPortPatch {
                level: (field == 0).then_some(input_values[0]),
                impedance: (field == 1).then_some(input_values[1]),
                input_type: (field == 2).then_some(input_values[2]),
                ground_lift: (field == 3).then_some(input_values[3]),
            };
            io_change_then_restore(
                &qc,
                &baseline,
                &changed,
                || qc.set_input_port(input, patch),
                || qc.set_input_port(input, restore),
                timeout,
                [
                    "input level",
                    "input impedance",
                    "input type",
                    "input ground lift",
                ][field],
            )?;
        }

        for field in 0..3 {
            let mut changed = baseline.clone();
            let (patch, restore) = match field {
                0 => {
                    let value = changed_value(output_values[0]);
                    changed.outputs[output_index].1.level = Some(value);
                    (
                        OutputPortPatch {
                            level: Some(value),
                            ..Default::default()
                        },
                        OutputPortPatch {
                            level: Some(output_values[0]),
                            ..Default::default()
                        },
                    )
                }
                1 => {
                    let value = if output_values[1] < 0.5 { 1.0 } else { 0.0 };
                    changed.outputs[output_index].1.ground_lift = Some(value);
                    (
                        OutputPortPatch {
                            ground_lift: Some(value),
                            ..Default::default()
                        },
                        OutputPortPatch {
                            ground_lift: Some(output_values[1]),
                            ..Default::default()
                        },
                    )
                }
                2 => {
                    changed.outputs[output_index].1.mute = Some(!output_mute);
                    (
                        OutputPortPatch {
                            mute: Some(!output_mute),
                            ..Default::default()
                        },
                        OutputPortPatch {
                            mute: Some(output_mute),
                            ..Default::default()
                        },
                    )
                }
                _ => unreachable!(),
            };
            io_change_then_restore(
                &qc,
                &baseline,
                &changed,
                || qc.set_output_port(output, patch),
                || qc.set_output_port(output, restore),
                timeout,
                ["output level", "output ground lift", "output mute"][field],
            )?;
        }

        for field in 0..3 {
            let mut changed = baseline.clone();
            changed.usb[field] = match field {
                0 => changed_value(baseline.usb[field]),
                1 | 2 => {
                    if baseline.usb[field] < 0.5 {
                        1.0
                    } else {
                        0.0
                    }
                }
                _ => unreachable!(),
            };
            let patch = UsbPortPatch {
                level: (field == 0).then_some(changed.usb[0]),
                headphone_source: (field == 1).then_some(changed.usb[1]),
                dry_wet: (field == 2).then_some(changed.usb[2]),
            };
            let restore = UsbPortPatch {
                level: (field == 0).then_some(baseline.usb[0]),
                headphone_source: (field == 1).then_some(baseline.usb[1]),
                dry_wet: (field == 2).then_some(baseline.usb[2]),
            };
            io_change_then_restore(
                &qc,
                &baseline,
                &changed,
                || qc.set_usb_port(patch),
                || qc.set_usb_port(restore),
                timeout,
                "USB-port field",
            )?;
        }

        let mut midi_changed = baseline.clone();
        midi_changed.midi_thru = if baseline.midi_thru >= 0.5 { 0.0 } else { 1.0 };
        io_change_then_restore(
            &qc,
            &baseline,
            &midi_changed,
            || qc.set_midi_thru(midi_changed.midi_thru >= 0.5),
            || qc.set_midi_thru(baseline.midi_thru >= 0.5),
            timeout,
            "MIDI Thru",
        )?;

        // Pairing is deliberately last. It may synchronize member settings, so
        // restore the pairing flag and both member ports before full comparison.
        for (xlr, members) in [
            (true, [OutputPort::Xlr1, OutputPort::Xlr2]),
            (false, [OutputPort::Out3, OutputPort::Out4]),
        ] {
            let changed_pairing = if xlr {
                !baseline.xlr12_linked
            } else {
                !baseline.out34_linked
            };
            qc.set_output_pairing(OutputPairingPatch {
                xlr12: xlr.then_some(changed_pairing),
                out34: (!xlr).then_some(changed_pairing),
            })?;
            poll_twice(
                || qc.io_settings_complete(timeout),
                |message| {
                    writable_io_snapshot(message).is_ok_and(|snapshot| {
                        if xlr {
                            snapshot.xlr12_linked == changed_pairing
                        } else {
                            snapshot.out34_linked == changed_pairing
                        }
                    })
                },
                "output pairing",
            )?;
            qc.set_output_pairing(OutputPairingPatch {
                xlr12: xlr.then_some(baseline.xlr12_linked),
                out34: (!xlr).then_some(baseline.out34_linked),
            })?;
            for member in members {
                let (_, patch) = baseline
                    .outputs
                    .iter()
                    .find(|(port, _)| *port == member)
                    .copied()
                    .expect("pairing members were checked before mutation");
                qc.set_output_port(member, patch)?;
            }
            poll_twice(
                || qc.io_settings_complete(timeout),
                |message| writable_io_snapshot(message).is_ok_and(|actual| actual == baseline),
                "pairing and member-port restoration",
            )?;
        }
        Ok(())
    })();

    let mut cleanup_error = None;
    record_cleanup(&mut cleanup_error, restore_io_snapshot(&qc, &baseline));
    record_cleanup(
        &mut cleanup_error,
        poll_twice(
            || qc.io_settings_complete(timeout),
            |message| writable_io_snapshot(message).is_ok_and(|actual| actual == baseline),
            "final complete IOSettings baseline",
        )
        .map(drop),
    );
    let final_snapshot = qc
        .io_settings_complete(timeout)
        .and_then(|message| writable_io_snapshot(&message));
    qc.disconnect();
    if let Ok(actual) = &final_snapshot
        && actual != &baseline
    {
        return Err(file_ops_failure(format!(
            "exercise: {}; cleanup: {}; baseline: {baseline:?}; actual: {actual:?}",
            exercise
                .as_ref()
                .err()
                .map_or_else(|| "ok".into(), ToString::to_string),
            cleanup_error
                .as_ref()
                .map_or_else(|| "ok".into(), ToString::to_string)
        )));
    }
    final_snapshot?;
    if let Some(error) = cleanup_error {
        return Err(error);
    }
    exercise
}
