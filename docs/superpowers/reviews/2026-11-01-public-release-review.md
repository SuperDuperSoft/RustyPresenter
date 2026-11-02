# Public release review: free for every church, any computer

**Date:** November 1, 2026. **Why:** the user decided RustyPresenter is free for every church, easy for anyone, on any Windows/macOS/Linux computer (PROJECT_PLAN R9).

Produced by four research sweeps (licensing, any computer, ease of use, codebase assumptions). Every blocking and high finding was checked by a separate skeptical reviewer against the repo and primary sources; findings that did not survive were dropped, and corrected ones show the corrected text. A completeness critic added the last group. Medium and low findings were not re-verified.

Some findings repeat across sweeps; they are kept because each adds different evidence.

| Severity | Count |
| --- | --- |
| blocking | 2 |
| high | 15 |
| medium | 28 |
| low | 7 |


## Blocking

### No LICENSE file: the repo is not legally open source yet

*Area:* codebase. *Fits:* new

The design spec already picks MIT OR Apache-2.0 (docs/superpowers/specs/2026-10-29-native-slint-design.md:18) and already plans Slint, libmpv, font and third-party license compliance (lines 124-127). None of it is in the repo yet. There is no LICENSE-MIT or LICENSE-APACHE file, Cargo.toml has no `license = "MIT OR Apache-2.0"`, README has no license section, and R9 at PROJECT_PLAN.md:53 still says "No public distribution", which contradicts both the spec and the new decision. What to do: add the two license files and the Cargo field, rewrite R9 (free for all churches, public open-source releases), and add a short CONTRIBUTING.md. Also say in README that the app's code is MIT/Apache but the Slint UI dependency has its own terms. Anyone who forks and redistributes a build must meet the Slint Royalty-Free v2 conditions (AboutSlint attribution) or use Slint under GPLv3. The user's own acceptance of the Slint license does not carry over to forks. Re-choosing the license is not needed unless the user wants copyleft.

**Recommendation:** Before any public release, pick a license and add LICENSE plus `license = "..."` in Cargo.toml. MIT/Apache-2.0 or GPL-3.0 both work: the Slint RF v2 license lets apps under any license ship as long as they keep the AboutSlint attribution, and Slint is also offered under GPLv3. Rewrite R9 to match. Add a short CONTRIBUTING.md.

*Verification:* The gap is real, but the license choice has already been made. The repo has no LICENSE, COPYING or CONTRIBUTING file, tracked or untracked. Cargo.toml has no `license` field. README never names a license. PROJECT_PLAN.md:53 (R9) still says "No public distribution". The finding wrongly treats the license as an open question: docs/superpowers/specs/2026-10-29-native-slint-design.md:18 already says the source license is "MIT OR Apache-2.0" and that the user can change this before the first public release. Lines 124-127 of the same spec already cover AboutSlint attribution, LGPL libmpv, OFL fonts and a cargo-about THIRD-PARTY-LICENSES file. So what is missing is carrying out that decision, not choosing a license. It is still blocking, because without a license file nobody else may legally use or fork the code.

### The repo has no LICENSE file and Cargo.toml has no license field, so the code is legally 'all rights reserved' today

*Area:* licensing. *Fits:* new (prerequisite for Plan 4 packaging and release)

The finding is correct as written. One addition: the release packaging (Plan 4 installers) should ship the project's own LICENSE-MIT and LICENSE-APACHE alongside THIRD-PARTY-LICENSES.txt. The README should also tell people who fork the app that they must meet the Slint Royalty-Free License v2.0 attribution conditions themselves (the AboutSlint widget or a badge), or build under Slint's GPLv3 option instead.

**Recommendation:** Add LICENSE-MIT and LICENSE-APACHE, plus `license = "MIT OR Apache-2.0"` in Cargo.toml and a 'License' section in the README. In the same README section, say that the distributed binaries also contain Slint under the Slint Royalty-Free License v2.0, which is not an OSI license, and LGPL libmpv/FFmpeg. Optionally add a short CONTRIBUTING note that contributions are licensed inbound=outbound (Apache-2.0 §5). Keep MIT/Apache rather than GPLv3 only if NDI or other proprietary SDKs are a real goal (spec:18). Otherwise GPLv3 is also viable (see the Linux libmpv finding).

*Verification:* I checked the repo and could not refute the finding. Nothing tracked in git has "licen" or "copying" in its name, and there is no LICENSE file at the root. Cargo.toml lines 1-8 set name, version, edition, description and `publish = false`, but no `license` field. The README mentions no license. The only place the license choice appears is docs/superpowers/specs/2026-10-29-native-slint-design.md:18 ("MIT OR Apache-2.0 ... the user can change this before the first public release"). The packaging plan docs/superpowers/plans/2026-11-01-native-imports-recovery-packaging.md (~lines 835-859) covers only THIRD-PARTY-LICENSES.txt and the notices for dependencies; no plan task adds the project's own license. The uncommitted PROJECT_PLAN.md:53 already changes R9 to "Free for every church ... open source", but a statement in the plan does not grant anyone rights. Without a license, others cannot legally copy, fork, package or contribute, so this blocks an open-source release. Small additions: a CI or packaging step should also put LICENSE-MIT and LICENSE-APACHE into the installers next to THIRD-PARTY-LICENSES.txt. `publish = false` can stay, because it only blocks publishing to crates.io. The Slint note is accurate: the Royalty-Free License v2.0 is not OSI-approved, and anyone who forks and redistributes must meet its attribution conditions too. The README should say so.


## High

### No code signing or funding plan, so a free public build is blocked or scary on Windows and macOS

*Area:* any-computer. *Fits:* new (Plan 4 says 'after this plan'; promote it into Plan 4 Task 15/16)

The repo confirms the gap. Signing is deferred to "after this plan" (Plan 4, line 1022; native spec, line 134). Plan 4 configures ad-hoc macOS signing (line 895), and the README tells volunteers to bypass SmartScreen, open Privacy & Security, or run `xattr` (lines 915, 922, 979-980). Nobody owns the certificates, and PROJECT_PLAN R9 still says "no public distribution".

Correction 1: the SAC line in Task 17 is out of date. Since the April 2026 updates, Smart App Control can be turned back on without reinstalling (KB5127648 sometimes greys this out). SAC is also off on most upgraded or evaluation-mode PCs, so most users get a SmartScreen warning they can click through rather than a hard block.

Correction 2: unsigned builds can legally be released, so this is high, not blocking. It would become a release gate only if "easy for anyone" is treated as a hard requirement.

The recommendation stands with these specifics. Apply to SignPath Foundation first. The fallback is Azure Artifact Signing, about $9.99 a month, open to organizations in the US, Canada, the EU or the UK and to individuals only in the US or Canada. Apple Developer ID plus notarization costs $99 a year. Record who owns the signing accounts, update R9, and mark unsigned builds as preview. Drop the keychain-prompt point; keep the macOS firewall re-prompt point, which follows from the ad-hoc signature changing with each build.

**Recommendation:** Make signing a release gate, not a later step. Windows: apply to SignPath Foundation (free signing for open-source projects, signs in GitHub Actions). If that is declined, budget for Azure Artifact Signing (about $10 a month; check eligibility). macOS: an Apple Developer ID costs $99 a year and there is no free route. Notarize through cargo-packager's macOS signing/notarization config in package.yml. Decide who owns the Apple and Windows signing accounts (a church or a small foundation, not one person) and record it in PROJECT_PLAN. Until then, label builds as 'preview'.

*Verification:* I checked the repo and the core claim holds. docs/superpowers/specs/2026-10-29-native-slint-design.md:134 says the packages are "unsigned at first" and leaves signing as "a later step". The Plan 4 file docs/superpowers/plans/2026-11-01-native-imports-recovery-packaging.md sets identifier = "church.gracefirst.rustypresenter" (line 879) and signingIdentity = "-" (line 895). Line 915 tells users to click SmartScreen's "More info → Run anyway", line 922 sends them to Privacy & Security → Open Anyway, and lines 979-980 are the README text with SAC "blocks it outright" and `xattr -dr`. Line 1022 puts code signing "After this plan". PROJECT_PLAN.md never mentions signing, and R9 (line 53) still says "No public distribution". Nobody is named to own the certificates.

Two parts of the finding are wrong or overstated:
(1) SAC: it is no longer true that the only way around it is to turn SAC off for good. Since the April 2026 cumulative updates (builds 26100/26200.8116), SAC can be switched back on in Windows Security without reinstalling. KB5127648 (October 2026) greys the toggle out for some users, and Microsoft has given a workaround for that. SAC is also only on for clean installs that passed its evaluation mode, so most church laptops will see the SmartScreen warning, not a hard block.
(2) Severity: nothing legal or technical stops a public unsigned release. Many open-source apps ship unsigned with exactly these README steps. So this is "high" (most non-technical volunteers will hit scary warnings, and some will be hard-blocked by SAC), not "blocking".

