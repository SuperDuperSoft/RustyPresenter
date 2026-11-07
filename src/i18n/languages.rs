//! Common languages for "Add a language…" in the translation editor: code, name in itself, name in English, and
//! gettext's `Plural-Forms` for it (how it says "1 song, 2 songs"). Not every language the app can show: the editor
//! also takes any code ("Custom code").

/// One language of the list.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Language {
    pub code: &'static str,
    pub native: &'static str,
    pub english: &'static str,
    pub plural_forms: &'static str,
}

const ONE: &str = "nplurals=1; plural=0;";
const EN: &str = "nplurals=2; plural=(n != 1);";
const FR: &str = "nplurals=2; plural=(n > 1);";
const RU: &str = "nplurals=3; plural=(n%10==1 && n%100!=11 ? 0 : n%10>=2 && n%10<=4 && (n%100<10 || n%100>=20) ? 1 : 2);";
const CS: &str = "nplurals=3; plural=(n==1 ? 0 : (n>=2 && n<=4) ? 1 : 2);";
const PL: &str = "nplurals=3; plural=(n==1 ? 0 : n%10>=2 && n%10<=4 && (n%100<10 || n%100>=20) ? 1 : 2);";
const LT: &str = "nplurals=3; plural=(n%10==1 && n%100!=11 ? 0 : n%10>=2 && (n%100<10 || n%100>=20) ? 1 : 2);";
const LV: &str = "nplurals=3; plural=(n%10==1 && n%100!=11 ? 0 : n != 0 ? 1 : 2);";
const RO: &str = "nplurals=3; plural=(n==1 ? 0 : (n==0 || (n%100 > 0 && n%100 < 20)) ? 1 : 2);";
const SL: &str = "nplurals=4; plural=(n%100==1 ? 0 : n%100==2 ? 1 : n%100==3 || n%100==4 ? 2 : 3);";
const AR: &str = "nplurals=6; plural=(n==0 ? 0 : n==1 ? 1 : n==2 ? 2 : n%100>=3 && n%100<=10 ? 3 : n%100>=11 ? 4 : 5);";
const GA: &str = "nplurals=5; plural=(n==1 ? 0 : n==2 ? 1 : n<7 ? 2 : n<11 ? 3 : 4);";
const CY: &str = "nplurals=4; plural=(n==1 ? 0 : n==2 ? 1 : (n != 8 && n != 11) ? 2 : 3);";
const GD: &str = "nplurals=4; plural=((n==1 || n==11) ? 0 : (n==2 || n==12) ? 1 : (n > 2 && n < 20) ? 2 : 3);";
const MK: &str = "nplurals=2; plural=(n%10==1 && n%100!=11 ? 0 : 1);";
const IS: &str = "nplurals=2; plural=(n%10!=1 || n%100==11);";
const MT: &str = "nplurals=4; plural=(n==1 ? 0 : n==0 || (n%100>1 && n%100<11) ? 1 : (n%100>10 && n%100<20) ? 2 : 3);";
const FIL: &str = "nplurals=2; plural=(n != 1 && n != 2 && n != 3 && (n % 10 == 4 || n % 10 == 6 || n % 10 == 9));";
const JV: &str = "nplurals=2; plural=(n != 0);";

const fn l(code: &'static str, native: &'static str, english: &'static str, plural_forms: &'static str) -> Language {
    Language { code, native, english, plural_forms }
}

