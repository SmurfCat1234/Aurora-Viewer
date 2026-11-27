//! Hosted video cards use a provider's own player and public oEmbed metadata.
//! Build both addresses from the video ID, never from returned HTML.

use reqwest::Url;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(super) enum Hosted {
    Streamable(String),
    Vimeo { id: String, hash: Option<String> },
    TikTok { account: String, id: String },
    Bilibili { id: String, part: u32 },
    Framatube(String),
    InstagramReel(String),
    FacebookReel(String),
    TwitchClip(String),
    TwitchVideo(String),
    TwitchLive(String),
    KickLive(String),
    Dailymotion { player: String, video: String },
    Arte { language: String, id: String },
    AdultEmbed { domain: &'static str, id: String },
    Youku { id: String, client: String },
    Regional { page: String, player: String },
}

fn id_ok(id: &str, max: usize) -> bool {
    !id.is_empty() && id.len() <= max && id.bytes().all(|b| b.is_ascii_alphanumeric())
}

fn digits(id: &str, min: usize, max: usize) -> bool {
    (min..=max).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_digit())
}

fn tiktok_account(account: &str) -> bool {
    !account.is_empty() && account.len() <= 24 && account.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.'))
}

fn bilibili_id(id: &str) -> bool {
    (id.starts_with("BV") && id.len() == 12 && id_ok(id, 12)) || id.strip_prefix("av").is_some_and(|aid| digits(aid, 1, 20))
}

fn instagram_id(id: &str) -> bool {
    (5..=20).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
}

fn peertube_id(id: &str) -> bool {
    (id.len() == 22 && id_ok(id, 22)) || (id.len() == 36 && uuid::Uuid::parse_str(id).is_ok())
}

fn twitch_clip_id(id: &str) -> bool {
    (8..=100).contains(&id.len())
        && id.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
        && !matches!(id, "dashboard" | "directory")
}

fn live_channel(name: &str) -> bool {
    (2..=25).contains(&name.len())
        && name.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
        && !matches!(
            name.to_ascii_lowercase().as_str(),
            "directory"
                | "downloads"
                | "jobs"
                | "p"
                | "settings"
                | "subscriptions"
                | "videos"
                | "wallet"
                | "browse"
                | "categories"
                | "following"
                | "search"
                | "about"
                | "terms"
                | "privacy"
                | "turbo"
                | "login"
                | "signup"
                | "dashboard"
                | "friends"
                | "inventory"
                | "clips"
                | "store"
                | "drops"
                | "partners"
                | "creatorcamp"
                | "community"
                | "support"
                | "explore"
                | "prime"
        )
}

pub(super) fn arte_id(id: &str) -> bool {
    let Some((number, variant)) = id.rsplit_once('-') else {
        return false;
    };
    variant.len() == 1
        && variant.bytes().all(|b| b.is_ascii_uppercase())
        && number
            .split_once('-')
            .is_some_and(|(program, part)| digits(program, 4, 9) && digits(part, 3, 3))
}

fn dailymotion_id(id: &str) -> bool {
    id.starts_with('x') && id.len() > 1 && id_ok(id, 24)
}

impl Hosted {
    pub(super) fn requires_player_click(&self) -> bool {
        matches!(
            self,
            Self::InstagramReel(_)
                | Self::FacebookReel(_)
                | Self::Dailymotion { .. }
                | Self::Arte { .. }
                | Self::AdultEmbed { .. }
                | Self::Youku { .. }
                | Self::Regional { .. }
        )
    }
    pub(super) fn regional(&self) -> bool {
        matches!(self, Self::Regional { .. })
    }

    pub(super) fn adult(&self) -> bool {
        matches!(self, Self::AdultEmbed { .. })
    }

    pub(super) fn minimum_width(&self) -> f32 {
        match self {
            // Twitch requires at least 400 × 300 CSS pixels.
            Self::TwitchClip(_) | Self::TwitchVideo(_) | Self::TwitchLive(_) => 534.0,
            _ => 0.0,
        }
    }
    pub(super) fn aspect(&self) -> Option<f32> {
        match self {
            Self::TikTok { .. } | Self::InstagramReel(_) | Self::FacebookReel(_) => Some(9.0 / 16.0),
            _ => None,
        }
    }