The other facts check out. Azure Artifact Signing is about $9.99 a month (a secondary source; Microsoft's pricing page does not show the figure). Individuals can only use it in the US or Canada, and organizations only in the US, Canada, the EU or the UK. SignPath Foundation offers free signing for open-source projects. Apple Developer ID costs $99 a year with no free route. The point that ad-hoc signing breaks the macOS firewall "allow incoming connections" approval after each update is plausible, because the firewall keys its approval on the code signature. The keychain-prompt part is weak, since the app does not appear to use the keychain.

Sources: https://blog-en.topedia.com/2026/04/smart-app-control-in-windows-11-can-now-be-re-enabled-without-reinstalling/ ; https://www.it-connect.tech/windows-11-kb5127648-breaks-smart-app-control-in-october-2026-update/ ; https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/code-signing-options ; https://azure.microsoft.com/pricing/details/artifact-signing/

### R9 'no public distribution' and the v1 spec's Windows-only parts contradict the new decision

*Area:* any-computer. *Fits:* Plan 4 Task 17 (records)

PROJECT_PLAN.md line 6 ("this church's personal use") and R9 (line 53, "No public distribution… Same" long-term) contradict the new decision. Neither has been amended, and no dated November 1 user decision for free public release appears in §0. R1's v1 cell ("Windows build", line 45) is only stale, because §0 line 30 and the native Slint spec already moved the app to Windows, macOS and Linux. Fix it for consistency, but it is not a new requirement. The v1 spec §9.5 (line 294) still names Windows Credential Manager as the only token store, and the native spec does not override it. It needs a cross-platform store: macOS Keychain, Linux Secret Service, with a fallback when no keyring is present. Plan 4's product-question defaults 3 (no one-click LibreOffice install) and 4 (arm64-only Mac), and its .deb floor of Ubuntu 24.04+ (lines 64-70), were set for one church's hardware. Re-ask them under "any computer". Recommendation stands: rewrite R9 and the Purpose line, sync R1's v1 cell, add a dated §0 decision, amend §9.5, and re-ask Plan 4 questions 3 and 4.

**Recommendation:** Rewrite R1 and R9 in PROJECT_PLAN §1 (Windows, macOS and Linux public releases, free, open source, no paid server) and add a dated user decision under §0. Re-ask Plan 4's product questions 3 (one-click LibreOffice install) and 4 (Intel Macs) in light of 'any computer'. Note in v1 spec §9.5 that token storage is cross-platform (see the Google finding).

*Verification:* Mostly confirmed. PROJECT_PLAN.md line 6 still says the app is "for this church's personal use", and line 53 (R9) still says "No public distribution, subscriptions, or cloud hosting", with "Same" as the long-term goal. Neither the §0 decisions nor the uncommitted §10b roadmap diff records the new free-for-all-churches decision. The v1 spec §9.5 (docs/superpowers/specs/2026-10-29-rustypresenter-v1-design.md:294) still says "Windows Credential Manager". The native Slint spec only defers Google Slides changes "beyond §9.5" (line 27), so it does not replace that token store. Plan 4 (docs/superpowers/plans/2026-11-01-native-imports-recovery-packaging.md:65-70) still lists product questions 3 (no one-click LibreOffice install) and 4 (arm64-only Mac) as defaults for Jake, and line 64 limits the .deb to Debian 12 / Ubuntu 24.04+. One part is overstated: v1 being Windows-only is already superseded. §0 (line 30) records the October 29–30 decision for a native app on Windows, macOS and Linux. The native spec also overrides the v1 spec's platform sections and already mentions a "first public release" with a MIT OR Apache-2.0 license (native spec line 18). So R1's "Windows build" cell is a stale table row, not an open decision. I lowered the severity from blocking to high. These are fixes to the records, not missing product work. They still matter, because the spec wins over the plan (§0), and R9 points implementers toward single-church choices.

### Intel Macs are excluded, and the plan for adding them misses runner and Rosetta facts

*Area:* any-computer. *Fits:* Plan 4 product question 4 / Task 16

The Mac build is arm64-only (plan line 69, product question 4; lines 937 and 948). Intel Macs from 2015–2020 that can run macOS 12+ are inside the native spec's "any machine from about the last 10 years" (native spec line 19), but they cannot run the app at all. Recommendation: ship either a universal2 build or a separate x86_64 build. Build libmpv and its dependencies for x86_64 on GitHub's `macos-15-intel` runner, which is available until about fall 2027, or cross-build on arm64 with `-arch x86_64` and meson cross files. Then `lipo` them with the arm64 build, add the `x86_64-apple-darwin` Rust target, and check `minos 12.0` on both slices. Add an Intel Mac row to the Task 17 checklist. Before Intel runners are retired in fall 2027, move to cross-compiling on arm64. Also move package.yml off `macos-14` now, because that image is being retired in November 2026. Apple's Rosetta notice (full Rosetta through macOS 27) is a reason not to ship x86_64-only for Apple silicon. It is not a reason to skip the Intel build.

**Recommendation:** Ship a universal2 build: build libmpv and its dependencies twice (arm64, and x86_64 via `-arch x86_64` / meson cross files on the arm64 runner), `lipo` them together, add the `x86_64-apple-darwin` Rust target and `lipo` the app binary, and use the universal pdfium archive. Keep `minos 12.0` checks for both slices. Add an Intel Mac row to the Task 17 checklist.

*Verification:* The core claim is confirmed. In docs/superpowers/plans/2026-11-01-native-imports-recovery-packaging.md, product question 4 (line 69) defaults to "arm64 only" and says an x86_64 or universal job can be added later. Line 937 has libmpv.yml on macos-15 arm64, and line 948 has package.yml on macos-14 arm64. The minimum OS is macOS 12. docs/superpowers/specs/2026-10-29-native-slint-design.md:19 defines "any computer" as any machine from about the last 10 years running macOS 12+. That definition includes 2015–2020 Intel Macs, and an arm64-only binary cannot run on them. So the gap is real under the new "any computer, all churches" decision.

The finding's runner facts are partly wrong. GitHub retired macos-13 in December 2025, but it added a `macos-15-intel` label as the x86_64 replacement (github.blog changelog 2025-09-19). Reports say Intel macOS runners end with the macos-15 image retirement in fall 2027. So an x86_64 job does not need cross-compiling on arm64 today. It does need a plan for after fall 2027.

The repo check also turned up something the finding missed. package.yml's `macos-14` runner image is itself being retired, with brownouts from 2026-10-05 and removal around 2026-11-02 (github.blog changelog 2026-10-01). The Mac packaging job therefore needs a new runner label anyway.

### No update mechanism for a free app with no server

*Area:* any-computer. *Fits:* new

No plan, spec or code checks for updates, even though Plan 4 already publishes installers to GitHub Releases (lines 52 and 950 of the Plan 4 doc). Volunteers will keep whatever version they first installed, including the phone-remote HTTP server. Firewall: an in-place update to the same install path keeps the Windows Firewall program rule, so the installer must keep the install path fixed between versions; it does not need to recreate the rule. macOS: ad-hoc-signed builds change their code signature on every build, so each update may bring back the quarantine and TCC prompts. Fix: a non-blocking check of the GitHub releases/latest endpoint, at most once a day, with a Settings switch to turn it off and silence when offline. The check only shows a notice while nothing is live. Later, the updater can install updates in place: cargo-packager-updater, with its signing key kept as a CI secret.

**Recommendation:** Use the free path: GitHub Releases plus a static `latest.json`. Simplest first step: on start-up (and from About), fetch `https://api.github.com/repos/<org>/<repo>/releases/latest` at most once a day, without blocking. When it is newer, show 'Version X is available — Download' and open the release page; stay silent when offline. Later, cargo-packager's updater crate (`cargo-packager-updater`) can install updates in place. Its signing key must be a CI secret and must never be committed. Add a Settings toggle to turn the check off for churches that want no network calls. Never auto-update during a service (only offer it while nothing is live).

*Verification:* I checked the repo and found no update mechanism. Nothing in PROJECT_PLAN.md, the native spec or any plan mentions an update check, an updater, latest.json or releases/latest. Plan 4 (docs/superpowers/plans/2026-11-01-native-imports-recovery-packaging.md) uploads the installers to a GitHub Release (lines 52, 950). Its follow-up list (line 1022) names only code signing and a hang watchdog. So the release channel the recommendation needs already exists, but the app never checks it. Under R9 (personal use at one church) this was acceptable. For a free public release to many churches it is a real gap: volunteers will stay on whatever version they first installed. The `cargo-packager-updater` crate exists in the same CrabNebula family as cargo-packager 0.11.8, which the plan already uses. One supporting detail in the finding is overstated: a Windows Firewall program rule is tied to the exe path. An in-place NSIS update that installs to the same path keeps the rule, so this only matters if the install path changes. The macOS part holds. Ad-hoc-signed builds get a new code signature on every build, so macOS privacy permissions (TCC) can be asked for again, and each new download is quarantined again (Plan 4 line 980 documents the first-run steps). Severity stays high rather than blocking: the app can be released without an update check, but most churches would end up on outdated builds.

### Screen sleep, screensaver and system sleep are never prevented during a service

*Area:* any-computer. *Fits:* new

One correction. libmpv's own `stop-screensaver` option (on by default) may keep the screen awake while a video is playing, depending on how the video is shown. That doesn't cover lyrics, Bible verses or slides, which are on screen for most of a service. So the app still needs its own lock that stays held the whole time something is live. In Rust this could use an existing crate such as `keepawake`, which covers Windows, macOS and Linux, or the platform calls the finding lists. For Linux, prefer the org.freedesktop.portal.Inhibit call (flag 8 = idle), with org.freedesktop.ScreenSaver.Inhibit as the fallback. Add an "idle for 20+ minutes on battery and plugged in, with lyrics live" step to the Task 17 checklist.

**Recommendation:** Add `platform::awake`: on Windows `SetThreadExecutionState(ES_CONTINUOUS | ES_DISPLAY_REQUIRED | ES_SYSTEM_REQUIRED)`; on macOS an `IOPMAssertionCreateWithName(kIOPMAssertPreventUserIdleDisplaySleep)` assertion; on Linux the `org.freedesktop.ScreenSaver.Inhibit` / `org.freedesktop.portal.Inhibit` D-Bus call (Wayland and X11). Hold it while the room or stage window is open, or while anything is live. Add it to the Task 17 checklist ('leave idle for 20 minutes on battery').

*Verification:* I couldn't refute this one. I searched the whole repo except target/ and assets/: src/, experiments/, PROJECT_PLAN.md and docs/superpowers. None of them mention SetThreadExecutionState, IOPMAssertion, ES_DISPLAY, keepawake, caffeinate, screensaver, inhibit, display sleep, power plans or lock screen. Every "sleep" hit is a thread or tokio timer. The plan's per-platform checklist (Task 17 in docs/superpowers/plans/2026-11-01-native-imports-recovery-packaging.md, line 973) doesn't test leaving the machine idle either. When Windows or macOS turns the display off for idle, every attached monitor goes dark, the projector and stage outputs included. Windows 11's default display-off timeout, plugged in or on battery, is shorter than a sermon (minutes, not an hour). The gap is real and most volunteers would hit it. This isn't caused by the public-release decision, since it affects the user's own church today. It just matters more now that the target is any church on any computer.

### Bible support is hardwired to bundled KJV plus a fixed list of six English names

*Area:* codebase. *Fits:* F5 / FEAT-5 (needs scope change); Plan 3 Alt+1..6 decision

The finding is accurate. Two additions:
- src/app.rs:521 also rejects any translation not in the const list.
- The parser only accepts ':' between chapter and verse, so European references written with a comma fail even when the book name is recognised.

The release gate in spec §8.5 (all six Ready) and PROJECT_PLAN.md:38 needs an explicit decision. For a public release it cannot mean that the project itself licenses six translations.

**Recommendation:** Replace the const list with a translation registry: bundled public-domain texts (KJV, and also WEB and BSB, both public domain and modern English) plus translations installed into `<data>/bibles/`. Add an "Add a Bible" import for standard formats (USFM/OSIS/Zefania XML) and a per-install API-key entry for licensed providers. Per-translation book-name aliases would cover other languages. Make the chips and Alt+N dynamic. Reframe R6/F5 as "each church brings its own licence" rather than "the project licenses six".

*Verification:* I checked the code and the plans, and the finding holds as written.

Code:
- src/bible.rs:27 hardcodes `TRANSLATIONS = ["NIV","ESV","CSB","HCSB","NLT","KJV"]`.
- src/bible.rs:29-37 has a single static `Kjv` built from `include_str!("../assets/bibles/kjv.txt")`. The parser only reads the eBible line format (`CODE c:v text`) and uses the BOOKS codes.
- src/app.rs:284 returns `vec!["KJV"]`. src/app.rs:443-444 rejects anything other than KJV. src/app.rs:521 rejects any name not in the const list.
- src/model.rs:205 defaults to KJV.
- `find_book` and `parse_reference` (src/bible.rs:104-130) match only English names and aliases, and only accept a colon between chapter and verse, so "Juan 3:16" and "Johannes 3,16" fail.

Plans and specs:
- Spec §8.2 (docs/superpowers/specs/2026-10-29-rustypresenter-v1-design.md:229) and Plan 3 (docs/superpowers/plans/2026-11-01-native-operator.md:34,355) fix six buttons and Alt+1..6.
- Spec §8.5 (lines 248-259) and PROJECT_PLAN.md:38 and 81-92 make all six translations being Ready a v1 release gate. That gate assumes one church gets its own licences or keys.
- Nothing in the repo mentions bring-your-own key, importing a user-supplied Bible, a translation registry, or non-English books.

So the gap is real and not already handled. WEB and BSB are both public domain (BSB since April 2023), so they can be bundled. I did not re-check current terms on the web.

Severity stays high rather than blocking, because a public release could still ship with public-domain texts only. One nuance: the "all six Ready" gate in spec §8.5 and PROJECT_PLAN.md:38 would itself block a public release until it is reframed. The finding's recommendation to reframe R6/F5 covers this.

### Mac build planned as Apple-silicon only, which excludes most older church Macs

*Area:* codebase. *Fits:* Plan 4 (product question 4)

Plan 4 builds and packages the Mac version for arm64 only (lines 69, 937, 948). The spec promises macOS 12+ on machines up to about 10 years old, and that includes 2015–2020 Intel Macs, which cannot run an arm64 binary. Intel Macs make up roughly half of that window, and they are the older donated machines churches tend to have. The fix: add an x86_64 build, either a universal2 build via lipo or a separate Intel DMG. Every bundled native library needs it too: libmpv, FFmpeg and its dependencies, and pdfium, each built with MACOSX_DEPLOYMENT_TARGET=12.0. A practical CI note from my general knowledge, not checked against GitHub's docs today: GitHub has retired its Intel macos-13 runners, so either use an x86_64 runner while one still exists (e.g. macos-15-intel), or cross-compile x86_64 from an arm64 runner. Cross-compiling Rust is easy, but the C media dependencies are harder to cross-compile. Test on a real Intel Mac running macOS 12. Also state an end-of-life policy for Intel support.

**Recommendation:** Change the Plan 4 default to also build x86_64 (a separate Intel DMG, or universal2 via lipo), including the LGPL libmpv and pdfium builds in libmpv.yml/package.yml. Test on at least one Intel Mac running macOS 12.

*Verification:* This is confirmed in the repo, and nothing in the plans handles it. docs/superpowers/plans/2026-11-01-native-imports-recovery-packaging.md:69 says "Support Intel Macs? Default: no. The Mac build is arm64 only". Line 937 says the macos-15 job is "arm64 only; Intel Macs are not supported", and line 948 makes the macos-14 package job arm64 too. On the other side, specs/2026-10-29-native-slint-design.md:19 and plans/2026-10-29-native-foundation.md:17 promise macOS 12+ on machines from about the last 10 years. macOS 12 runs on Intel Macs from 2015 onward, and an arm64-only binary cannot run on an Intel Mac at all (Rosetta only translates the other way). So the current default breaks the plan's own "any computer" definition, and the new decision makes that worse. PROJECT_PLAN.md says nothing about Mac architecture. One claim is overstated: "mostly Intel Macs". Apple silicon has shipped since late 2020, so within a 10-year window about half the Macs are Intel, not most of them. Older hand-me-down church Macs are still a large share, so high severity is fair.

### No data-format migration or downgrade guard: the `format` field is written but never checked

*Area:* completeness critic. *Fits:* new (pairs with the update-mechanism finding; Plan 4 packaging)

`Data { format: u32, ... }` defaults to 1 (src/model.rs:214-228), but nothing reads it. `Store::load` (src/store.rs:39-47) just runs serde_json::from_str. Serde ignores unknown fields by default (there is no deny_unknown_fields anywhere in src/), and `save` rewrites the whole file (src/store.rs:49-52). So when an older build opens a data.json written by a newer build (a church rolls back a bad update, or two booth/office PCs run different versions on a shared or restored library), it loads without error and the next save silently deletes every field it doesn't know about. Going the other way, a newer build has no migration step for older files, and backups (store.rs:114-130) are restored the same way. When one church upgraded by hand this did not matter. A free public app updated across thousands of installs will hit it, and none of the update-mechanism findings covers it.

**Recommendation:** Treat `format` as a real schema version. Migrate step by step on load (format N to N+1, with a backup made before migrating). Refuse to open, or open read-only with a clear message, when the format is newer than the build understands. Stamp backups with the app version and format, and check both on restore. Add tests that load a fixture from each format. Make it part of the release checklist for the update mechanism.

### R9 and spec framing still say single-church, private use

*Area:* ease-of-use. *Fits:* new

Confirmed with small corrections. The purpose line is PROJECT_PLAN.md:6, not :7: "for this church's personal use during live services". R9 at PROJECT_PLAN.md:53 says "No public distribution, subscriptions, or cloud hosting". §0 has no record of the November 1 decision, and the plan never mentions open source or other churches. The v1 spec §1 (lines 11-14) assumes one church: it runs EasyWorship, has SongSelect and a CCLI license, and uses one Windows laptop. §14.2 (line 362) is titled "on the church laptop". The finding also missed two points. First, the spec scope table (around line 20) still says "Windows only for v1". The plan superseded that, but the spec is the document that controls v1 (PROJECT_PLAN §0). Second, the spec writes several items in personal terms that affect a public release: F3 "personal-use Google Cloud project with the drive.file scope" (line 41), PowerPoint export that assumes PowerPoint is installed, and EasyWorship as the only migration source. Recommendation: rewrite R9 and the purpose line, record the November 1 decision in §0, and add a requirement for non-technical users at any church on any OS. Revise spec §1, §2 and §14.2 so acceptance covers low-end machines on each OS and at least one first-time user. Keep this item as the documentation change. The work it unlocks (Google OAuth verification, licenses for the other Bible translations, packaging) are separate findings.

**Recommendation:** Rewrite R9 as "free, open-source, public release for any church" and add an R12 "usable by non-technical volunteers at any church" with acceptance criteria: install-to-first-slide in under 10 minutes without docs, tested by someone who has never seen the app. Record this as the November 1 user decision in §0. Re-run acceptance on at least one low-end laptop per OS.

*Verification:* The repo confirms every quoted claim, except that the purpose line is at line 6, not 7. Nothing in PROJECT_PLAN.md or the specs records the free, open-source, any-church decision. I lowered the severity from blocking to high: the code does not depend on this wording, and the user's November 1 decision already overrides R9. Fixing it is a quick documentation edit, not a technical barrier to release. It still matters, because the spec controls v1 and still says Windows only, one church laptop and one church's setup, so the planning documents could steer future work wrongly.

### UI is English-only, with no translation infrastructure

*Area:* ease-of-use. *Fits:* new (must shape Plan 3)

The finding is right, with three refinements. (1) The English-only design is a written rule in the imports plan (2026-11-01-native-imports-recovery-packaging.md:37, "plain English... show the core's message verbatim"), so that rule has to change, not just the strings. (2) Translating the UI strings is only half the work. The Bible feature is also tied to English: book-name and abbreviation parsing in the reference parser, display names in errors (src/bible.rs:73-90), and the English public-domain translations in the PROJECT_PLAN.md §3 table. A Spanish- or Portuguese-speaking church needs localized book names in the parser and a way to load Bibles in its own language, which arguably matters more to them than translated menus. (3) The phone remote web page (web/ and the remote server) has its own strings, so it needs the same treatment and a language choice that follows the app's setting. The recommendation still holds: use structured error enums or codes, with tests asserting the codes, plus @tr() in Slint. Do this before Plan 3 adds more strings, because that is the cheapest time.

**Recommendation:** Decide now, before Plan 3 adds hundreds of strings: wrap all Slint strings in @tr() (Slint 1.18 supports gettext and bundled translations) and change core errors to error codes or enums with arguments that the UI translates, so tests assert on codes instead of English text. Add a UI language setting (default: OS locale) and a contributor workflow for .po files (e.g. Weblate hosted free for open source). Format the stage clock and dates by locale.

*Verification:* Confirmed in the repo. A grep across the repo (excluding target/) finds no @tr(, i18n, locale, gettext or language-setting mentions. ui/*.slint (app, room, stage, theme) has no @tr calls. src/bible.rs:73-90 builds English error strings with format!, and the test at src/bible.rs:225 asserts the exact text "Jude 2 does not exist.". native-operator.md:67, :72 and :81 add more English literals and tests that check them. The imports plan (2026-11-01-native-imports-recovery-packaging.md:37) says outright that user-facing messages are "plain English and are quoted exactly in the tasks" and that the UI shows "the core's message verbatim", so English-only is a written design rule, not something the plans forgot. Nothing in PROJECT_PLAN.md or the specs covers localization. This was acceptable under R9 (personal use at one church) but does not fit "free for ALL churches". It is not blocking, because an English-only release is still possible and many target churches speak English. High is the right severity. Slint's @tr() with gettext .po files or bundled translations is a supported, documented feature in current Slint 1.x, and Weblate offers free hosting to libre projects, so the recommendation is workable.

### Bible is hard-wired to English book names, English parser and six US translations

*Area:* ease-of-use. *Fits:* FEAT-5 / F5 (extend)

Confirmed as written. One clarification: PROJECT_PLAN.md:178 already notes that the KJV source includes the deuterocanon, but the code drops those books (bible.rs:50) and no plan item says to keep them. The six-button gate is R6 (PROJECT_PLAN.md:50) together with spec §8.2 (lines 228-229).

**Recommendation:** Make the Bible a user-importable module: import common open formats (OSIS, USFX/USFM, Zefania XML, the OpenLP/OpenSong Bible XML formats) and bundle a set of public-domain non-English Bibles. Give each module its own localized book names and abbreviations for parsing and labels, and support a dynamic translation list in place of six fixed buttons. Keep deuterocanonical books when the source has them. Treat R6 as the user's own church's selection, not a global gate. Check whether NIV/ESV and others can legally ship in a public free app; it is likely bring-your-own-licence (another dimension, but it affects the UI).

*Verification:* I checked the repo and the finding holds. src/bible.rs:7 BOOKS is a fixed table of 66 English entries, and a test at bible.rs:245 asserts len()==66. bible.rs:50 throws away any book not in BOOKS (the comment says "skips the Apocrypha"). Labels come from BOOKS[..].1 (bible.rs:67), and the book lookup at bible.rs:109-111 only matches English names and abbreviations. Spec §8.2 (2026-10-29-rustypresenter-v1-design.md:228-229) lists only English reference forms and fixes six buttons, NIV/ESV/CSB/HCSB/NLT/KJV, on Alt+1..6. PROJECT_PLAN R6 (line 50) makes those six a release gate. F5 (line 77) and FEAT-5 (line 206) cover only licensing, keyword search, side-by-side display and slide breaks. Nothing in the plan or specs mentions other languages, importing Bible modules or OSIS/USFM/Zefania. PROJECT_PLAN.md:178 does say the KJV source "includes deuterocanon", but only as a parsing note; nothing decides to keep those books. Under R9 (no public distribution, line 53) that was fine. The new decision to serve all churches turns it into a real gap for any non-English congregation and for Catholic, Orthodox and Anglican users. Two small refinements. First, the licensing point is right to stay tentative: NIV, ESV, NLT and CSB cannot be bundled in a free public app without publisher permission, so bring-your-own-licence or an importer is the realistic route. That belongs in a separate finding. Second, high (not blocking) is about right: an English-only first public release is possible, but it shuts out a large share of churches worldwide.

### Song and service import limited to SongSelect .txt and EasyWorship

*Area:* ease-of-use. *Fits:* new (F4 extend; FEAT-4 song workflow)

The gap is confirmed, with a few technical details to correct:
- **ProPresenter versions:** ProPresenter 6 .pro6 files are XML. ProPresenter 7 .pro files are protobuf, and the format is not publicly documented, so a ProPresenter 7 importer depends on reverse-engineered schemas and is the riskiest one to build.
- **ChordPro file names:** ChordPro files can also end in .pro, which clashes with ProPresenter 7. The importer has to look at the file's contents to tell them apart, not just the extension.
- **OpenLP:** .osz is OpenLP's service file (a zip archive). OpenLP exports songs as OpenLyrics XML, which has an open spec. That makes OpenLyrics the best choice for both import and an open export format, and handles OpenLP and several other apps at once.
- **SongSelect:** SongSelect also offers ChordPro downloads, so a ChordPro importer adds to the SongSelect route the plan already has.
- **EasyWorship:** the planned EasyWorship import reads the EasyWorship database directly and runs on Windows only. That is fine, because EasyWorship itself only runs on Windows.
- **Roadmap fit:** add this as a new importers/exporters item (or under F4 and FEAT-4), using the staged-import pipeline from v1 spec §11 and linking it from the first-run wizard.
- **Suggested order:** OpenLyrics (import and export), ChordPro, plain text, OpenSong, ProPresenter 6, then ProPresenter 7 (best effort).

**Recommendation:** Add a FEAT for importers, ordered by likely demand: OpenLyrics XML (used by OpenLP and others, with an open spec, which also gives a standard export), ChordPro, OpenSong, ProPresenter 6/7, plain text, plus F4 EasyWorship. Run each through the existing staged-import pipeline. Offer an OpenLyrics and ChordPro export so churches are never locked in. Link the importer into the first-run wizard.

*Verification:* I checked the citations and they are accurate. PROJECT_PLAN.md:37 and :48 (R4) limit song import to SongSelect .txt plus a one-time EasyWorship songs-only import. v1 spec §9.2–9.3 say the same, and spec §16 (line 391) defers EasyWorship services and themes. The native Slint spec §9 (line 120) keeps those two imports unchanged. I searched PROJECT_PLAN.md, docs/ (excluding the .bak copies) and src/ for OpenLyrics, OpenLP, ChordPro, OpenSong, MediaShout, CSV and ProPresenter import. ProPresenter only shows up as a model for how the app should behave; none of the others appear. The §10b roadmap (FEAT-1 to FEAT-7) has no importer or export item either. FEAT-4 covers song editing and FEAT-6 covers export of the service order, not song data. So the gap is real. Under the earlier rule R9, the app was only for this church, which uses EasyWorship. Releasing it to all churches is what turns this gap into a problem. I would still rate it high rather than blocking. Most churches with a CCLI licence can re-download songs from SongSelect as .txt files, so nobody is completely stuck, only slowed down.

### R9 'personal church use, no public distribution' and the R6 'all six translations' release gate contradict the new decision

*Area:* licensing. *Fits:* F5 / FEAS-02, PROJECT_PLAN §1

R9 has already been rewritten in the working tree (PROJECT_PLAN.md:53, not yet committed), but three other places still describe the old single-church model:
- PROJECT_PLAN.md:6, the Purpose line ('this church's personal use')
- v1 spec F3 at :41 ('personal-use Google Cloud project')
- README.md:52 ('need a licensed text source')

The real conflict is R6 (PROJECT_PLAN.md:50) and spec :29/:257, which require all six translations (NIV, ESV, CSB, HCSB, NLT, KJV) to be Ready before v1 ships. A free app that anyone can download cannot bundle the five copyrighted texts. Release then waits on the user approving exceptions, which both documents allow.

Fix:
- Rewrite R6 and spec §8.5 so the release gate is that the built-in public-domain texts work offline, and the copyrighted translations are optional, each switched on by the church with its own key or licence.
- Update the Purpose line and F3 to match.
- Make TRANSLATIONS in src/bible.rs:27 data-driven.

**Recommendation:** Rewrite R9 as 'free public distribution to any church; no paid tiers or hosted services'. Change R6 so the release gate is: built-in public-domain/open texts work offline, and copyrighted translations are optional connectors each church switches on with its own key. Make TRANSLATIONS data-driven: built-in texts plus connector-backed ones.

*Verification:* Partly out of date. The R9 half is already handled. The uncommitted PROJECT_PLAN.md (git status shows M) has rewritten R9 at :53 to 'Free for every church ... Free, open source, no subscriptions or cloud hosting ... Any church, any Windows/macOS/Linux computer'. So the quoted text 'No public distribution...' is no longer in the file. The R6 half still stands. PROJECT_PLAN.md:50 still says 'All six Ready for release'. The v1 spec still says 'v1 ships only with all six Ready' (docs/superpowers/specs/2026-10-29-rustypresenter-v1-design.md:29, :257). src/bible.rs:27 still hard-codes the six names. Spec F3 (:41) still assumes a 'personal-use Google Cloud project'. PROJECT_PLAN.md:6 still says 'Purpose: ... for this church's personal use', which is stale next to the new R9. The gate does not literally make release impossible: R6 and spec §8.5 both let the user approve an exception. That lowers the severity from blocking to high. The release still needs that recorded decision, or a rewritten gate.

### Copyrighted Bibles (NIV/ESV/NLT/CSB/HCSB): the app can't embed an API key, and caching limits break offline use and keyword search

*Area:* licensing. *Fits:* F5 / FEAS-02, FEAT-5

Change consequence (2): offline use is limited to a small pre-fetched cache (ESV at most 500 verses or half a book; API.Bible refreshed within 30 days, recommended 14 days, fewer than 500 consecutive verses), and full-text search over NIV/ESV/NLT must use each provider's online search endpoint, not a local index. So FEAT-5 keyword search can still work online but not offline. Add that the ESV API terms are written for websites; ask Crossway whether a desktop app needs a separate licence. Mark API.Bible's Starter call limit as unconfirmed (5,000 per month vs per day; API.Bible's own pages disagree). Also note that R6 (PROJECT_PLAN.md:50) makes all six translations a release gate, so the user must approve an exception or this becomes blocking.

**Recommendation:** Design F5 as per-church connectors: a Settings → Bible screen with a 'Get a free key' guided flow per provider, keys stored locally, never in the repo. Respect each provider's cache cap (ESV ≤500 verses; API.Bible refresh ≤30 days) and pre-fetch only the verses in this Sunday's service so it still works offline. Limit keyword search to built-in texts. Show each provider's required notice in About and keep the '(ESV)'/'(NIV)' tag already in slides.rs:109. Send API.Bible FUMS events from the native app if the terms require it. Mark HCSB 'not available' unless Lifeway grants it. Before release, email Crossway, Biblica/API.Bible and Tyndale to confirm that a free open-source app whose users bring their own keys is acceptable.

*Verification:* The repo does not cover this yet. PROJECT_PLAN.md:86-90 lists only 'candidate' sources marked 'Needs setup'. Line 18 bans committing API keys, but nothing addresses the per-church key flow, cache caps, FUMS, or attribution. README.md:52 and spec §8.5 defer the whole topic to F5. The primary sources mostly confirm the finding. ESV (api.esv.org): non-commercial only; 'You may not sell, share, or publish your access key'; at most 500 verses or half a book per query, displayed, and stored locally; a link to esv.org and a copyright notice are required. NLT (api.nlt.to): anonymous 50 verses per request and 500 requests per day; with a key 500 verses and 5,000 per day; non-commercial. API.Bible (care.api.bible articles 378 and 396, docs.api.bible): Starter is non-commercial with up to 3 licensed Bibles; caches must be refreshed within 30 days (the FAQ recommends 14 days or less and asks for fewer than 500 consecutive verses); FUMS is required for web apps; no overage, calls stop at the cap. The monthly call limit is stated inconsistently: some API.Bible pages say 5,000 per month, others 5,000 per day. Two parts are overstated. (1) Keyword search is not banned outright. ESV (/v3/passage/search) and API.Bible (/search) both offer server-side search, so online search over these translations works. What the caps rule out is offline full-text search over a locally stored copy. (2) The ESV terms are written for a 'website'. Whether they cover a desktop app at all is unclear, and Crossway may want a separate app licence. That supports the recommendation to contact the publishers. Severity: R6 (PROJECT_PLAN.md:50) makes all six translations a release gate unless the user approves an exception, so this is arguably blocking under the plan. High fits if exceptions are allowed.

### Ship modern free translations so a church works with zero setup

*Area:* licensing. *Fits:* F5, FEAT-5

Three corrections to the finding:

1. **WEB name:** the text of the World English Bible is public domain, but "World English Bible" is a trademark. The app may only use that name for the unmodified text. This should be recorded in the WEB SOURCE notice.
2. **NET:** its free-use terms are conditional and are written mainly for print and non-commercial use. It should not be bundled until its terms are checked. WEB plus BSB is enough.
3. **Code work:** bundling the texts is not enough. `src/bible.rs` hard-codes a single `Kjv` struct, a `OnceLock` and `include_str!`, plus a fixed six-entry `TRANSLATIONS` list. To add translations, it needs a general per-translation loader, the alias/picker entries (`PickTranslation`), and a setting for the default translation. R6 and the §3 table in PROJECT_PLAN.md also need a row for "bundled free translations" alongside the six licensed ones.

Each new text adds about 5 MB to the binary, because it is embedded with `include_str!`. On low-end machines it may be better to load the texts from the data folder instead. The UK projection remark is a reasonable reading, but it is not legal advice and should stay in the docs as a note only.

**Recommendation:** Bundle at least WEB and BSB from eBible.org. Keep each one's SOURCE/notice next to the text and list them in THIRD-PARTY-LICENSES. Pick one as the default translation for new installs.

*Verification:* The repo confirms the gap. assets/bibles/ holds only kjv.txt and SOURCE.html. src/bible.rs:27 hard-codes TRANSLATIONS = ["NIV","ESV","CSB","HCSB","NLT","KJV"], and line 2 says the other translations "need a licensed source" and report "not set up". The readiness table in PROJECT_PLAN.md §3 (lines 83-90) and R6 (line 50) cover only those six, and none of the plans or specs mention WEB, BSB, ASV, LSV or a default translation. So the plan only offers the five licensed translations, which need per-church setup, plus the 1769 KJV. Nothing gives a church a modern translation with zero setup. That is a real gap for "easy for anyone".

The licensing facts hold. WEB is public domain. BSB has been dedicated to the public domain since April 2023. ASV 1901 is public domain. LSV is CC BY-SA 4.0. eBible.org distributes all of them in the same VPL format the existing KJV parser already reads. SOURCE.html contains the UK Crown letters-patent note on the KJV. Three corrections are needed (see corrected_detail).


## Medium

### The OpenGL-to-software fallback misses GL failures at window creation, and volunteers cannot switch it themselves

*Area:* any-computer. *Fits:* Plan 4 (Task 10 supervisor / Task 17 checklist); new setting

mpv GL failure already falls back per player (src/video/surface.rs:274-277), and a missing rendering notifier already falls back to software (surface.rs:166-170). What is missing is a fallback when Slint's FemtoVG/OpenGL window cannot be created. choose_backend fixes the renderer for the whole process after select() succeeds, but the GL surface is only made at window show. There, room.rs:342 and stage.rs:88 return an error that screens.rs only logs, and the planned operator window shares the same renderer. The only way out is the RP_FORCE_SOFTWARE env var, and the Plan 3 About tab and Plan 4 Video tab only display the mode. Fix: (1) add a persisted graphics=auto|software setting in the data folder, read before choose_backend. (2) If the first window's show() fails in OpenGL mode, write 'software' and exit with a dedicated code; the Plan 4 supervisor relaunches in software mode with a notice. (3) Add a 'Use software drawing (for older computers)' checkbox in Settings. (4) Add a manual check on the Basic Display Adapter or a VM. Severity is medium: it hits machines without proper GPU drivers, VMs, RDP sessions and some ARM laptops, not most church PCs.

**Recommendation:** Add a persisted `graphics: auto|software` setting in the data folder, read before `choose_backend`. When the room window or mpv GL init fails, write 'software', and have the supervisor (Plan 4 Task 10) relaunch with `--restore` in software mode, with a notice to the operator. Add a Settings → About 'Use software drawing (for older computers)' checkbox. Add a test for the GL-failure path (fake a failed GL context), plus a manual check on a VM or the Basic Display Adapter.

*Verification:* The main point holds. choose_backend (src/video/surface.rs:25-41) decides only on whether BackendSelector::select() for "femtovg" succeeds. With an explicitly named renderer, Slint builds the GL surface later, when a window is created or shown. So a broken or too-old OpenGL surfaces as an error from ui.show() (src/ui/room.rs:342 "Could not open the room screen", src/ui/stage.rs:88), and src/ui/screens.rs:105 only logs it. Slint's platform and renderer are set once per process, so nothing can drop back to software without a relaunch. The operator window planned for Plan 3 runs in the same process and would fail the same way. The only override is RP_FORCE_SOFTWARE=1 (surface.rs:28; Plan 2 line 18; Plan 4 checklist line 1013). Plan 3's About tab and Plan 4's Video tab only display the mode. Neither has a toggle, and the Plan 4 supervisor (Task 10) has no graphics-mode retry. Two parts of the finding are wrong. First, mpv GL render-context failure is already handled per player: surface.rs:274-277 catches a create_gl error and falls back to create_sw. surface.rs:166-170 also falls back to software when the rendering notifier is missing. So the unhandled case is Slint's own FemtoVG/GL window creation, not mpv. Second, 'high / most churches would hit it' overstates it. Most church PCs, including Intel HD 2000/3000 with vendor drivers, have the GL 2/3-level support FemtoVG needs. The failure is mainly the Basic Display Adapter with no driver, some VMs and RDP sessions, very old GMA chips, and some Windows-on-ARM setups. That matters for the 'any computer' goal, but it is not the common case, so medium.

### Google Slides OAuth as a public open-source client needs Google brand verification, an owned project and cross-platform token storage

*Area:* any-computer. *Fits:* F3 / v1 spec §9.5

Narrow the finding to what §9.5/F3 lacks for a public release. (1) Publish the consent screen "In production". Testing status gives 7-day refresh tokens and a test-user cap. (2) Own the Google Cloud project through a church or foundation account and complete brand verification: homepage and privacy policy on a domain verified in Search Console. This is not for clearing an unverified-app warning, which `drive.file` (non-sensitive) does not trigger. It is for showing the app's identity and logo and meeting Google's policy for public apps. (3) A shared client secret and a Picker API key will be in the open-source code. Restrict the key to the Picker API and to localhost referrers, and allow a user-supplied client ID. (4) Replace the Windows-only Credential Manager with the cross-platform `keyring` crate, with a fallback when Linux has no Secret Service. Drop the "ship the PDF/PPTX download path first" item: Plan 4 Task 6 (plans/2026-11-01-native-imports-recovery-packaging.md:473) already does that.

**Recommendation:** Before building §9.5: create the Google Cloud project under a church or foundation account (not a personal Gmail), publish a privacy-policy and homepage page (for example a GitHub Pages site on a verified domain), submit brand verification, restrict the API key, and let advanced users supply their own client ID through Settings or an env var. Use the `keyring` crate, with an encrypted-file fallback when no Secret Service is present. Ship the no-OAuth path first: 'In Google Slides choose File → Download → PDF, then import the PDF'. PDF import already exists (Plan 4 product question 1) and works offline with no Google project at all.

*Verification:* The core of the finding holds. v1 spec §9.5 (docs/superpowers/specs/2026-10-29-rustypresenter-v1-design.md:290-296) assumes one church's OAuth client and stores the refresh token only in "Windows Credential Manager" (line 294). It says nothing about the Google Cloud project's owner, publishing status, brand verification, a privacy policy or homepage, the shipped client secret, or a Picker API key. PROJECT_PLAN.md F3 (line 75) covers only "token lifetime with drive.file". These facts check out against Google's docs. Testing-status refresh tokens expire after 7 days and only listed test users can sign in. Brand verification (homepage, privacy policy, verified domain) applies to public apps, and the "verification not needed" exemptions are personal use under 100 users, internal or Workspace use, and testing (support.google.com/cloud/answer/13464323). The Picker needs a developer key and an app ID.

Two parts are overstated or already handled:
(a) The unverified-app warning is tied to sensitive and restricted scopes (support.google.com/cloud/answer/7454865). `drive.file` is non-sensitive, so skipping brand verification mainly means no logo, a less trusted consent screen and possible policy enforcement, not the warning screen.
(b) The "ship the no-OAuth path first" recommendation is already in the plan. Plan 4 (docs/superpowers/plans/2026-11-01-native-imports-recovery-packaging.md:473) changes the Slides tab note to "in Google Slides choose File → Download → PowerPoint (.pptx) or PDF, then import". Plan 4 also lists §9.5 as still not built (line 992), and the Plan 4 deck import covers PDF on every platform.

Because churches already have a working Google Slides path without OAuth, this is not "high". Most churches would not hit it until the in-app Google feature is built, so the severity is medium.

### No PowerPoint import without PowerPoint or LibreOffice, even though Macs already have Keynote and every app can export PDF

*Area:* any-computer. *Fits:* Plan 4 Task 6c / Task 1

Plan 4 Task 1 `powerpoint_installed()` is false off Windows, and Task 6c falls back to an install hint for LibreOffice, a download of roughly 350 MB that a volunteer must find, approve and install. Every Mac ships with Keynote, which opens .pptx and can export a PDF through AppleScript (`osascript`), and PowerPoint for Mac can do the same. The 'no converter' message also never mentions the zero-install route: save as PDF from PowerPoint, Keynote or Google Slides, then import the PDF, which pdfium (bundled) already handles. Separately, Linux LibreOffice from Flatpak is unsupported (Task 1: 'Flatpak is not supported') and Snap is refused. On Fedora Silverblue, Steam Deck or a Flathub-only setup, LibreOffice is often only available as a Flatpak.

**Recommendation:** (a) Change the no-converter message to offer 'Save your presentation as PDF and import that' with a button that opens the PDF picker. (b) Add a macOS Keynote/PowerPoint-for-Mac export route through `osascript` (it triggers the macOS Automation 'control Keynote' consent prompt once, so add NSAppleEventsUsageDescription to Info.plist). (c) Optionally support `flatpak run --filesystem=<work dir> org.libreoffice.LibreOffice --headless ...` on Linux.

### Linux packages leave out common distros, and Wayland/macOS fullscreen behaviour is untested and has known traps

*Area:* any-computer. *Fits:* Plan 4 Task 15 / Task 17 checklist, C2

Packaging: the `.deb` needs `libmpv2` (Debian 12 / Ubuntu 24.04+), so Linux Mint 21.x and Ubuntu 22.04 users (very common on donated laptops) can only use the AppImage, which needs FUSE 2 (libfuse2 / libfuse2t64 is not installed by default on Ubuntu 24.04), has no menu entry without extra tools, and has nothing for Fedora/openSUSE (no rpm). No Flatpak/Flathub, which is the easiest route on Mint, Fedora and Steam Deck software stores. If linuxdeploy bundles libva/libdrm into the AppImage with libmpv, they can mismatch the host's VA-API drivers. Fullscreen: src/platform/monitors.rs:99-110 first places the window with `set_position`, which Wayland ignores, and winit returns `None` for `primary_monitor()` on Wayland (used by `primary()`, line 92). macOS `Fullscreen::Borderless(Some(m))` puts the room window in its own Space, and with 'Displays have separate Spaces' off it can blank the operator's display or hide the operator window. C2 is 'Not run' on macOS and Linux (checks doc rows 13–14).

**Recommendation:** Add a Flatpak manifest (org.freedesktop.Platform plus a bundled LGPL libmpv; `--socket=wayland --socket=x11 --share=network --device=dri`, with LibreOffice reached through `flatpak-spawn --host` or the Flatpak LibreOffice). Keep the AppImage, but set `--appimage-extract-and-run` guidance and exclude libva and libdrm from bundling. Add an rpm through cargo-packager if it is supported. In C2 for Linux, test GNOME Wayland, KDE Wayland and X11 with two monitors, and add an 'Use X11 (XWayland)' escape hatch (WAYLAND_DISPLAY unset / winit backend env) set from Settings. On macOS, test with 'Displays have separate Spaces' both on and off, and consider winit's macOS simple fullscreen or a borderless window sized to the screen at screen-saver level for the room.

### 4K and HiDPI room outputs and low-RAM machines are not sized or tested

*Area:* any-computer. *Fits:* Plan 4 Task 17 checklist; new setting

The room renders slides at the monitor's full physical size (src/ui/room.rs:132-135, `m.size`). A 4K projector or TV gives 3840×2160 RGBA, about 33 MB per slide, so the 256 MB render cache (src/render/cache.rs:21) holds only about 7 slides, alongside double-buffered layers, a 4K video and mpv's buffers. On a 4 GB laptop that risks swapping during a service. In software mode, every video frame is copied through a CPU buffer. C1 measured 65–77% of one core at 1600×900 on a fast PC (checks doc row 10), and at 4K that is about 5.8 times the pixels, likely too slow on the old hardware that falls back to software. Imported slide images are capped at 1920 wide (Plan 4 'Slide image size'), so decks look soft on 4K. None of this is in the Task 17 checklist.

**Recommendation:** Cap the room's render size at 1920×1080 (scale up on the GPU) by default, with an 'Output resolution: Auto / 1080p / 4K' setting. In software mode, always render video and slides at 1080p or less. Scale the cache cap to available RAM (for example 1/16 of physical RAM, minimum 64 MB). Add checklist rows: a 4K room output, 4 GB of RAM, an old Intel iGPU in software mode, and the operator on a 150–200% HiDPI screen.

### The default theme font is Segoe UI, which exists only on Windows

*Area:* any-computer. *Fits:* FEAT-1 (themes) / new small fix

src/model.rs:170 sets the default `Theme.font` to "Segoe UI" (also src/render/layout.rs:24 DEFAULT_FAMILY). On macOS and Linux it falls back to another face, so lyrics wrap differently. A service or backup made on a Windows PC and run on a Mac or Linux laptop changes its line breaks, and Plan 3's 'missing font' note will show on every new Mac or Linux install. Noto Sans is already bundled (layout.rs BUNDLED_FAMILY) for exactly this reason.

**Recommendation:** Make the default for new data folders `Noto Sans` (bundled), keep existing saved themes as they are, and show 'This font is not installed on this computer — using Noto Sans' only for fonts the user chose.

### Windows: standard-user accounts, ARM64 and runtime prerequisites are not addressed

*Area:* any-computer. *Fits:* Plan 4 Task 15–17

(1) The firewall prompt (Plan 4 Task 17 README) needs an administrator to allow a per-user exe (installMode currentUser installs under %LOCALAPPDATA%\Programs). Church PCs often run a standard volunteer account, so the phone remote silently fails to connect. (2) Windows on ARM (Snapdragon X laptops, increasingly sold) only gets an x64 build running under emulation. libmpv and FemtoVG then need OpenGL, which on Qualcomm comes from the 'OpenCL, OpenGL and Vulkan Compatibility Pack' (GL on D3D12) and may be missing, which hits the GL-failure gap above. Not in any matrix. (3) Windows 10 is still the most common OS on church laptops; MSYS2 UCRT64 libmpv and windows-gnu builds must not pull in DLLs that are missing there (libgcc, libwinpthread, libstdc++), and nothing tests on Windows 10.

**Recommendation:** (1) Add an optional 'Allow phone remote through the firewall' step to the NSIS installer (an admin-elevated `netsh advfirewall firewall add rule` scoped to private profiles). Have the app detect when no phone can reach it and explain the admin step. (2) Add an aarch64-pc-windows-msvc build, or at least a Windows-on-ARM checklist row, and document the compatibility pack. (3) Add a Windows 10 22H2 VM row to the Task 17 checklist, and a CI `ldd`/`dumpbin /dependents` check that only system DLLs are imported.

### macOS privacy prompts and Info.plist entries are not planned

*Area:* any-computer. *Fits:* Plan 4 Task 15 / Task 17

Packager.toml (Plan 4 Task 15) has no Info.plist additions. The app runs an HTTP server for the phone remote (macOS Application Firewall prompt; macOS 15 Local Network privacy applies to some local-network traffic, which needs an `NSLocalNetworkUsageDescription`). It reads media from Desktop, Documents and Downloads, external drives and network shares (TCC folder prompts on first access when a path is restored from a service file rather than chosen in a picker). If a Keynote route is added, it needs `NSAppleEventsUsageDescription`. Missing usage strings cause silent denial or a crash on newer macOS. None of this is in the Task 17 macOS column.

**Recommendation:** Add an Info.plist fragment (cargo-packager `macos.infoPlistPath`) with NSLocalNetworkUsageDescription and NSAppleEventsUsageDescription where needed. Add a macOS 15 checklist row: pair a phone on a fresh Mac, play a video from ~/Documents through a saved service, and play from a USB drive. With a stable Developer ID signature (see the signing finding), these prompts appear once instead of after every update.

### No localization layer: every UI string, message and parser keyword is English literals

*Area:* codebase. *Fits:* new (affects Plan 3 constraints and Plan 4 message rule)

Nothing in the plans, specs or code covers localization. Plan 3 is about to copy English web strings into .slint files as literals. Plan 4 (imports-recovery-packaging.md:37) requires messages in plain English, quoted exactly. Core errors are English format! strings (for example bible.rs:73-93). The stage clock is 12-hour only (ui/stage.rs:51). Bare section headings are recognized only from an English word list (slides.rs:13-14), although bracketed headings like [Coro] already work (slides.rs:10). The footer hard-codes "CCLI Song #" and "CCLI License #" (slides.rs:96-97). The Bible reference parser and all six required translations are English too, so a non-English church gets little Bible value without more data. This should not block an English-first public release. Retrofitting gets expensive after Plan 3, though. Recommendation: wrap .slint strings in Slint `@tr()` during Plan 3 and route core messages through one catalog, at almost no extra cost. Add a 12/24-hour clock setting, and make section keywords extensible. Ship English only.

**Recommendation:** Before Plan 3 writes the operator UI, adopt Slint's built-in `@tr()` (gettext) for .slint text and one Rust message catalog (e.g. a `tr!` over gettext or fluent) for core errors. Make section keywords and the 12/24-hour clock locale- or setting-driven. Ship English first, but make translation possible for volunteer contributors. Also send the phone remote page (web/remote.*) through the same catalog.

*Verification:* Confirmed that nothing handles localization. A search of PROJECT_PLAN.md, the specs, the plans and src finds no i18n, @tr, gettext or fluent. Everywhere "translation" appears, it means Bible translations. The cited evidence checks out. Plan 4 says messages "are plain English and are quoted exactly" (2026-11-01-native-imports-recovery-packaging.md:37). The stage clock uses `%-I:%M %p` (src/ui/stage.rs:51). The section keyword list is English-only (src/slides.rs:13-14). The lyric footer hard-codes "CCLI Song #" and "CCLI License #" (src/slides.rs:96-97). Two parts are overstated. First, slides.rs:10 accepts any bracketed heading, so `[Coro]` already works; only bare headings such as "Coro" or "Strophe" are shown as lyrics. Second, the severity. The release target is "free for all churches", but English-speaking churches can use an English-only first release, so most churches would not hit this. It is not blocking. The real cost is retrofitting: Plan 3 is about to write every .slint string as a literal. That makes it medium, with a cheap fix if adopted before Plan 3. Slint's built-in `@tr()` with gettext/bundled translations is the right tool.

### Default font is Segoe UI everywhere, so every Mac/Linux install starts in fallback with a warning

*Area:* codebase. *Fits:* Plan 3 (Settings → Look, Theme global) + model default

model.rs:170 `Theme::default()` uses font "Segoe UI", and render/layout.rs:24 `DEFAULT_FAMILY = "Segoe UI"`. Plan 1 accepted that this falls back to Noto Sans on macOS/Linux. But Plan 3 (native-operator.md:626) then shows "“Segoe UI” isn't installed on this computer, so slides use Noto Sans" under the font box, so every fresh non-Windows install warns about its own default. Slides also look different on each OS, which goes against the native spec §4 promise of "looks the same everywhere". The Settings font list Plan 3 copies (operator.js:876, 20 fonts) is mostly Windows/Office fonts (Segoe UI x3, Bahnschrift, Calibri, Candara, Corbel, Franklin Gothic, Gill Sans MT, Century Gothic). Montserrat/Open Sans/Lato/Roboto are listed but not bundled. Plan 3's operator Theme uses `font: "Segoe UI Variable Text"` (native-operator.md:163). On Linux, Slint falls back to the fontconfig default for that, not Noto, unless Noto is registered with Slint. Only Latin Noto Sans is bundled (assets/fonts), so CJK/Arabic/Devanagari depend on system fonts.

**Recommendation:** Make the default theme font the bundled "Noto Sans" (existing data.json files keep their saved value). Fill the font ComboBox from the installed families (cosmic-text db) plus bundled faces, not the Windows list. Register the bundled font with Slint for the operator UI so it looks the same on every OS. Consider bundling or downloading Noto CJK/Arabic on demand.

### Fixed port 4318 is also the single-instance lock, and it clashes with OpenTelemetry's standard port

*Area:* codebase. *Fits:* Plan 4 Task 11 (bind/supervisor)

startup.rs:24 binds 0.0.0.0:4318; the only override is the env var RUSTYPRESENTER_PORT, which volunteers cannot set. If anything else holds the port, startup.rs:18 fails with "Port 4318 is in use by another program, so RustyPresenter cannot start.", and Plan 4 makes this a hard exit with no restart (PORT_TAKEN, imports-recovery-packaging.md:45 and Review Focus 14). 4318 is the IANA/OpenTelemetry OTLP-HTTP default, so OTel collectors and agents (Grafana Alloy etc.) use it, and those are increasingly installed on donated or IT-managed machines. The same port is also how the app detects a second launch, so the instance lock and the phone server cannot be separated. Firewall prompting is already covered by Plan 4 (:982).

**Recommendation:** Separate single-instance detection (a lock file or named pipe/Unix socket in the data dir) from the HTTP port. Pick a less common default, and if it is taken, try the next free port automatically, save it in Settings and show it in the phone-pairing QR, so the app never refuses to start because of the port.

### The library is per user account and per computer: shared church PCs and prepare-at-home workflows are not handled

*Area:* completeness critic. *Fits:* new (could become a FEAT item; touches Plan 3 settings and Plan 4 packaging)

The data folder is `dirs::data_local_dir()/RustyPresenter` (src/server.rs:458-459), which is %LOCALAPPDATA% on Windows, ~/Library/Application Support on macOS and ~/.local/share on Linux. Many churches give each volunteer their own Windows/macOS login on the booth PC. Each of them then opens an empty library with no songs, services or settings. Another common setup has the worship leader building the service on a home or office laptop and running it on the booth PC, but the only way to move data is a full backup restore that replaces the whole library (v1 spec §11, 'keeps a copy of the data being replaced'). The only override is the RUSTYPRESENTER_DATA environment variable, which a volunteer can't be expected to set. The plan and specs don't mention multiple accounts, a portable/USB mode, or merging individual services or songs between computers. The 'one operator on one laptop' finding is about team size, not where the data lives.

**Recommendation:** Add a library-location setting in the UI: per user (default), shared for all users on this computer (ProgramData on Windows, /Users/Shared on macOS, with permissions set at install time), or a chosen folder or USB drive (a portable mode, with a lock file so two instances can't write at once). Add 'Export service' / 'Import service' (a service plus its songs and assets as one file, merged by ID/hash rather than replacing the library) so a service can move from a prep machine to the booth. Say in the docs that the folder must not be a OneDrive/Dropbox-synced live folder (the plan already avoids OneDrive for the dev copy, v1 spec §11 line 309).

### The whole library is one JSON file rewritten on every save; spec says SQLite

*Area:* completeness critic. *Fits:* new (Plan 4 / low-end hardware acceptance)

All songs, media, decks and services live in a single data.json, serialized with to_vec_pretty and rewritten atomically on every change (src/store.rs:49-52; App::save, src/app.rs:201-208). The v1 spec still says 'SQLite database in the local app data folder' (v1 spec line 309), so spec and code disagree. One church's library is small. A church importing an EasyWorship or SongSelect library (thousands of songs, many services of history) onto an old laptop with a spinning disk or eMMC storage would rewrite megabytes of JSON on every edit, and memory grows with the library. None of the low-end-hardware or 4K/low-RAM findings sizes data volume.

**Recommendation:** Set a target library size for the acceptance criteria (for example 5,000 songs and 2 years of services) and measure save latency on a low-end machine. Then either move to SQLite as the spec says (WAL, per-row updates; also makes migrations easier) or split data.json into per-entity files. In both cases, update the spec so it matches the code.

### No first-run setup wizard or sample content

*Area:* ease-of-use. *Fits:* new (touches Plan 3 operator, FEAT-6 practice mode)

Onboarding today is an empty-state Welcome panel with 4 steps, planned in Plan 3 (native-operator.md:271-272, ported from web/operator.js:199-209). Plan 4 Task 17 already plans a per-OS README rewrite for installers. Still missing: (a) the spec §10 Displays "Identify", which Plan 3 explicitly defers (native-operator.md:39). For volunteers on unfamiliar hardware it should be built, along with a test pattern, and the Welcome panel's step 1 should use it. On macOS/Linux, the Windows-only "Win+P" wording must be replaced with per-OS 'extend display' hints. (b) There is no sample/demo content. Bundle a small public-domain demo service, e.g. 2-3 PD hymns, a KJV verse and a CC0 background, so FEAT-6 practice mode has something to run. (c) Optionally, a church name/logo step and a pointer to song import in the Welcome steps. A full multi-page wizard is optional. Extending the existing Welcome panel into a re-runnable checklist covers most of the value.

**Recommendation:** Add a first-run wizard to Plan 3 or a new FEAT: (1) welcome and language, (2) pick the room screen using Identify and a test pattern, plus optional stage, (3) church name, logo, optional licence numbers, (4) import songs from the other app (see the portability finding), (5) a demo service with 3 public-domain hymns, KJV John 3:16, a CC0 background and a short video, so volunteers can practise with Next and Clear right away. Make the wizard re-runnable from Settings. Rewrite the README for installer users once Plan 4 packaging lands.

*Verification:* Partly right. There is no first-run wizard and no sample content anywhere in PROJECT_PLAN.md, the specs or Plans 3/4. But the finding overstates the gap in three ways. (1) A new install does not open to a blank window. Plan 3 (docs/superpowers/plans/2026-11-01-native-operator.md:251, 271-272, 289) ports the web operator's empty-service state and its Welcome panel. That panel gives the same 4 steps as the README: Screens, Add to service, Bible box, Next (web/operator.js:199-209). (2) The README rewrite for installer users is already planned. Plan 4 Task 17 (native-imports-recovery-packaging.md:976-984, 1000-1001) covers install and first launch per OS, SmartScreen, macOS Open Anyway, Linux chmod and the Firewall prompt. (3) Displays "Identify" is already specified in v1 spec §10 (2026-10-29-rustypresenter-v1-design.md:304). Plan 3 (native-operator.md:39) explicitly leaves it unbuilt, so the real gap is that Identify was deferred, not that nobody thought of it. What still holds: there is no guided screen setup with a test pattern, no church name/logo step, no step that points users to song import, and no public-domain demo service to practise on. FEAT-6 practice mode (PROJECT_PLAN.md:207) has no content to practise with. The current README (README.md:7, 17) is also Windows-only ("Win + P", RustyPresenter.cmd builds from source), but Task 17 is set to replace it. With the Welcome panel and the planned README, most churches can still get started, so this is a usability polish item, not something most churches would get stuck on: medium.

### Default theme font Segoe UI is Windows-only; bundled fallback covers Latin, Greek and Cyrillic only

*Area:* ease-of-use. *Fits:* new (render) / FEAT-1 looks

The default theme font "Segoe UI" (model.rs:170, layout.rs:24) and the operator UI fonts (Segoe UI Variable and Consolas, Plan 3 :163/:167) exist only on Windows. On macOS and Linux the default theme always falls back to Noto Sans. The planned note "font isn't installed" (Plan 3 :626) would then appear on every non-Windows machine with the default theme. The fix is to make the bundled "Noto Sans" the default for new data in both places, and to map "Segoe UI" to the platform's default font without showing a warning. Non-Latin scripts work on Windows and macOS through the system fallback in Fonts::new (production already uses the OS locale; the en-US setting is test-only). Minimal Linux installs may have no CJK or Arabic fonts. Bundling Noto CJK, Arabic, Hebrew or Devanagari, or offering them as optional downloads, is worthwhile but not urgent. A concrete gap to add: scripture uses absolute Align::Left (layout.rs:148) and the reference uses absolute Align::Right (layout.rs:128). Hebrew and Arabic Bibles would sit on the wrong side, and no RTL tests exist. The alignment should follow the paragraph's text direction, and reference-image tests should cover RTL and CJK line breaking.

**Recommendation:** Change the default to the bundled 'Noto Sans' (layout.rs and model.rs together), and keep Segoe UI only as a migration value for existing data. Bundle or offer as an optional download Noto Sans CJK, Arabic, Hebrew and Devanagari, and add them as explicit fallbacks in Fonts::new rather than relying on the OS. Pass the UI or content locale to FontSystem so CJK Han variants are chosen correctly (it is fixed to en-US in bundled_only). Add reference-image tests for Arabic or Hebrew (RTL, alignment) and Chinese/Japanese/Korean line breaking. Apply the same fallback check to the Slint operator UI (CopyText uses 'Consolas', Plan 3 :167, which is also Windows-only).

*Verification:* The core facts check out in the repo. src/model.rs:170 sets the default font to "Segoe UI", and src/render/layout.rs:24 has DEFAULT_FAMILY = "Segoe UI". layout.rs:117 sets font_fallback whenever the family is missing. assets/fonts holds only NotoSans Regular, Medium and SemiBold. Other scripts depend on the system fallback in Fonts::new (layout.rs:41-45). Plan 3 (docs/superpowers/plans/2026-11-01-native-operator.md:626) adds a note when the font is missing: "<font> isn't installed on this computer, so slides use Noto Sans". With the current default, every Mac and Linux user would see that note on the default theme, which would confuse non-technical volunteers. The native-foundation plan (:27) treats the Segoe UI fallback only as a no-panic case. It never reconsiders the default for cross-platform users. Plan 3 :163 also defaults the operator UI theme to "Segoe UI Variable", and :167 uses Consolas, both Windows-only. There are no RTL, Arabic or Hebrew tests in src/render. Scripture is hard-coded to Align::Left (layout.rs:148) and the reference to Align::Right (layout.rs:128). These are absolute alignments, so Hebrew or Arabic scripture would sit on the wrong side. That RTL problem is real and the finding does not mention it. Parts of the finding are overstated, though. Rendering does not break: the Noto fallback draws correctly, and Windows and macOS ship CJK, Arabic and Hebrew system fonts, so tofu boxes are mostly a risk on minimal Linux installs. The finding says there is no script coverage, but a Korean test with an installed font exists (raster.rs:368). The locale point is also misdirected. The fixed "en-US" locale is only in bundled_only(), which is used for tests. Production Fonts::new() calls FontSystem::new(), which reads the operating system's locale. For these reasons I rate it medium rather than high.

### No accessibility requirements; tests deliberately bypass accessible labels

*Area:* ease-of-use. *Fits:* Plan 3 (extend)

There are 0 accessible-* properties in ui/*.slint. Plan 3 (native-operator.md:23) says tests use element ids, "never accessible labels", so nothing checks that screen readers get names. Plan 3 widgets have icon-only TopBtn (:167) with tooltips only. The spec has no contrast, UI scaling or large-button requirement, but volunteers are often older or low-vision. Key shortcuts assume a PC keyboard: F1-F5 need Fn on most Mac and many laptop keyboards, and Ctrl+B should be Cmd+B on macOS (spec §7.3, README key table).

**Recommendation:** Add an accessibility section to the native spec: accessible-label and accessible-role on every interactive widget (Slint maps these to AccessKit and UIA/NSAccessibility/AT-SPI), and a unit test that every element with an id has a non-empty label. Require WCAG AA contrast for the operator theme and a UI zoom setting (e.g. 100-200%). Use platform-correct modifiers (Cmd on mac) and give every F-key action a visible button. Document Fn behaviour.

### CCLI and SongSelect assumed; licence fields are US/CCLI-specific

*Area:* ease-of-use. *Fits:* FEAT-7 / spec §9.1

Settings has church_ccli (src/model.rs:191) and songs have ccli (model.rs:63). The credits footer prints 'CCLI License #' (spec §9.1 :268). Spec §1 :14 assumes a SongSelect subscription and a CCLI licence. Many churches use OneLicense, LicenSing, regional CCLI, or only public-domain music, and FEAT-7 'CCLI usage reports' assumes CCLI too.

**Recommendation:** Make the licence footer label and number configurable (e.g. 'CCLI License #', 'OneLicense #', or none), keep it optional, and hide the SongSelect and CCLI UI when no licence is set. Keep CCLI-specific features as optional integrations.

### Phone remote fails silently on guest Wi-Fi and client-isolated networks

*Area:* ease-of-use. *Fits:* new (spec §12.3, Plan 3 phone dialog, Plan 4 installer)

src/startup.rs:37 advertises one IP address, chosen from the default route. Nothing in the plans handles guest or client-isolated Wi-Fi, the macOS firewall prompt, more than one network interface, or telling the volunteer why a phone cannot connect. The Windows firewall is partly covered: Plan 4 Task 17 (packaging plan lines 982 and 1001) has a README step to "allow on Private networks". That fails when Windows has set the church Wi-Fi as Public, which is its usual setting for a new network, so add an installer firewall rule or detect the Public profile and say so in the app. The status bar already shows paired phones (v1 spec §12.1). What is missing is a pairing dialog that waits for the phone to open the page, has a fallback to pick another address, and shows plain-language troubleshooting (same Wi-Fi? guest network? firewall? use a hotspot) if nothing connects within about 30 seconds. Severity is medium, not high, because the phone remote is optional and the service can run from the laptop alone.

**Recommendation:** In the pairing dialog: list all candidate interfaces (or the QR for each), have the phone page report back so the operator sees 'phone connected', and if nothing connects within ~30 s show a plain-language troubleshooting panel (same Wi-Fi as the laptop? guest/isolated network? allow firewall?). On Windows, have the installer add a firewall rule scoped to private networks. Document hotspot use (laptop hotspot or phone hotspot) as the fallback. Optionally advertise via mDNS (rustypresenter.local).

*Verification:* Mostly holds, with two corrections. Confirmed: src/startup.rs:37 picks a single LAN IP by doing a UDP connect to 192.0.2.1. A laptop with Ethernet plus Wi-Fi, a VPN, or Hyper-V/WSL adapters can therefore put the wrong address in the QR code. The v1 spec §12.3 (docs/superpowers/specs/2026-10-29-rustypresenter-v1-design.md:340) assumes a "trusted church LAN". No plan or spec mentions guest Wi-Fi or AP client isolation, the macOS incoming-connections prompt, more than one network interface, hotspot as a fallback, mDNS, or any troubleshooting when the phone cannot reach the laptop. Two parts are already partly handled. First, Plan 4 Task 17 (docs/superpowers/plans/2026-11-01-native-imports-recovery-packaging.md:982 and :1001) adds a README step and a manual check for the Windows Firewall prompt ("tick Private networks and Allow"). That only works if the church Wi-Fi is set as a Private network. Windows usually treats a new Wi-Fi network as Public, so following the README can still leave the phone blocked. That gap is real, and it is a reason to add an installer firewall rule or to detect the problem. Second, the v1 spec §12.1:329 already shows "paired phones" in the status bar, so some connection feedback exists, but only after a PIN succeeds. Nothing tells the volunteer why a phone never reached the page. Severity is lowered from high to medium. The phone remote is optional: a service still runs from the laptop, so this does not block a release and does not break the main workflow, although many churches will hit it.

### No support channel, docs site, or in-app help beyond the shortcut list

*Area:* ease-of-use. *Fits:* new (Plan 4 packaging adjacent)

The repo and plans have no community or support files: no CONTRIBUTING, CODE_OF_CONDUCT, SECURITY or SUPPORT file, and no .github/ISSUE_TEMPLATE (.github has only workflows/ci.yml). They also have no troubleshooting or FAQ beyond the Plan 4 Task 17 README install and first-launch steps, and no single place that collects diagnostics for a bug report. In-app help is not limited to the shortcut list. Plan 3's Settings → Help tab has the keys plus a tip, the editors have inline field help, and About shows the version, data folder, GPU/software mode, libmpv/LGPL status and, after Plan 4, converter status. Crash logs are written to logs\native-*.log. Error messages such as the LibreOffice timeout (Plan 4 :329) do not link to any help. The README is still Windows- and Tauri-centric. Add the following to Plan 4 Task 17 or a release task:
- a SUPPORT.md that states the app is volunteer-maintained with no guaranteed support;
- bug and feature issue templates that ask for the About details and the crash log;
- a "Copy details for a bug report" button in About that copies the existing About fields and the path of the latest log (no automatic upload);
- an OS-neutral README, or an mdBook/GitHub Pages guide, with a short troubleshooting section (no projector output, phone can't connect, video won't play, slides won't import);
- optionally, GitHub Discussions.

**Recommendation:** Add a public-release item: a docs site (e.g. GitHub Pages or mdBook) with an illustrated Sunday-morning guide and troubleshooting per OS, GitHub Issues with bug and feature templates, GitHub Discussions for volunteer questions, and a 'Report a problem' button in the app that collects version, OS, GPU and the panic log (already written in Plan 2) into a file the user attaches themselves, with no automatic upload. Give each error a short code linking to a docs page. Set expectations: volunteer-maintained, no guaranteed support.

*Verification:* Mostly confirmed. The repo has no CONTRIBUTING, CODE_OF_CONDUCT, SECURITY or SUPPORT file, and no issue templates: .github/ holds only workflows/ci.yml. There is no docs site, FAQ, troubleshooting page or "Report a problem" flow in PROJECT_PLAN.md or any plan or spec. Error strings such as the LibreOffice timeout (Plan 4 imports-recovery-packaging.md:329) and the probe's "took too long" message (:517) give no code and no link to help. The finding overstates three things. (1) In-app help is more than the shortcut list. Plan 3 Settings has a Help tab with keys plus a tip (native-operator.md:635), the song and text editors have inline field help (:548, :572), and About is a diagnostics panel: version, data folder, GPU or software drawing, libmpv/LGPL, and in Plan 4 Task 14 (:850) also pdfium, LibreOffice and PowerPoint status. (2) There is a user guide. README.md is already a plain-language quick start, and Plan 4 Task 17 (:977-985) adds per-OS install and first-launch steps (SmartScreen and Smart App Control, macOS Open Anyway, Linux chmod and libfuse2, the Windows Firewall prompt, LibreOffice hints), which are manually checked (:1000-1001). (3) Crash logs already exist (logs\native-YYYYMMDD.log, Plan 4 recovery::panic). The real gaps are smaller: a way to gather what is already collected into one bug report, issue templates and community/support files, troubleshooting beyond install, and a volunteer-support disclaimer. The README is also still Windows-only and describes the Tauri app. These gaps hurt adoption and put the support load on one maintainer, but they do not block a release, and GitHub Issues works without templates. Medium, not high.

### Telemetry, privacy and network policy not stated

*Area:* ease-of-use. *Fits:* new

No telemetry exists (no analytics found in src or Cargo.toml), but neither the plan nor the spec states a privacy policy. Churches, including those in GDPR jurisdictions and those whose libraries hold people's names in nursery or prayer messages (FEAT-2), will ask. There is also no stated policy on network access: update checks, Google Slides (R8), Bible download providers (F5).

**Recommendation:** Record a decision: no telemetry; the app contacts the network only for features the user turns on (list them). Show this in the README, docs and Settings → About. If an update check is added, make it opt-in or clearly disclosed.

### Assumes one operator on one laptop; nothing for small or large church setups

*Area:* ease-of-use. *Fits:* FEAT-6 (move earlier) / new

Data is a local SQLite DB that is explicitly not on OneDrive (spec §11), and the only way to move content is the service bundle export. There is no way to plan a service on a home PC and run it on the church laptop besides manual zip transfer, no library sync between machines, and no multi-campus or multiple-operator story. Remote control is limited to the allowlist (spec §12.3), so a second volunteer cannot e.g. run Bible lookups from a tablet. For tiny churches the reverse problem applies: a one-person setup with no second monitor needs a usable single-screen practice or test mode (FEAT-6 practice mode is post-parity).

**Recommendation:** Specify and document library export/import (whole library, not only services) as the supported way to move between computers, and say where the data folder is in Settings (already in snapshot data_folder). Defer real sync but record it as a goal. Bring FEAT-6 practice mode forward so it is in the first public release, since every new church needs it for training.

### Low-end and old hardware not covered by acceptance criteria

*Area:* ease-of-use. *Fits:* Plan 4 / new

Acceptance (spec §14.2) runs on the one church laptop. Nothing checks minimum specs: Slint renderer choice (femtovg/Skia need GL; software fallback for old GPUs and VMs), 4K/HiDPI scaling, 1366x768 operator screens (Plan 3 has a 'narrow' top bar but no minimum size stated), or libmpv hardware decode falling back on old iGPUs.

**Recommendation:** State minimum supported hardware and OS versions. Add a software-renderer fallback switch (Slint's software renderer) selected automatically when GL init fails, a 1366x768 layout test, and a 'performance mode' (lower preview frame rate, no thumbnails for video). Include one old laptop in acceptance.

### The Linux .deb would use the distro's GPL libmpv inside a process with Slint Royalty-Free code

*Area:* licensing. *Fits:* Plan 4 (packaging, Task 15/16)

The .deb plan (spec :132; Plan 4 lines 64, 900, 909, 926) depends on Debian/Ubuntu libmpv2. Debian builds it without -Dgpl=false and with cdda/dvdnav enabled (sources.debian.org mpv debian/rules), so it is GPL. That contradicts Plan 4's own LGPL-only rule (lines 51, 96). It also makes About report "(not an LGPL build)" on every .deb install. Shipping a package whose only video backend is GPL while the app uses Slint under the Royalty-Free licence is a licensing gray area, since the package only declares a dependency and loads libmpv at runtime. It is not a clear violation, and it affects only the .deb channel. macOS/Homebrew is not affected because macOS bundles the CI LGPL build. Fix: bundle the CI LGPL libmpv.so.2 in a private lib dir in the .deb (as the AppImage already does) and drop the libmpv2 dependency, or decide to license the app GPLv3 with Slint's GPL option. Record the decision in spec §10.

**Recommendation:** Either (a) bundle the CI-built LGPL libmpv in the Linux packages too (AppImage/Flatpak, or a private lib dir in the .deb) and drop the libmpv2 dependency, or (b) switch Slint to its GPLv3 option and license the app GPLv3, which removes the LGPL-only constraint and the Slint attribution duty but rules out NDI and other proprietary SDKs later. Record the choice in spec §10.

*Verification:* The finding is confirmed in the repo. Spec docs/superpowers/specs/2026-10-29-native-slint-design.md:132 says the .deb uses the distro libmpv2. Plan 4 (docs/superpowers/plans/2026-11-01-native-imports-recovery-packaging.md) does the same at line 64, line 900 (depends = ["libmpv2", ...]), line 909 ("the .deb uses the distro libmpv2") and line 926. That conflicts with the plan's own rule at line 51 (ship only libmpv built with -Dgpl=false) and line 96 (no GPL libmpv in a package). The about_info test at line 856 means every .deb install would show "(not an LGPL build)". I checked Debian's mpv debian/rules on sources.debian.org: it never passes -Dgpl=false, so mpv's default gpl=true applies. It also enables cdda and dvdnav, which are GPL-only. The distro libmpv2 is therefore a GPL build. The plan lists no LICENSE file and no app licence anywhere (Cargo.toml, README, PROJECT_PLAN), and Slint is accepted only under the Royalty-Free licence (line 844). So nothing in the plan resolves the GPL plus Slint-RF combination for the .deb. 

I lowered the severity from high to medium for three reasons. (1) The .deb does not ship the GPL code. It only declares a dependency, and libmpv is loaded at runtime with dlopen. Whether that counts as distributing a combined work is a real gray area, not a clear violation, and the risk of enforcement is low. (2) Linux .deb users are a small share of churches, so most would not hit this ("high" means most would). The Windows, macOS and AppImage packages already bundle the CI-built LGPL libmpv. (3) The Homebrew/macOS point is beside the issue, because macOS bundles its own libmpv (Plan 4 line 896). The RP_LIBMPV and shinchiro points apply only to libraries users supply themselves, which the project does not distribute, so they are not a release problem.

### The LGPL source offer covers libmpv only, not FFmpeg and the other LGPL libraries compiled into libmpv-2.dll

*Area:* licensing. *Fits:* Plan 4 Task 14/16

Plan 4 (2026-11-01-native-imports-recovery-packaging.md:52) puts only 'the libmpv source tarball' on the GitHub Release. An LGPL libmpv-2.dll for Windows/macOS normally includes FFmpeg (LGPL-2.1+), libplacebo (LGPL-2.1+), fribidi (LGPL-2.1+) and others (libass ISC, freetype FTL/GPLv2, harfbuzz MIT). LGPL-2.1 §6 needs the full corresponding source of every LGPL part, plus the build scripts to relink.

**Recommendation:** Have the Task 16 libmpv workflow publish one source bundle with the exact FFmpeg, libplacebo, fribidi and other LGPL sources, plus the build script/config, next to each release. List every component in NOTICE-LGPL.txt and THIRD-PARTY-LICENSES.

### Patent licensing for H.264/HEVC/AAC decoders shipped in the bundled FFmpeg is not addressed

*Area:* licensing. *Fits:* Plan 4 (new item in spec §10)

Building FFmpeg without --enable-gpl/--enable-nonfree (Plan 4:51) sorts out copyright but not patents. The LGPL FFmpeg still contains H.264, HEVC and AAC decoders. Via LA (which absorbed MPEG LA in 2023) runs the AVC and AAC pools. Historically the first 100,000 units/year were royalty-free, but a licence agreement is still expected (via-la.com/licensing-2/avc-h264). HEVC has several pools (Access Advance, Via LA) with less clear terms for free software. A free app given to many churches is a decoder distributor. Nothing in the spec or plans mentions patents.

**Recommendation:** Write a short policy in spec §10. Use the OS's licensed decoders first (mpv hwdec: D3D11VA/DXVA on Windows, VideoToolbox on macOS, VA-API on Linux), with FFmpeg's software decoders as fallback. On Linux, prefer distro or Flatpak codec packages. Note in the README that the user is responsible for codec patent licences where they apply, as VLC/mpv/OBS do. If the risk ever matters, offer a build without the patent-encumbered software decoders. Not a release blocker in practice for most FOSS projects, but it should be a recorded decision.

### Slint Royalty-Free v2.0 does not cover embedded systems; Raspberry Pi or kiosk setups are a gray area

*Area:* licensing. *Fits:* Plan 3 (About), spec §10

Slint's terms (slint.dev/terms-and-conditions, 'Royalty-Free Desktop, Mobile, and Web Applications License, Version 2.0') say it 'does not permit the use of the Software within Embedded Systems'. They also forbid distributing an application 'that exposes the APIs' of Slint, and require the AboutSlint widget or a badge on the download page. The spec covers attribution (native-slint-design.md:124) and has no plugin system. But 'any computer, old/low-end' will draw requests for Raspberry Pi or a dedicated stage-display box, which could count as an 'Embedded System'. Downstream forks inherit the same conditions. A future plugin or scripting feature (from 'we want it all') could break the no-exposed-API rule.

**Recommendation:** Document the supported targets as desktop OSes only, check Slint's 'Embedded Systems' definition before advertising Pi or kiosk images (or ask Slint), and add a rule to spec §10: any future plugin or scripting API must not expose Slint types. Keep the AboutSlint widget and also put the badge in the README, as already planned.

### EasyWorship import must be a clean-room implementation and ship no EasyWorship data

*Area:* licensing. *Fits:* F4

F4 (PROJECT_PLAN.md:76) plans schema and RTF research for EasyWorship's database. Reading a church's own database for interoperability is generally lawful (US DMCA §1201(f) and case law; EU Software Directive Art. 6), and the lyrics are covered by the church's own CCLI licence. Two risks come with public release. (1) The best-known reference, OpenLP's EasyWorship importer, is GPL: copying its code or close translations into this MIT/Apache repo would bring in GPL code. (2) Test fixtures made from a real EasyWorship install (its sample database or copyrighted lyrics) cannot be committed to a public repo. The existing songselect.rs test uses invented text, which is good.

**Recommendation:** Add a rule to F4: write the importer from your own inspection of the format (or permissively licensed docs), don't copy OpenLP code, and build fixtures from invented songs created in EasyWorship. Refer to EasyWorship only by name ('Import from EasyWorship'), with no logo, plus a 'not affiliated' line.

### Google Slides sign-in was planned for a single-church Google Cloud project

*Area:* licensing. *Fits:* F3

v1 spec line 41 (F3): 'Token lifetime for a personal-use Google Cloud project with the drive.file scope'. A public app needs one shared OAuth client. Google limits unverified or 'Testing' apps to 100 users, and refresh tokens in Testing mode expire after 7 days. Publishing needs brand verification: homepage, privacy policy, authorized domain. The Picker needs an API key and app ID, which end up in the public binary (normal for installed apps, but quota-shared).

**Recommendation:** Add to F3: a project-owned Google Cloud project in Production with brand verification, a privacy policy page (the app sends nothing to the project), only the drive.file scope (non-sensitive, so no security assessment), and keep File → Download → .pptx as the no-setup route.


## Low

### Contributor build path is Windows/MinGW-only and still includes the private remote-build hook

*Area:* codebase. *Fits:* Plan 4 Task 12/15 (README), new for CONTRIBUTING

README.md:57 requires `.tools\cargo`, the `stable-x86_64-pc-windows-gnu` toolchain and `.tools\mingw64` (WinLibs). Every launcher/build script is a .cmd (RustyPresenter.cmd:5-14, RustyPresenter-Native.cmd, dev-build.cmd:5-14) that sets `CARGO_TARGET_DIR=%LOCALAPPDATA%\RustyPresenter\build` and dlltool RUSTFLAGS. dev-watch.cmd builds, runs and taskkills based on files appearing in the repo folder. README.md:48 and the Collaborators section talk about OneDrive and %LOCALAPPDATA%. Plan 4 Task 12 only edits dev-build.cmd/RustyPresenter.cmd and does not add a cross-platform contributor path. Shipped packages come from CI (`windows-latest` MSVC), so end users are not affected.

**Recommendation:** For the public repo, document plain `cargo build`/`cargo run` on any OS (rustup default toolchain, plus the Linux packages from ci.yml), move the .tools/MinGW scripts to a `scripts/windows-local/` folder or out of the repo, drop dev-watch.cmd, and split the README into user install and developer sections.

### No security disclosure policy for a LAN-exposed HTTP server shipped to thousands of churches

*Area:* completeness critic. *Fits:* new (relates to the existing phone-remote pairing work, v1 spec §12.3)

The app runs an HTTP server on the LAN for the phone remote (src/server.rs), with PIN pairing and no TLS. TLS is listed only as long-term in v1 spec line 391. Once the app is public, outside researchers or users will find bugs in pairing, upload (server.rs:171) or asset serving (server.rs:80). The repo has no SECURITY.md and no private reporting channel, and the maintainers are one non-developer plus Claude (memory: 'no coders at church'). The existing support-channel finding covers user help, not vulnerability intake or patch timelines.

**Recommendation:** Add SECURITY.md and turn on GitHub private vulnerability reporting before the repo goes public. Document what the remote server exposes (bind address, the port, what an unpaired device can reach) and the default (remote off until pairing is opened). Add a fuzz/property test for the upload and pairing handlers. Keep TLS for the remote on the roadmap.

### Bundled KJV is described only as 'public domain', but it is under Crown copyright in the UK

*Area:* completeness critic. *Fits:* F5 / §3 Bible table

PROJECT_PLAN.md §3 (line 85) lists KJV as 'Public domain', and assets/bibles/SOURCE.html points to eBible.org. That holds in the US and most countries. In the United Kingdom the Authorized Version is under perpetual Crown letters patent, administered by Cambridge University Press. Projecting it in worship is in practice tolerated, but a free app distributed worldwide, bundling and redistributing the full text, should not claim 'public domain' without qualification. The Bible-licensing findings cover copyrighted modern translations, not this.

**Recommendation:** In the third-party notices and the Bible source note, describe KJV as public domain except in the UK (Crown copyright, CUP), keeping the eBible.org attribution. Have the first-run or translation picker word licensing neutrally. This is a documentation fix only.

### New churches can project lyrics without a CCLI number; there is no first-run reminder

*Area:* licensing. *Fits:* Plan 3 (settings/onboarding), FEAT-7

The footer correctly appends 'CCLI Song #' and 'CCLI License #' (src/slides.rs:96-97). But `church_ccli` defaults to empty (src/model.rs:204), and nothing asks for it. One field per install is fine, since each church holds its own licence. A free public app will reach churches that don't know they need a CCLI Church Copyright License, plus a separate Streaming License for FEAT-7. CCLI also doesn't operate in every country (there are other licensing bodies).

**Recommendation:** First-run setup asks for the church name and CCLI licence number ('Skip: I'll add it later'), and an empty number shows a gentle notice next to song items. FEAT-7 livestream output should say that streaming lyrics needs a CCLI Streaming License. Refer to CCLI/SongSelect by name only, no logos.

### Unclear files in git history should be removed before the repo goes public

*Area:* licensing. *Fits:* new (pre-release checklist, Plan 4)

Tracked: .rp-src.tgz (truncated: `tar tzf` fails with 'unexpected end of file'), experiment-archive/rp-v0.2.tgz, PROJECT_PLAN.md.bak-* (three copies), build.log in the working tree, icons/icon.ico of unknown origin (likely the Tauri default, which is MIT/Apache), and gen/schemas from Tauri. Their contents and licensing can't be checked quickly, and they bloat a public repo. Commit metadata also contains the user's church email.

**Recommendation:** Before publishing: delete the tgz and backup files (and consider rewriting history or starting a fresh public repo), confirm or replace the icon with original art (avoid the Rust logo, which the Rust Foundation policy restricts), and decide which commit email appears publicly.

### Third-party notices: check that pdfium's bundled libraries are covered; fonts are fine

*Area:* licensing. *Fits:* Plan 4 Task 14

Plan 4 Task 14 ships only `pdfium-LICENSE.txt` (Plan 4:841). bblanchon/pdfium-binaries builds also contain FreeType (FTL), libjpeg-turbo (IJG/BSD), libpng, zlib, lcms2 (MIT), OpenJPEG (BSD-2) and abseil (Apache-2.0), whose notices must ship too. Noto Sans (assets/fonts/*.ttf + OFL.txt, no Reserved Font Name) is fine. Web pages only reference system fonts (Segoe UI, Consolas) and bundle none. LibreOffice and PowerPoint are run as external programs, not bundled, so no licence obligations.

**Recommendation:** In Task 14, copy the full licence/notice file(s) from the pinned pdfium-binaries archive (verify it contains the third_party notices) instead of a single pdfium-LICENSE.txt.

### Product name and references to other products: low trademark risk, a disclaimer is enough

*Area:* licensing. *Fits:* new (README), FEAT-1..7

The Rust Foundation trademark policy (rustfoundation.org/policy/rust-trademark-policy, 2024 revision) allows 'Rust' to identify projects written in Rust, and forbids implying endorsement or using the Rust logo. 'RustyPresenter' vs Renewed Vision's 'ProPresenter' shares 'Presenter', a descriptive word, so the risk is low. The README and plan name EasyWorship, ProPresenter, PowerPoint, SongSelect and CCLI, which is nominative fair use if used only to describe compatibility.

**Recommendation:** Add a README footer: 'RustyPresenter is not affiliated with or endorsed by the Rust Foundation, Renewed Vision, Softouch (EasyWorship), Microsoft, Google or CCLI; product names are trademarks of their owners.' Don't copy their logos, icons or UI artwork when building the ProPresenter/EasyWorship-inspired FEAT items.
