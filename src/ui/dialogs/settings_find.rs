//! Settings → "Find a setting" (November 17, 2026 reorganisation, design
//! `docs/superpowers/specs/2026-11-17-settings-design.md`): every setting's row with the words on it, its help and
//! the other words people use for it ("phone mic", "chords", "NDI", "key"), the pages and their categories, and the
//! search over them. The rows are drawn by the page files (`ui/dialogs/settings-*.slint`, each a `SetRow` with the
//! same key, label and help); `settings_tests` checks that each row here is on its page, and no row is on two pages
//! or none.
use super::SettingsTab::{self, *};
use crate::tr;

/// One setting's row: its key (the `SetRow`'s), its page, whether it shows only with Show advanced settings, and the
/// words on it.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub key: &'static str,
    pub page: SettingsTab,
    pub advanced: bool,
    pub label: String,
    pub help: String,
}

fn row(key: &'static str, page: SettingsTab, advanced: bool, label: impl Into<String>, help: impl Into<String>) -> Row {
    Row { key, page, advanced, label: label.into(), help: help.into() }
}

/// Every setting's row, page by page in the sidebar's order.
pub fn rows() -> Vec<Row> {
    vec![
        row("language", Language, false, tr!("Language"), ""),
        row("room-on-top", Video, false, tr!("Audience screen in front"), tr!("Keeps the audience screen above other windows when it has its own monitor, so fades stay smooth.")),
        row("screens-at-start", Video, false, tr!("At start"), ""),
        row("graphics", Video, false, tr!("Graphics"), tr!("Change this only if the screens flicker, stay black or RustyPresenter crashes. Used from the next start.")),
        row("light", Video, false, tr!("Light mode"), tr!("For a slow computer: slides change without fading, and the live preview's video is refreshed less often. Automatic turns it on when the audience screen cannot keep up.")),
        row("performance", Video, true, tr!("Performance"), tr!("Shows under the live preview how many frames a second the screens draw, and the free video memory when the graphics card says. Low numbers mean the computer is struggling.")),
        row("lines", Songs, false, tr!("Lines per slide"), ""),
        row("credits", Songs, false, tr!("Credits"), tr!("Songs and Bible passages. Some Bibles require their credit to be shown; it appears once per passage.")),
        row("credit-template", Songs, true, tr!("Copyright line"), tr!("Use {{title}}, {{artist}}, {{author}}, {{publisher}}, {{year}}, {{copyright}}, {{ccli}} and {{license}}. Parts between · with nothing in them are left out. Empty: the usual line.")),
        row("ccli", Songs, false, tr!("Church CCLI licence #"), ""),
        row("updates", About, false, tr!("New versions"), tr!("Once a day RustyPresenter asks GitHub which version is the newest, and tells you when there is a newer one. Nothing about your church is sent, and nothing is downloaded or installed until you choose to.")),
        row("songselect-inbox", SongSelect, false, "SongSelect", tr!("Songs you download from SongSelect in your web browser come into the song library by themselves. Only SongSelect song files are read; nothing in Downloads is moved or deleted.")),
        row("songs-folder", SongsFolder, false, tr!("Songs folder"), tr!("Every song is also kept as a ChordPro file in a folder you can open, back up or edit in Notepad. Song files you put in the folder (ChordPro, SongSelect, OpenLyrics, OpenSong or plain text with Verse/Chorus headings) come into the song library by themselves.")),
        row("media-scale", Media, false, tr!("When the shape doesn't match the screen"), ""),
        row("video-end", Media, false, tr!("When a video ends"), tr!("For videos added from now on. Change one video in its Edit window.")),
        row("picture-secs", Media, false, tr!("Pictures added together as slides"), tr!("Each picture stays this long, then the next one comes up.")),
        row("media-cleanup", Media, false, tr!("Media library"), tr!("Every file you add is copied into RustyPresenter's own folder, so moving or deleting the original never breaks a slide.")),
        row("audio-device", Sound, false, tr!("Sound goes to"), ""),
        row("listen", Sound, false, tr!("Headphones (Listen)"), tr!("View → Listen in headphones sends the sound here only, so you can check a video without the room hearing it.")),
        row("room-delay", Sound, true, tr!("Delay the room's sound (milliseconds)"), tr!("If the sound comes before the picture on the projector, try 50 to 150. 0 is no delay.")),
        row("speakers", Sound, true, tr!("Speakers"), ""),
        row("stage-layout", Stage, false, tr!("Stage layout"), tr!("What the stage screen shows. Window → Stage layouts… makes your own.")),
        row("stage-drawings", Stage, false, tr!("Drawings"), ""),
        row("font", Look, false, tr!("Font"), ""),
        row("song-size", Look, false, tr!("Song text size"), tr!("(biggest; long lines shrink to fit)")),
        row("bible-size", Look, false, tr!("Bible text size"), tr!("Text shrinks automatically when a slide holds more words.")),
        row("text-colour", Look, false, tr!("Text colour"), ""),
        row("text-position", Look, false, tr!("Text position"), ""),
        row("text-style", Look, false, tr!("Text style"), ""),
        row("look-fade", Look, true, tr!("Fade when the theme changes"), ""),
        row("logo", Look, false, tr!("Logo"), ""),
        row("default-background", Look, false, tr!("Default background"), ""),
        row("sections", Sections, false, tr!("Sections"), tr!("Each kind of section has a colour and a key. Leave a box empty for the usual name or key.")),
        row("section-names", Sections, false, tr!("More section names"), tr!("A name your songs use, like “Spontaneous”. It gets the colour and key of the kind you choose.")),
        row("slide-labels", Sections, false, tr!("Slide labels"), tr!("In the song editor, “Label this slide” puts one on a slide. It shows on the slide’s thumbnail.")),
        row("default-bible", Bible, false, tr!("Default Bible"), ""),
        row("also-show", Bible, false, tr!("Also show with every new passage"), tr!("Pick up to two. Each slide then shows the same verses from every Bible.")),
        row("bible-per-slide", Bible, false, tr!("Verses per slide"), tr!("For new passages. Text shrinks automatically when a slide holds more words.")),
        row("bible-library", Bible, false, tr!("Add Bibles"), tr!("KJV, WEB and BSB are built in and work offline. The others come from the internet or from their publisher's service.")),
        row("bible-translations", BibleOnline, false, tr!("Translations"), ""),
        row("bible-online", BibleOnline, false, tr!("Online services"), ""),
        row("bible-files", BibleOnline, false, tr!("Bible files"), tr!("Import a Zefania, OSIS, USX or OpenSong file (.xml or .usx). Only import Bibles that are public domain or that your church is licensed to use. Give it the short name of a translation above (like HCSB) to use the file for that translation.")),
        row("remote-pair", Remote, false, tr!("Use your phone or tablet as a remote"), tr!("Pair it once with the code or the QR code; it stays paired. It runs the slides from anywhere in the room on the same Wi-Fi.")),
        row("remote-password", Remote, false, tr!("Remote password"), tr!("Phones and tablets still pair with the code or the QR code; the password is asked too. Ones already paired stay paired.")),
        row("remote-drawing", Remote, false, tr!("Drawing from a phone"), tr!("On the phone, “Fades” ink works like a laser pointer: it goes away by itself.")),
        row("drawing-limit", Remote, true, tr!("Limits"), ""),
        row("phone-stage", PhoneStage, false, tr!("Stage screen on phones and tablets"), tr!("Singers and musicians see the stage screen on their own phone or tablet (read only), on the same Wi-Fi as this computer.")),
        row("phone-camera", PhoneCamMic, false, tr!("Phone as a camera"), tr!("Scanning the code connects the phone. Once it sends its picture, the camera lists call it Phone camera.")),
        row("phone-mic", PhoneCamMic, false, tr!("Phone as a microphone"), tr!("For a pastor's lapel or a room microphone without cables. Scanning the code connects the phone; then press Start on it.")),
        row("tcp", OtherGear, false, tr!("Control from other gear (TCP)"), tr!("For a stream deck, a lighting desk or a control system that sends text commands.")),
        row("section-keys", Help, false, tr!("Section keys (C, B, 1–9…)"), tr!("Turn off if letters typed by mistake move the screen.")),
        row("keyboard", Help, false, tr!("Keyboard shortcuts"), tr!("Change any key that has a Change button. New keys work once you save the settings.")),
        row("slide-editor-keys", Help, false, tr!("In the slide editor"), ""),
        row("clear-groups", ClearGroups, false, tr!("Clear groups"), tr!("A clear group takes several layers off the screen with one key. The View menu lists them too.")),
        row("follow-input", AutoFollow, false, tr!("Sound input"), tr!("Best: the lead singer's microphone on its own, sent from the sound board. Next best: the main mix. A microphone in the room hears too much else.")),
        row("follow-mode", AutoFollow, false, tr!("Songs follow the singers"), tr!("A song can have its own in Edit item. Key A turns Auto on and off for the live song, H holds it.")),
        row("follow-lead", AutoFollow, false, tr!("Show the next slide before it is sung"), tr!("Early enough to read, not so early that people sing ahead.")),
        row("follow-blank", AutoFollow, false, tr!("When nobody sings"), ""),
        row("follow-talk", AutoFollow, false, tr!("When someone talks or prays"), ""),
        row("follow-jump", AutoFollow, true, tr!("Jump to another part of the song after (words heard there)"), tr!("More words: fewer wrong jumps, but slower to follow a change in the order.")),
        row("follow-model", FollowModel, false, tr!("Speech model"), tr!("Downloaded once, then it works without internet. Apache-2.0, by Facebook AI.")),
        row("follow-tape", FollowPractice, false, tr!("Record a practice"), tr!("Keeps the sound auto-follow hears and every slide change, on this computer only. Move the slides by hand as on a Sunday, then check how auto-follow would have done.")),
        row("follow-order", FollowPractice, false, tr!("Song order from practice"), tr!("Auto-follow learns from the recorded practice the order each song was really sung in (the chorus after every verse, the bridge twice) and how long each slide was up. On Sunday it expects that order, and when it cannot make out the words it goes by those times. Your slides stay as they are.")),
        row("follow-practice", FollowPractice, false, tr!("Recording"), ""),
        row("follow-practice-song", FollowPractice, false, tr!("Song"), ""),
        row("stream-helper", StreamWhere, false, tr!("Getting started"), tr!("A few steps: where to stream, which camera, which sound, and a check.")),
        row("stream-to", StreamWhere, false, tr!("Stream to"), tr!("Switch a destination on and a Go Live button appears beside Record. A stream key is a password: whoever has it can stream as you. It is kept in this computer's own password store as you type it, never in a backup or a service file.")),
        row("stream-broadcast", StreamWhere, false, tr!("Broadcast title"), tr!("What YouTube and Facebook call each broadcast. {{date}} becomes the day's date, {{weekday}} the day's name and {{time}} the time; the description can have them too.")),
        row("stream-speed", StreamWhere, false, tr!("Upload speed"), tr!("Sends a few megabytes of zeros to a public speed test and says whether the internet can carry the stream to every destination that is on. It picks the recording size that fits.")),
        row("stream-tall", StreamWhere, true, tr!("Tall for phones"), tr!("These get a tall picture (for YouTube Shorts, Reels, TikTok and other phone apps): the wide picture across the middle, black above and below. The recording stays wide.")),
        row("stream-delay", StreamWhere, true, tr!("Stream delay"), tr!("What goes out to the stream is held back this long, so Hold or Mute can stop something before anyone watching sees it. The recording is not held back. Viewers' comments come in later than what they answer.")),
        row("stream-lighter", StreamWhere, true, tr!("Slow internet"), tr!("When a destination's internet cannot keep up, it gets a lighter stream until it can, instead of skipping. The recording and the other destinations keep the full picture. This uses more of the computer.")),
        row("stream-own-quality", StreamWhere, true, tr!("Each destination's quality"), tr!("A destination can get a smaller picture or a lower bitrate than the main stream (a phone app, a slow server). Each one that differs costs the computer a second encode.")),
        row("stream-network", StreamWhere, true, tr!("Network"), tr!("Send from a wired network card to keep the stream off busy Wi-Fi (YouTube, Facebook, Twitch and other RTMP servers; an SRT server is reached however the computer chooses). Safer send gives an SRT server time to ask again for what got lost on a poor line; the picture arrives that much later.")),
        row("record-size", StreamPicture, false, tr!("Picture size"), tr!("For the livestream and the recording. Bigger is sharper and needs a faster computer, a faster upload and more disk space.")),
        row("record-custom-size", StreamPicture, true, tr!("Custom size"), tr!("Any width and height from 320 × 180 to 7680 × 4320 (made even). Most services and screens expect 16:9.")),
        row("stream-fps", StreamPicture, true, tr!("Pictures a second"), tr!("30 suits most churches. 50 or 60 makes movement smoother and needs a faster computer and upload.")),
        row("record-bitrate", StreamPicture, true, tr!("Bitrate"), tr!("How much picture goes out each second. Automatic grows with the picture size and pictures a second. More is sharper and needs a faster upload.")),
        row("record-encoder", StreamPicture, true, tr!("Video encoder"), tr!("Automatic suits most computers. A graphics card's encoder leaves the processor free; it is tried first, and Automatic is used if it does not work on this computer.")),
        row("stream-codec", StreamPicture, true, tr!("Video format"), tr!("HEVC and AV1 give a sharper picture for the same upload. HEVC is used when every destination that is on takes it (YouTube and your own server do); otherwise H.264. AV1 is for recordings only: while live, H.264 is used. Both need a newer computer or graphics card.")),
        row("stream-encoder-details", StreamPicture, true, tr!("Encoder details"), tr!("Leave these on Automatic unless a service or a video editor asks for something else. Faster leaves more of the computer free; Baseline is only for very old players.")),
        row("stream-sound-quality", StreamPicture, true, tr!("Sound quality"), ""),
        row("stream-cameras", StreamCameras, false, tr!("Cameras"), tr!("A camera or capture card for the stream's camera scenes (and its recording). More cameras can be switched to while live, from the Cameras box beside the live screen.")),
        row("camera-lost", StreamCameras, false, tr!("When a camera sends nothing"), tr!("What the stream shows in a camera's place while it is unplugged or sends no picture. The camera comes back by itself as soon as it sends again.")),
        row("remote-guest", StreamCameras, false, tr!("Remote guest"), tr!("A guest speaker or missionary joins from a browser at home: their picture comes in as a camera, their voice through this computer's sound. The connection goes directly between the two computers; some internets do not allow that, and the pages then say so.")),
        row("stream-auto-scenes", StreamScenes, false, tr!("Scenes"), tr!("Songs and Bible verses go out as the camera with the words; videos and presentations as the audience screen; nothing on the screen as the camera. A scene chosen with the scene button beside Live holds until the next service item.")),
        row("scene-layout", StreamScenes, false, tr!("Scene layout"), tr!("Where the camera goes when it is in a corner of the audience screen, and where the words go over the camera.")),
        row("own-scenes", StreamScenes, false, tr!("Your scenes"), tr!("Make scenes of your own from cameras, the audience screen, the words, pictures, colours and text. They are listed after the built-in scenes while live.")),
        row("stream-cards", StreamScenes, false, tr!("Cards"), tr!("Starting soon shows before the service, with the countdown under the words while it runs; Ending after the last item; Hold for a break, chosen with the scene button.")),
        row("record-folder", Recording, false, tr!("Save recordings in"), ""),
        row("record-file", Recording, true, tr!("File type and name"), tr!("{{date}} becomes the day, {{time}} the time it started, {{title}} the service's name.")),
        row("record-split", Recording, true, tr!("Long recordings"), ""),
        row("record-extra", Recording, true, tr!("Extra recordings"), tr!("While you are live, a second video is saved next to the recording, its name ending in words.mov: only the words, with nothing behind them. When you edit the service video, put it on top to add the words. It stays small because the words seldom change.")),
        row("captions", StreamCaptions, false, tr!("Live captions"), tr!("Captions are made on this computer from the microphone, in English only. They are often wrong: names, Bible places, quiet or echoing speech and songs come out badly, and there is no punctuation. They show about two seconds after the words are said.")),
        row("website", StreamCaptions, false, tr!("On your website"), tr!("To play the livestream on your church's website: copy the code, then in your website's editor add an \"HTML\", \"Embed\" or \"Code\" block where the video should go and paste it there. You do this once: the same code plays every week's service.")),
        row("ndi", StreamNdi, false, tr!("NDI"), tr!("Other computers and programs on this network (OBS, vMix, a video switcher) can take these while you are live. NDI cameras are listed with the cameras.")),
        row("stream-setups", StreamSetups, false, tr!("Saved setups"), tr!("Keep everything set for the livestream (destinations, cameras, sound, scenes and these settings) under a name, and use it again in one go. Stream keys stay as they are.")),
        row("stream-sound-setup", StreamSound, false, tr!("Sound setup"), tr!("One choice sets the microphone and how each sound is cleaned up. Anything changed by hand below makes it Custom.")),
        row("stream-mics", StreamSound, false, tr!("Microphone or line-in"), tr!("For the livestream and the recording: the sound board, a microphone, or a phone (choose Phone). Up to three are mixed while you are live.")),
        row("stream-pc-sound", StreamSound, false, tr!("Sound from this computer"), tr!("Everything that plays on this output goes out with the stream (videos, songs, and any other sound sent there), mixed with the microphone or line-in chosen above.")),
        row("stream-listen", StreamSound, false, tr!("Listen to the stream"), tr!("While you are live, the stream's sound plays here as viewers hear it (cleaned up and with the sound delay), for headphones at the desk. The room does not hear it unless it is the room's output.")),
        row("stream-sound-delay", StreamSound, true, tr!("Sound delay"), tr!("If the sound in the stream comes before people's lips move, the camera's picture is late: hold the sound back until they match.")),
        row("stream-levels", StreamSound, true, tr!("Sound levels"), tr!("Turn each sound up or down in the stream only (not in the room).")),
        row("stream-cleanup", StreamSound, true, tr!("Sound clean-up"), tr!("For the stream only. Evening out keeps quiet speaking easy to hear at home and loud singing from crackling.")),
        row("stream-filters", StreamSound, true, tr!("More sound settings"), tr!("For the stream only. Each sound has its own: start with the Sound setup above and change little at a time.")),
    ]
}