    pub(super) fn of(url: &Url) -> Option<Self> {
        let parts: Vec<_> = url.path().trim_end_matches('/').strip_prefix('/')?.split('/').collect();
        let host = url.host_str()?;
        if let Some((page, player)) = regional_player(url, host, &parts) {
            return Some(Self::Regional { page, player });
        }
        match (host, parts.as_slice()) {
            ("www.redgifs.com" | "redgifs.com", ["ifr", id]) if id_ok(id, 80) => {
                return Some(Self::AdultEmbed {
                    domain: "redgifs.com",
                    id: (*id).to_owned(),
                });
            }
            ("www.pornhub.com" | "pornhub.com", ["embed", id]) if id_ok(id, 48) && id.len() >= 5 => {
                return Some(Self::AdultEmbed {
                    domain: "pornhub.com",
                    id: (*id).to_owned(),
                });
            }
            ("www.xvideos.com" | "xvideos.com", ["embedframe", id]) if digits(id, 1, 24) => {
                return Some(Self::AdultEmbed {
                    domain: "xvideos.com",
                    id: (*id).to_owned(),
                });
            }
            ("player.youku.com", ["embed", id]) => {
                let id = id.replace("%3D", "=").replace("%3d", "=");
                let core = id.strip_prefix('X')?.trim_end_matches('=');
                if !id_ok(core, 64) || id.len() - core.len() > 3 {
                    return None;
                }
                let client = url.query_pairs().find(|(key, _)| key == "client_id")?.1;
                if !id_ok(&client, 80) {
                    return None;
                }
                return Some(Self::Youku {
                    id,
                    client: client.into_owned(),
                });
            }
            ("geo.dailymotion.com", ["player", file]) => {
                let player = file.strip_suffix(".html").filter(|id| dailymotion_id(id))?;
                let video = url.query_pairs().find(|(key, _)| key == "video")?.1;
                return dailymotion_id(&video).then(|| Self::Dailymotion {
                    player: player.to_owned(),
                    video: video.into_owned(),
                });
            }
            ("www.arte.tv" | "arte.tv", ["embeds", language, id])
                if matches!(*language, "en" | "fr" | "de" | "es" | "pl" | "it" | "ro") && arte_id(id) =>
            {
                return Some(Self::Arte {
                    language: (*language).to_owned(),
                    id: (*id).to_owned(),
                });
            }
            ("instagram.com" | "www.instagram.com", ["reel", id]) if instagram_id(id) => {
                return Some(Self::InstagramReel((*id).to_owned()));
            }
            ("facebook.com" | "www.facebook.com" | "m.facebook.com", ["reel", id]) if digits(id, 8, 20) => {
                return Some(Self::FacebookReel((*id).to_owned()));
            }
            ("framatube.org", ["w", id] | ["videos", "watch" | "embed", id]) if peertube_id(id) => {
                return Some(Self::Framatube((*id).to_owned()));
            }
            ("clips.twitch.tv", [id]) | ("www.twitch.tv" | "twitch.tv" | "m.twitch.tv", [_, "clip", id]) if twitch_clip_id(id) => {
                return Some(Self::TwitchClip((*id).to_owned()));
            }
            ("www.twitch.tv" | "twitch.tv" | "m.twitch.tv", ["videos", id]) if digits(id, 1, 20) => {
                return Some(Self::TwitchVideo((*id).to_owned()));
            }
            ("www.twitch.tv" | "twitch.tv" | "m.twitch.tv", [channel]) if live_channel(channel) && !channel.contains('-') => {
                return Some(Self::TwitchLive(channel.to_ascii_lowercase()));
            }
            ("kick.com" | "www.kick.com", [channel]) if live_channel(channel) => {
                return Some(Self::KickLive(channel.to_ascii_lowercase()));
            }
            ("tiktok.com" | "www.tiktok.com" | "m.tiktok.com", [account, "video", id])
                if account.strip_prefix('@').is_some_and(tiktok_account) && digits(id, 15, 20) =>
            {
                return Some(Self::TikTok {
                    account: (*account).to_owned(),
                    id: (*id).to_owned(),
                });
            }
            ("www.bilibili.com" | "bilibili.com" | "m.bilibili.com", ["video", id]) if bilibili_id(id) => {
                let part = url
                    .query_pairs()
                    .find(|(key, _)| key == "p")
                    .and_then(|(_, value)| value.parse::<u32>().ok())
                    .filter(|&part| part > 0)
                    .unwrap_or(1);
                return Some(Self::Bilibili {
                    id: (*id).to_owned(),
                    part,
                });
            }
            _ => {}
        }
        let (id, path_hash) = match (host, parts.as_slice()) {
            ("streamable.com" | "www.streamable.com", [id] | ["e", id] | ["o", id]) if id_ok(id, 16) => {
                // A video shortcode is at least three characters. Reject site navigation.
                if id.len() < 3
                    || matches!(
                        *id,
                        "login"
                            | "signup"
                            | "upload"
                            | "pricing"
                            | "blog"
                            | "about"
                            | "terms"
                            | "privacy"
                            | "dashboard"
                            | "videos"
                            | "features"
                            | "help"
                            | "tools"
                            | "contact"
                            | "register"
                    )
                {
                    return None;
                }
                return Some(Self::Streamable((*id).to_owned()));
            }
            ("vimeo.com" | "www.vimeo.com", [id] | ["channels", _, id] | ["groups", _, "videos", id])
            | ("player.vimeo.com", ["video", id]) => (*id, None),
            ("vimeo.com" | "www.vimeo.com", [id, hash]) => (*id, Some(*hash)),
            _ => return None,
        };
        if !id_ok(id, 12) || !id.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        let hash = path_hash
            .map(str::to_owned)
            .or_else(|| url.query_pairs().find(|(k, _)| k == "h").map(|(_, v)| v.into_owned()));
        if hash.as_deref().is_some_and(|h| !id_ok(h, 32)) {
            return None;
        }
        Some(Self::Vimeo { id: id.to_owned(), hash })
    }

