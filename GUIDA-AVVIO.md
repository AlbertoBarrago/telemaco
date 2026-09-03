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

# Tutti i link, uno per riga (formato: URL<TAB>testo)
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

# Attende che un selettore compaia, poi scarica la pagina
$T fetch https://example.com --selector "h1" --dump text

# Override dello User-Agent
$T fetch https://example.com --user-agent "TestAgent/1.0" --eval "navigator.userAgent"

# Cookie e storage persistenti tra le esecuzioni
$T fetch https://example.com --storage-dir ~/.telemaco-store --dump cookies

# Sub-risorse referenziate dalla pagina, una JSON per riga
$T fetch https://example.com --dump assets
```

Valori di `--dump`: `assets`, `html`, `text`, `links`, `markdown`,
`original`, `cookies`.

Nota su `--selector`: non restringe l'output. Attende che l'elemento
corrispondente compaia (utile per contenuti creati via JS) e poi esegue il
dump normale. Con `--screenshot`, un eventuale `--eval` gira prima della
cattura (utile per scrollare o preparare lo stato della pagina).

**Batch mode con `--file`:** URL uno per riga da file o da stdin (`-`);
righe vuote e righe di commento `#` vengono ignorate. Ogni URL è scaricato
grezzo (`--dump original`) e stampa una riga di stato JSON:

```bash
$T fetch --file urls.txt --concurrency 5
cat urls.txt | $T fetch --file - --concurrency 5
# {"url":"...","ok":true,"status":200,"content_type":"text/html","bytes":446,"elapsed_ms":27}
```

Per l'output renderizzato/DOM in batch usa `scrape` (§6). In batch
`--screenshot` non è disponibile.

**Valutare JS multilinea:** uno `--eval` che inizia con `const` e ha più
istruzioni ritorna `null` (V8 dà a `const` un completion value vuoto).
Avvolgi lo snippet in una IIFE:

```bash
$T fetch https://example.com --eval "(function(){ const h = document.querySelector('h1'); return h ? h.textContent : null; })()"
```

**Flag V8:** `--v8-flags` non è globale: va prima del sottocomando.

```bash
$T --v8-flags "--expose-gc" fetch https://example.com --eval "typeof gc"   # -> function
```

## 4. Siti in localhost / LAN

Di default Telemaco blocca gli indirizzi privati (protezione SSRF). Per
testare in locale passa `--allow-private-network` (o la variabile d'ambiente
`TELEMACO_ALLOW_PRIVATE_NETWORK=1`):

```bash
$T fetch http://127.0.0.1:3000 --allow-private-network --dump text
$T serve --port 9222 --allow-private-network
```

Senza il flag l'errore è: `Access to private/internal IP address ... is not
allowed`.

## 5. Server CDP: Puppeteer e Playwright

```bash
$T serve --port 9222
# endpoint: ws://127.0.0.1:9222

# Verifica rapida (in un altro terminale):
curl -s http://127.0.0.1:9222/json/version
# {"Browser":"Chrome/145.0.0.0", ..., "webSocketDebuggerUrl":"ws://127.0.0.1:9222/devtools/browser"}
curl -s http://127.0.0.1:9222/json/list
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

Flag utili di `serve`: `--host` (default `127.0.0.1`), `--workers <N>` (più
processi worker dietro la stessa porta), `--max-connections` (default 128),
`--allow-file-access` (permette ai client CDP di navigare `file://`, off di
default), `--storage-dir`, `--quiet`.

## 6. Scraping parallelo: `scrape`

```bash
# JS in parallelo su più URL, output JSON
$T scrape https://example.com https://news.ycombinator.com \
  --concurrency 25 \
  --eval "document.querySelector('h1')?.textContent ?? document.title" \
  --format json
```

Output tipico (JSON):