/// The list, by English name.
pub const LANGUAGES: &[Language] = &[
    l("af", "Afrikaans", "Afrikaans", EN),
    l("ak", "Akan", "Akan", FR),
    l("sq", "Shqip", "Albanian", EN),
    l("am", "አማርኛ", "Amharic", FR),
    l("ar", "العربية", "Arabic", AR),
    l("hy", "Հայերեն", "Armenian", EN),
    l("as", "অসমীয়া", "Assamese", EN),
    l("az", "Azərbaycanca", "Azerbaijani", EN),
    l("bm", "Bamanankan", "Bambara", ONE),
    l("eu", "Euskara", "Basque", EN),
    l("be", "Беларуская", "Belarusian", RU),
    l("bem", "Ichibemba", "Bemba", EN),
    l("bn", "বাংলা", "Bengali", EN),
    l("bs", "Bosanski", "Bosnian", RU),
    l("bg", "Български", "Bulgarian", EN),
    l("my", "မြန်မာ", "Burmese", ONE),
    l("ca", "Català", "Catalan", EN),
    l("ceb", "Cebuano", "Cebuano", EN),
    l("ckb", "کوردیی ناوەندی", "Central Kurdish", EN),
    l("ny", "Chichewa", "Chichewa", EN),
    l("zh-CN", "简体中文", "Chinese (Simplified)", ONE),
    l("zh-TW", "繁體中文", "Chinese (Traditional)", ONE),
    l("hr", "Hrvatski", "Croatian", RU),
    l("cs", "Čeština", "Czech", CS),
    l("da", "Dansk", "Danish", EN),
    l("nl", "Nederlands", "Dutch", EN),
    l("en-GB", "English (UK)", "English (UK)", EN),
    l("eo", "Esperanto", "Esperanto", EN),
    l("et", "Eesti", "Estonian", EN),
    l("ee", "Eʋegbe", "Ewe", EN),
    l("fo", "Føroyskt", "Faroese", EN),
    l("fj", "Vosa Vakaviti", "Fijian", EN),
    l("fil", "Filipino", "Filipino", FIL),
    l("fi", "Suomi", "Finnish", EN),
    l("fr", "Français", "French", FR),
    l("ff", "Fulfulde", "Fula", EN),
    l("gl", "Galego", "Galician", EN),
    l("lg", "Luganda", "Ganda", EN),
    l("ka", "ქართული", "Georgian", EN),
    l("de", "Deutsch", "German", EN),
    l("el", "Ελληνικά", "Greek", EN),
    l("gn", "Avañeʼẽ", "Guarani", EN),
    l("gu", "ગુજરાતી", "Gujarati", EN),
    l("ht", "Kreyòl ayisyen", "Haitian Creole", EN),
    l("ha", "Hausa", "Hausa", EN),
    l("haw", "ʻŌlelo Hawaiʻi", "Hawaiian", EN),
    l("he", "עברית", "Hebrew", EN),
    l("hi", "हिन्दी", "Hindi", EN),
    l("hmn", "Hmoob", "Hmong", ONE),
    l("hu", "Magyar", "Hungarian", EN),
    l("is", "Íslenska", "Icelandic", IS),
    l("ig", "Igbo", "Igbo", ONE),
    l("ilo", "Ilokano", "Ilocano", EN),
    l("id", "Bahasa Indonesia", "Indonesian", ONE),
    l("ga", "Gaeilge", "Irish", GA),
    l("it", "Italiano", "Italian", EN),
    l("ja", "日本語", "Japanese", ONE),
    l("jv", "Basa Jawa", "Javanese", JV),
    l("kn", "ಕನ್ನಡ", "Kannada", EN),
    l("kk", "Қазақ тілі", "Kazakh", EN),
    l("km", "ខ្មែរ", "Khmer", ONE),
    l("rw", "Kinyarwanda", "Kinyarwanda", EN),
    l("rn", "Ikirundi", "Kirundi", EN),
    l("ko", "한국어", "Korean", ONE),
    l("ku", "Kurdî", "Kurdish", EN),
    l("ky", "Кыргызча", "Kyrgyz", EN),
    l("lo", "ລາວ", "Lao", ONE),
    l("la", "Latina", "Latin", EN),
    l("lv", "Latviešu", "Latvian", LV),
    l("ln", "Lingála", "Lingala", FR),
    l("lt", "Lietuvių", "Lithuanian", LT),
    l("lb", "Lëtzebuergesch", "Luxembourgish", EN),
    l("mk", "Македонски", "Macedonian", MK),
    l("mg", "Malagasy", "Malagasy", FR),
    l("ms", "Bahasa Melayu", "Malay", ONE),
    l("ml", "മലയാളം", "Malayalam", EN),
    l("mt", "Malti", "Maltese", MT),
    l("mi", "Māori", "Maori", FR),
    l("mr", "मराठी", "Marathi", EN),
    l("mn", "Монгол", "Mongolian", EN),
    l("ne", "नेपाली", "Nepali", EN),
    l("nb", "Norsk bokmål", "Norwegian Bokmål", EN),
    l("nn", "Norsk nynorsk", "Norwegian Nynorsk", EN),
    l("or", "ଓଡ଼ିଆ", "Odia", EN),
    l("om", "Afaan Oromoo", "Oromo", EN),
    l("ps", "پښتو", "Pashto", EN),
    l("fa", "فارسی", "Persian", FR),
    l("pl", "Polski", "Polish", PL),
    l("pt", "Português", "Portuguese", EN),
    l("pt-BR", "Português (Brasil)", "Portuguese (Brazil)", FR),
    l("pa", "ਪੰਜਾਬੀ", "Punjabi", EN),
    l("qu", "Runasimi", "Quechua", EN),
    l("ro", "Română", "Romanian", RO),
    l("ru", "Русский", "Russian", RU),
    l("sm", "Gagana Samoa", "Samoan", ONE),
    l("gd", "Gàidhlig", "Scottish Gaelic", GD),
    l("sr", "Српски", "Serbian", RU),
    l("sn", "chiShona", "Shona", EN),
    l("sd", "سنڌي", "Sindhi", EN),
    l("si", "සිංහල", "Sinhala", EN),
    l("sk", "Slovenčina", "Slovak", CS),
    l("sl", "Slovenščina", "Slovenian", SL),
    l("so", "Soomaali", "Somali", EN),
    l("st", "Sesotho", "Southern Sotho", EN),
    l("es", "Español", "Spanish", EN),
    l("es-419", "Español (Latinoamérica)", "Spanish (Latin America)", EN),
    l("su", "Basa Sunda", "Sundanese", ONE),
    l("sw", "Kiswahili", "Swahili", EN),
    l("sv", "Svenska", "Swedish", EN),
    l("tl", "Tagalog", "Tagalog", FIL),
    l("tg", "Тоҷикӣ", "Tajik", EN),
    l("ta", "தமிழ்", "Tamil", EN),
    l("tt", "Татар", "Tatar", EN),
    l("te", "తెలుగు", "Telugu", EN),
    l("th", "ไทย", "Thai", ONE),
    l("ti", "ትግርኛ", "Tigrinya", FR),
    l("to", "Lea faka-Tonga", "Tongan", ONE),
    l("tn", "Setswana", "Tswana", EN),
    l("tr", "Türkçe", "Turkish", EN),
    l("tk", "Türkmençe", "Turkmen", EN),
    l("tw", "Twi", "Twi", FR),
    l("uk", "Українська", "Ukrainian", RU),
    l("ur", "اردو", "Urdu", EN),
    l("ug", "ئۇيغۇرچە", "Uyghur", EN),
    l("uz", "Oʻzbekcha", "Uzbek", EN),
    l("vi", "Tiếng Việt", "Vietnamese", ONE),
    l("cy", "Cymraeg", "Welsh", CY),
    l("fy", "Frysk", "Western Frisian", EN),
    l("wo", "Wolof", "Wolof", ONE),
    l("xh", "isiXhosa", "Xhosa", EN),
    l("yi", "ייִדיש", "Yiddish", EN),
    l("yo", "Yorùbá", "Yoruba", EN),
    l("zu", "isiZulu", "Zulu", EN),
];