    pub(super) fn page(&self) -> String {
        match self {
            Self::Dailymotion { .. } | Self::Arte { .. } | Self::AdultEmbed { .. } => self.player(),
            Self::Regional { page, .. } => page.clone(),
            Self::Youku { id, .. } => format!("https://v.youku.com/v_show/id_{id}.html"),
            Self::InstagramReel(id) => format!("https://www.instagram.com/reel/{id}/"),
            Self::FacebookReel(id) => format!("https://www.facebook.com/reel/{id}/"),
            Self::Framatube(id) => format!("https://framatube.org/w/{id}"),
            Self::TwitchClip(id) => format!("https://clips.twitch.tv/{id}"),
            Self::TwitchVideo(id) => format!("https://www.twitch.tv/videos/{id}"),
            Self::TwitchLive(channel) => format!("https://www.twitch.tv/{channel}"),
            Self::KickLive(channel) => format!("https://kick.com/{channel}"),
            Self::TikTok { account, id } => format!("https://www.tiktok.com/{account}/video/{id}"),
            Self::Bilibili { id, part } => format!("https://www.bilibili.com/video/{id}?p={part}"),
            Self::Streamable(id) => format!("https://streamable.com/{id}"),
            Self::Vimeo { id, hash: Some(hash) } => format!("https://vimeo.com/{id}/{hash}"),
            Self::Vimeo { id, hash: None } => format!("https://vimeo.com/{id}"),
        }
    }

