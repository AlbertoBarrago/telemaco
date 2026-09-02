# Guida di avvio — Telemaco

Guida rapida e pratica per compilare e usare Telemaco sul tuo Mac.
Per la documentazione completa vedi `README.md` e `docs/`.

Telemaco è un browser headless in Rust: esegue JavaScript vero (V8), mantiene
un DOM reale, fa rendering nativo, parla il Chrome DevTools Protocol e si
comporta da sostituto drop-in di headless Chrome per Puppeteer e Playwright.

---

## 1. Binari già pronti

Il build locale esiste già in `target/release/`:

| Binario | Cosa fa |
|---|---|
| `telemaco` | CLI: `fetch`, `serve` (server CDP), `scrape`, `mcp` |
| `telemaco-worker` | Worker per lo scraping parallelo (serve accanto a `telemaco` per `scrape`) |
| `telemaco-gui` | Finestra nativa con rendering live della pagina |

Il build attuale ha il **rendering attivo** (screenshot e PDF funzionano), ma
**non** la feature `stealth` completa: il flag `--stealth` funziona (impronta
browser coerente), mentre la TLS impersonation richiede un rebuild (vedi §2).

Tutti gli esempi qui sotto usano il percorso diretto:

```bash
T=/Users/albz/Projects/telemaco/target/release/telemaco
$T --version        # telemaco 0.1.0
```

Se preferisci, crea un alias o un symlink in `~/.local/bin`.

## 2. Compilare

Prima build: compila V8 dai sorgenti, ~5 minuti e qualche GB di disco. Le
build successive sono questione di secondi.

```bash
cd /Users/albz/Projects/telemaco

# Rendering (screenshot, PDF, screencast)
CARGO_INCREMENTAL=0 cargo build --release -p telemaco-cli --bins --features render

# Rendering + stealth completo (wreq/BoringSSL, serve cmake installato)
CARGO_INCREMENTAL=0 cargo build --release -p telemaco-cli --bins --features render,stealth

# GUI (richiede la feature render, che è default per quel crate)
cargo build --release -p telemaco-gui
```

Requisiti: Rust 1.75+ (`rustup.rs`). Per lo stealth serve anche CMake + Clang.
Usa `-p telemaco-cli` per non rilinkare l'intero workspace: la ricompilazione
di V8 è il costo da evitare.

## 3. Primo utilizzo: `fetch`

Carica una pagina, esegue il JS e stampa il contenuto.

```bash
T=$PWD/target/release/telemaco

# Titolo della pagina
$T fetch https://example.com --eval "document.title"

# Tutti i link, uno per riga
$T fetch https://example.com --dump links

# HTML renderizzato (dopo l'esecuzione del JavaScript)
$T fetch https://news.ycombinator.com --dump html

# Markdown (comodo per gli LLM)
$T fetch https://example.com --dump markdown

# Testo con attesa del contenuto dinamico
$T fetch https://example.com --wait-until networkidle0 --timeout 10 --dump text

# Scrive su file
$T fetch https://example.com --dump text --output page.txt

# Corpo HTTP grezzo, binario-safe (bypassa JS/DOM)
$T fetch https://picsum.photos/200/300 --dump original > photo.jpg

# Screenshot PNG (viewport 1280x720 di default)
$T fetch https://example.com --screenshot page.png
```

Valori di `--dump`: `assets`, `html`, `text`, `links`, `markdown`,
`original`, `cookies`.

**Valutare JS multilinea:** uno `--eval` che inizia con `const` e ha più
istruzioni ritorna `null` (V8 dà a `const` un completion value vuoto).
Avvolgi lo snippet in una IIFE:

```bash
$T fetch https://example.com --eval "(function(){ const h = document.querySelector('h1'); return h ? h.textContent : null; })()"
```

## 4. Siti in localhost / LAN

Di default Telemaco blocca gli indirizzi privati (protezione SSRF). Per
testare in locale passa `--allow-private-network` (o la variabile d'ambiente
`TELEMACO_ALLOW_PRIVATE_NETWORK=1`):

```bash
$T fetch http://127.0.0.1:3000 --allow-private-network --dump text
$T serve --port 9222 --allow-private-network
```

## 5. Server CDP: Puppeteer e Playwright

```bash
$T serve --port 9222
# endpoint: ws://127.0.0.1:9222
```

Puppeteer:

```javascript
import puppeteer from 'puppeteer-core';
const browser = await puppeteer.connect({ browserWSEndpoint: 'ws://127.0.0.1:9222/devtools/browser' });
const page = await browser.newPage();
await page.goto('https://example.com');
console.log(await page.title());
await browser.disconnect();
```

Playwright:

