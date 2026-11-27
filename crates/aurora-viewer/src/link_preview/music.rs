//! Provider-hosted players for music links shared in conversations.

use reqwest::Url;

use super::{Failure, Picture};

pub struct Info {
    pub title: String,
    pub detail: Option<String>,
    pub cover: Option<Picture>,
}

#[derive(Default)]
struct Metadata {
    title: Option<String>,
    detail: Option<String>,
    artwork: Option<Url>,
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

    pub fn app_uri(&self) -> Option<String> {
        if self.provider != Provider::Spotify {
            return None;
        }
        let (kind, id) = self.embed.path().strip_prefix("/embed/")?.split_once('/')?;
        Some(format!("spotify:{kind}:{id}"))
    }

    fn public_page(&self) -> Option<Url> {
        let path = self.embed.path();
        match self.provider {
            Provider::Spotify => Url::parse(&format!("https://open.spotify.com{}", path.strip_prefix("/embed")?)).ok(),
            Provider::AppleMusic => {
                let mut url = self.embed.clone();
                url.set_host(Some("music.apple.com")).ok()?;
                Some(url)
            }
            Provider::YouTubeMusic => {
                let (_, list) = self.embed.query_pairs().find(|(key, _)| key == "list")?;
                Url::parse_with_params("https://music.youtube.com/playlist", [("list", list)]).ok()
            }
            Provider::Suno => Url::parse(&format!("https://suno.com/song/{}", path.strip_prefix("/embed/")?)).ok(),
            Provider::SoundCloud => {
                let (_, page) = self.embed.query_pairs().find(|(key, _)| key == "url")?;
                Url::parse(&page).ok()
            }
        }
    }

    pub async fn metadata(&self, client: &reqwest::Client) -> Result<Info, Failure> {
        let page = self.public_page().ok_or(Failure::Unavailable)?;
        let head = async {
            super::text_head(client, page.as_str(), "text/html", "</head>")
                .await
                .0
                .ok()
                .map(|html| page_metadata(self.provider, &html))
                .unwrap_or_default()
        };
        let oembed = async |url: &str| {
            super::download(client, url, "application/json", 32 * 1024)
                .await
                .ok()
                .and_then(|(body, _)| oembed_metadata(self.provider, &body))
                .unwrap_or_default()
        };
        let data = match self.provider {
            Provider::Spotify => {
                let endpoint = Url::parse_with_params("https://open.spotify.com/oembed", [("url", page.as_str())])
                    .map_err(|_| Failure::Unavailable)?;
                let (embed, html) = tokio::join!(oembed(endpoint.as_str()), head);
                Metadata {
                    title: embed.title.or(html.title),
                    detail: html.detail.or(embed.detail),
                    artwork: embed.artwork.or(html.artwork),
                }
            }
            Provider::SoundCloud => {
                let endpoint = Url::parse_with_params("https://soundcloud.com/oembed", [("format", "json"), ("url", page.as_str())])
                    .map_err(|_| Failure::Unavailable)?;
                oembed(endpoint.as_str()).await
            }
            _ => head.await,
        };
        let title = data.title.ok_or(Failure::Unavailable)?;
        let cover = match data.artwork {
            Some(url) => super::fetch_picture_sized(client, url.as_str(), 96).await.ok(),
            None => None,
        };
        Ok(Info {
            title,
            detail: data.detail,
            cover,
        })
    }

    pub fn player_page(&self, original: &str) -> String {
        super::player_server()
            .and_then(|port| Url::parse_with_params(&format!("http://127.0.0.1:{port}/music"), [("url", original)]).ok())
            .map_or_else(|| self.embed.to_string(), String::from)
    }
}

fn display_text(value: &str) -> Option<String> {
    let text: String = value.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(200).collect();
    (!text.is_empty()).then_some(text)
}

