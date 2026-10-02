use std::str::FromStr;
use std::sync::Arc;

use crate::errors::{MhError, MhResult};
use crate::ipc_contract::{
    BackendLogEvent, SavedStateChangedEvent, ServiceLibraryPlaylistCreateRequest,
    ServiceLibraryPlaylistIdRequest, ServiceLibraryPlaylistMutateTracksRequest,
    ServiceLibraryPlaylistRenameRequest, ServiceLibraryPlaylistReorderRequest,
    ServiceLibraryRadioRequest, ServiceLibrarySavedStateRefreshRequest,
    ServiceLibrarySavedStateRequest, ServiceLibrarySetSavedRequest, ServicePlaylistChangedEvent,
};
use crate::services::common::library::{
    build_mutations, OwnedPlaylistRow, PlaylistCreateInput, PlaylistMutateResult, RadioResult,
    RadioSeedKind, SaveKind, ServiceLibraryMutations, ServicePlatform,
};

const SOURCE: &str = "service-library";

/// One saved-state mutation, boxed so every `(kind, action)` arm has one type.
type SavedMutation = Box<
    dyn for<'a> FnOnce(
            &'a Arc<dyn ServiceLibraryMutations>,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = MhResult<()>> + Send + 'a>,
        > + Send,
>;

/// The name this mutation logs under. Derived from `(kind, saved)` so the eight
/// spellings cannot drift apart the way the hand-written ones did.
fn saved_op_name(kind: SaveKind, saved: bool) -> &'static str {
    match (kind, saved) {
        (SaveKind::Track, true) => "save_tracks",
        (SaveKind::Track, false) => "unsave_tracks",
        (SaveKind::Album, true) => "save_albums",
        (SaveKind::Album, false) => "unsave_albums",
        (SaveKind::Artist, true) => "follow_artists",
        (SaveKind::Artist, false) => "unfollow_artists",
        (SaveKind::Playlist, true) => "follow_playlists",
        (SaveKind::Playlist, false) => "unfollow_playlists",
    }
}

impl crate::BackendState {
    async fn mutator(
        &self,
        platform_str: &str,
    ) -> MhResult<(ServicePlatform, Arc<dyn ServiceLibraryMutations>)> {
        let platform = ServicePlatform::from_str(platform_str)?;
        let mutator = build_mutations(platform, self).await?.ok_or_else(|| {
            MhError::Unsupported(format!(
                "{platform_str} does not let MediaHarbor change your library"
            ))
        })?;
        Ok((platform, mutator))
    }

    fn emit_saved(
        &self,
        platform: &str,
        kind: SaveKind,
        added: Vec<String>,
        removed: Vec<String>,
    ) -> SavedStateChangedEvent {
        let event = SavedStateChangedEvent {
            platform: platform.to_string(),
            kind: kind.as_str().to_string(),
            added,
            removed,
        };
        self.emitter.emit_saved_state_changed(&event);
        event
    }

    fn emit_playlist(
        &self,
        platform: &str,
        playlist_id: &str,
        change: &str,
        snapshot_id: Option<String>,
    ) -> ServicePlaylistChangedEvent {
        let event = ServicePlaylistChangedEvent {
            platform: platform.to_string(),
            playlist_id: playlist_id.to_string(),
            change: change.to_string(),
            snapshot_id,
        };
        self.emitter.emit_service_playlist_changed(&event);
        event
    }

    fn log_info(&self, title: &str, message: &str) {
        self.emitter
            .emit_log(&BackendLogEvent::info(SOURCE, title, message));
    }

    fn log_error(&self, title: &str, message: &str) {
        self.emitter
            .emit_log(&BackendLogEvent::error(SOURCE, title, message));
    }