```json
{
  "total_urls": 2,
  "concurrency": 2,
  "total_time_ms": 1372,
  "avg_time_ms": 686.0,
  "results": [
    {
      "url": "https://example.com",
      "title": "Example Domain",
      "eval": "Example Domain",
      "time_ms": 686,
      "worker": 0
    }
  ]
}
```

Nota: `scrape` **non** legge da stdin e non accetta `-`: tutti gli URL vanno
passati come argomenti. Per il batch grezzo da stdin usa `fetch --file -` (§3).

Richiede `telemaco-worker` nella stessa directory di `telemaco` (o nel PATH).

## 7. MCP (per Claude Desktop, Cursor, ecc.)

```bash
$T mcp                      # stdio (il client lancia il processo)
$T mcp --http --port 3000   # HTTP, endpoint http://127.0.0.1:3000/mcp (default 3000)
```

Configurazione Claude Desktop:

```json
{
  "mcpServers": {
    "telemaco": { "command": "/Users/albz/Projects/telemaco/target/release/telemaco", "args": ["mcp"] }
  }
}
```

Strumenti disponibili (37 alla versione 0.1.0): `browser_navigate`,
`browser_snapshot`, `browser_interactive_elements`, `browser_click`,
`browser_fill`, `browser_fill_form`, `browser_detect_forms`, `browser_type`,
`browser_press_key`, `browser_select_option`, `browser_evaluate`,
`browser_extract`, `browser_count`, `browser_get_attribute`, `browser_scroll`,
`browser_wait_for`, `browser_wait_for_text`, `browser_network_requests`,
`browser_console_messages`, `browser_get_cookies`, `browser_set_cookie`,
`browser_clear_cookies`, `browser_storage_state`, `browser_set_storage_state`,
`browser_tab_new`, `browser_tab_list`, `browser_tab_switch`,
`browser_tab_close`, `browser_back`, `browser_forward`, `browser_reload`,
`browser_markdown`, `browser_links`, `browser_search`, `browser_close`,
`browser_screenshot`, `browser_pdf`.

Verifica rapida dell'handshake MCP (stdio):

```bash
printf '%s\n%s\n%s\n' \
  '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"smoke","version":"1.0"}}}' \
  '{"jsonrpc":"2.0","method":"notifications/initialized"}' \
  '{"jsonrpc":"2.0","id":2,"method":"tools/list"}' \
  | $T mcp 2>/dev/null | grep -c browser_
# -> 37
```

## 8. Flag globali e variabili d'ambiente

Valgono prima o dopo il sottocomando, su `fetch`, `serve`, `scrape` e `mcp`:

| Flag | Effetto |
|---|---|
| `--proxy <URL>` | Proxy HTTP o SOCKS5 (es. `socks5://user:pass@host:1080`) |
| `--stealth` | Impronta browser coerente + tracker blocking; con build stealth anche TLS impersonation |
| `--allow-private-network` | Consenti loopback/RFC1918/link-local (per dev locale) |
| `--obey-robots` | Rispetta robots.txt prima di navigare (fetch e scrape) |
| `--user-agent <UA>` | Override dello User-Agent |
| `--storage-dir <DIR>` | Cookie e localStorage persistenti tra le esecuzioni |
| `-v, --verbose` | Log di debug |
| `--v8-flags "<FLAGS>"` | Flag raw per V8; **non globale**: va prima del sottocomando |

Variabili d'ambiente utili (elenco completo in `docs/Environment-variables.md`):

| Variabile | Default | A cosa serve |
|---|---|---|
| `TELEMACO_NAV_TIMEOUT_MS` | 30000 | Timeout massimo di una navigazione |
| `TELEMACO_SCRIPT_DEADLINE_MS` | 30000 | Budget fase script della pagina |
| `TELEMACO_ALLOW_PRIVATE_NETWORK` | off | Equivalente del flag SSRF |
| `TELEMACO_TIMEZONE` | Europe/Berlin | Fissa il fuso (coerente con il proxy) |
| `TELEMACO_PROXY` | — | Proxy di default per i worker di `scrape` |

