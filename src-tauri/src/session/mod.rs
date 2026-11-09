use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    error::{AppError, Result},
    storage::{clamp_text, Game, ImageSpec, TEXT_MAX},
};

pub const HISTORY_LIMIT: usize = 500;
/// A start time slightly in the future is tolerated to absorb clock skew.
const FUTURE_SLACK_MS: i64 = 60_000;

/// Everything that appears on the Discord card for a session. It starts as a
/// copy of the game's defaults and can then be edited freely.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct PresenceConfig {
    pub game_name: String,
    pub details: String,
    pub state: String,
    pub large_image: ImageSpec,
    pub large_image_text: String,
    pub small_image: ImageSpec,
    pub small_image_text: String,
    /// Unix ms shown as the start of the elapsed timer; `None` = session start.
    pub start_timestamp: Option<i64>,
    /// Mark the card as live: a "방송 중" badge on the state line that links to
    /// the stream address from the settings.
    pub live: bool,
}

impl PresenceConfig {
    pub fn normalize(&mut self) {
        self.game_name = clamp_text(&self.game_name, TEXT_MAX);
        self.details = clamp_text(&self.details, TEXT_MAX);
        self.state = clamp_text(&self.state, TEXT_MAX);
        self.large_image_text = clamp_text(&self.large_image_text, TEXT_MAX);
        self.small_image_text = clamp_text(&self.small_image_text, TEXT_MAX);
        self.large_image.normalize();
        self.small_image.normalize();
    }

