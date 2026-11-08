# Attribution

Third-party code adapted into optobench (MIT or Apache-2.0 only) and bundled data or
assets (truly open licences only; see docs/design.md, Ground rules, and
docs/licence-review.md). Every bundled file is
also listed in `assets/manifest.toml`, which CI checks.

| What | Where in this repo | Source | Licence | Notes |
| --- | --- | --- | --- | --- |
| Water n and k at 25 °C, 0.2–200 µm (table) | `assets/materials/water-hale-querry-1973.csv` | G. M. Hale and M. R. Querry, Appl. Opt. 12, 555–563 (1973), transcribed from the refractiveindex.info database, `main/H2O/nk/Hale.yml` | CC0-1.0 | not yet verified against the original paper |
| Water index, Sellmeier coefficients at 19, 20, 21.5, 24 °C | `crates/optobench-materials/src/bundled.rs` | M. Daimon and A. Masumura, Appl. Opt. 46, 3811–3820 (2007), transcribed from the refractiveindex.info database, `main/H2O/nk/Daimon-*.yml` | CC0-1.0 | not yet verified against the original paper |
| Fused silica, Sellmeier coefficients | `crates/optobench-materials/src/bundled.rs` | I. H. Malitson, J. Opt. Soc. Am. 55, 1205–1208 (1965), transcribed from the refractiveindex.info database, `main/SiO2/nk/Malitson.yml` | CC0-1.0 | not yet verified against the original paper |
| Standard air, dispersion formula | `crates/optobench-materials/src/bundled.rs` | P. E. Ciddor, Appl. Opt. 35, 1566–1573 (1996), transcribed from the refractiveindex.info database, `other/mixed gases/air/nk/Ciddor.yml` | CC0-1.0 | not yet verified against the original paper |
| Interface colours, corner radii, spacing and type sizes of the dark theme | `apps/optobench-ui/src/theme.rs` | SolidCraft 0.40.0-rc.3, `assets/styles/ui/palettes.rs` and `crates/ui-egui/src/theme.rs` (https://github.com/tangency-labs/solidcraft) | MIT OR Apache-2.0 | token values only; the workspace accents are optobench's own |
