# Browser release

This game is a static Cloudflare Pages direct-upload project. It uses the included
`pages.dev` hostname and has no R2 bucket, Functions, or custom domain.

## One-time setup

Use Node.js 22 or newer. The commands run `npx --yes wrangler`, which downloads
Wrangler on first use if needed. Authenticate with `npx wrangler login`.
The release commands look for a compatible Node on `PATH`, in nvm, or in
Homebrew. Set `NODE_BIN` to its absolute path if Node is installed elsewhere.
You can set `WRANGLER_BIN` to an existing Wrangler
executable to skip `npx`. For an API token, Wrangler also
accepts `CLOUDFLARE_ACCOUNT_ID` and `CLOUDFLARE_API_TOKEN` in the environment.
Do not put credentials in the repository.

```sh
make web-project
```

The project defaults to `mtl-jobines` with production branch `main`. Set
`PAGES_PROJECT` or `PAGES_BRANCH` to choose other values. The project-creation
command is only needed once. Its Pages address is shown by Wrangler; if the
preferred name is taken, choose another project name.

## Build and deploy

```sh
cargo xtask web-deploy --dry-run  # Build and validate without publishing
make web-deploy                   # Build, validate, then publish
```

`make web-check` validates an existing `web/dist` without rebuilding. Validation
requires every listed asset, rejects files above Pages' 25 MiB limit or a site
above 20,000 files, and confirms that the native `[bug]` report UI is absent
from the browser files. The deploy command always rebuilds first, so old files
cannot be uploaded accidentally.

Direct-upload projects do not use Git-integrated automatic deployment. See the
[Cloudflare Pages direct-upload
guide](https://developers.cloudflare.com/pages/get-started/direct-upload/)
and [Wrangler Pages commands](https://developers.cloudflare.com/workers/wrangler/commands/pages/).

## Bundled assets

The web build bundles `mobile-controls.js`, `mobile-controls.css`, and the existing PT Sans
font and license. No third-party icon or font requests are needed at runtime.
