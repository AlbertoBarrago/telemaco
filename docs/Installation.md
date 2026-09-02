## Linux x86_64

```bash
curl -LO https://github.com/AlbertoBarrago/telemaco/releases/latest/download/telemaco-x86_64-linux.tar.gz
tar xzf telemaco-x86_64-linux.tar.gz
./telemaco --version
```

## Linux ARM64

```bash
curl -LO https://github.com/AlbertoBarrago/telemaco/releases/latest/download/telemaco-aarch64-linux.tar.gz
tar xzf telemaco-aarch64-linux.tar.gz
./telemaco --version
```

Linux builds target Ubuntu 22.04 and require glibc 2.35+.

## macOS Apple Silicon

```bash
curl -LO https://github.com/AlbertoBarrago/telemaco/releases/latest/download/telemaco-aarch64-macos.tar.gz
tar xzf telemaco-aarch64-macos.tar.gz
./telemaco --version
```

## macOS Intel

```bash
curl -LO https://github.com/AlbertoBarrago/telemaco/releases/latest/download/telemaco-x86_64-macos.tar.gz
tar xzf telemaco-x86_64-macos.tar.gz
./telemaco --version
```

## Windows

Download the `.zip` from [Releases](https://github.com/AlbertoBarrago/telemaco/releases), extract, run `telemaco.exe --version`.

## Arch Linux (AUR)

```bash
yay -S telemaco-browser
```

## Docker

```bash
docker run -d --name telemaco -p 127.0.0.1:9222:9222 AlbertoBarrago/telemaco
```

Image: [AlbertoBarrago/telemaco](https://hub.docker.com/r/AlbertoBarrago/telemaco). Built on `distroless/cc`, with no shell or package manager in the runtime image.

Official archives and the Docker image include the rendering engine. Source
builders must pass `--features render`; see [Build from source](Build-from-source.md).

## From source

See [Build from source](Build-from-source.md).

## What's in the archive

- `telemaco`: CLI and CDP server.
- `telemaco-worker`: helper for the parallel `scrape` command. Keep both in the same directory.

Archive suffixes identify the feature set: no suffix includes rendering,
`-stealth` includes rendering and stealth, `-no-render` includes neither, and
`-no-render-stealth` includes stealth without rendering.

## Smoke test

```bash
./telemaco fetch https://example.com --eval "document.title"
./telemaco fetch https://example.com --screenshot smoke.png
```

Expected output: `"Example Domain"`, followed by a nonempty PNG at `smoke.png`.

## Troubleshooting

`cannot execute binary file`: wrong arch. Check `uname -m`.

`GLIBC_2.35 not found`: distro is older than Ubuntu 22.04. Use Docker or build from source.

macOS Gatekeeper warning: `xattr -d com.apple.quarantine ./telemaco`.