    /// Image specs are intentionally not rejected here: an unusable image is
    /// reported as a warning and replaced by the fallback instead.
    pub fn validate(&self, now: i64) -> Result<()> {
        if self.game_name.is_empty() {
            return Err(AppError::Invalid("게임 이름을 입력하세요.".into()));
        }
        if let Some(ts) = self.start_timestamp {
            if ts <= 0 {
                return Err(AppError::Invalid("시작 시간이 올바르지 않습니다.".into()));
            }
            if ts > now + FUTURE_SLACK_MS {
                return Err(AppError::Invalid("시작 시간은 미래일 수 없습니다.".into()));
            }
        }
        Ok(())
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    pub id: String,
    pub game_id: String,
    pub game_name: String,
    pub platform: String,
    pub started_at: i64,
    /// Heartbeat written periodically; used to close a session that was
    /// interrupted by a crash at the time the app last ran.
    pub last_seen_at: i64,
    pub presence: PresenceConfig,
    /// `false` after "clear presence": the session keeps running but nothing
    /// is shown on Discord.
    pub presence_visible: bool,
}

impl Session {
    pub fn effective_start(&self) -> i64 {
        self.presence.start_timestamp.unwrap_or(self.started_at)
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SessionRecord {
    pub id: String,
    pub game_id: String,
    pub game_name: String,
    pub platform: String,
    pub started_at: i64,
    pub ended_at: i64,
    pub duration_secs: i64,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RecentGame {
    pub game_id: String,
    pub last_played_at: i64,
    pub total_secs: i64,
    pub sessions: u32,
}

/// The persisted session state. It never talks to Discord: the connection and
/// the session are independent, so a dropped connection cannot end a session.
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct SessionStore {
    pub active: Option<Session>,
    pub history: Vec<SessionRecord>,
}

impl SessionStore {
    /// Starts a session for `game`. At most one session exists at a time:
    /// starting the same game again only refreshes its presence, starting
    /// another game ends the current one first.
    pub fn start(&mut self, game: &Game, mut presence: PresenceConfig, now: i64) -> Session {
        presence.normalize();
        if let Some(active) = self.active.as_mut() {
            if active.game_id == game.id {
                active.presence = presence;
                active.presence_visible = true;
                active.last_seen_at = now;
                return active.clone();
            }
        }
        self.stop(now);
        let session = Session {
            id: Uuid::new_v4().to_string(),
            game_id: game.id.clone(),
            game_name: game.name.clone(),
            platform: game.platform.clone(),
            started_at: now,
            last_seen_at: now,
            presence,
            presence_visible: true,
        };
        self.active = Some(session.clone());
        session
    }

    /// Ends the active session at `now` and records it in the history.
    pub fn stop(&mut self, now: i64) -> Option<SessionRecord> {
        let session = self.active.take()?;
        let ended_at = now.max(session.started_at);
        let record = SessionRecord {
            duration_secs: (ended_at - session.started_at) / 1000,
            id: session.id,
            game_id: session.game_id,
            game_name: session.game_name,
            platform: session.platform,
            started_at: session.started_at,
            ended_at,
        };
        self.history.push(record.clone());
        if self.history.len() > HISTORY_LIMIT {
            let excess = self.history.len() - HISTORY_LIMIT;
            self.history.drain(..excess);
        }
        Some(record)
    }

    pub fn update_presence(&mut self, mut presence: PresenceConfig, now: i64) -> Result<Session> {
        presence.normalize();
        presence.validate(now)?;
        let active = self.active_mut()?;
        active.presence = presence;
        active.presence_visible = true;
        active.last_seen_at = now;
        Ok(active.clone())
    }

    pub fn set_visible(&mut self, visible: bool) -> Result<Session> {
        let active = self.active_mut()?;
        active.presence_visible = visible;
        Ok(active.clone())
    }

    /// Records a heartbeat. Returns whether there was a session to update.
    pub fn touch(&mut self, now: i64) -> bool {
        match self.active.as_mut() {
            Some(active) => {
                active.last_seen_at = now;
                true
            }
            None => false,
        }
    }

    pub fn recent_games(&self, now: i64, limit: usize) -> Vec<RecentGame> {
        let mut games: Vec<RecentGame> = Vec::new();
        let mut add = |game_id: &str, played_at: i64, secs: i64| {
            match games.iter_mut().find(|g| g.game_id == game_id) {
                Some(g) => {
                    g.last_played_at = g.last_played_at.max(played_at);
                    g.total_secs += secs;
                    g.sessions += 1;
                }
                None => games.push(RecentGame {
                    game_id: game_id.to_string(),
                    last_played_at: played_at,
                    total_secs: secs,
                    sessions: 1,
                }),
            }
        };
        for record in &self.history {
            add(&record.game_id, record.ended_at, record.duration_secs);
        }
        if let Some(active) = &self.active {
            add(&active.game_id, now, (now - active.started_at).max(0) / 1000);
        }
        games.sort_by(|a, b| b.last_played_at.cmp(&a.last_played_at));
        games.truncate(limit);
        games
    }

    fn active_mut(&mut self) -> Result<&mut Session> {
        self.active
            .as_mut()
            .ok_or_else(|| AppError::NotFound("진행 중인 세션이 없습니다.".into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const T0: i64 = 1_762_646_400_000;

    fn game(id: &str) -> Game {
        Game {
            id: id.into(),
            name: format!("Game {id}"),
            platform: "Nintendo Switch".into(),
            ..Game::default()
        }
    }

    fn presence(name: &str) -> PresenceConfig {
        PresenceConfig {
            game_name: name.into(),
            ..PresenceConfig::default()
        }
    }

    #[test]
    fn start_creates_the_active_session() {
        let mut store = SessionStore::default();
        let s = store.start(&game("a"), presence("A"), T0);
        assert_eq!(s.game_id, "a");
        assert_eq!(s.started_at, T0);
        assert!(s.presence_visible);
        assert_eq!(store.active.as_ref().unwrap().id, s.id);
        assert!(store.history.is_empty());
    }

    #[test]
    fn starting_the_same_game_twice_does_not_create_a_second_session() {
        let mut store = SessionStore::default();
        let first = store.start(&game("a"), presence("A"), T0);
        let again = store.start(&game("a"), presence("A (edited)"), T0 + 5_000);
        assert_eq!(first.id, again.id);
        assert_eq!(again.started_at, T0, "start time must not reset");
        assert_eq!(again.presence.game_name, "A (edited)");
        assert!(store.history.is_empty());
    }

    #[test]
    fn starting_another_game_ends_the_previous_session() {
        let mut store = SessionStore::default();
        let a = store.start(&game("a"), presence("A"), T0);
        let b = store.start(&game("b"), presence("B"), T0 + 90_000);
        assert_ne!(a.id, b.id);
        assert_eq!(store.history.len(), 1);
        assert_eq!(store.history[0].game_id, "a");
        assert_eq!(store.history[0].duration_secs, 90);
        assert_eq!(store.active.as_ref().unwrap().game_id, "b");
    }

    #[test]
    fn stop_records_duration_and_clears_the_active_session() {
        let mut store = SessionStore::default();
        store.start(&game("a"), presence("A"), T0);
        let record = store.stop(T0 + 3_600_000).unwrap();
        assert_eq!(record.duration_secs, 3600);
        assert!(store.active.is_none());
        assert!(store.stop(T0 + 4_000_000).is_none(), "nothing left to stop");
    }

    #[test]
    fn stop_never_records_a_negative_duration() {
        let mut store = SessionStore::default();
        store.start(&game("a"), presence("A"), T0);
        let record = store.stop(T0 - 10_000).unwrap();
        assert_eq!(record.duration_secs, 0);
        assert_eq!(record.ended_at, T0);
    }

    #[test]
    fn update_presence_needs_an_active_session_and_a_sane_start_time() {
        let mut store = SessionStore::default();
        assert!(matches!(
            store.update_presence(presence("A"), T0),
            Err(AppError::NotFound(_))
        ));

        store.start(&game("a"), presence("A"), T0);
        let mut future = presence("A");
        future.start_timestamp = Some(T0 + 3_600_000);
        assert!(store.update_presence(future, T0).is_err());

        let mut past = presence("A");
        past.start_timestamp = Some(T0 - 3_600_000);
        let s = store.update_presence(past, T0).unwrap();
        assert_eq!(s.effective_start(), T0 - 3_600_000);
        assert_eq!(s.started_at, T0, "the recorded session start is unchanged");

        assert!(store.update_presence(presence("  "), T0).is_err());
    }

    #[test]
    fn hiding_presence_keeps_the_session_running() {
        let mut store = SessionStore::default();
        store.start(&game("a"), presence("A"), T0);
        let hidden = store.set_visible(false).unwrap();
        assert!(!hidden.presence_visible);
        assert!(store.active.is_some());
        let shown = store.update_presence(presence("A"), T0).unwrap();
        assert!(shown.presence_visible, "updating re-shows the presence");
    }

    #[test]
    fn recent_games_aggregates_and_orders_by_last_played() {
        let mut store = SessionStore::default();
        store.start(&game("a"), presence("A"), T0);
        store.start(&game("b"), presence("B"), T0 + 60_000);
        store.start(&game("a"), presence("A"), T0 + 120_000);
        store.stop(T0 + 180_000);

        let recent = store.recent_games(T0 + 200_000, 10);
        assert_eq!(recent.len(), 2);
        assert_eq!(recent[0].game_id, "a", "a was played most recently");
        assert_eq!(recent[0].sessions, 2);
        assert_eq!(recent[0].total_secs, 60 + 60);
        assert_eq!(recent[1].game_id, "b");
        assert_eq!(store.recent_games(T0, 1).len(), 1, "limit is honoured");
    }

    #[test]
    fn the_running_game_counts_as_most_recent() {
        let mut store = SessionStore::default();
        store.start(&game("a"), presence("A"), T0);
        store.stop(T0 + 1_000);
        store.start(&game("b"), presence("B"), T0 + 2_000);
        let recent = store.recent_games(T0 + 500_000, 10);
        assert_eq!(recent[0].game_id, "b");
    }

    #[test]
    fn history_is_capped_to_the_most_recent_entries() {
        let mut store = SessionStore::default();
        for i in 0..(HISTORY_LIMIT as i64 + 25) {
            store.start(&game("a"), presence("A"), T0 + i * 1_000);
            store.stop(T0 + i * 1_000 + 500);
        }
        assert_eq!(store.history.len(), HISTORY_LIMIT);
        assert_eq!(
            store.history.last().unwrap().started_at,
            T0 + (HISTORY_LIMIT as i64 + 24) * 1_000
        );
    }

    #[test]
    fn session_state_survives_a_json_round_trip() {
        let mut store = SessionStore::default();
        store.start(&game("a"), presence("A"), T0);
        let json = serde_json::to_string(&store).unwrap();
        let restored: SessionStore = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.active, store.active);
    }
}