## 9. Test

### 9.1 Smoke test CLI (offline, ~1 minuto)

Verifiche end-to-end sul binario con una fixture locale: nessuna rete
esterna, nessun sito da controllare. Copia e incolla tutto in un terminale;
ogni test stampa `PASS` o `FAIL`. Tutti i comandi sono verificati sul build
0.1.0 con feature `render`.

```bash
T=$PWD/target/release/telemaco
D=$(mktemp -d); PORT=8099; CDP=9223; U="http://127.0.0.1:$PORT"
FAILED=0; ok(){ echo "PASS: $1"; }; bad(){ echo "FAIL: $1"; FAILED=1; }

# --- fixture locale ---
mkdir -p "$D/fixture/private"
cat > "$D/fixture/index.html" <<'EOF'
<!DOCTYPE html>
<html><head><title>Fixture Telemaco</title></head><body>
  <h1 id="titolo">Titolo fixture</h1><p>Paragrafo statico.</p>
  <a href="/page2.html">Pagina 2</a>
  <script>
    document.cookie = "tm_cookie=123; path=/";
    const d = document.createElement('p');
    d.id = 'dinamico';
    d.textContent = 'Contenuto injectato da JS';
    document.body.appendChild(d);
  </script>
</body></html>
EOF
printf '<!DOCTYPE html><html><head><title>Fixture pagina 2</title></head><body>due</body></html>' > "$D/fixture/page2.html"
printf 'User-agent: *\nDisallow: /private/\n' > "$D/fixture/robots.txt"
printf '<!DOCTYPE html><html><body>segreta</body></html>' > "$D/fixture/private/segreta.html"
printf '%s\n# commento\n%s\n' "$U/index.html" "$U/page2.html" > "$D/urls.txt"
python3 -m http.server "$PORT" --directory "$D/fixture" >/dev/null 2>&1 &
HTTPD=$!; sleep 1

# 1) versione
[ "$($T --version)" = "telemaco 0.1.0" ] && ok "versione" || bad "versione"

# 2) gate SSRF: senza flag il fetch a 127.0.0.1 deve fallire
$T fetch "$U/index.html" --dump text 2>&1 | grep -q "not allowed" \
  && ok "SSRF bloccato senza flag" || bad "gate SSRF"

# 3) fetch locale con flag SSRF
$T fetch "$U/index.html" --allow-private-network -q --dump text | grep -q "Titolo fixture" \
  && ok "fetch + --allow-private-network" || bad "fetch locale"

# 4) --eval su contenuto creato dal JS (IIFE)
$T fetch "$U/index.html" --allow-private-network -q \
  --eval "(function(){ return document.querySelector('#dinamico').textContent; })()" \
  | grep -q "Contenuto injectato" && ok "--eval dinamico" || bad "--eval"

# 5) --dump cookies
$T fetch "$U/index.html" --allow-private-network -q --dump cookies | grep -q "tm_cookie" \
  && ok "--dump cookies" || bad "cookies"

# 6) --screenshot: PNG 1280x720 non vuoto (sips è di macOS); lo salviamo
#    dentro fixture/ perché il test 7 lo riscarichi via HTTP
$T fetch "$U/index.html" --allow-private-network -q --screenshot "$D/fixture/shot.png" >/dev/null 2>&1
[ -s "$D/fixture/shot.png" ] \
  && [ "$(sips -g pixelWidth "$D/fixture/shot.png" | awk '/pixelWidth/{print $2}')" = "1280" ] \
  && ok "--screenshot 1280x720" || bad "screenshot"

# 7) --dump original binario-safe: gli hash devono coincidere
$T fetch "$U/shot.png" --allow-private-network -q --dump original > "$D/copy.png" 2>/dev/null
[ "$(shasum "$D/fixture/shot.png" | awk '{print $1}')" = "$(shasum "$D/copy.png" | awk '{print $1}')" ] \
  && ok "--dump original binario-safe" || bad "dump original"

# 8) batch mode da stdin: 2 URL ok
N=$(cat "$D/urls.txt" | $T fetch --file - --allow-private-network 2>/dev/null | grep -c '"ok":true')
[ "$N" = "2" ] && ok "fetch --file (2 URL)" || bad "batch mode"

# 9) scrape parallelo
$T scrape "$U/index.html" "$U/page2.html" --allow-private-network --concurrency 2 \
  --eval "document.title" --format json 2>/dev/null | grep -q '"total_urls": 2' \
  && ok "scrape parallelo" || bad "scrape"

# 10) --obey-robots: i percorsi in Disallow vengono bloccati
$T fetch "$U/private/segreta.html" --allow-private-network --obey-robots 2>&1 \
  | grep -q "Blocked by robots.txt" && ok "--obey-robots" || bad "obey-robots"

# 11) --storage-dir: il cookie sopravvive tra le esecuzioni
rm -rf "$D/store"
$T fetch "$U/index.html" --allow-private-network -q --storage-dir "$D/store" >/dev/null 2>&1
$T fetch "$U/page2.html" --allow-private-network -q --storage-dir "$D/store" \
  --eval "document.cookie" | grep -q "tm_cookie=123" \
  && ok "--storage-dir persiste i cookie" || bad "storage-dir"

# 12) server CDP
$T serve --port "$CDP" --allow-private-network >/dev/null 2>&1 &
SVR=$!; sleep 2
curl -s "http://127.0.0.1:$CDP/json/version" | grep -q "webSocketDebuggerUrl" \
  && ok "serve CDP" || bad "serve"

# 13) MCP: handshake stdio e lista strumenti
printf '%s\n%s\n%s\n' \
  '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"smoke","version":"1.0"}}}' \
  '{"jsonrpc":"2.0","method":"notifications/initialized"}' \
  '{"jsonrpc":"2.0","id":2,"method":"tools/list"}' \
  | $T mcp 2>/dev/null | grep -q '"browser_navigate"' \
  && ok "MCP tools/list" || bad "MCP"

kill $HTTPD $SVR 2>/dev/null
[ "$FAILED" = "0" ] && echo "TUTTI I TEST PASSANO" || echo "CI SONO FALLIMENTI"
```

