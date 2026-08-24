# qrate-site design system — how to build with it

**This is a tokens-and-styles system, not a component library.** `window.QrateSite`
is intentionally empty and there are no `components/` entries. Build screens from
plain HTML/JSX and the utility classes below — that is the whole idiom. Do not
import components from the bundle; there are none.

Everything comes from one file: `styles.css`, which `@import`s `_ds_bundle.css`
(the compiled Tailwind v4 output of `src/styles/global.css`) and the Google Fonts
`@import` for **Archivo** (sans) and **IBM Plex Mono** (mono). Read
`_ds_bundle.css` before styling — it is the authoritative class and token list.

## Theming

Six palettes ship. Set `data-theme` on `<html>` (or any ancestor) to switch:
`qrate-light` (default, also applied on bare `:root`), `qrate-dark`,
`gruvbox-light`, `gruvbox-dark`, `everforest-light`, `everforest-dark`.
Only the accent / wash / `--app-*` values swap; the paper-and-ink page shell is
fixed across all six.

`_ds_bundle.css` also styles `html` and `body` directly: `body` is a centred
sheet capped at `max-width: 1280px` on `--paper`, 15px/1.6 Archivo, with
`--ground` behind it. Build page content inside that sheet — do not re-set the
body background or width.

## Token families (CSS custom properties)

- **Page shell, theme-invariant:** `--paper` `--ink` `--body` `--dim` `--faint`
  `--nav-fg` `--rule` `--rule-soft` `--rule-nav` `--rust` `--ground`
- **Theme-swapped:** `--accent` `--on-accent` `--hero-wash` `--shot-filter`
  `--logo-a` `--logo-b`
- **App-chrome (for screenshots/mockups of the qrate desktop app):**
  `--app-bg` `--app-fg` `--app-muted` `--app-border` `--app-title` `--app-head`
  `--app-head-fg` `--app-even` `--app-active-bg` `--app-active-border`
  `--app-row-border` `--app-danger` `--app-warn`
- **Type:** `--font-sans` (Archivo), `--font-mono` (IBM Plex Mono)

## Utility vocabulary

Standard Tailwind v4 utilities, compiled with this theme. **Colour comes from the
token names below, not Tailwind's palettes** — reach for `bg-accent` and
`text-body`, never `bg-blue-500` or `text-gray-600`, even where a palette class
happens to have been compiled in:

| Family | Real names |
|---|---|
| Background | `bg-paper` `bg-ink` `bg-ground` `bg-wash` `bg-accent` `bg-rule` `bg-white` `bg-app-bg` `bg-app-even` `bg-app-head` `bg-app-title` `bg-app-active-bg` |
| Text | `text-ink` `text-body` `text-dim` `text-faint` `text-nav` `text-rust` `text-accent` `text-on-accent` `text-paper` `text-app-fg` `text-app-muted` `text-app-danger` `text-app-warn` |
| Border | `border-rule` `border-rule-soft` `border-rule-nav` `border-ink` `border-accent` `border-app-border` `border-app-row-border` |
| Type | `text-xs`…`text-5xl`, `font-sans` `font-mono`, `font-medium`/`semibold`/`bold`, `tracking-tight`/`wide`, `leading-tight`…`leading-loose` |
| Layout | `flex` `grid` `grid-cols-1`…`grid-cols-12` `col-span-*` `gap-*` `items-*` `justify-*` `p-*` `px-*` `mt-*` `max-w-*` `w-*` `rounded-*` `shadow-*` |
| Variants | `sm:` `md:` `lg:` `xl:` on layout/spacing/type; `hover:` `focus-visible:` `active:` `disabled:` on colour, `underline`, `opacity-*` |

Three custom utilities carry the site's own conventions — prefer them over
hand-rolled equivalents:

- **`gut`** — the standard side gutter (18px, 34px at ≥48rem). Use on every
  full-bleed band instead of `px-*`.
- **`eyebrow`** — mono uppercase 11px accent label; the section kicker.
- **`shot`** — white frame + shadow around a light-mode screenshot; its `img`
  child is re-tinted by `--shot-filter` so screenshots match the active theme.

Headings already get `letter-spacing: -0.03em` and `text-wrap: balance`; links
are `color: inherit`, underlined on hover. Don't restate those.

## Idiomatic snippet

```jsx
<section className="gut py-10 border-b border-rule-soft">
  <p className="eyebrow">Archival, offline-first</p>
  <h2 className="text-3xl text-ink mt-2">Digitize a collection once.</h2>
  <p className="text-body mt-3 max-w-3xl">
    qrate reads your folders where they already live.
  </p>
  <div className="grid grid-cols-1 md:grid-cols-2 gap-6 mt-5">
    <figure className="shot"><img src="/shots/light-table.webp" alt="" /></figure>
    <ul className="text-body flex flex-col gap-2">
      <li className="border-l border-accent pl-3">No import step</li>
      <li className="border-l border-rule pl-3">Plugins in Lua</li>
    </ul>
  </div>
  <a href="#" className="inline-block mt-5 bg-accent text-on-accent px-4 py-2 hover:underline">
    Download
  </a>
</section>
```
