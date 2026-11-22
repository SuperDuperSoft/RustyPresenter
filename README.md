# RustyPresenter

A free presentation app for churches. It puts song words, Bible verses, slides, videos and pictures on the screen in the room, gives the band and the speaker their own stage screen, and lets a phone run the slides.

Free for any church. No account and no subscription.

> **0.5 Beta.** This is an early version. Try it on a weekday before you rely on it on a Sunday, and [report what breaks](https://github.com/SuperDuperSoft/RustyPresenter/issues).

## What it does

- **Songs.** Type or paste the words, or bring them in from SongSelect. Each verse and chorus gets its own label and colour.
- **Bible.** Type `John 3:16` and press Enter. KJV, WEB and BSB are built in and need no internet. NIV works online. Over a thousand more Bibles, in over a thousand languages, can be added. Up to three show side by side.
- **Slides, videos, pictures and music.** PowerPoint, Keynote and PDF files, and almost any video or audio file.
- **Two screens.** The audience screen for the room, and a stage screen for the band and the speaker, with the words and messages from the operator.
- **Phone remote.** Run the slides from a phone or tablet on the same Wi-Fi. Nothing to install on the phone.
- **Livestream and recording.** Send the service to YouTube or Facebook, or save it as a video.
- **Auto-follow.** The app listens to the singing and moves the song slides by itself.
- **14 languages** for the app itself, English included.

## Get it

Everything is on the [releases page](https://github.com/SuperDuperSoft/RustyPresenter/releases/latest).

| Computer | File to download |
| --- | --- |
| **Windows 10 or 11** | `RustyPresenter-Setup-<version>.exe` |
| **Mac** (Apple silicon, macOS 13 or later) | `RustyPresenter-<version>-macos.dmg` |
| **Linux** | `RustyPresenter-<version>-linux.AppImage`, or `RustyPresenter-<version>-linux.deb` for Debian and Ubuntu |
| **Android phone, as a remote** (optional) | `RustyPresenter-Remote-<version>.apk` |

The Windows version is the one tested most. The Mac and Linux versions are new and have had far less testing.

### Windows

1. Double-click the setup file. It needs no administrator password.
2. Windows may warn you, because the installer is not signed yet. Click **More info**, then **Run anyway**.
3. On the first start Windows asks whether RustyPresenter may use the network. Tick **Private networks** and click **Allow**, or phones cannot connect later.

### Mac

1. Open the `.dmg` and drag RustyPresenter into **Applications**.
2. The first time you open it, macOS says it cannot check the app. Open **System Settings → Privacy & Security**, scroll down, and click **Open Anyway**.

### Linux

- **AppImage:** right-click the file, open **Properties**, allow it to run as a program, then double-click it.
- **Debian or Ubuntu:** double-click the `.deb` file, or run `sudo apt install ./RustyPresenter-<version>-linux.deb`.

## Your first service in five minutes

1. **Plug in the projector or TV** and set the computer to extend the desktop onto it (on Windows: `Win + P`, then **Extend**). Open **Window → Screens…** and pick which monitor is the audience screen.
2. **Make this week's service:** **File → New service…**
3. **Add things:** **File → Add to service** for a song, a Bible passage, a video or slides. You can also drag files onto the window.
4. **Show a slide:** click it. It is on the screen at once. The red outline shows what the room sees.
5. **During the service:** press `→` or `Space` for the next slide and `←` to go back. `F1` clears the screen.

That is enough for a whole service. The guides below cover the rest.

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

## Updating

RustyPresenter tells you when a new version is out. Close the app, download the new file and install it over the old one. Songs, services and settings stay as they are.

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

[![Made with Slint](https://raw.githubusercontent.com/slint-ui/slint/master/logo/MadeWithSlint-logo-whitebg.png)](https://slint.dev)
