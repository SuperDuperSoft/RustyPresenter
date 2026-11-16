# Setting up "Sign in" for YouTube, Facebook and Twitch

RustyPresenter can stream with a pasted stream key and needs none of this. Sign-in lets Go Live make the broadcast
itself, with its title and description, show how many are watching, and give the replay link afterwards.

For that, the app has to be registered once with Google and with Meta, under the church's own accounts (Jake,
November 16, 2026). Registering gives four ids (five with Twitch). They go in a private file on the computer that builds the app,
`.tools\sign-in-ids.cmd`, which is never committed:

```bat
set "RP_GOOGLE_CLIENT_ID=…"
set "RP_GOOGLE_CLIENT_SECRET=…"
set "RP_FACEBOOK_APP_ID=…"
set "RP_FACEBOOK_CLIENT_TOKEN=…"
set "RP_TWITCH_CLIENT_ID=…"
```

`try-latest.cmd`, `windows-check.cmd` and `make-installer.cmd` read it. A copy built without it shows no Sign in
buttons. Either service can be left out.

## Google (YouTube), about 15 minutes

Sign in to Google with the church's account (the one that owns the YouTube channel).

1. YouTube must allow live streaming on the channel: open youtube.com/features and turn on Live streaming. The
   first time, YouTube takes up to 24 hours to switch it on.
2. Open console.cloud.google.com. Make a new project called RustyPresenter.
3. **APIs & Services → Library:** find "YouTube Data API v3" and press Enable.
4. **Google Auth Platform → Branding:** app name RustyPresenter, the church's email for support and contact.
   **Audience:** External.
   **Data Access → Add or remove scopes:** tick `.../auth/youtube` ("Manage your YouTube account").
5. **Audience → Publish app**, to put it "In production". In "Testing" instead, every sign-in stops working
   after 7 days.
6. **Clients → Create client:** Application type **Desktop app**, name RustyPresenter. Copy the **Client ID** and
   the **Client secret**.

Until Google verifies the app, signing in shows "Google hasn't verified this app". Press **Advanced**, then
**Go to RustyPresenter (unsafe)**. That is expected for the church's own use. Google allows 100 accounts before
verification. Before other churches use sign-in, ask Google to verify the app (from the Branding page; it needs
a privacy policy page and a short video of the sign-in).

## Meta (Facebook), about 15 minutes

Sign in to Facebook with an account that is an admin of the church's Page.

1. Open developers.facebook.com, register as a developer if asked, then **My Apps → Create App**. Name it
   RustyPresenter. Choose the use case for managing a Page (or "Other", then type **Business**).
2. Add the **Facebook Login** product. In its **Settings**, switch **Login from Devices** to **Yes**.
3. **App settings → Basic:** copy the **App ID**.
   **App settings → Advanced → Security:** copy the **Client token**.
4. Leave the app in **Development** mode. Everyone with a role on the app (add other Page admins under
   **App roles**) can then sign in with every permission the app asks for: `pages_show_list`,
   `pages_read_engagement`, `pages_manage_posts`, `publish_video`.

Before other churches use Facebook sign-in, Meta must review the app (**App Review**, one request per permission
above), and it then goes **Live**.

## Twitch, about 5 minutes

Sign in to Twitch with the church's channel account. Twitch asks for two-factor sign-in to be on first.

1. Open dev.twitch.tv/console and **Register Your Application**. Name: RustyPresenter. OAuth Redirect URL:
   `http://localhost` (not used, but asked for). Category: Broadcaster Suite. **Client Type: Public.**
2. Copy the **Client ID**. A public client has no secret.

Signing in shows a short code to type at twitch.tv/activate. Go Live then sets the channel's title and gets its
stream key by itself. Twitch needs no review before other churches use it.

## Using it

Settings → Media → Livestream: **Sign in** beside YouTube opens the web browser. For Facebook, the app shows a
short code to type at facebook.com/device, on any phone or computer. Once signed in, set the broadcast's title
(`{date}` becomes the day's date) and description, and on YouTube who can watch.

What sign-in keeps is in Windows' own password store (Credential Manager), like stream keys, and never in the
church's data or backups. **Sign out** forgets it on that computer.