/// The other words people search with for a row; empty for none. In English: most are names of things that are the
/// same in every language (NDI, 4K, YouTube, EQ) or words volunteers meet in English anyway; a row's own label and
/// help, which are translated, are searched too.
pub fn words(key: &str) -> &'static str {
    match key {
        "language" => "language, translate, English, Spanish, translation",
        "room-on-top" => "projector, front, on top, smooth, fade",
        "graphics" => "OpenGL, Direct3D, software, graphics card, flicker, black screen, crash, renderer",
        "light" => "slow, old computer, laptop, low spec, fade, cut, stutter, lag, performance",
        "performance" => "frames, fps, slow, memory",
        "lines" => "lines, words per slide, verses",
        "credits" => "copyright, licence, CCLI, author",
        "ccli" => "licence number, CCLI, report, song use",
        "updates" => "update, upgrade, new version, newest, latest, download, install, GitHub",
        "songselect-inbox" => "CCLI, download, import songs, chords",
        "songs-folder" => "ChordPro, files, Documents, OneDrive, Dropbox, backup, import songs, export songs, Notepad, tidy",
        "media-scale" => "fit, fill, stretch, black bars, crop",
        "video-end" => "loop, repeat, last frame",
        "picture-secs" => "slideshow, photos, timer",
        "media-cleanup" => "disk space, unused files, delete",
        "audio-device" => "speakers, sound card, output, audio",
        "listen" => "headphones, preview sound, cue",
        "room-delay" => "lip sync, sound late, audio delay",
        "speakers" => "mono, stereo, left, right, channels",
        "stage-layout" => "confidence monitor, band, chords, clock, notes, next slide",
        "stage-drawings" => "draw, ink, pen",
        "font" => "typeface, letters, text",
        "song-size" => "font size, big, small, words",
        "bible-size" => "font size, verses, scripture",
        "text-colour" => "color, colour, white, text",
        "text-position" => "lower third, middle, place",
        "text-style" => "shadow, capitals, caps, upper case",
        "look-fade" => "transition, fade, cut",
        "logo" => "church logo, picture, black",
        "default-background" => "background, picture, colour, video",
        "sections" => "verse, chorus, bridge, colours, keys",
        "section-names" => "tag, spontaneous, sections",
        "slide-labels" => "label, colour, slide",
        "default-bible" => "translation, version, KJV, ESV, NIV",
        "also-show" => "two Bibles, parallel, side by side, language",
        "bible-per-slide" => "verses per slide, split",
        "bible-library" => "download Bibles, translations, languages",
        "bible-online" => "API key, ESV, NIV, API.Bible",
        "bible-files" => "import, Zefania, OSIS, USX, OpenSong",
        "remote-pair" => "phone, tablet, iPad, remote control, QR code, pair",
        "remote-password" => "phone, tablet, security, pair",
        "remote-drawing" => "phone, draw, ink, laser pointer",
        "drawing-limit" => "phone, draw, lines, limit",
        "phone-stage" => "phone, tablet, iPad, singers, band, chords, stage screen, network",
        "phone-camera" => "phone camera, iPhone, Android, camera, not private, certificate, QR code, scan, firewall",
        "phone-mic" => "phone mic, phone microphone, iPhone, Android, lapel, wireless, not private, certificate, QR code, scan, firewall",
        "tcp" => "Companion, Stream Deck, control, network, port, commands",
        "section-keys" => "key, keyboard, letters, numbers, jump",
        "keyboard" => "key, keys, shortcuts, hotkeys, F keys, keyboard",
        "slide-editor-keys" => "key, keys, copy style, undo",
        "clear-groups" => "clear, key, layers, F keys",
        "follow-input" => "auto-follow, microphone, listen, sound board, singer",
        "follow-mode" => "auto-follow, automatic, suggest, manual",
        "follow-lead" => "auto-follow, early, timing",
        "follow-blank" | "follow-talk" => "auto-follow, words off, blank, prayer",
        "follow-model" => "auto-follow, speech, download, captions, Whisper",
        "stream-helper" => "livestream, Go Live, set up, wizard, first time",
        "stream-to" => "livestream, YouTube, Facebook, Twitch, Instagram, TikTok, X, RTMP, SRT, stream key, Go Live",
        "stream-broadcast" => "livestream, YouTube, Facebook, title, description, thumbnail, schedule, privacy",
        "stream-speed" => "livestream, internet, upload, bandwidth, Mbit/s",
        "stream-tall" => "livestream, vertical, Shorts, Reels, TikTok, 9:16",
        "stream-delay" => "livestream, delay, hold back",
        "stream-lighter" => "livestream, slow internet, adaptive, bitrate",
        "stream-own-quality" => "livestream, resolution, bitrate, each destination",
        "stream-network" => "livestream, network card, Ethernet, Wi-Fi, SRT, latency",
        "record-size" => "resolution, 480p, 720p, 1080p, 1440p, 4K, size, recording, livestream",
        "record-custom-size" => "resolution, width, height, custom size",
        "stream-fps" => "frame rate, fps, 60, 30, 25, smooth",
        "record-bitrate" => "bitrate, quality, kbit/s, Mbit/s",
        "record-encoder" => "NVENC, Quick Sync, AMF, x264, graphics card, encoder",
        "stream-codec" => "H.264, HEVC, H.265, AV1, codec, CBR, VBR",
        "stream-encoder-details" => "keyframe, GOP, preset, profile, B-frames",
        "stream-sound-quality" => "audio, sample rate, 48 kHz, bitrate, mono, stereo",
        "stream-cameras" => "camera, webcam, capture card, NDI, PTZ, network camera, RTSP",
        "camera-lost" => "camera, unplugged, lost, black, logo",
        "remote-guest" => "guest, Zoom, call, missionary, remote speaker",
        "stream-auto-scenes" => "scenes, camera, automatic, picture in picture",
        "scene-layout" => "picture in picture, corner, lower third",
        "own-scenes" => "scenes, custom, layers, overlay",
        "stream-cards" => "starting soon, countdown, ending, hold, break",
        "record-folder" => "recording, folder, save, videos",
        "record-file" => "recording, MP4, MKV, file name",
        "record-split" => "recording, split, replay, save last",
        "record-extra" => "recording, clean feed, words alone, tracks, editing",
        "captions" => "subtitles, closed captions, transcription, deaf",
        "website" => "embed, website, player, code",
        "ndi" => "NDI, OBS, vMix, switcher, network video, keying",
        "stream-setups" => "presets, profiles, Sunday, wedding, setups",
        "stream-sound-setup" => "sound, audio, preset, mixer",
        "stream-mics" => "microphone, mic, line-in, sound board, mixer, phone mic, audio input",
        "stream-pc-sound" => "sound, audio, computer sound, videos, loopback",
        "stream-listen" => "headphones, monitor, listen",
        "stream-sound-delay" => "lip sync, sound early, audio delay",
        "stream-levels" => "volume, gain, louder, quieter, dB",
        "stream-cleanup" => "compressor, noise, hum, hiss",
        "stream-filters" => "EQ, equaliser, gate, compressor, limiter",
        _ => "",
    }
}

