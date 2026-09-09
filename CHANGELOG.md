# Changelog

Tutte le modifiche notevoli a Telemaco sono documentate in questo file.

Il formato segue [Keep a Changelog](https://keepachangelog.com/it/1.1.0/),
e il progetto aderisce al [Versionamento Semantico](https://semver.org/lang/it/).

## [0.2.1] - 2026-09-09

### Aggiunto

- **Custom Elements v1: costruzione diretta con `new Ctor()`.** Il costruttore
  di `Element` ora gestisce la creazione di un custom element fuori da
  `document.createElement` (pattern usato da LWC, Lit e Stencil): alloca un
  nodo reale dal nome registrato via `customElements.getName(new.target)`.
  Una classe non registrata lancia `TypeError: Illegal constructor`
  (comportamento da spec).
- **Custom Elements v1: lifecycle `attributeChangedCallback`.** Implementato
  l'hook `observedAttributes`/`attributeChangedCallback`, agganciato a
  `setAttribute`, `removeAttribute` e allo step di upgrade. Prima non era
  implementato per nulla.

### Corretto

- **Falsi allarmi di hydration di React/Next.js.** L'errore
  `Minified React error #418` (mismatch di hydration SSR, non un crash del
  motore) ora è loggato come warning invece che come errore. La pagina
  continua a renderizzare normalmente.

### Problema noto

- **Le pagine di documentazione Salesforce (LWR/LWC) non renderizzano il
  contenuto.** Su
  `developer.salesforce.com/docs/...` il componente `doc-xml-content` monta
  uno shadow root vuoto: la catena di caricamento moduli di Lightning Web
  Runtime non arriva a richiedere il contenuto. I fix Custom Elements sopra
  risolvono il crash e il lifecycle hook, ma non il caricamento del contenuto
  Apex, che resta non risolto. Il resto del motore funziona normalmente.

[0.2.1]: https://github.com/AlbertoBarrago/telemaco/releases/tag/v0.2.1
