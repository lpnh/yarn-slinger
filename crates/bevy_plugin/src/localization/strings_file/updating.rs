use crate::filesystem_events::CreateMissingStringsFilesEvent;
use crate::localization::line_id_generation::LineIdUpdateSystemSet;
use crate::localization::strings_file::creation::CreateMissingStringsFilesSystemSet;
use crate::prelude::*;
use crate::project::{YarnCompilation, YarnFilesInProject};
use bevy::asset::LoadState;
use bevy::prelude::*;
use bevy::utils::{HashMap, HashSet};

pub(crate) fn strings_file_updating_plugin(app: &mut App) {
    app.add_event::<UpdateAllStringsFilesForStringTableEvent>()
        .add_systems(
            (
                send_update_events_on_yarn_file_changes.run_if(in_development),
                send_update_events_on_localization_changes.run_if(
                    resource_exists::<YarnCompilation>()
                        .and_then(resource_exists_and_changed::<Localizations>()),
                ),
                update_all_strings_files_for_string_table
                    .pipe(panic_on_err)
                    .after(LineIdUpdateSystemSet)
                    .after(CreateMissingStringsFilesSystemSet)
                    .run_if(resource_exists::<Localizations>().and_then(in_development)),
            )
                .chain(),
        );
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Reflect, FromReflect)]
#[reflect(Debug, Default, PartialEq)]
pub struct UpdateAllStringsFilesForStringTableEvent(
    pub std::collections::HashMap<LineId, StringInfo>,
);

fn send_update_events_on_yarn_file_changes(
    mut events: EventReader<AssetEvent<YarnFile>>,
    yarn_files_in_project: Res<YarnFilesInProject>,
    mut update_writer: EventWriter<UpdateAllStringsFilesForStringTableEvent>,
    yarn_files: Res<Assets<YarnFile>>,
) {
    for event in events.iter() {
        let (AssetEvent::Created { handle } | AssetEvent::Modified { handle }) = event else {
            continue;
        };
        if yarn_files_in_project.0.contains(handle) {
            let yarn_file = yarn_files.get(handle).unwrap();
            update_writer.send(UpdateAllStringsFilesForStringTableEvent(
                yarn_file.string_table.clone(),
            ));
        }
    }
}

fn send_update_events_on_localization_changes(
    yarn_compilation: Res<YarnCompilation>,
    mut creation_writer: EventWriter<CreateMissingStringsFilesEvent>,
    mut update_writer: EventWriter<UpdateAllStringsFilesForStringTableEvent>,
) {
    creation_writer.send(CreateMissingStringsFilesEvent);
    let string_table = &yarn_compilation.0.string_table;
    update_writer.send(UpdateAllStringsFilesForStringTableEvent(
        string_table.clone(),
    ));
}

fn update_all_strings_files_for_string_table(
    mut events: EventReader<UpdateAllStringsFilesForStringTableEvent>,
    mut missing_writer: EventWriter<CreateMissingStringsFilesEvent>,
    mut strings_files: ResMut<Assets<StringsFile>>,
    asset_server: Res<AssetServer>,
    localizations: Res<Localizations>,
    mut languages_to_update: Local<HashMap<Language, Handle<StringsFile>>>,
    current_strings_file: Res<CurrentStringsFile>,
) -> SystemResult {
    if !events.is_empty() {
        let supported_languages: HashSet<_> = localizations
            .translations
            .iter()
            .map(|t| t.language.clone())
            .collect();
        let updated_languages: HashSet<_> = languages_to_update.keys().cloned().collect();
        let languages_to_remove = updated_languages.difference(&supported_languages);
        let languages_to_add = supported_languages.difference(&updated_languages);
        for language in languages_to_remove {
            languages_to_update.remove(language);
        }
        for language in languages_to_add {
            let strings_file_path = localizations.strings_file_path(language).unwrap();
            let handle = if let Some(handle) = &current_strings_file.0 {
                handle.clone()
            } else if asset_server.asset_io().is_file(strings_file_path) {
                asset_server.load(strings_file_path)
            } else {
                missing_writer.send(CreateMissingStringsFilesEvent);
                return Ok(());
            };
            languages_to_update.insert(language.clone(), handle);
        }
        for handle in languages_to_update.values() {
            if asset_server.get_load_state(handle) != LoadState::Loaded {
                return Ok(());
            }
        }
    }
    let mut dirty_paths = HashSet::new();

    for string_table in events.iter().map(|e| &e.0) {
        let file_names: HashSet<_> = string_table
            .values()
            .map(|s| s.file_name.as_str())
            .collect();
        let file_names = file_names.into_iter().collect::<Vec<_>>().join(", ");
        for (language, strings_file_handle) in languages_to_update.drain() {
            let Some(strings_file) = strings_files.get_mut(&strings_file_handle) else {
                continue;
            };
            let strings_file_path = localizations.strings_file_path(&language).unwrap();

            let new_strings_file = match StringsFile::from_string_table(
                language.clone(),
                string_table,
            ) {
                Ok(new_strings_file) => new_strings_file,
                Err(e) => {
                    if localizations.file_generation_mode == FileGenerationMode::Development {
                        info!("Updating \"{}\" soon (lang: {language}) because the following yarn files were changed or loaded but do not have full line IDs yet: {file_names}",
                            strings_file_path.display())
                    } else {
                        warn!(
                            "Tried to update \"{}\" (lang: {language}) because the following yarn files were changed or loaded: {file_names}, but couldn't because: {e}",
                            strings_file_path.display(),
                        );
                    }
                    continue;
                }
            };
            if strings_file.update_file(new_strings_file)? {
                dirty_paths.insert((strings_file_handle, strings_file_path));
            }

            info!(
                "Updated \"{}\" (lang: {language}) because the following yarn files were changed or loaded: {file_names}",
                strings_file_path.display(),
            );
        }
    }
    for (handle, path) in &dirty_paths {
        let strings_file = strings_files.get(handle).unwrap();
        strings_file.write_asset(&asset_server, path)?;
    }
    Ok(())
}
