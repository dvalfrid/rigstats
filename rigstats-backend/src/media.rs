//! What is playing (#718): the title, artist, album, state and cover picture
//! of a media session — the one shown keeps its place while it plays, so a
//! video started in a browser doesn't take over from music playing in the
//! background; otherwise the one Windows considers current, the one its own
//! media flyout shows ([`pick_session`]). Read through Windows' media sessions
//! (`GlobalSystemMediaTransportControlsSessionManager`), so it works for any
//! player that tells Windows what it plays (Spotify, browsers, …) with no
//! account and nothing per player.
//!
//! A [`Watcher`] reads on a thread of its own about once a second, so a slow
//! or hanging player never holds up the poll loop; the thread starts on the
//! first [`Watcher::latest`] and ends a few seconds after the last one, so
//! nothing runs while no panel shows it.
//!
//! What someone listens to is personal: nothing here is logged except
//! whether the media source works, never a title.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// A track, as the player describes it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Track {
    pub title: String,
    /// Empty when the player doesn't say (often a browser video).
    pub artist: String,
    /// Empty when the player doesn't say.
    pub album: String,
}

/// The current session: its track, whether it plays, and the cover picture
/// once read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NowPlaying {
    pub track: Track,
    pub playing: bool,
    pub cover: Option<Cover>,
}

/// The cover picture as the player gave it (PNG or JPEG bytes), read once
/// per track. `id` changes with every new picture, so the panel decodes and
/// uploads it once.
#[derive(Clone, PartialEq, Eq)]
pub struct Cover {
    pub id: u64,
    pub bytes: Arc<[u8]>,
}

impl std::fmt::Debug for Cover {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Cover {{ id: {}, {} bytes }}", self.id, self.bytes.len())
    }
}

/// Windows' playback status, from `GlobalSystemMediaTransportControlsSessionPlaybackStatus`
/// (0 Closed, 1 Opened, 2 Changing, 3 Stopped, 4 Playing, 5 Paused).
pub fn is_playing(status: i32) -> bool {
    status == 4
}

/// A track from what the player reports, or `None` when it reports no title
/// (a session between tracks, or one that says nothing): the panel then
/// shows "Nothing playing" rather than an empty row.
pub fn track(title: &str, artist: &str, album: &str) -> Option<Track> {
    let title = title.trim();
    if title.is_empty() {
        return None;
    }
    Some(Track {
        title: title.to_string(),
        artist: artist.trim().to_string(),
        album: album.trim().to_string(),
    })
}

/// Which player's session to show, by its app id, from every session and
/// whether it plays: the one shown so far while it still plays — Windows
/// makes the newest session current, so a Twitch stream started in a
/// browser would otherwise push aside music that goes on playing — then
/// Windows' current one if it plays, then any that plays, and with nothing
/// playing Windows' current one (paused or stopped), as its flyout does.
pub fn pick_session(
    shown: Option<&str>,
    current: Option<&str>,
    sessions: &[(String, bool)],
) -> Option<String> {
    let plays = |id: &str| sessions.iter().any(|(s, p)| s == id && *p);
    if let Some(id) = shown.filter(|id| plays(id)) {
        return Some(id.to_owned());
    }
    if let Some(id) = current.filter(|id| plays(id)) {
        return Some(id.to_owned());
    }
    if let Some((id, _)) = sessions.iter().find(|(_, p)| *p) {
        return Some(id.clone());
    }
    current.map(str::to_owned)
}

/// How many times a track's missing picture is asked for: players often
/// publish the title first and the picture a moment later.
pub const COVER_TRIES: u8 = 5;

/// Whether to read the cover now: on a new track, or again for the same
/// track while it has none and tries are left.
pub fn read_cover(before: Option<&Track>, now: &Track, have_cover: bool, tries: u8) -> bool {
    before != Some(now) || (!have_cover && tries < COVER_TRIES)
}

/// The largest picture read; a player's thumbnail is a few hundred KB at most.
const MAX_COVER_BYTES: u64 = 4 * 1024 * 1024;

/// How long the thread keeps reading after the last [`Watcher::latest`].
const IDLE: Duration = Duration::from_secs(5);

const PERIOD: Duration = Duration::from_secs(1);

#[derive(Default)]
struct Shared {
    latest: Mutex<Option<NowPlaying>>,
    last_read: Mutex<Option<Instant>>,
    running: AtomicBool,
}

/// Reads the current media session on its own thread (see the module).
#[derive(Clone, Default)]
pub struct Watcher {
    shared: Arc<Shared>,
}

impl Watcher {
    pub fn new() -> Self {
        Self::default()
    }

