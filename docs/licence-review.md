# Licence review of candidate datasets

Status: **draft for decision** (M2c). Nothing listed here is bundled until the decisions
at the end are made and recorded. This is an engineering review of published terms, not
legal advice.

Reviewed 2026-11-07. Criteria from `docs/design.md`, Ground rules: bundled data must be
truly open (clear licence compatible with MIT OR Apache-2.0); everything else is
downloaded by the user; shared designs embed only the parameters they used, with
provenance.

## Findings

### refractiveindex.info database

- Source: <https://refractiveindex.info>, data in
  <https://github.com/polyanskiy/refractiveindex.info-database>.
- Terms: the about page states "The database is made freely available under the CC0 1.0
  Universal Public Domain Dedication." The repository README says copyright and related
  rights "were waived by Mikhail Polyanskiy through the CC0 1.0 Universal Public Domain
  Dedication" and asks users to "also cite the original publications from which specific
  datasets were obtained". The `LICENSE` file is the CC0 1.0 legal text.
- Limit: CC0 waives rights only "to the extent that he or she is an owner", and the
  affirmer "disclaims responsibility for clearing rights of other persons". The database
  is "a compilation of data from … scientific journal articles and material datasheets
  published by manufacturers". The waiver therefore covers the compilation and the
  maintainer's own work, not rights a manufacturer may hold in its catalogue data.
- Layout: `database/data/{main, glass, organic, other, specs}`. `main/` and `other/`
  hold data from the scientific literature; `specs/` (and vendor glass) holds
  manufacturer catalogue data, e.g. `specs/schott/optical/N-BK7.yml`.
- Companion code: `refractiveindex.info-scripts` is GPL-3.0. Per AGENTS.md it is never
  read for adaptation. It was not opened during this review.

### Primary literature behind the candidate bundled materials

Values taken from the CC0 database files above, each citing its original paper. Formula
coefficients and measured values are facts reported in the literature; the papers
themselves are not redistributed.

| Material | Source paper | Database file | Model | Range (µm) | Conditions |
| --- | --- | --- | --- | --- | --- |
| Fused silica | Malitson, J. Opt. Soc. Am. 55, 1205 (1965); range check Tan, J. Non-Cryst. Solids 223, 158 (1998) | `main/SiO2/nk/Malitson.yml` | Sellmeier, 3 terms | 0.21–6.7 | 20 °C |
| Water, index | Daimon & Masumura, Appl. Opt. 46, 3811 (2007) | `main/H2O/nk/Daimon-{19.0,20.0,21.5,24.0}C.yml` | Sellmeier, 4 terms per temperature | 0.182–1.129 | 19, 20, 21.5, 24 °C |
| Water, n and k | Hale & Querry, Appl. Opt. 12, 555 (1973) | `main/H2O/nk/Hale.yml` | table, 169 rows | 0.2–200 | 25 °C |
| Water, n and k (alternative) | Segelstein, M.Sc. thesis, Univ. Missouri–Kansas City (1981) | `main/H2O/nk/Segelstein.yml` | table | not yet read | not yet read |
| Air | Ciddor, Appl. Opt. 35, 1566 (1996) | `other/mixed gases/air/nk/Ciddor.yml` | two-term dispersion formula | 0.23–1.69 | standard dry air only: 15 °C, 101.325 kPa, 450 ppm CO₂ |

Notes:
- Kou, Labrie & Chylek (1993), named in the design as a water absorption candidate, is
  not in the database; it would have to come from the paper.
- The database's air entry covers standard air only. Temperature, pressure, humidity and
  CO₂ dependence need Ciddor's full procedure, implemented from the paper.
- Water temperature dependence: Daimon & Masumura give coefficients at four temperatures
  (19–24 °C). Wider ranges need another source.

### Known differences between the water datasets

Daimon & Masumura (2007, 20 °C) and Hale & Querry (1973, 25 °C) agree to within
±1.8 × 10⁻³ from 0.5 to 1.0 µm, as expected from Hale & Querry's three-decimal values and
the 5 K temperature difference. Below 0.45 µm they diverge steadily, reaching 0.028 at
0.2 µm. The bundled `Water` material therefore takes its index from Daimon & Masumura and
only its extinction coefficient from Hale & Querry. To be checked against both papers.

Also to be checked: the Daimon 20 °C formula gives n = 1.33335 at 589.3 nm, while the
paper reports agreement within 6 × 10⁻⁶ with the 1938 formula of Tilton and Taylor.

### Manufacturer glass catalogues (Schott, Ohara, Hoya, CDGM, Sumita, Hikari)

- No public licence or redistribution terms were found for the downloadable catalogue
  data files. Absence of terms is not permission.
- Treatment: never bundled. The user downloads a catalogue (from the manufacturer, or the
  vendor files mirrored in refractiveindex.info) and imports it (`optobench import-material`), and
  it is marked `redistribute = false` until its terms are known, so the tool asks before
  embedding values in a shared design.

### Optiland

- <https://github.com/optiland/optiland>, `LICENSE`: "MIT License", "Copyright (c) 2024
  Kramer Harrison". Code may be adapted with a row in `ATTRIBUTION.md`, and it may
  produce cross-check fixtures (docs/design.md, Tests and validation).
- Its material library is built on refractiveindex.info, so its material data carries
  the same split as above.

## Recommendation

1. **Bundle** (as code constants or small asset files with a manifest entry and an
   `ATTRIBUTION.md` row): fused silica (Malitson), standard air (Ciddor), water index
   (Daimon & Masumura, four temperatures) and the water n,k table (Hale & Querry). Each
   cites the original paper and the CC0 database file it was transcribed from, licence
   recorded as `CC0-1.0`.
2. **Fetch, never bundle**: every manufacturer catalogue, including vendor files in the
   refractiveindex.info database, marked `redistribute = false` by default.
3. **Fetch on request**: the rest of the literature part of refractiveindex.info
   (`main/`, `other/`), shown with its CC0 notice and per-file references.
4. **Verify before bundling**: each bundled value is checked against the original
   paper's own tables, not only against the database transcription. A first evaluation of
   the Daimon 20 °C coefficients gives n = 1.33335 at 589.3 nm; this must be compared with
   the paper's tabulated values before water is bundled.

## Decisions needed

- [x] Approve recommendation 1 (bundle four literature datasets as CC0 transcriptions).
- [x] Approve recommendation 2 (manufacturer catalogues are fetch-only, `redistribute = false`).
- [x] Approve recommendation 3 (fetch from the literature part of refractiveindex.info).
- [ ] Original papers available for verification (Malitson 1965, Daimon & Masumura 2007,
      Hale & Querry 1973, Ciddor 1996), placed in the local `corpus/`. Not available yet.

## Recommendation 4 while the papers are unavailable

Recommendation 4 asks for each bundled value to be checked against its original paper
before bundling, and the papers are not available yet. Decided 2026-11-07: bundle now
(see the decision record).

## Decision record

| Date | Decision | By |
| --- | --- | --- |
| 2026-11-07 | Recommendations 1, 2 and 3 approved. Original papers not available yet. | Jurjen Broeke |
| 2026-11-07 | Bundle the four datasets now as transcriptions from the CC0 database, each marked "not yet verified against the original paper" in its provenance, `ATTRIBUTION.md` and `assets/manifest.toml`. The M2 exit criterion for water stays open until the values are checked against the papers. | Jurjen Broeke |