/// The list's entry for `code` (any case).
pub fn find(code: &str) -> Option<&'static Language> { LANGUAGES.iter().find(|l| l.code.eq_ignore_ascii_case(code)) }

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::plural::Plurals;

    #[test]
    fn about_a_hundred_with_good_plural_rules() {
        assert!(LANGUAGES.len() >= 100, "{}", LANGUAGES.len());
        let mut codes: Vec<&str> = LANGUAGES.iter().map(|l| l.code).collect();
        codes.sort();
        codes.dedup();
        assert_eq!(codes.len(), LANGUAGES.len(), "no code twice");
        for lang in LANGUAGES {
            let p = Plurals::parse(lang.plural_forms).unwrap_or_else(|e| panic!("{}: {e}", lang.code));
            // Every form is used by some number.
            for form in 0..p.count { assert!((0..200).any(|n| p.index(n) == form), "{} form {form}", lang.code) }
            assert!(crate::i18n::user::valid_code(lang.code), "{}", lang.code);
        }
        assert_eq!(find("PT-br").map(|l| l.native), Some("Português (Brasil)"));
    }

    #[test]
    fn built_in_languages_are_in_the_list_with_the_same_rule() {
        for info in crate::i18n::languages().iter().skip(1) {
            let lang = find(&info.code).unwrap_or_else(|| panic!("{} is not in the list", info.code));
            let builtin = Plurals::from_header(crate::i18n::po::parse(crate::i18n::builtin_po(&info.code).unwrap()).unwrap().header("Plural-Forms"));
            let listed = Plurals::parse(lang.plural_forms).unwrap();
            assert_eq!(builtin.count, listed.count, "{}", info.code);
            assert!((0..200).all(|n| builtin.index(n) == listed.index(n)), "{}", info.code);
        }
    }
}