    /// What is playing, as last read — `None` when nothing is, or before the
    /// first read. Starts the reading thread when it isn't running.
    pub fn latest(&self) -> Option<NowPlaying> {
        if let Ok(mut t) = self.shared.last_read.lock() {
            *t = Some(Instant::now());
        }
        if !self.shared.running.swap(true, Ordering::AcqRel) {
            let shared = Arc::clone(&self.shared);
            let spawned = std::thread::Builder::new()
                .name("media".into())
                .spawn(move || {
                    run(&shared);
                    if let Ok(mut l) = shared.latest.lock() {
                        *l = None;
                    }
                    shared.running.store(false, Ordering::Release);
                });
            if spawned.is_err() {
                self.shared.running.store(false, Ordering::Release);
            }
        }
        self.shared.latest.lock().ok().and_then(|l| l.clone())
    }
}

fn run(shared: &Shared) {
    let keep = || {
        shared
            .last_read
            .lock()
            .ok()
            .and_then(|t| *t)
            .is_some_and(|t| t.elapsed() < IDLE)
    };
    let mut reader = win::Reader::default();
    let mut before: Option<Track> = None;
    let mut cover: Option<Cover> = None;
    let mut tries = 0u8;
    while keep() {
        let now = reader.read().map(|(track, playing, session)| {
            if read_cover(before.as_ref(), &track, cover.is_some(), tries) {
                if before.as_ref() != Some(&track) {
                    cover = None;
                    tries = 0;
                }
                tries = tries.saturating_add(1);
                if let Some(bytes) = win::cover(&session) {
                    cover = Some(Cover {
                        id: next_cover_id(),
                        bytes,
                    });
                }
            }
            before = Some(track.clone());
            NowPlaying {
                track,
                playing,
                cover: cover.clone(),
            }
        });
        if now.is_none() {
            before = None;
            cover = None;
            tries = 0;
        }
        if let Ok(mut l) = shared.latest.lock() {
            *l = now;
        }
        std::thread::sleep(PERIOD);
    }
}

