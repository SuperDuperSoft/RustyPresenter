# Livestream extras — design (November 17, 2026, branch `ls2-extras`)

The items the livestreaming design (`2026-11-09-livestreaming-design.md`) left for later (§8 "Later") and the
Go Live panel parts of §4 item 9 that were still missing.

## 1. Live captions (`stream::captions`)

- **Where the words come from:** auto-follow's English *letter* model (`follow::fetch::FAST`,
  wav2vec2-base-960h), run on this computer with Candle. Auto-follow's other model writes speech sounds, not words,
  so captions need the fast model to be downloaded (Settings → Auto-follow); Settings says so when it is not. No
  new model.
- **How it hears:** a small FFmpeg of its own reads the microphone (as auto-follow does), so the stream's own sound
  path (`board_feed`) is untouched. It listens to the stream's sound input unless another input is chosen.
- **Reading the model:** the same rolling window as the ear (1.5 s seen, every 0.5 s), greedy CTC: the most likely
  letter per 20 ms, repeats and blanks collapsed, "|" or a 0.3 s pause between words (`Letters`, `Listen`). Tidied
  for church use: "I", God, Jesus, Lord… with capitals, a capital after a pause of 1 s.
- **On screen:** two lines of up to 32 letters (CEA-608's line), rolling up as TV captions do, gone after 4 s of
  quiet (`Board`). Drawn on the captions thread (white on a 75 % dark band, lower third) into a slot the recorder's
  writer lays over the stream picture (`Controls::captions`, `Mixer::captions`). Not over song words, verses or
  cards (`shows_over`): singing captions badly, and two texts on each other read worse than one. Mute stops them.
- **File:** `<recording>.vtt` beside the recording (WebVTT), written cue by cue so a crash keeps it. Players load it
  with the video; YouTube Studio takes it for the replay's subtitles.
- **YouTube's own captions (CC button):** FFmpeg has no encoder that turns text into CEA-608/708 inside H.264 (only
  passing through existing ones), so real captions on YouTube go by YouTube's "POST captions to URL": the church
  pastes the captions address from YouTube Studio; each change of the lines is posted (at most once a second,
  `seq` numbered, stamped with the time said plus the stream delay). Facebook and the others get the picture
  captions only.
- **Accuracy, in Settings' words:** English only; often wrong with names, Bible places, quiet or echoing speech
  and songs; no punctuation; about two seconds late.

## 2. Website player (`stream::website`)

Settings → Livestream → "On your website": the YouTube channel id (filled in from a signed-in account, or typed /
pasted as a channel address) and the Facebook Page address, each with "Copy the code" and plain instructions.
- YouTube: `youtube.com/embed/live_stream?channel=UC…` in a 16:9 box that fits any width: it plays whatever is
  live, so the code is pasted once.
- Facebook has no "always the live video" player; the Page plugin (timeline) shows the live video at the top while
  live. Said plainly.
The app serves no page for this (the church's website is elsewhere).

## 3. Remote guests (`network::guest`, `web/guest-host.html`, `web/guest.html`)

**What blocks WebRTC inside the app:** no WebRTC stack is in `Cargo.lock` or the offline registry (no `webrtc`,
`str0m`, DTLS, SRTP, SCTP, ICE/STUN or SDP crate; `rustls` does TLS, not DTLS), and FFmpeg 9 only *sends* WebRTC
(its `whip` muxer); it has no WHIP/WHEP input. Writing DTLS-SRTP by hand is security work not to be done here.

**What is built instead (no paid server):** the presentation computer's own web browser does the WebRTC part.
- Settings → "Invite a guest…" opens `http://127.0.0.1:4318/guest/host` in the browser (127.0.0.1 counts as a
  secure page, so WebRTC works). It makes an invitation code (the offer, compressed), and the operator saves the
  guest's page as a file ("Join the livestream.html") and emails both.
- The guest double-clicks the file (a file on their own computer may use the camera; a page on the church's
  network over http may not), pastes the code, allows the camera, and gets a reply code to send back. The
  operator pastes it: the two browsers connect directly, with free public STUN servers (Google, Cloudflare) to
  find each other. No relay (TURN) server: when both internets block direct connections, both pages say so and
  suggest a phone hotspot.
- The host page plays the guest's voice (the room and the stream hear it through this computer's sound, which the
  stream already takes) and sends the guest's picture back to the app as WebM pieces every 200 ms
  (`POST /guest/video`, this computer only, with a header a page elsewhere cannot send without asking first). The
  app hands them out as one endless WebM at `/guest/video.webm`, which the stream reads as a network camera
  ("Use the guest as a camera" puts it in the first free camera row; it is labelled "Remote guest").
- Codes travel by text or email: no signalling server.

## 4. First-time Go Live helper (`stream::helper`, `ui::dialogs::go_live_helper`)

Go Live (File → Go Live, or the Live button) with no destination on and no NDI opens four steps instead of a
message: where (YouTube / Facebook / Twitch / another server; paste the key, with where each service shows it; or
"Sign in instead…", which opens Settings), camera, sound, check (upload speed, one picture from the camera, three
seconds of the sound; nothing sent to viewers) with an optional 20-second test stream that stops by itself (its
summary says whether it reached the service; the step says who could see it), then "Go Live now". Each step saves
into Settings as it is left. Settings → Livestream → "Set up step by step…" opens it too.

## 5. Go Live panel (§4 item 9)

Already there: title template (`{date}`, `{service}`), per-destination toasts, viewers and chat when signed in.
Added:
- **Health light** beside Live: green "All good", amber "Check the stream", red "Not going out" (every destination
  failing several tries and no NDI). Clicking it lists each destination (going out / connecting / lighter for a slow
  internet / not connecting), NDI, the camera, the sound, a slow computer, Freeze/Mute and the captions, plus the
  live captions as they are shown (`stream::health`).
- **Templates:** `{time}` and `{weekday}` besides `{date}` and `{service}` (`health::fill`), used for the title and
  description shown in the status list, with Copy buttons for services streamed by key (whose own page needs them).
- **File → Go Live** in the menu (the Live button only shows once a destination is on).