    async fn run_mutation<F, T>(&self, platform_str: &str, title: &str, f: F) -> MhResult<T>
    where
        F: for<'a> FnOnce(
            &'a Arc<dyn ServiceLibraryMutations>,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = MhResult<T>> + Send + 'a>,
        >,
    {
        let (_, mutator) = self.mutator(platform_str).await.map_err(|e| {
            self.log_error(title, &format!("[{platform_str}] {e}"));
            e
        })?;
        match f(&mutator).await {
            Ok(v) => {
                self.log_info(title, &format!("[{platform_str}] ok"));
                Ok(v)
            }
            Err(e) => {
                self.log_error(title, &format!("[{platform_str}] {e}"));
                Err(e)
            }
        }
    }

    pub async fn service_library_saved_state_for(
        &self,
        req: ServiceLibrarySavedStateRequest,
    ) -> MhResult<Vec<String>> {
        let kind = SaveKind::from_str(&req.kind)?;
        let db = self.library.db();
        let stale = db.saved_state_is_stale(&req.platform, &req.kind)?;
        if stale {
            let key = (req.platform.clone(), req.kind.clone());
            let already_in_flight = {
                let mut guard = self.mirror_refresh_in_flight.lock().unwrap();
                if guard.contains(&key) {
                    true
                } else {
                    guard.insert(key.clone());
                    false
                }
            };
            if !already_in_flight {
                let refresh_result = self.refresh_saved_kinds(&req.platform, &[kind]).await;
                self.mirror_refresh_in_flight.lock().unwrap().remove(&key);
                if let Err(e) = refresh_result {
                    self.log_error(
                        "saved_state_for",
                        &format!("[{}] inline refresh failed: {e}", req.platform),
                    );
                }
            }
        }
        db.saved_state_ids_for(&req.platform, &req.kind)
    }

    async fn refresh_saved_kinds(&self, platform: &str, kinds: &[SaveKind]) -> MhResult<()> {
        let (_, mutator) = self.mutator(platform).await?;
        let set = mutator.fetch_saved_ids(kinds).await?;
        let db = self.library.db();
        for kind in kinds {
            let (ids, library_ids) = set.for_kind(*kind);
            db.saved_state_replace_all(platform, kind.as_str(), ids, library_ids)?;
            self.emit_saved(platform, *kind, ids.to_vec(), vec![]);
        }
        self.log_info(
            "saved_state_refresh",
            &format!(
                "[{platform}] kinds={:?}",
                kinds.iter().map(|k| k.as_str()).collect::<Vec<_>>()
            ),
        );
        Ok(())
    }

    pub async fn service_library_saved_state_refresh(
        &self,
        req: ServiceLibrarySavedStateRefreshRequest,
    ) -> MhResult<()> {
        let kinds: Vec<SaveKind> = req
            .kinds
            .iter()
            .map(|s| SaveKind::from_str(s))
            .collect::<MhResult<_>>()?;
        self.refresh_saved_kinds(&req.platform, &kinds).await
    }

    pub async fn service_library_owned_playlists(
        &self,
        platform_str: &str,
    ) -> MhResult<Vec<OwnedPlaylistRow>> {
        let db = self.library.db();
        let cached = db.owned_playlists_for(platform_str)?;
        if cached.is_empty() {
            let (_, mutator) = self.mutator(platform_str).await?;
            let rows = mutator.fetch_owned_playlists().await?;
            db.owned_playlists_replace_all(platform_str, &rows)?;
            return Ok(rows);
        }
        Ok(cached)
    }

    /// Shared body of the save/unsave/follow/unfollow family: run the service
    /// mutation, mirror it into the saved-state DB, then emit the change.
    ///
    /// The DB kind string comes from `SaveKind::as_str()` rather than being
    /// hand-written per method, so the two cannot drift apart.
    async fn apply_saved(
        &self,
        req: ServiceLibrarySetSavedRequest,
        f: SavedMutation,
    ) -> MhResult<SavedStateChangedEvent> {
        let kind = req.kind;
        self.run_mutation(&req.platform, saved_op_name(kind, req.saved), f)
            .await?;
        let db = self.library.db();
        if req.saved {
            db.saved_state_upsert(&req.platform, kind.as_str(), &req.ids, &[])?;
            Ok(self.emit_saved(&req.platform, kind, req.ids, vec![]))
        } else {
            db.saved_state_remove(&req.platform, kind.as_str(), &req.ids)?;
            Ok(self.emit_saved(&req.platform, kind, vec![], req.ids))
        }
    }