fn artwork_url(provider: Provider, value: &str) -> Option<Url> {
    let url = Url::parse(value).ok()?;
    if url.scheme() != "https" || !url.username().is_empty() || url.password().is_some() || url.port().is_some() {
        return None;
    }
    let host = url.host_str()?;
    let known = match provider {
        Provider::Spotify => matches!(
            host,
            "i.scdn.co" | "mosaic.scdn.co" | "image-cdn-ak.spotifycdn.com" | "image-cdn-fa.spotifycdn.com" | "daylist.spotifycdn.com"
        ),
        Provider::AppleMusic => matches!(
            host,
            "is1-ssl.mzstatic.com" | "is2-ssl.mzstatic.com" | "is3-ssl.mzstatic.com" | "is4-ssl.mzstatic.com" | "is5-ssl.mzstatic.com"
        ),
        Provider::YouTubeMusic => matches!(host, "i.ytimg.com" | "yt3.googleusercontent.com" | "yt3.ggpht.com"),
        Provider::Suno => matches!(host, "cdn.suno.ai" | "cdn1.suno.ai" | "cdn2.suno.ai"),
        Provider::SoundCloud => matches!(host, "i1.sndcdn.com" | "i2.sndcdn.com"),
    };
    known.then_some(url)
}

fn oembed_metadata(provider: Provider, body: &[u8]) -> Option<Metadata> {
    let data: serde_json::Value = serde_json::from_slice(body).ok()?;
    let title = data["title"].as_str()?;
    let detail = data["author_name"].as_str().and_then(display_text);
    let suffix = detail.as_ref().map(|author| format!(" by {author}"));
    let title = suffix.as_ref().and_then(|suffix| title.strip_suffix(suffix)).unwrap_or(title);
    Some(Metadata {
        title: Some(display_text(title)?),
        detail,
        artwork: data["thumbnail_url"].as_str().and_then(|value| artwork_url(provider, value)),
    })
}

fn page_metadata(provider: Provider, html: &str) -> Metadata {
    let mut tags = std::collections::HashMap::new();
    for (key, value) in super::services::meta_tags(html) {
        tags.entry(key).or_insert(value);
    }
    let field = |key| tags.get(key).and_then(|value| display_text(value));
    let mut title = field("og:title");
    let mut detail = field("og:description");
    match provider {
        Provider::AppleMusic => {
            title = field("apple:title").or(title);
            if let Some(artist) = apple_artist(html) {
                detail = Some(detail.map_or_else(|| artist.clone(), |description| format!("{artist} · {description}")));
            }
        }
        Provider::Suno => detail = field("music:musician_description"),
        _ => {}
    }
    Metadata {
        title,
        detail,
        artwork: tags.get("og:image").and_then(|value| artwork_url(provider, value)),
    }
}

