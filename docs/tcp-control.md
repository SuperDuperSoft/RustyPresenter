# TCP control (N31)

RustyPresenter can take simple commands from other gear on the church network: a lighting desk, a Stream Deck,
a Companion button, or a script. It is **off** until you turn it on in **Settings → Network…**.

- **Port:** 4319 unless you choose another (1024–65535, not the app's own port).
- **Password:** optional. When set, the first line must be `password <your password>`.
  Three wrong passwords close the connection, and each wrong one waits a second.
- **Connections:** up to 8 at once. Lines are at most 1024 bytes.

## How it works

Open a TCP connection to the presentation computer (for example `nc 192.168.1.20 4319`) and send one command a
line, ending with a newline (`\n` or `\r\n`). Every line gets one answer line:

- `OK` (sometimes with more after it) when it worked,
- `ERR <why>` when it did not.

Words are not case-sensitive.

## Commands

| Command | What it does | Answer |
|---|---|---|
| `next` | Next slide (into the next item at the end of one) | `OK` |
| `prev` (or `back`, `previous`) | Previous slide | `OK` |
| `clear` (or `clear all`) | Clear everything | `OK` |
| `clear slide` (or `clear text`) | Clear the words, keep the background | `OK` |
| `clear media` (or `clear video`) | Stop the video or picture | `OK` |
| `clear audio` | Stop the music | `OK` |
| `logo` | Show the church logo | `OK` |
| `go <item>` | Show the first slide of item number `<item>` of the current service (from 1) | `OK` |
| `go <item> <slide>` | Show slide `<slide>` of that item (from 1) | `OK` |
| `status` | What is live | `OK live <item> <slide> <slides> <title>` or `OK nothing live` |
| `ping` | Check the connection | `OK pong` |
| `password <password>` | Give the password | `OK`, or `ERR wrong password` |
| `help` | List the commands | `OK commands: …` |
| `quit` (or `exit`) | Close the connection | `OK bye` |

Examples of errors: `ERR there is no item 9 (the service has 6)`, `ERR item 2 has 4 slides`,
`ERR password needed: send password <password> first`, `ERR unknown command "dance"; send help`.

## Example

```
$ nc 192.168.1.20 4319
password lights
OK
go 2
OK
status
OK live 2 1 5 Amazing Grace
next
OK
clear
OK
quit
OK bye
```

## Security

The control is plain text on the local network: anyone who can reach the port can send commands unless a password
is set, and the password itself travels unencrypted. Turn it on only on a church network you trust, and set a
password. The app stores only a salted hash of the password.