/// A category's name, as the sidebar shows it.
pub fn category(page: SettingsTab) -> String {
    match page {
        Language | Video => tr!("General"),
        Look | Themes => tr!("Look"),
        Songs | Sections | SongSelect | SongsFolder => tr!("Songs"),
        Bible | BibleOnline => tr!("Bible"),
        Media => tr!("Video and media"),
        Sound => tr!("Sound"),
        Stage => tr!("Stage screen"),
        Remote | PhoneStage | PhoneCamMic | OtherGear => tr!("Remote and network"),
        Help | ClearGroups => tr!("Keys"),
        AutoFollow | FollowModel | FollowPractice | FollowReview => tr!("Auto-follow"),
        StreamWhere | StreamPicture | StreamCameras | StreamSound | StreamScenes | Recording | StreamCaptions | StreamNdi | StreamSetups => tr!("Livestream"),
        Backups => tr!("Backups"),
        About => tr!("Help and about"),
    }
}

/// A page's own name.
pub fn page_title(page: SettingsTab) -> String {
    match page {
        Language => tr!("Language"),
        Video => tr!("Screens and graphics"),
        Look => tr!("Text and logo"),
        Themes => tr!("Themes and backgrounds"),
        Songs => tr!("Slides and credits"),
        Sections => tr!("Sections and labels"),
        SongSelect => "SongSelect".into(),
        SongsFolder => tr!("Songs folder"),
        Bible => tr!("Bibles"),
        BibleOnline => tr!("Online services and files"),
        Media => tr!("Pictures and videos"),
        Sound => tr!("Sound in the room"),
        Stage => tr!("Stage screen"),
        Remote => tr!("Remote"),
        PhoneStage => tr!("Stage screen on phones"),
        PhoneCamMic => tr!("Phone camera and microphone"),
        OtherGear => tr!("Other gear"),
        Help => tr!("Keyboard shortcuts"),
        ClearGroups => tr!("Clear groups"),
        AutoFollow => tr!("Listening"),
        FollowModel => tr!("Speech model"),
        FollowPractice => tr!("Practice"),
        FollowReview => tr!("Review log"),
        StreamWhere => tr!("Where to stream"),
        StreamPicture => tr!("Picture and quality"),
        StreamCameras => tr!("Cameras"),
        StreamSound => tr!("Sound"),
        StreamScenes => tr!("Scenes"),
        Recording => tr!("Recording"),
        StreamCaptions => tr!("Captions and website"),
        StreamNdi => "NDI".into(),
        StreamSetups => tr!("Saved setups"),
        Backups => tr!("Backups"),
        About => tr!("About"),
    }
}