fn apple_artist(html: &str) -> Option<String> {
    for script in html.split("<script").skip(1) {
        let (attributes, body) = script.split_once('>')?;
        if super::services::attribute(attributes, "type") != Some("application/ld+json") {
            continue;
        }
        let Some((json, _)) = body.split_once("</script>") else { continue };
        let Ok(data) = serde_json::from_str::<serde_json::Value>(json) else {
            continue;
        };
        let artists = data.get("byArtist").or_else(|| data["audio"].get("byArtist"));
        let name = artists.and_then(|by| {
            let first = by.as_array().and_then(|list| list.first()).unwrap_or(by);
            first["name"].as_str().and_then(display_text)
        });
        if name.is_some() {
            return name;
        }
    }
    None
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
        let uri = music.app_uri()?;
        let nonce = uuid::Uuid::new_v4().simple();
        let api = "https://open.spotify.com/embed/iframe-api/v1";
        // Spotify's public loader fetches an eval-based bundle from this directory.
        // Keep that exception on this Spotify-only player document.
        let api_bundle = "https://embed-cdn.spotifycdn.com/_next/static/";
        (
            format!("; script-src 'nonce-{nonce}' 'unsafe-eval' {api} {api_bundle}"),
            format!(
                "<div id=\"spotify-player\" data-uri=\"{uri}\"></div>\
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
        assert!(page.contains("'unsafe-eval'"));
        assert!(page.contains("https://open.spotify.com/embed/iframe-api/v1"));
        assert!(page.contains("https://embed-cdn.spotifycdn.com/_next/static/"));
        let apple = player_answer("GET /music?url=https%3A%2F%2Fmusic.apple.com%2Fus%2Falbum%2Fdreams%2F594061854 HTTP/1.1").unwrap();
        assert!(!apple.contains("script-src"));
    }

    #[test]
    fn spotify_metadata_accepts_catalog_artwork_only() {
        let provider = Provider::Spotify;
        let data = oembed_metadata(
            provider,
            br#"{"title":" Pop Cube ","thumbnail_url":"https://image-cdn-fa.spotifycdn.com/image/example"}"#,
        )
        .unwrap();
        assert_eq!(data.title.as_deref(), Some("Pop Cube"));
        assert!(data.artwork.is_some());
        let data = oembed_metadata(
            provider,
            br#"{"title":"Tune","thumbnail_url":"https://i.scdn.co.example.org/image/example"}"#,
        )
        .unwrap();
        assert!(data.artwork.is_none());
        assert!(oembed_metadata(provider, br#"{"title":" "}"#).is_none());
    }

    #[test]
    fn public_metadata_supplies_artist_and_catalog_details() {
        let spotify = page_metadata(
            Provider::Spotify,
            r#"
            <meta property="og:title" content="DAN DAN">
            <meta property="og:description" content="ZARD · TODAY IS ANOTHER DAY · Song · 1996">
        "#,
        );
        assert_eq!(spotify.detail.as_deref(), Some("ZARD · TODAY IS ANOTHER DAY · Song · 1996"));
        let apple = page_metadata(
            Provider::AppleMusic,
            r#"
            <meta name="apple:title" content="Dreams">
            <meta property="og:description" content="Song · 1977 · Duration 4:17">
            <script id=schema:song type="application/ld+json">{"audio":{"byArtist":[{"name":"Fleetwood Mac"}]}}</script>
        "#,
        );
        assert_eq!(apple.title.as_deref(), Some("Dreams"));
        assert_eq!(apple.detail.as_deref(), Some("Fleetwood Mac · Song · 1977 · Duration 4:17"));
        let soundcloud = oembed_metadata(
            Provider::SoundCloud,
            br#"{"title":"Poland by Erased Tapes","author_name":"Erased Tapes"}"#,
        )
        .unwrap();
        assert_eq!(soundcloud.title.as_deref(), Some("Poland"));
        assert_eq!(soundcloud.detail.as_deref(), Some("Erased Tapes"));
        let suno = page_metadata(
            Provider::Suno,
            r#"<meta property="og:title" content="Don&#x27;t"><meta property="og:description" content="Listen on Suno">"#,
        );
        assert_eq!(suno.title.as_deref(), Some("Don't"));
        assert!(suno.detail.is_none());
    }

    #[test]
    fn only_validated_spotify_catalog_links_produce_app_uris() {
        let album = Url::parse("https://open.spotify.com/intl-de/album/3mlVZ7OTZgKJJAr275tHZX?si=ignored").unwrap();
        assert_eq!(
            Music::of(&album).unwrap().app_uri().as_deref(),
            Some("spotify:album:3mlVZ7OTZgKJJAr275tHZX")
        );
        for url in [
            "https://open.spotify.com.example.org/album/3mlVZ7OTZgKJJAr275tHZX",
            "spotify:album:3mlVZ7OTZgKJJAr275tHZX",
        ] {
            assert!(Music::of(&Url::parse(url).unwrap()).is_none());
        }
        let apple = Url::parse("https://music.apple.com/us/song/dreams/594061856").unwrap();
        assert!(Music::of(&apple).unwrap().app_uri().is_none());
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
