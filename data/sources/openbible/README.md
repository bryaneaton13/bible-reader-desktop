# openbible.info data

- `topic-scores.txt`: topics with ranked passages (https://www.openbible.info/topics, data from https://a.openbible.info/data/topic-scores.zip).
- `cross_references.txt`: verse cross-references with votes (https://www.openbible.info/labs/cross-references/, derived from the public-domain Treasury of Scripture Knowledge).
- `aliases.txt`: ours. Words people search for that should land on a topic ("arrogant" -> pride).
- `drop.txt`: ours. Topic names (openbible.info, Nave's, Torrey's) a review found useless: names, typos, trivia, vague or oddly phrased. The importer skips them. One name per line, lowercase.

openbible.info data is licensed CC-BY 4.0. The app credits it in Settings > About.
`bible-import` trims it into `data/build/study.db` (weak topic entries and long cross-reference tails are dropped).