### 9.2 Suite Rust

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

### 9.3 Obstacle course

Il gate comportamentale è l'obstacle course nel repo companion
`telemaco-benchmark` (33 stadi, attesi 33/33, tutto offline; il repo va
clonato a parte):

```bash
TELEMACO_BIN=./target/release/telemaco python3 obstacle-course/run.py --runs 1 --warmup 0
```

## 10. Problemi comuni

| Sintomo | Causa e rimedio |
|---|---|
| `--eval` ritorna `null` | Multi-statement che inizia con `const`: usa una IIFE (§3) |
| Fetch a `localhost` rifiutato | SSRF gate: aggiungi `--allow-private-network` (§4) |
| `--screenshot` non disponibile | Serve il build con `--features render` (§2) |
| Stealth "debole" | Il binario attuale è senza feature `stealth`: ricompila con `--features render,stealth` (serve cmake) |
| `scrape` non parte | Manca `telemaco-worker` accanto al binario `telemaco` |
| `scrape -` o pipe in `scrape` fallisce | `scrape` non legge da stdin: passa gli URL come argomenti, o usa `fetch --file -` (§3) |
| `--v8-flags` rifiutato dopo il sottocomando | Non è globale: va prima, es. `$T --v8-flags "..." fetch ...` (§3) |
| `--selector` sembra non filtrare l'output | Comportamento corretto: attende l'elemento e poi fa il dump completo (§3) |
| Build lentissima la prima volta | Normale: compila V8 (~5 min). Le successive sono in secondi; usa `-p` per limitare il perimetro |
| Test runtime falliti con `cargo test` | Non usare `cargo test`: usa `cargo nextest run` (§9.2) |