# Changelog

All notable changes to Telemaco are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.2.1] - 2026-09-09

### Added

- **Custom Elements v1: direct construction with `new Ctor()`.** The `Element`
  constructor now handles creating a custom element outside
  `document.createElement` (the pattern used by LWC, Lit and Stencil): it
  allocates a real node from the registered name via
  `customElements.getName(new.target)`. An unregistered class throws
  `TypeError: Illegal constructor` (spec-correct behavior).
- **Custom Elements v1: `attributeChangedCallback` lifecycle.** Implemented the
  `observedAttributes`/`attributeChangedCallback` hook, wired into
  `setAttribute`, `removeAttribute` and the upgrade step. It was not
  implemented at all before.

### Fixed

- **React/Next.js hydration false alarms.** The `Minified React error #418`
  (an SSR hydration mismatch, not an engine crash) is now logged as a warning
  instead of an error. The page still renders normally.

### Known issue

- **Salesforce documentation pages (LWR/LWC) do not render their content.** On
  `developer.salesforce.com/docs/...` the `doc-xml-content` component mounts an
  empty shadow root: the Lightning Web Runtime module-loading chain never gets
  to request the content. The Custom Elements fixes above resolve the crash and
  the lifecycle hook, but not the Apex content loading, which remains
  unresolved. The rest of the engine works normally.

[0.2.1]: https://github.com/AlbertoBarrago/telemaco/releases/tag/v0.2.1
