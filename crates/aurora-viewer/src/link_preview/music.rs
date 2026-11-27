//! Provider-hosted players for music links shared in conversations.

use reqwest::Url;

use super::{Failure, Picture};

pub struct Info {
    pub title: String,
    pub cover: Option<Picture>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Provider {
    Spotify,
    AppleMusic,
    YouTubeMusic,
    Suno,
    SoundCloud,
}

impl Provider {
    pub fn name(self) -> &'static str {
        match self {
            Self::Spotify => "Spotify",
            Self::AppleMusic => "Apple Music",
            Self::YouTubeMusic => "YouTube Music",
            Self::Suno => "Suno",
            Self::SoundCloud => "SoundCloud",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    Track,
    Album,
    Playlist,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Music {
    pub provider: Provider,
    pub kind: Kind,
    embed: Url,
}

fn id(value: &str, min: usize, max: usize, punctuation: &[u8]) -> bool {
    (min..=max).contains(&value.len()) && value.bytes().all(|b| b.is_ascii_alphanumeric() || punctuation.contains(&b))
}

fn digits(value: &str) -> bool {
    !value.is_empty() && value.bytes().all(|b| b.is_ascii_digit())
}

impl Music {
    pub(super) fn of(url: &Url) -> Option<Self> {
        if url.scheme() != "https" || !url.username().is_empty() || url.password().is_some() || url.port().is_some() {
            return None;
        }
        let provider = match url.host_str()? {
            "open.spotify.com" => Provider::Spotify,
            "music.apple.com" | "embed.music.apple.com" => Provider::AppleMusic,
            "music.youtube.com" => Provider::YouTubeMusic,
            "suno.com" | "www.suno.com" => Provider::Suno,
            "soundcloud.com" | "www.soundcloud.com" | "m.soundcloud.com" => Provider::SoundCloud,
            _ => return None,
        };
        let mut path: Vec<_> = url.path().trim_end_matches('/').strip_prefix('/')?.split('/').collect();
        let query = |key| url.query_pairs().find(|(name, _)| name == key).map(|(_, value)| value.into_owned());
        let (kind, embed) = match provider {
            Provider::Spotify => {
                if path.first().is_some_and(|part| part.starts_with("intl-") && id(part, 7, 12, b"-")) {
                    path.remove(0);
                }
                if path.first() == Some(&"embed") {
                    path.remove(0);
                }
                let [category, spotify_id] = path.as_slice() else { return None };
                if !id(spotify_id, 22, 22, b"") {
                    return None;
                }
                let kind = match *category {
                    "track" => Kind::Track,
                    "album" => Kind::Album,
                    "playlist" => Kind::Playlist,
                    _ => return None,
                };
                (kind, format!("https://open.spotify.com/embed/{category}/{spotify_id}"))
            }
            Provider::AppleMusic => {
                let [storefront, category, item @ ..] = path.as_slice() else {
                    return None;
                };
                let ([identifier] | [_, identifier]) = item else { return None };
                if storefront.len() != 2 || !storefront.bytes().all(|b| b.is_ascii_alphabetic()) {
                    return None;
                }
                let track = query("i").filter(|value| digits(value));
                let kind = match *category {
                    "song" if digits(identifier) => Kind::Track,
                    "album" if digits(identifier) && track.is_some() => Kind::Track,
                    "album" if digits(identifier) => Kind::Album,
                    "playlist" if identifier.strip_prefix("pl.").is_some_and(|value| id(value, 1, 96, b"-_.")) => Kind::Playlist,
                    _ => return None,
                };
                let mut embed = format!("https://embed.music.apple.com/{storefront}/{category}/{identifier}");
                if *category == "album"
                    && let Some(track) = track
                {
                    embed.push_str(&format!("?i={track}"));
                }
                (kind, embed)
            }
            Provider::YouTubeMusic => {
                let playlist = query("list").filter(|value| id(value, 12, 128, b"-_"))?;
                let embed = match path.as_slice() {
                    ["playlist"] => format!("https://www.youtube-nocookie.com/embed/videoseries?list={playlist}"),
                    ["watch"] => {
                        let video = query("v").filter(|value| id(value, 11, 11, b"-_"))?;
                        let start = query("t")
                            .or_else(|| query("start"))
                            .map_or(0, |value| super::start_seconds(&value));
                        format!("https://www.youtube-nocookie.com/embed/{video}?list={playlist}&start={start}")
                    }
                    _ => return None,
                };
                let kind = if playlist.starts_with("OLAK5uy_") {
                    Kind::Album
                } else {
                    Kind::Playlist
                };
                (kind, embed)
            }
            Provider::Suno => {
                let ["song" | "embed", song] = path.as_slice() else { return None };
                let song = uuid::Uuid::parse_str(song).ok()?;
                (Kind::Track, format!("https://suno.com/embed/{song}"))
            }
            Provider::SoundCloud => {
                let kind = match path.as_slice() {
                    [artist, track] if id(artist, 1, 100, b"-_") && id(track, 1, 100, b"-_") && *track != "sets" => Kind::Track,
                    [artist, "sets", set] if id(artist, 1, 100, b"-_") && id(set, 1, 100, b"-_") => Kind::Playlist,
                    _ => return None,
                };
                let page = format!("https://soundcloud.com/{}", path.join("/"));
                let embed =
                    Url::parse_with_params("https://w.soundcloud.com/player/", [("url", page.as_str()), ("auto_play", "false")]).ok()?;
                (kind, embed.into())
            }
        };
        Some(Self {
            provider,
            kind,
            embed: Url::parse(&embed).ok()?,
        })
    }

    pub fn height(&self) -> f32 {
        match (self.provider, self.kind) {
            (Provider::Spotify, Kind::Track) => 152.0,
            (Provider::Spotify, _) => 352.0,
            (Provider::AppleMusic, Kind::Track) => 175.0,
            (Provider::Suno | Provider::SoundCloud, Kind::Track) => 166.0,
            (Provider::YouTubeMusic, _) => 240.0,
            _ => 450.0,
        }
    }

    pub fn minimum_width(&self) -> f32 {
        match self.provider {
            // Spotify documents a 280-pixel minimum for audio embeds.
            Provider::Spotify => 280.0,
            _ => 200.0,
        }
    }

    pub fn has_metadata(&self) -> bool {
        self.provider == Provider::Spotify
    }

    pub async fn metadata(&self, client: &reqwest::Client) -> Result<Info, Failure> {
        let catalog = self.embed.path().strip_prefix("/embed").ok_or(Failure::Unavailable)?;
        let catalog = format!("https://open.spotify.com{catalog}");
        let endpoint = Url::parse_with_params("https://open.spotify.com/oembed", [("url", &catalog)]).map_err(|_| Failure::Unavailable)?;
        let (body, _) = super::download(client, endpoint.as_str(), "application/json", 32 * 1024).await?;
        let (title, artwork) = spotify_metadata(&body).ok_or(Failure::Unavailable)?;
        let cover = match artwork {
            Some(url) => super::fetch_picture_sized(client, url.as_str(), 96).await.ok(),
            None => None,
        };
        Ok(Info { title, cover })
    }

    pub fn player_page(&self, original: &str) -> String {
        super::player_server()
            .and_then(|port| Url::parse_with_params(&format!("http://127.0.0.1:{port}/music"), [("url", original)]).ok())
            .map_or_else(|| self.embed.to_string(), String::from)
    }
}

fn spotify_metadata(body: &[u8]) -> Option<(String, Option<Url>)> {
    let data: serde_json::Value = serde_json::from_slice(body).ok()?;
    let title: String = data["title"].as_str()?.trim().chars().take(200).collect();
    if title.is_empty() {
        return None;
    }
    let artwork = data["thumbnail_url"]
        .as_str()
        .and_then(|value| Url::parse(value).ok())
        .filter(|url| {
            url.scheme() == "https"
                && url.username().is_empty()
                && url.password().is_none()
                && url.port().is_none()
                && matches!(
                    url.host_str(),
                    Some(
                        "i.scdn.co"
                            | "mosaic.scdn.co"
                            | "image-cdn-ak.spotifycdn.com"
                            | "image-cdn-fa.spotifycdn.com"
                            | "daylist.spotifycdn.com"
                    )
                )
        });
    Some((title, artwork))
}

pub(super) fn player_answer(request: &str) -> Option<String> {
    let path = request.strip_prefix("GET ")?.split(' ').next()?;
    if !path.starts_with("/music?") {
        return None;
    }
    let request = Url::parse(&format!("http://127.0.0.1{path}")).ok()?;
    let (_, url) = request.query_pairs().find(|(name, _)| name == "url")?;
    let music = Music::of(&Url::parse(&url).ok()?)?;
    let src = super::attribute_text(music.embed.as_str());
    let origin = music.embed.origin().ascii_serialization();
    let name = music.provider.name();
    let iframe = format!(
        "<iframe src=\"{src}\" title=\"{name}\" allow=\"autoplay; clipboard-write; encrypted-media; fullscreen; picture-in-picture\" allowfullscreen></iframe>"
    );
    let (scripts, body) = if music.provider == Provider::Spotify {
        let catalog = music.embed.path().strip_prefix("/embed/")?;
        let (kind, id) = catalog.split_once('/')?;
        let nonce = uuid::Uuid::new_v4().simple();
        let api = "https://open.spotify.com/embed/iframe-api/v1";
        (
            format!("; script-src 'nonce-{nonce}' {api}"),
            format!(
                "<div id=\"spotify-player\" data-uri=\"spotify:{kind}:{id}\"></div>\
                 <template id=\"spotify-fallback\">{iframe}</template>\
                 <script nonce=\"{nonce}\">{}</script><script src=\"{api}\" async></script>",
                include_str!("spotify_player.js")
            ),
        )
    } else {
        (String::new(), iframe)
    };
    Some(format!(
        "<!doctype html><html><head><meta charset=\"utf-8\">\
         <meta name=\"referrer\" content=\"strict-origin-when-cross-origin\">\
         <meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; style-src 'unsafe-inline'; frame-src {origin}{scripts}\">\
         <style>html,body,iframe{{display:block;margin:0;border:0;width:100%;height:100%;background:#13141a}}</style>\
         </head><body>{body}</body></html>"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::link_preview::{Place, Sender, Settings, Start, Target, decide, target};

    #[test]
    fn spotify_page_requests_playback_and_keeps_a_manual_player_fallback() {
        let url = "https%3A%2F%2Fopen.spotify.com%2Ftrack%2F0F1G4ssr0RMdm9h0GEbt40";
        let page = player_answer(&format!("GET /music?url={url} HTTP/1.1")).unwrap();
        assert!(page.contains("data-uri=\"spotify:track:0F1G4ssr0RMdm9h0GEbt40\""));
        assert!(page.contains("controller.play()"));
        assert!(page.contains("<template id=\"spotify-fallback\"><iframe"));
        assert!(page.contains("script-src 'nonce-"));
        assert!(page.contains("https://open.spotify.com/embed/iframe-api/v1"));
    }

    #[test]
    fn spotify_metadata_accepts_catalog_artwork_only() {
        let (title, url) =
            spotify_metadata(br#"{"title":" Pop Cube ","thumbnail_url":"https://image-cdn-fa.spotifycdn.com/image/example"}"#).unwrap();
        assert_eq!(title, "Pop Cube");
        assert!(url.is_some());
        let (_, url) = spotify_metadata(br#"{"title":"Tune","thumbnail_url":"https://i.scdn.co.example.org/image/example"}"#).unwrap();
        assert!(url.is_none());
        assert!(spotify_metadata(br#"{"title":" "}"#).is_none());
    }

    #[test]
    fn shared_catalog_links_map_to_provider_players() {
        for (url, provider, kind, embed) in [
            (
                "https://open.spotify.com/intl-de/track/4yQw7FR9IcvL6RHtegbJBh?si=abc",
                Provider::Spotify,
                Kind::Track,
                "https://open.spotify.com/embed/track/4yQw7FR9IcvL6RHtegbJBh",
            ),
            (
                "https://open.spotify.com/album/2noRn2Aes5aoNVsU6iWThc",
                Provider::Spotify,
                Kind::Album,
                "https://open.spotify.com/embed/album/2noRn2Aes5aoNVsU6iWThc",
            ),
            (
                "https://open.spotify.com/playlist/37i9dQZF1DX4JAvHpjipBk",
                Provider::Spotify,
                Kind::Playlist,
                "https://open.spotify.com/embed/playlist/37i9dQZF1DX4JAvHpjipBk",
            ),
            (
                "https://music.apple.com/us/album/dreams/594061854?i=594061856&ls=1",
                Provider::AppleMusic,
                Kind::Track,
                "https://embed.music.apple.com/us/album/594061854?i=594061856",
            ),
            (
                "https://music.apple.com/jp/playlist/todays-hits/pl.f4d106fed2bd41149aaacabb233eb5eb",
                Provider::AppleMusic,
                Kind::Playlist,
                "https://embed.music.apple.com/jp/playlist/pl.f4d106fed2bd41149aaacabb233eb5eb",
            ),
            (
                "https://music.youtube.com/watch?v=jNQXAC9IVRw&list=PLFgquLnL59alCl_2TQvOiD5Vgm1hCaGSI&t=1m2s",
                Provider::YouTubeMusic,
                Kind::Playlist,
                "https://www.youtube-nocookie.com/embed/jNQXAC9IVRw?list=PLFgquLnL59alCl_2TQvOiD5Vgm1hCaGSI&start=62",
            ),
            (
                "https://music.youtube.com/playlist?list=OLAK5uy_test_album_id",
                Provider::YouTubeMusic,
                Kind::Album,
                "https://www.youtube-nocookie.com/embed/videoseries?list=OLAK5uy_test_album_id",
            ),
            (
                "https://suno.com/song/015e1b03-5f5e-428c-9a5b-4da71ae8338f",
                Provider::Suno,
                Kind::Track,
                "https://suno.com/embed/015e1b03-5f5e-428c-9a5b-4da71ae8338f",
            ),
            (
                "https://m.soundcloud.com/djxerothehero/sets/monstercat",
                Provider::SoundCloud,
                Kind::Playlist,
                "https://w.soundcloud.com/player/?url=https%3A%2F%2Fsoundcloud.com%2Fdjxerothehero%2Fsets%2Fmonstercat&auto_play=false",
            ),
        ] {
            let Some(Target::Music(music)) = target(url) else {
                panic!("no music target for {url}")
            };
            assert_eq!((music.provider, music.kind, music.embed.as_str()), (provider, kind, embed));
        }
        assert!(matches!(
            target("https://music.youtube.com/watch?v=jNQXAC9IVRw"),
            Some(Target::Video(_))
        ));
    }

    #[test]
    fn music_wrapper_only_frames_its_recognized_provider() {
        let line = |url| {
            let uri = Url::parse_with_params("http://127.0.0.1/music", [("url", url)]).unwrap();
            format!("GET /music?{} HTTP/1.1", uri.query().unwrap())
        };
        for url in [
            "http://open.spotify.com/track/4yQw7FR9IcvL6RHtegbJBh",
            "https://open.spotify.com.bad.example/track/4yQw7FR9IcvL6RHtegbJBh",
            "https://user@open.spotify.com/track/4yQw7FR9IcvL6RHtegbJBh",
            "https://open.spotify.com:444/track/4yQw7FR9IcvL6RHtegbJBh",
            "https://open.spotify.com/track/<script>",
            "https://music.youtube.com/playlist?list=abc%22onload%3Dtest",
            "https://music.apple.com/us/playlist/https%3A%2F%2Fexample.org",
            "https://suno.com/playlist/015e1b03-5f5e-428c-9a5b-4da71ae8338f",
            "https://w.soundcloud.com/player/?url=https://example.org",
        ] {
            assert!(player_answer(&line(url)).is_none(), "{url}");
        }
        let page = player_answer(&line("https://music.apple.com/us/album/123?i=456&foo=%22onload")).unwrap();
        assert!(page.contains("frame-src https://embed.music.apple.com"));
        assert!(page.contains("src=\"https://embed.music.apple.com/us/album/123?i=456\""));
        assert!(!page.contains("onload"));
    }

    #[test]
    fn music_cards_obey_conversation_and_hidden_site_settings() {
        let url = "https://open.spotify.com/track/4yQw7FR9IcvL6RHtegbJBh";
        let mut settings = Settings {
            ims: true,
            nearby: false,
            ..Settings::default()
        };
        assert!(matches!(
            decide(&settings, Place::Im, Sender::Object, url),
            Some((Target::Music(_), Start::Ask))
        ));
        assert!(decide(&settings, Place::Nearby, Sender::Resident, url).is_none());
        settings.hidden_sites = vec!["spotify.com".into()];
        assert!(decide(&settings, Place::Im, Sender::Resident, url).is_none());
    }
}
