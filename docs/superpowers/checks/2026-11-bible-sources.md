# Bible sources (F5) — November 2, 2026

Branch `bible-esv`. Answers spec §8.5 / check F5: where each translation comes from, on what terms, and what the church has to do once.

## How it works

- **Built in, offline, no setup:** KJV, WEB (World English Bible), BSB (Berean Standard Bible). All public domain, from eBible.org (`assets/bibles/SOURCE*.html`).
- **Copyrighted translations:** each church connects the publisher's free service with **its own key** (Settings → Bible → Online services). The open-source app ships no keys; keys live in `secrets.json` in the data folder, never in backups, the app state or the phone remote.
- **Offline on Sunday:** a passage added to a service is saved with the service, so it still shows if the internet drops. Fetched passages also go into a small cache (`bible-cache/<TR>.json`) that follows each provider's storage terms.
- **Imported files:** Zefania, OSIS, USX or OpenSong XML the church is licensed to use. Importing under a translation's name (e.g. `HCSB`) makes that file the offline source for it.
- **Pasted verses:** for a translation that isn't connected, Add → Bible → "Paste the verses instead". Verse numbers split it into slides.

## Readiness

| Translation | Source | Terms that matter | Cost | Status |
| --- | --- | --- | --- | --- |
| KJV | Built in | Public domain (UK Crown patent only affects printing in the UK) | Free | Ready — offline |
| WEB | Built in | Public domain; "World English Bible" is an eBible.org trademark (don't change the text) | Free | Ready — offline |
| BSB | Built in | Public domain | Free | Ready — offline |
| ESV | ESV API (api.esv.org) | Non-commercial church/ministry use; key not shared; ≤ 500 verses or half a book stored; "(ESV)" after quotes plus the notice | Free | Needs the church's key |
| NLT | NLT API (api.nlt.to), or API.Bible | Non-commercial; ≤ 500 verses without written permission, never a whole book; "(NLT)" after quotes | Free | Needs the church's key |
| CSB | API.Bible (Lifeway is a partner), or YouVersion | API.Bible Starter: 3 copyrighted Bibles, 5,000 calls/month, cache refreshed within 30 days and deleted on disconnect, FUMS usage reports | Free (Starter) | Needs the church's key |
| NIV | YouVersion Platform (accept Biblica's licence in the portal), or API.Bible | Non-commercial only; ≤ 500 verses, no whole book | Free | Needs the church's key; **availability not yet confirmed with a real key** |
| HCSB | YouVersion (version 72) **if offered**, an imported licensed file, or pasted verses | Holman allows up to 1,000 HCSB verses in any form (under 50% of the work, no whole book) with the HCSB credit line | Free | Paste works now; YouVersion availability **unconfirmed** |

HCSB was replaced by CSB in 2017. No public catalog lists it on API.Bible or the YouVersion Platform, and new licences look unlikely. It's in the YouVersion Bible app as version 72, so the quickest check is: register at platform.youversion.com, save the key in Settings → Bible, then look up a verse in HCSB. If YouVersion offers it, accept the HCSB licence in their portal and it works. If not, pasting stays within Holman's free 1,000-verse allowance. For a full offline HCSB, write to Holman Bible Publishers / Lifeway for a licensed file and import it.

## Not verified (needs a real key; nobody signed up on the church's behalf)

- Live responses from all four services. The parsers are tested against each service's documented response format, not against live text.
- NLT: a made-up key still returns text (Tyndale falls back to anonymous access), so a wrong NLT key can't be detected when it's saved.
- API.Bible: the exact catalog abbreviations (e.g. `NASB` vs `NASB2020`) and which Bibles a Starter key may pick.
- YouVersion: whether NIV (111), HCSB (72), CSB (1713) and NLT (116) are on the Platform, and the Platform's own caching terms (shown only after sign-in).
- ESV terms are written for websites. A desktop app used in church is non-commercial ministry use, but asking Crossway (crossway.org/permissions) would confirm it.

## Open item: text saved in services

A Scripture item saves its verses in `data.json`, which is why Sunday works offline, and backups copy it. With an online source (ESV especially, which allows 500 verses or half a book stored), months of saved services can add up to more than the allowed amount. The provider caches are capped; saved services are not. A bounded fix would record each item's source and, for online sources, keep text only for upcoming services, fetching it again (through the capped cache) for older ones. It also needs a rule for backups. That touches the service data model the native-operator work is changing, so it's left for after that work merges.

## Decision for the user

Spec §2/§8.5 and PROJECT_PLAN R6 say **v1 ships only with all six translations Ready**. Now that the app is free for any church, no copyrighted translation can come built in, so that gate can't be met as written. Suggested replacement: *"KJV, WEB and BSB work offline with no setup; ESV, NLT, CSB and NIV work once the church connects its own free key; HCSB by paste, imported licensed file, or YouVersion if offered."* Only the user can approve this.