```javascript
import { chromium } from 'playwright-core';
const browser = await chromium.connectOverCDP({ endpointURL: 'ws://127.0.0.1:9222' });
const page = await browser.newContext().then(ctx => ctx.newPage());
await page.goto('https://example.com');
console.log(await page.title());
```

Con i build render-enabled funzionano anche `page.screenshot()` (anche
fullPage) e `page.pdf()`.

## 6. Scraping parallelo: `scrape`

```bash
# JS in parallelo su più URL, output JSON
$T scrape https://example.com https://news.ycombinator.com \
  --concurrency 25 \
  --eval "document.querySelector('h1')?.textContent ?? document.title" \
  --format json

# Lista URL da stdin
cat urls.txt | $T scrape - --eval "document.title" --concurrency 20 --quiet
```

Richiede `telemaco-worker` nella stessa directory di `telemaco` (o nel PATH).

## 7. MCP (per Claude Desktop, Cursor, ecc.)

```bash
$T mcp                      # stdio (il client lancia il processo)
$T mcp --http --port 8080   # HTTP, endpoint http://127.0.0.1:8080/mcp
```

Configurazione Claude Desktop:

```json
{
  "mcpServers": {
    "telemaco": { "command": "/Users/albz/Projects/telemaco/target/release/telemaco", "args": ["mcp"] }
  }
}
```

Strumenti disponibili: `browser_navigate`, `browser_snapshot`,
`browser_screenshot`, `browser_pdf`, `browser_click`, `browser_fill`,
`browser_type`, `browser_press_key`, `browser_select_option`,
`browser_evaluate`, `browser_wait_for`, `browser_network_requests`,
`browser_console_messages`, `browser_close`.

## 8. GUI

```bash
./target/release/telemaco-gui https://example.com
```

Finestra nativa con omnibar, back/forward, streaming live della pagina e input
reale (mouse, tastiera, scroll). Per aprire pagine locali aggiungi
`--allow-private-network`. Per pacchettizzare l'app macOS:
`scripts/make-dmg.sh` (produce `Telemaco.dmg`).

## 9. Flag globali e variabili d'ambiente

Valgono prima o dopo il sottocomando, su `fetch`, `serve`, `scrape` e `mcp`:

| Flag | Effetto |
|---|---|
| `--proxy <URL>` | Proxy HTTP o SOCKS5 (es. `socks5://user:pass@host:1080`) |
| `--stealth` | Impronta browser coerente + tracker blocking; con build stealth anche TLS impersonation |
| `--allow-private-network` | Consenti loopback/RFC1918/link-local (per dev locale) |

Variabili d'ambiente utili (elenco completo in `docs/Environment-variables.md`):

| Variabile | Default | A cosa serve |
|---|---|---|
| `TELEMACO_NAV_TIMEOUT_MS` | 30000 | Timeout massimo di una navigazione |
| `TELEMACO_SCRIPT_DEADLINE_MS` | 30000 | Budget fase script della pagina |
| `TELEMACO_ALLOW_PRIVATE_NETWORK` | off | Equivalente del flag SSRF |
| `TELEMACO_TIMEZONE` | Europe/Berlin | Fissa il fuso (coerente con il proxy) |
| `TELEMACO_PROXY` | — | Proxy di default per i worker di `scrape` |

## 10. Test

**Usa `cargo nextest`, non `cargo test`**: il motore ha un solo isolato V8 per
processo, quindi i test runtime falliscono con `cargo test`. Nextest esegue
ogni test in un processo separato.

```bash
cargo install cargo-nextest   # se manca

# Suite completa
cargo nextest run --release --features render --no-fail-fast

# Solo un crate
cargo nextest run --release --features render -p telemaco-cli
```

Il gate comportamentale è l'obstacle course nel repo companion
`telemaco-benchmark` (33 stadi, attesi 33/33, tutto offline):

```bash
TELEMACO_BIN=./target/release/telemaco python3 obstacle-course/run.py --runs 1 --warmup 0
```

## 11. Problemi comuni

| Sintomo | Causa e rimedio |
|---|---|
| `--eval` ritorna `null` | Multi-statement che inizia con `const`: usa una IIFE (§3) |
| Fetch a `localhost` rifiutato | SSRF gate: aggiungi `--allow-private-network` |
| `--screenshot` non disponibile | Serve il build con `--features render` (§2) |
| Stealth "debole" | Il binario attuale è senza feature `stealth`: ricompila con `--features render,stealth` (serve cmake) |
| `scrape` non parte | Manca `telemaco-worker` accanto al binario `telemaco` |
| Build lentissima la prima volta | Normale: compila V8 (~5 min). Le successive sono in secondi; usa `-p` per limitare il perimetro |
| Test runtime falliti con `cargo test` | Non usare `cargo test`: usa `cargo nextest run` (§10) |