    pub(super) fn player(&self) -> String {
        match self {
            Self::Dailymotion { player, video } => format!("https://geo.dailymotion.com/player/{player}.html?video={video}"),
            Self::Arte { language, id } => format!("https://www.arte.tv/embeds/{language}/{id}"),
            Self::AdultEmbed { domain, id } => {
                let path = match *domain {
                    "redgifs.com" => "ifr",
                    "pornhub.com" => "embed",
                    _ => "embedframe",
                };
                format!("https://www.{domain}/{path}/{id}")
            }
            Self::Regional { player, .. } => player.clone(),
            Self::Youku { id, client } => format!("https://player.youku.com/embed/{id}?client_id={client}"),
            Self::InstagramReel(id) => format!("https://www.instagram.com/reel/{id}/embed/"),
            Self::FacebookReel(id) => {
                format!("https://www.facebook.com/plugins/video.php?href=https%3A%2F%2Fwww.facebook.com%2Freel%2F{id}%2F&autoplay=false")
            }
            Self::Framatube(id) => format!("https://framatube.org/videos/embed/{id}?autoplay=1&muted=0&p2p=0"),
            Self::TwitchClip(id) => format!("https://clips.twitch.tv/embed?clip={id}&parent=127.0.0.1&autoplay=true&muted=false"),
            Self::TwitchVideo(id) => format!("https://player.twitch.tv/?video=v{id}&parent=127.0.0.1&autoplay=true&muted=false"),
            Self::TwitchLive(channel) => format!("https://player.twitch.tv/?channel={channel}&parent=127.0.0.1&autoplay=true&muted=false"),
            Self::KickLive(channel) => format!("https://player.kick.com/{channel}?autoplay=true&muted=false"),
            Self::TikTok { id, .. } => format!("https://www.tiktok.com/player/v1/{id}?autoplay=1&force_muted=0&rel=0"),
            Self::Bilibili { id, part } => {
                let id = id
                    .strip_prefix("av")
                    .map_or_else(|| format!("bvid={id}"), |aid| format!("aid={aid}"));
                format!("https://player.bilibili.com/player.html?{id}&p={part}&autoplay=1&muted=0")
            }
            Self::Streamable(id) => format!("https://streamable.com/e/{id}?autoplay=1"),
            Self::Vimeo { id, hash } => {
                let hash = hash.as_ref().map(|h| format!("h={h}&")).unwrap_or_default();
                format!("https://player.vimeo.com/video/{id}?{hash}autoplay=1&muted=0")
            }
        }
    }

    pub(super) fn oembed(&self) -> Option<String> {
        let endpoint = match self {
            Self::Dailymotion { player, video } => {
                return Url::parse_with_params(
                    "https://www.dailymotion.com/services/oembed",
                    [
                        ("url", format!("https://www.dailymotion.com/video/{video}")),
                        ("player", player.clone()),
                    ],
                )
                .ok()
                .map(String::from);
            }
            Self::Streamable(_) => "https://api.streamable.com/oembed.json",
            Self::Vimeo { .. } => "https://vimeo.com/api/oembed.json",
            Self::TikTok { .. } => "https://www.tiktok.com/oembed",
            Self::Framatube(_) => "https://framatube.org/services/oembed",
            Self::Bilibili { .. }
            | Self::TwitchClip(_)
            | Self::TwitchVideo(_)
            | Self::TwitchLive(_)
            | Self::KickLive(_)
            | Self::InstagramReel(_)
            | Self::FacebookReel(_)
            | Self::Arte { .. }
            | Self::AdultEmbed { .. }
            | Self::Regional { .. }
            | Self::Youku { .. } => {
                return None;
            }
        };
        Url::parse_with_params(endpoint, [("url", self.page())]).ok().map(String::from)
    }

    pub(super) fn poster(&self, value: &str) -> Option<String> {
        // Streamable also returns protocol-relative thumbnails.
        let url = if value.starts_with("//") {
            format!("https:{value}")
        } else {
            value.to_owned()
        };
        let parsed = Url::parse(&url).ok()?;
        if parsed.scheme() != "https" || !parsed.username().is_empty() || parsed.password().is_some() || parsed.port().is_some() {
            return None;
        }
        let host = parsed.host_str()?;
        let permitted = match self {
            Self::Dailymotion { .. } => super::in_domain(host, "dmcdn.net"),
            Self::Streamable(_) => matches!(
                host,
                "images.streamable.com" | "cdn-cf-east.streamable.com" | "cdn-cf-west.streamable.com"
            ),
            Self::Vimeo { .. } => host == "i.vimeocdn.com",
            Self::Framatube(_) => host == "framatube.org" && parsed.path().starts_with("/static/"),
            Self::TikTok { .. } => host.ends_with(".tiktokcdn.com") || host.ends_with(".tiktokcdn-us.com"),
            Self::Bilibili { .. }
            | Self::TwitchClip(_)
            | Self::TwitchVideo(_)
            | Self::TwitchLive(_)
            | Self::KickLive(_)
            | Self::InstagramReel(_)
            | Self::FacebookReel(_)
            | Self::Arte { .. }
            | Self::AdultEmbed { .. } => false,
            Self::Regional { page, .. } => super::services::publisher_image_url(page, &url).is_some(),
            Self::Youku { .. } => super::in_domain(host, "ykimg.com"),
        };
        permitted.then_some(url)
    }
}

