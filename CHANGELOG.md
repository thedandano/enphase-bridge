# Changelog

## 0.1.0 (2026-04-30)


### Features

* dockerize deployment with multi-platform image and GHCR push ([4e75f76](https://github.com/thedandano/enphase-bridge/commit/4e75f76d23ccefbd605492376b98857f28d934d1))
* implement enphase-ds — full data collection, API, TOU, and CI/CD ([90a0ab9](https://github.com/thedandano/enphase-bridge/commit/90a0ab98c0f10f2a6c3cd3e4ffebcfc5bd2f420c))


### Bug Fixes

* add check_jwt session auth for IQ Gateway firmware 7.x+ IVP endpoints ([bec01d4](https://github.com/thedandano/enphase-bridge/commit/bec01d4224cf0c1ad9b16f66e2a44188a1e827e0))
* add curl to Docker image and log internal errors ([66f432a](https://github.com/thedandano/enphase-bridge/commit/66f432ad67b1c541352bd3a9e1d7bc41c5497ccd))
* **cd:** bump Dockerfile to rust:1.90; inline architecture into README ([2dd8b93](https://github.com/thedandano/enphase-bridge/commit/2dd8b93404e99e1d10ba113985f2c4688d360d9c))
* derive consumption from energy balance using net meter bidirectional counters ([c0ff1ff](https://github.com/thedandano/enphase-bridge/commit/c0ff1ffa80e22bdc0a9f9fea66d24440d7afa387))
* make TOU provider-agnostic; rename sdge_rate_label to rate_label ([1a643e9](https://github.com/thedandano/enphase-bridge/commit/1a643e981ed9035bd0987acdbfa8c768904514ce))
* **release:** grant GITHUB_TOKEN permissions for PR creation ([5394344](https://github.com/thedandano/enphase-bridge/commit/53943448491dfe337a396ebe1370929374d58924))
* **release:** remove workflow-level permissions conflict ([a33912a](https://github.com/thedandano/enphase-bridge/commit/a33912a6eb4b2d53b8fa7a67f1956fb8d87a55ab))
* source grid counters from EID_CONSUMPTION and add startup meter probe ([0484347](https://github.com/thedandano/enphase-bridge/commit/04843479037a212fb5ad1187001e2f1d743b5814))
* use double-underscore prefix for env var config loading ([158d7bb](https://github.com/thedandano/enphase-bridge/commit/158d7bb6c6b34dc3d30a9e8929ce3a4f631b77f1))
