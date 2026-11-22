# RustyPresenter

A free presentation app for churches. It shows song words, Bible verses, slides, videos and pictures on the screen in the room, with a second screen for the band and a phone as the remote.

It is free for any church, with no account and no subscription.

> **0.5 Beta.** This is an early version. Try it on a weekday before you rely on it on a Sunday, and report what breaks.

[![Made with Slint](https://raw.githubusercontent.com/slint-ui/slint/master/logo/MadeWithSlint-logo-whitebg.png)](https://slint.dev)

## What it does

- **Songs:** type or paste the words, or bring them in from SongSelect.
- **Bible:** type `John 3:16` and press Enter. Three Bibles work with no internet, NIV works online, and over a thousand more in other languages can be added. Up to three can show side by side.
- **Slides, videos, pictures and music:** PowerPoint, Keynote and PDF files, and almost any video or audio file.
- **Two screens:** the audience screen for the room and a stage screen for the band and speaker.
- **Remote:** run the slides from a phone or tablet on the church Wi-Fi.
- **Livestream and recording:** send the service to YouTube or Facebook, or save it as a video.
- **Auto-follow (new):** listens to the singing and moves the song slides for you.
- **English and 13 other languages** for the app itself.

## Get it

1. Go to the [releases page](https://github.com/SuperDuperSoft/RustyPresenter/releases/latest).
2. Download `RustyPresenter-Setup-<version>.exe` (Windows 10 or 11).
3. Double-click it. It needs no administrator password.

Windows may warn you the first time, because the installer is not signed yet. Click **More info**, then **Run anyway**.

On the first start Windows asks whether RustyPresenter may use the network. Tick **Private networks** and click **Allow**, or phones cannot connect later.

Mac and Linux versions are not ready yet.

## Your first service in five minutes

1. **Plug in the projector or TV.** Press `Win + P` and choose **Extend**. Then open **Window → Screens…** and pick which monitor is the audience screen.
2. **Make this week's service:** **File → New service…**
3. **Add things:** **File → Add to service** for a song, a Bible passage, a video or slides. You can also drag files onto the window.
4. **Show a slide:** click it. It is on the screen at once. The red outline shows what the room sees.
5. **During the service:** press `→` or `Space` for the next slide and `←` to go back. `F1` clears the screen.

That is all you need for a Sunday. The guides below cover the rest.

## Guides

| Guide | What it covers |
| --- | --- |
| [Your first Sunday](docs/guides/first-sunday.md) | Setting up from nothing, step by step |
| [Songs](docs/guides/songs.md) | Adding, importing and editing songs |
| [Bible](docs/guides/bible.md) | Passages, more Bibles, two or three side by side |
| [Videos, pictures and slides](docs/guides/media.md) | PowerPoint, videos, backgrounds, music |
| [Screens](docs/guides/screens.md) | The audience screen and the stage screen |
| [Phone remote](docs/guides/remote.md) | Running the slides from a phone or tablet |
| [Livestream and recording](docs/guides/livestream.md) | YouTube, Facebook and saving a video |
| [Auto-follow](docs/guides/auto-follow.md) | Letting the app move the song slides |
| [Keys](docs/guides/keys.md) | Every keyboard shortcut on one page |
| [Updates and backups](docs/guides/updates-and-backups.md) | Getting new versions, keeping your work safe |
| [When something goes wrong](docs/guides/troubleshooting.md) | Common problems and what to do |

The app also has its own short tutorials: **Help → Tutorials**.

## Getting help

- **Help → Check the service** finds missing files and empty items before the service starts.
- **Help → Report a problem…** writes a note to fill in and shows where the log files are.
- Problems and ideas: [open an issue](https://github.com/SuperDuperSoft/RustyPresenter/issues).

## Helping out

- **Translate it** into your language, with no programming: [docs/TRANSLATING.md](docs/TRANSLATING.md).
- **Build it or change the code:** [docs/DEVELOPING.md](docs/DEVELOPING.md).

## Licences

RustyPresenter's own code is under the MIT licence or the Apache License 2.0, whichever you prefer (`LICENSE-MIT`, `LICENSE-APACHE`).

It uses libmpv and FFmpeg under the GNU Lesser General Public License. **Help → About RustyPresenter** lists every third-party licence, and the installer includes the same text.

Bible texts belong to their publishers. Each Bible's notice is shown in **Settings → Bible** and on its slides where the publisher asks for it.