/// Other words for a whole page (the pages that are one list rather than rows).
fn page_words(page: SettingsTab) -> &'static str {
    match page {
        Themes => "themes, backgrounds, fonts, layouts, styles",
        FollowPractice => "auto-follow, test, practise, recording",
        FollowReview => "auto-follow, log, history",
        Backups => "backup, restore, undo, copy, safe",
        About => "version, licence, data folder, help, about",
        _ => "",
    }
}

/// The pages of a category, in order.
fn pages_of(page: SettingsTab) -> Vec<SettingsTab> {
    let c = category(page);
    SettingsTab::ALL.into_iter().filter(|p| category(*p) == c).collect()
}

/// Where a page is, as Settings → paths are written: "Livestream → Sound", or "Backups" for a page that is its
/// category's only one.
pub fn path(page: SettingsTab) -> String {
    if pages_of(page).len() == 1 { category(page) } else { format!("{} → {}", category(page), page_title(page)) }
}

/// The sidebar: each category (a heading, then its pages), or a category of one page as that page alone (under the
/// category's name).
pub fn nav() -> Vec<(String, Option<SettingsTab>)> {
    let mut out = vec![];
    let mut last = String::new();
    for page in SettingsTab::ALL {
        let c = category(page);
        if pages_of(page).len() == 1 {
            out.push((c.clone(), Some(page)));
        } else {
            if c != last { out.push((c.clone(), None)) }
            out.push((page_title(page), Some(page)));
        }
        last = c;
    }
    out
}

