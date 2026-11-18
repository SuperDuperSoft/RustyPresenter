# CMU Pronouncing Dictionary, for auto-follow

`cmudict-ipa.tsv` is the CMU Pronouncing Dictionary (Carnegie Mellon University, licence in `LICENSE`, also in
`packaging/licenses/cmudict-LICENSE.txt`), downloaded on November 14, 2026 from
`https://raw.githubusercontent.com/cmusphinx/cmudict/master/cmudict.dict`, turned into what `src/follow/g2p.rs`
reads:

- one line per word: the word, then each way it is said, separated by tabs;
- the word in a–z only (apostrophes dropped, as the app folds lyrics: "I'm" is `im`); entries with other characters
  left out; comments after `#` left out;
- each way of saying it in the IPA the phoneme model writes (espeak style): ARPAbet symbols one for one (`AA` ɑː,
  `AE` æ, `AO` ɔː, `AW` aʊ, `AY` aɪ, `CH` tʃ, `DH` ð, `EH` ɛ, `EY` eɪ, `HH` h, `IH` ɪ, `IY` iː, `JH` dʒ, `NG` ŋ,
  `OW` oʊ, `OY` ɔɪ, `R` ɹ, `SH` ʃ, `TH` θ, `UH` ʊ, `UW` uː, `Y` j, `ZH` ʒ, the rest as their letter), `AH` as ə
  unstressed and ʌ stressed, `ER` as ɚ unstressed and ɜː stressed; stress marks dropped; a way given twice kept once.

121,735 words, 130,613 ways of saying them.
