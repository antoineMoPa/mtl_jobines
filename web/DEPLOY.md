# Browser release

This game is a static Cloudflare Pages direct-upload project. It uses the included
`pages.dev` hostname and has no R2 bucket, Functions, or custom domain.

The intro is at `/` and its Play links open `/game/`. Game launch parameters
such as `?start=-24,3,90` belong on the game URL. Existing root launch links
forward their parameters to the game.

## One-time setup

Use Node.js 22 or newer. Each release looks up the current Wrangler version on
npm and uses that version throughout the command. npx caches each version
separately. Deploy checks that Wrangler starts before building the game.
Authenticate with `npx --yes wrangler@latest login`.
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

In Moon, select **Publish to Cloudflare Pages**. Build runs the release dry run.
Run publishes the validated `web/dist` to the configured Pages project and branch.

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