/// Something "Find a setting" found: a row (`key`), or a whole page (`key` empty).
#[derive(Clone, Debug, PartialEq)]
pub struct Found {
    pub key: &'static str,
    pub page: SettingsTab,
    pub title: String,
    pub path: String,
}

/// The text as Slint shows it: `{{` and `}}` (a literal brace in `@tr`) are single.
pub fn shown(s: &str) -> String { s.replace("{{", "{").replace("}}", "}") }

fn fold(s: &str) -> String { s.to_lowercase().replace(['\u{2019}', '\u{2018}'], "'") }

/// Every row and page whose words have every word typed (in any order, any part of a word), the best first: the
/// ones whose name has them, then by their place in the sidebar. At most 40.
pub fn search(query: &str) -> Vec<Found> {
    let typed: Vec<String> = fold(query).split(|c: char| c.is_whitespace() || c == ',').filter(|w| !w.is_empty()).map(str::to_string).collect();
    if typed.is_empty() { return vec![] }
    let has_all = |text: &str| typed.iter().all(|w| text.contains(w.as_str()));
    let mut found: Vec<(u32, usize, Found)> = vec![];
    for (i, page) in SettingsTab::ALL.into_iter().enumerate() {
        let name = fold(&format!("{} {}", page_title(page), category(page)));
        let all = format!("{name} {}", fold(page_words(page)));
        if has_all(&all) {
            let score = if has_all(&name) { 15 } else { 5 };
            found.push((score, i * 1000, Found { key: "", page, title: path(page), path: tr!("Page") }));
        }
    }
    for (j, r) in rows().into_iter().enumerate() {
        let label = fold(&r.label);
        let other = fold(words(r.key));
        let all = format!("{label} {} {other} {} {}", fold(&shown(&r.help)), fold(&page_title(r.page)), fold(&category(r.page)));
        if !has_all(&all) { continue }
        let score = if has_all(&label) { 20 } else if has_all(&other) { 12 } else { 1 };
        let place = SettingsTab::ALL.iter().position(|p| *p == r.page).unwrap_or(0) * 1000 + j;
        found.push((score, place, Found { key: r.key, page: r.page, title: shown(&r.label), path: path(r.page) }));
    }
    found.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    found.into_iter().take(40).map(|(_, _, f)| f).collect()
}

/// The row of `key`, if there is one.
pub fn find_row(key: &str) -> Option<Row> { rows().into_iter().find(|r| r.key == key) }