fn next_cover_id() -> u64 {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

#[cfg(windows)]
mod win {
    use super::{is_playing, pick_session, track, Track, MAX_COVER_BYTES};
    use std::sync::Arc;
    use windows::Media::Control::{
        GlobalSystemMediaTransportControlsSession as Session,
        GlobalSystemMediaTransportControlsSessionManager as Manager,
    };
    use windows::Storage::Streams::DataReader;

    /// Keeps the session manager between reads; asked for again after an
    /// error (Windows' media service restarted, say). `shown` is the app id
    /// of the session read last, kept while it plays ([`super::pick_session`]).
    #[derive(Default)]
    pub struct Reader {
        manager: Option<Manager>,
        shown: Option<String>,
    }

    impl Reader {
        /// The current session's track and state, with the session for the
        /// cover; `None` when there is none or it can't be read.
        pub fn read(&mut self) -> Option<(Track, bool, Session)> {
            if self.manager.is_none() {
                self.manager = Manager::RequestAsync().and_then(|op| op.join()).ok();
            }
            let manager = self.manager.as_ref()?;
            let app = |s: &Session| {
                s.SourceAppUserModelId()
                    .map(|id| id.to_string_lossy())
                    .unwrap_or_default()
            };
            let sessions: Vec<Session> = manager
                .GetSessions()
                .map(|list| list.into_iter().collect())
                .unwrap_or_default();
            let list: Vec<(String, bool)> = sessions
                .iter()
                .map(|s| {
                    let playing = s
                        .GetPlaybackInfo()
                        .and_then(|i| i.PlaybackStatus())
                        .is_ok_and(|st| is_playing(st.0));
                    (app(s), playing)
                })
                .collect();
            // No current session is an error here too ("not found"), not a
            // reason to ask for the manager again.
            let current = manager.GetCurrentSession().ok();
            let current_app = current.as_ref().map(app);
            let chosen = pick_session(self.shown.as_deref(), current_app.as_deref(), &list)?;
            let session = match sessions.into_iter().find(|s| app(s) == chosen) {
                Some(s) => s,
                None => current?,
            };
            self.shown = Some(chosen);
            let read = || -> windows::core::Result<Option<(Track, bool)>> {
                let props = session.TryGetMediaPropertiesAsync()?.join()?;
                let status = session.GetPlaybackInfo()?.PlaybackStatus()?;
                Ok(track(
                    &props.Title()?.to_string_lossy(),
                    &props.Artist()?.to_string_lossy(),
                    &props.AlbumTitle()?.to_string_lossy(),
                )
                .map(|t| (t, is_playing(status.0))))
            };
            match read() {
                Ok(Some((t, playing))) => Some((t, playing, session)),
                Ok(None) => None,
                Err(_) => None,
            }
        }
    }

    /// The session's cover picture, or `None` when it has none (yet).
    pub fn cover(session: &Session) -> Option<Arc<[u8]>> {
        let read = || -> windows::core::Result<Option<Arc<[u8]>>> {
            let props = session.TryGetMediaPropertiesAsync()?.join()?;
            let Ok(reference) = props.Thumbnail() else {
                return Ok(None);
            };
            let stream = reference.OpenReadAsync()?.join()?;
            let size = stream.Size()?;
            if size == 0 || size > MAX_COVER_BYTES {
                return Ok(None);
            }
            let reader = DataReader::CreateDataReader(&stream.GetInputStreamAt(0)?)?;
            let loaded = reader.LoadAsync(size as u32)?.join()?;
            let mut bytes = vec![0u8; loaded as usize];
            reader.ReadBytes(&mut bytes)?;
            Ok(Some(bytes.into()))
        };
        read().ok().flatten()
    }
}

#[cfg(not(windows))]
mod win {
    use super::Track;
    use std::sync::Arc;

    pub struct Session;

    #[derive(Default)]
    pub struct Reader;

    impl Reader {
        pub fn read(&mut self) -> Option<(Track, bool, Session)> {
            None
        }
    }

    pub fn cover(_: &Session) -> Option<Arc<[u8]>> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(title: &str) -> Track {
        track(title, "Artist", "Album").unwrap()
    }

    #[test]
    fn a_track_needs_a_title() {
        assert_eq!(track("", "Artist", "Album"), None);
        assert_eq!(track("   ", "Artist", "Album"), None);
        let got = track(" Song ", " Artist ", "").unwrap();
        assert_eq!(got.title, "Song");
        assert_eq!(got.artist, "Artist");
        assert_eq!(got.album, "");
    }

    fn list(sessions: &[(&str, bool)]) -> Vec<(String, bool)> {
        sessions
            .iter()
            .map(|(s, p)| ((*s).to_owned(), *p))
            .collect()
    }

    /// Music goes on playing when a browser video starts (seen 2026-11-19:
    /// Windows made Brave current over Apple Music).
    #[test]
    fn a_new_player_doesnt_take_over_from_one_that_plays() {
        let both = list(&[("music", true), ("brave", true)]);
        assert_eq!(
            pick_session(Some("music"), Some("brave"), &both).as_deref(),
            Some("music")
        );
    }

    #[test]
    fn a_paused_player_gives_way_to_one_that_plays() {
        let s = list(&[("music", false), ("brave", true)]);
        assert_eq!(
            pick_session(Some("music"), Some("brave"), &s).as_deref(),
            Some("brave")
        );
        // Windows' current one paused, another playing: the playing one.
        let s = list(&[("music", true), ("brave", false)]);
        assert_eq!(
            pick_session(None, Some("brave"), &s).as_deref(),
            Some("music")
        );
    }

    #[test]
    fn with_nothing_playing_windows_choice_shows() {
        let s = list(&[("music", false), ("brave", false)]);
        assert_eq!(
            pick_session(Some("music"), Some("brave"), &s).as_deref(),
            Some("brave")
        );
        assert_eq!(pick_session(None, None, &s), None);
        assert_eq!(pick_session(None, None, &[]), None);
    }

    #[test]
    fn a_closed_player_is_let_go() {
        let s = list(&[("brave", true)]);
        assert_eq!(
            pick_session(Some("music"), Some("brave"), &s).as_deref(),
            Some("brave")
        );
    }

    #[test]
    fn only_playing_counts_as_playing() {
        assert!(is_playing(4));
        for status in [0, 1, 2, 3, 5, 99] {
            assert!(!is_playing(status), "{status}");
        }
    }

    #[test]
    fn the_cover_is_read_for_a_new_track() {
        assert!(read_cover(None, &t("A"), false, 0));
        assert!(read_cover(Some(&t("A")), &t("B"), true, 1));
    }

    #[test]
    fn the_same_track_keeps_its_cover() {
        assert!(!read_cover(Some(&t("A")), &t("A"), true, 1));
    }

    #[test]
    fn a_missing_cover_is_asked_for_a_few_times() {
        assert!(read_cover(Some(&t("A")), &t("A"), false, 1));
        assert!(read_cover(Some(&t("A")), &t("A"), false, COVER_TRIES - 1));
        assert!(!read_cover(Some(&t("A")), &t("A"), false, COVER_TRIES));
    }

    #[test]
    fn a_cover_prints_its_size_not_its_bytes() {
        let c = Cover {
            id: 7,
            bytes: vec![1u8; 300].into(),
        };
        assert_eq!(format!("{c:?}"), "Cover { id: 7, 300 bytes }");
    }
}
