//! Canonical 66-book Protestant canon in English order.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Testament {
    Old,
    New,
}

#[derive(Debug)]
pub struct Book {
    /// 1-based canonical number (Genesis = 1, Revelation = 66).
    pub num: u8,
    /// USFM 3-letter book code.
    pub code: &'static str,
    pub name: &'static str,
    pub chapters: u16,
    /// Extra lowercase abbreviations used by the reference parser.
    /// The full name and USFM code are matched automatically.
    pub aliases: &'static [&'static str],
}

impl Book {
    pub fn testament(&self) -> Testament {
        if self.num <= 39 {
            Testament::Old
        } else {
            Testament::New
        }
    }
}

macro_rules! book {
    ($num:expr, $code:expr, $name:expr, $chapters:expr, [$($alias:expr),* $(,)?]) => {
        Book { num: $num, code: $code, name: $name, chapters: $chapters, aliases: &[$($alias),*] }
    };
}

pub static BOOKS: [Book; 66] = [
    book!(1, "GEN", "Genesis", 50, ["gen", "ge", "gn"]),
    book!(2, "EXO", "Exodus", 40, ["ex", "exo", "exod"]),
    book!(3, "LEV", "Leviticus", 27, ["lev", "le", "lv"]),
    book!(4, "NUM", "Numbers", 36, ["num", "nu", "nm", "nb"]),
    book!(5, "DEU", "Deuteronomy", 34, ["deut", "dt", "de"]),
    book!(6, "JOS", "Joshua", 24, ["josh", "jos", "jsh"]),
    book!(7, "JDG", "Judges", 21, ["judg", "jdg", "jg", "jdgs"]),
    book!(8, "RUT", "Ruth", 4, ["ru", "rth"]),
    book!(
        9,
        "1SA",
        "1 Samuel",
        31,
        ["1sam", "1sa", "1sm", "isam", "1stsamuel", "firstsamuel"]
    ),
    book!(
        10,
        "2SA",
        "2 Samuel",
        24,
        ["2sam", "2sa", "2sm", "iisam", "2ndsamuel", "secondsamuel"]
    ),
    book!(
        11,
        "1KI",
        "1 Kings",
        22,
        ["1kgs", "1ki", "1k", "ikgs", "1stkings", "firstkings"]
    ),
    book!(
        12,
        "2KI",
        "2 Kings",
        25,
        ["2kgs", "2ki", "2k", "iikgs", "2ndkings", "secondkings"]
    ),
    book!(
        13,
        "1CH",
        "1 Chronicles",
        29,
        ["1chr", "1ch", "1chron", "ichr", "firstchronicles"]
    ),
    book!(
        14,
        "2CH",
        "2 Chronicles",
        36,
        ["2chr", "2ch", "2chron", "iichr", "secondchronicles"]
    ),
    book!(15, "EZR", "Ezra", 10, ["ezr", "ez"]),
    book!(16, "NEH", "Nehemiah", 13, ["neh", "ne"]),
    book!(17, "EST", "Esther", 10, ["esth", "est", "es"]),
    book!(18, "JOB", "Job", 42, ["jb"]),
    book!(
        19,
        "PSA",
        "Psalms",
        150,
        ["ps", "psa", "psalm", "pss", "psm", "pslm"]
    ),
    book!(20, "PRO", "Proverbs", 31, ["prov", "pro", "prv", "pr"]),
    book!(21, "ECC", "Ecclesiastes", 12, ["eccl", "ecc", "ec", "qoh"]),
    book!(
        22,
        "SNG",
        "Song of Songs",
        8,
        ["song", "sos", "so", "songofsolomon", "canticles", "sg"]
    ),
    book!(23, "ISA", "Isaiah", 66, ["isa", "is"]),
    book!(24, "JER", "Jeremiah", 52, ["jer", "je", "jr"]),
    book!(25, "LAM", "Lamentations", 5, ["lam", "la"]),
    book!(26, "EZK", "Ezekiel", 48, ["ezek", "eze", "ezk"]),
    book!(27, "DAN", "Daniel", 12, ["dan", "da", "dn"]),
    book!(28, "HOS", "Hosea", 14, ["hos", "ho"]),
    book!(29, "JOL", "Joel", 3, ["joel", "jl"]),
    book!(30, "AMO", "Amos", 9, ["am", "amo"]),
    book!(31, "OBA", "Obadiah", 1, ["obad", "ob", "oba"]),
    book!(32, "JON", "Jonah", 4, ["jon", "jnh"]),
    book!(33, "MIC", "Micah", 7, ["mic", "mc"]),
    book!(34, "NAM", "Nahum", 3, ["nah", "na"]),
    book!(35, "HAB", "Habakkuk", 3, ["hab", "hb"]),
    book!(36, "ZEP", "Zephaniah", 3, ["zeph", "zep", "zp"]),
    book!(37, "HAG", "Haggai", 2, ["hag", "hg"]),
    book!(38, "ZEC", "Zechariah", 14, ["zech", "zec", "zc"]),
    book!(39, "MAL", "Malachi", 4, ["mal", "ml"]),
    book!(40, "MAT", "Matthew", 28, ["matt", "mt", "mat"]),
    book!(41, "MRK", "Mark", 16, ["mrk", "mk", "mar", "mr"]),
    book!(42, "LUK", "Luke", 24, ["luk", "lk", "lu"]),
    book!(43, "JHN", "John", 21, ["jn", "jhn", "joh"]),
    book!(44, "ACT", "Acts", 28, ["act", "ac"]),
    book!(45, "ROM", "Romans", 16, ["rom", "ro", "rm"]),
    book!(
        46,
        "1CO",
        "1 Corinthians",
        16,
        ["1cor", "1co", "icor", "firstcorinthians"]
    ),
    book!(
        47,
        "2CO",
        "2 Corinthians",
        13,
        ["2cor", "2co", "iicor", "secondcorinthians"]
    ),
    book!(48, "GAL", "Galatians", 6, ["gal", "ga"]),
    book!(49, "EPH", "Ephesians", 6, ["eph", "ephes"]),
    book!(50, "PHP", "Philippians", 4, ["phil", "php", "pp"]),
    book!(51, "COL", "Colossians", 4, ["col", "co"]),
    book!(
        52,
        "1TH",
        "1 Thessalonians",
        5,
        ["1thess", "1th", "1thes", "ithess", "firstthessalonians"]
    ),
    book!(
        53,
        "2TH",
        "2 Thessalonians",
        3,
        ["2thess", "2th", "2thes", "iithess", "secondthessalonians"]
    ),
    book!(
        54,
        "1TI",
        "1 Timothy",
        6,
        ["1tim", "1ti", "itim", "firsttimothy"]
    ),
    book!(
        55,
        "2TI",
        "2 Timothy",
        4,
        ["2tim", "2ti", "iitim", "secondtimothy"]
    ),
    book!(56, "TIT", "Titus", 3, ["tit", "ti"]),
    book!(57, "PHM", "Philemon", 1, ["philem", "phm", "pm", "phlm"]),
    book!(58, "HEB", "Hebrews", 13, ["heb"]),
    book!(59, "JAS", "James", 5, ["jas", "jm", "jam"]),
    book!(
        60,
        "1PE",
        "1 Peter",
        5,
        ["1pet", "1pe", "1pt", "ipet", "firstpeter"]
    ),
    book!(
        61,
        "2PE",
        "2 Peter",
        3,
        ["2pet", "2pe", "2pt", "iipet", "secondpeter"]
    ),
    book!(
        62,
        "1JN",
        "1 John",
        5,
        ["1jn", "1jo", "1john", "ijn", "firstjohn"]
    ),
    book!(
        63,
        "2JN",
        "2 John",
        1,
        ["2jn", "2jo", "2john", "iijn", "secondjohn"]
    ),
    book!(
        64,
        "3JN",
        "3 John",
        1,
        ["3jn", "3jo", "3john", "iiijn", "thirdjohn"]
    ),
    book!(65, "JUD", "Jude", 1, ["jud", "jude", "jd"]),
    book!(
        66,
        "REV",
        "Revelation",
        22,
        ["rev", "re", "rv", "revelations", "apocalypse"]
    ),
];

pub fn book_by_num(num: u8) -> Option<&'static Book> {
    BOOKS.get(num.checked_sub(1)? as usize)
}

pub fn book_by_code(code: &str) -> Option<&'static Book> {
    BOOKS.iter().find(|b| b.code.eq_ignore_ascii_case(code))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbering_is_contiguous() {
        for (i, book) in BOOKS.iter().enumerate() {
            assert_eq!(book.num as usize, i + 1, "{}", book.name);
        }
        assert_eq!(BOOKS.iter().map(|b| b.chapters as u32).sum::<u32>(), 1189);
    }

    #[test]
    fn lookups() {
        assert_eq!(book_by_code("jhn").unwrap().name, "John");
        assert_eq!(book_by_num(66).unwrap().code, "REV");
        assert!(book_by_num(0).is_none());
        assert!(book_by_num(67).is_none());
        assert_eq!(book_by_num(40).unwrap().testament(), Testament::New);
    }
}