fn regional_player(url: &Url, host: &str, parts: &[&str]) -> Option<(String, String)> {
    match (host, parts) {
        ("rutube.ru" | "www.rutube.ru", ["video", id] | ["play", "embed", id])
            if id.len() == 32 && id.bytes().all(|b| b.is_ascii_hexdigit()) =>
        {
            Some((
                format!("https://rutube.ru/video/{id}/"),
                format!("https://rutube.ru/play/embed/{id}/"),
            ))
        }
        ("ok.ru" | "www.ok.ru" | "m.ok.ru", ["video" | "videoembed", id]) if digits(id, 1, 24) => {
            Some((format!("https://ok.ru/video/{id}"), format!("https://ok.ru/videoembed/{id}")))
        }
        ("dzen.ru" | "www.dzen.ru", ["embed", id]) if id_ok(id, 80) => {
            let url = format!("https://dzen.ru/embed/{id}");
            Some((url.clone(), url))
        }
        ("vk.com" | "www.vk.com" | "vkvideo.ru" | "www.vkvideo.ru", ["video_ext.php"]) => {
            let param = |name| url.query_pairs().find(|(key, _)| key == name).map(|(_, value)| value);
            let oid = param("oid")?;
            let id = param("id")?;
            let hash = param("hash");
            vk_player(host, &oid, &id, hash.as_deref())
        }
        ("vk.com" | "www.vk.com" | "m.vk.com" | "vkvideo.ru" | "www.vkvideo.ru", [video]) => {
            let (owner, id) = video.strip_prefix("video")?.split_once('_')?;
            let player_host = if host.ends_with("vkvideo.ru") { "vkvideo.ru" } else { "vk.com" };
            vk_player(player_host, owner, id, None)
        }
        _ => None,
    }
}

fn vk_player(host: &str, owner: &str, id: &str, hash: Option<&str>) -> Option<(String, String)> {
    if !digits(owner.strip_prefix('-').unwrap_or(owner), 1, 20) || !digits(id, 1, 20) || hash.is_some_and(|hash| !id_ok(hash, 100)) {
        return None;
    }
    let hash = hash.map_or_else(String::new, |hash| format!("&hash={hash}"));
    Some((
        format!("https://vkvideo.ru/video{owner}_{id}"),
        format!("https://{host}/video_ext.php?oid={owner}&id={id}{hash}"),
    ))
}

#[test]
fn public_regional_players_rebuild_only_supported_urls() {
    let parse = |url| Hosted::of(&Url::parse(url).unwrap());
    let vk = parse("https://vkvideo.ru/video_ext.php?oid=-123&id=456&hash=abc123&tracking=example").unwrap();
    assert_eq!(vk.page(), "https://vkvideo.ru/video-123_456");
    assert_eq!(vk.player(), "https://vkvideo.ru/video_ext.php?oid=-123&id=456&hash=abc123");
    assert!(vk.requires_player_click());
    assert_eq!(
        parse("https://vkvideo.ru/video-84793390_456239888").unwrap().player(),
        "https://vkvideo.ru/video_ext.php?oid=-84793390&id=456239888"
    );
    assert!(parse("https://vkvideo.ru/video_ext.php?oid=-123&id=456&hash=bad%26hash").is_none());
    assert!(parse("https://vkvideo.ru.example/video_ext.php?oid=-123&id=456&hash=abc123").is_none());
    assert_eq!(
        parse("https://rutube.ru/video/6840af8411a8be4e7da9f82cb4a25103/").unwrap().player(),
        "https://rutube.ru/play/embed/6840af8411a8be4e7da9f82cb4a25103/"
    );
    assert_eq!(
        parse("https://m.ok.ru/video/8069521017563").unwrap().player(),
        "https://ok.ru/videoembed/8069521017563"
    );
    assert!(parse("https://dzen.ru/embed/example123").is_some());
}
