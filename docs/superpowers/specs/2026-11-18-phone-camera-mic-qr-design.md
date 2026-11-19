# Phone camera and microphone made easy (November 18, 2026)

Jake: "I can't get the phone as a microphone or phone as a camera to work. Can you add a QR code to that as well?"

## Why it failed (found without a phone)

1. **Windows Firewall, public networks.** On this PC the installed app's firewall rules (made by Windows' own
   question the first time the app listened) allow it on **Private** networks only. Church Wi-Fi is often marked
   **Public**, and then every phone is kept out, silently. Cancelling Windows' question makes "keep out" rules, which
   win over any "let in" rule. Nothing in the app said so, and the installer added no rule.
2. **The camera's address in Settings was the plain page** (`http://…:4318/camera`), which cannot use the camera and
   only links on to the secure page: an extra hop, and a second "not private" warning to get through.
3. **The camera needed pairing first** with the 6-digit code from another dialog (Phone → Pair a phone or tablet),
   open at the same time, within two minutes.
4. **The microphone needed "Show the stage screen to phones" turned on** (off by default); when off, its page only
   linked to the stage view page, which said "off": a dead end.
5. **The microphone page started its sound after waiting for the microphone.** iPhones start sound only straight
   from a tap; after the wait the sound stayed stopped, so nothing was sent and nothing said why. It also asked for
   48 kHz sound, which some browsers (Firefox) refuse when the phone's microphone runs at another rate: the Start
   button then did nothing. An iPhone that locks its screen "interrupts" the sound; it was never started again.
6. **The secure server's failures were invisible.** It printed to a console the app does not show; when its port
   (4320) was taken it gave up for good.
7. **Only five phones stayed paired**: a sixth pushed one out (a church with two cameras, a microphone and two
   remotes lost one). Now sixteen.

8. **Checked and not a cause: the certificate.** Jake is on Android Chrome, and "it seemed to block things" while the
   remote (plain http) worked. The hand-written certificate was checked strictly: `openssl x509 -text` (version 3,
   16-byte positive serial, ECDSA P-256 with SHA-256, UTCTime validity, subjectAltName with the IP addresses as
   iPAddress entries plus `localhost`, basicConstraints CA:FALSE, extKeyUsage serverAuth), `openssl verify
   -x509_strict -purpose sslserver` OK, a live handshake with `openssl s_client` (TLS 1.3 and 1.2, ALPN http/1.1,
   verify OK with the certificate trusted, "self-signed" without), curl 200 on both pages, and Chromium (the desktop
   app's built-in browser) answers `ERR_CERT_AUTHORITY_INVALID`: the ordinary, bypassable "Your connection is not
   private → Advanced → Proceed" page, not `ERR_CERT_INVALID` (a certificate Chrome cannot read, which has no
   Proceed). So what Jake most likely saw is that warning page, with nothing telling him to go on, then the 6-digit
   code or the stage-view switch. After Proceed, Chrome treats the page as https (getUserMedia allowed) and its
   decision covers the same host, so the secure WebSocket works; the page also falls back to one picture a request.
   For the case of a phone that shows no Proceed at all, the plain page on Android explains Chrome's own setting
   (chrome://flags → "Insecure origins treated as secure" with this computer's plain address).

What was right: the https server starts at every start (`startup::serve`), on the app's port + 2, with a certificate
for this computer's network address; the pages, the secure WebSocket and the sound posts work over it with the app's
certificate (tested end to end below); addresses use the network address, not a computer name.

## What changes

- **A QR code beside each address** (Settings → Remote and network → Phone camera and microphone): the secure page
  with a one-time secret, `https://<address>:4320/camera#pair=<secret>` (and `/stage-view/mic`). The pages read it,
  wipe it from the address bar and post it to `/phone/pair`, which pairs the phone like the remote (same cookie,
  same list of paired phones, the remote's password when set). Used: gone, and Settings shows a new code within a
  second, so each phone gets its own; unused codes last 30 minutes and are renewed 5 minutes before. "Show the QR
  code" opens a big window of it for scanning from across the room.
- **The microphone no longer needs the stage screen on phones**: a paired phone may send; phones allowed to see the
  stage view still may.
- **The "not private" step in one line beside each code**, and on the phone's plain page in words for Android
  (Advanced → Proceed) and iPhone (Show Details → visit this website → Visit Website).
- **A live line under each code**: the secure server's problem ("isn't running: port 4320 is in use by another
  program…"), "Sending pictures: Balcony" / "Sending sound: iPhone", "Phone connected: iPhone", Windows Firewall
  keeping phones out (with "Let phones in"), or "Waiting for a phone". The pages ask every few seconds while open.
- **Windows Firewall**: `platform::firewall::check` asks Windows (INetFwMgr::IsPortAllowed) whether a phone may reach
  the secure port now, every 15 seconds while Settings is open. "Let phones in" removes the program's own rules and
  lets it in on private and public networks (netsh, with Windows' administrator question). The installer's last
  page offers the same (ticked; Windows asks for the OK).
- **The secure server** reports why it is not running and tries its port again every 5 seconds.
- **Phone pages**: the microphone page starts its sound straight from the tap, at the phone's own rate (the computer
  converts it), starts it again after an interruption, pairs with the code or the 6-digit code, and stops plainly
  when the phone is unpaired. The camera page pairs with the code (the password asked when set).

Endpoints kept as they were for phones already paired and for the Android app: `/camera`, `/camera/info`,
`/camera/ws`, `/camera/frame`, `/stage-view/mic`, `/stage-view/mic/status` (new fields only), `/stage-view/mic/sound`,
`/remote/pair`. New: `POST /phone/pair { pair, name, password }`.

## Needs a real phone

The certificate warning screens and their words on real Android and iPhone browsers, the camera's Allow prompt, the
iPhone microphone after a screen lock, and "Let phones in" with a real administrator OK on a public network.