    pub async fn service_library_set_saved(
        &self,
        req: ServiceLibrarySetSavedRequest,
    ) -> MhResult<SavedStateChangedEvent> {
        let ids = req.ids.clone();
        let f: SavedMutation = match (req.kind, req.saved) {
            (SaveKind::Track, true) => {
                Box::new(move |m| Box::pin(async move { m.save_tracks(&ids).await }))
            }
            (SaveKind::Track, false) => {
                Box::new(move |m| Box::pin(async move { m.unsave_tracks(&ids).await }))
            }
            (SaveKind::Album, true) => {
                Box::new(move |m| Box::pin(async move { m.save_albums(&ids).await }))
            }
            (SaveKind::Album, false) => {
                Box::new(move |m| Box::pin(async move { m.unsave_albums(&ids).await }))
            }
            (SaveKind::Artist, true) => {
                Box::new(move |m| Box::pin(async move { m.follow_artists(&ids).await }))
            }
            (SaveKind::Artist, false) => {
                Box::new(move |m| Box::pin(async move { m.unfollow_artists(&ids).await }))
            }
            (SaveKind::Playlist, true) => Box::new(move |m| {
                Box::pin(async move {
                    for id in &ids {
                        m.follow_playlist(id).await?;
                    }
                    Ok(())
                })
            }),
            (SaveKind::Playlist, false) => Box::new(move |m| {
                Box::pin(async move {
                    for id in &ids {
                        m.unfollow_playlist(id).await?;
                    }
                    Ok(())
                })
            }),
        };
        self.apply_saved(req, f).await
    }

    pub async fn service_library_follow_user(
        &self,
        req: crate::ipc_contract::ServiceLibraryIdRequest,
    ) -> MhResult<()> {
        self.run_mutation(&req.platform, "follow_user", {
            let id = req.id.clone();
            move |m| Box::pin(async move { m.follow_user(&id).await })
        })
        .await
    }

    pub async fn service_library_unfollow_user(
        &self,
        req: crate::ipc_contract::ServiceLibraryIdRequest,
    ) -> MhResult<()> {
        self.run_mutation(&req.platform, "unfollow_user", {
            let id = req.id.clone();
            move |m| Box::pin(async move { m.unfollow_user(&id).await })
        })
        .await
    }

    pub async fn service_library_playlist_create(
        &self,
        req: ServiceLibraryPlaylistCreateRequest,
    ) -> MhResult<PlaylistMutateResult> {
        let input = PlaylistCreateInput {
            name: req.name.clone(),
            description: req.description.clone(),
            is_public: req.is_public,
            is_collaborative: req.is_collaborative,
            initial_track_ids: req.initial_track_ids.clone(),
        };
        let result = self
            .run_mutation(&req.platform, "playlist_create", {
                let input = input.clone();
                move |m| Box::pin(async move { m.create_playlist(input).await })
            })
            .await?;
        self.library.db().owned_playlists_upsert_one(
            &req.platform,
            &result.playlist_id,
            &req.name,
            None,
            req.initial_track_ids.len() as i64,
        )?;
        self.emit_playlist(
            &req.platform,
            &result.playlist_id,
            "created",
            result.snapshot_id.clone(),
        );
        Ok(result)
    }

