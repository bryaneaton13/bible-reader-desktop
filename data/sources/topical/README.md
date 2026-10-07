# Older topical Bibles (public domain)

- `nave.xml`: Nave's Topical Bible (1896), from CCEL: https://www.ccel.org/ccel/nave/bible.xml
- `torrey.xml`: Torrey's New Topical Textbook (1897), from CCEL: https://www.ccel.org/ccel/torrey/ttt.xml

Both are CCEL ThML files. `bible-import` (`topical.rs`) reads the entries and their verse
citations and merges them with the openbible.info topics in `../openbible/`: a verse several
sources list under a topic ranks above one a single source lists, and topics only these books
have are added unless they are names, thin, or already served by plain text search.
Nave's "See X" entries become aliases.