    pub async fn service_library_playlist_rename(
        &self,
        req: ServiceLibraryPlaylistRenameRequest,
    ) -> MhResult<()> {
        self.run_mutation(&req.platform, "playlist_rename", {
            let id = req.id.clone();
            let name = req.name.clone();
            let desc = req.description.clone();
            let is_public = req.is_public;
            let is_collaborative = req.is_collaborative;
            move |m| {
                Box::pin(async move {
                    m.rename_playlist(&id, &name, desc.as_deref(), is_public, is_collaborative)
                        .await
                })
            }
        })
        .await?;
        self.library
            .db()
            .owned_playlists_upsert_one(&req.platform, &req.id, &req.name, None, 0)?;
        self.emit_playlist(&req.platform, &req.id, "renamed", None);
        Ok(())
    }

    pub async fn service_library_playlist_delete(
        &self,
        req: ServiceLibraryPlaylistIdRequest,
    ) -> MhResult<()> {
        self.run_mutation(&req.platform, "playlist_delete", {
            let id = req.id.clone();
            move |m| Box::pin(async move { m.delete_playlist(&id).await })
        })
        .await?;
        self.library
            .db()
            .owned_playlists_delete_one(&req.platform, &req.id)?;
        self.emit_playlist(&req.platform, &req.id, "deleted", None);
        Ok(())
    }

    pub async fn service_library_playlist_add_tracks(
        &self,
        req: ServiceLibraryPlaylistMutateTracksRequest,
    ) -> MhResult<PlaylistMutateResult> {
        let result = self
            .run_mutation(&req.platform, "playlist_add_tracks", {
                let id = req.id.clone();
                let ids = req.track_ids.clone();
                move |m| Box::pin(async move { m.add_playlist_tracks(&id, &ids).await })
            })
            .await?;
        self.emit_playlist(
            &req.platform,
            &req.id,
            "tracks_added",
            result.snapshot_id.clone(),
        );
        Ok(result)
    }

    pub async fn service_library_playlist_remove_tracks(
        &self,
        req: ServiceLibraryPlaylistMutateTracksRequest,
    ) -> MhResult<PlaylistMutateResult> {
        let result = self
            .run_mutation(&req.platform, "playlist_remove_tracks", {
                let id = req.id.clone();
                let ids = req.track_ids.clone();
                let positions = req.positions.clone();
                move |m| {
                    Box::pin(async move {
                        m.remove_playlist_tracks(&id, &ids, positions.as_deref())
                            .await
                    })
                }
            })
            .await?;
        self.emit_playlist(
            &req.platform,
            &req.id,
            "tracks_removed",
            result.snapshot_id.clone(),
        );
        Ok(result)
    }

    pub async fn service_library_playlist_reorder(
        &self,
        req: ServiceLibraryPlaylistReorderRequest,
    ) -> MhResult<PlaylistMutateResult> {
        let result = self
            .run_mutation(&req.platform, "playlist_reorder", {
                let id = req.id.clone();
                let from = req.from;
                let to = req.to;
                move |m| Box::pin(async move { m.reorder_playlist(&id, from, to).await })
            })
            .await?;
        self.emit_playlist(
            &req.platform,
            &req.id,
            "reordered",
            result.snapshot_id.clone(),
        );
        Ok(result)
    }

    pub async fn service_library_radio_for(
        &self,
        req: ServiceLibraryRadioRequest,
    ) -> MhResult<RadioResult> {
        let seed_kind = RadioSeedKind::from_str(&req.seed_kind)?;
        self.run_mutation(&req.platform, "radio_for", {
            let seed_id = req.seed_id.clone();
            move |m| Box::pin(async move { m.radio_for(seed_kind, &seed_id).await })
        })
        .await
    }

    pub async fn service_library_radio_continue(
        &self,
        req: crate::ipc_contract::ServiceLibraryRadioContinueRequest,
    ) -> MhResult<RadioResult> {
        self.run_mutation(&req.platform, "radio_continue", {
            let token = req.continuation.clone();
            move |m| Box::pin(async move { m.radio_continue(&token).await })
        })
        .await
    }
}
