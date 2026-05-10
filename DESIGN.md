---
name: ZynC
description: Browser profile sync for Zen — warm, clear, a little delightful.
colors:
  parchment: "#f2f0e3"
  terracotta-signal: "#f76f53"
  terracotta-deep: "#e5614a"
  near-black-ink: "#28262f"
  near-black-hover: "#38363f"
  deep-ink: "#1a1914"
  aged-ochre: "#9b9781"
  dusty-divider: "#d4d2ca"
  sage-wash: "#e7f9d9"
  forest-affirmation: "#336d3f"
  blush-alert: "#fadbd1"
  alarm-red: "#b80000"
typography:
  display:
    fontFamily: "'Bricolage Grotesque', -apple-system, BlinkMacSystemFont, 'Segoe UI', system-ui, sans-serif"
    fontSize: "32px"
    fontWeight: 600
    lineHeight: 1.2
  button:
    fontFamily: "'Bricolage Grotesque', system-ui, sans-serif"
    fontSize: "24px"
    fontWeight: 600
    letterSpacing: "0.04em"
  body:
    fontFamily: "'Bricolage Grotesque', system-ui, sans-serif"
    fontSize: "14px"
    fontWeight: 500
    lineHeight: 1.4
  label:
    fontFamily: "'Bricolage Grotesque', system-ui, sans-serif"
    fontSize: "11px"
    fontWeight: 400
    letterSpacing: "0.05em"
  small:
    fontFamily: "'Bricolage Grotesque', system-ui, sans-serif"
    fontSize: "12px"
    fontWeight: 400
rounded:
  component: "10px"
  message: "8px"
  small: "6px"
  checkbox: "4px"
spacing:
  content: "32px"
  section: "24px"
  gap: "18px"
  tight: "10px"
  micro: "8px"
components:
  button-primary:
    backgroundColor: "{colors.terracotta-signal}"
    textColor: "#ffffff"
    rounded: "{rounded.component}"
    padding: "16px 24px"
    typography: "{typography.button}"
  button-primary-hover:
    backgroundColor: "{colors.terracotta-deep}"
    textColor: "#ffffff"
    rounded: "{rounded.component}"
  button-secondary:
    backgroundColor: "{colors.near-black-ink}"
    textColor: "#ffffff"
    rounded: "{rounded.component}"
    padding: "16px 24px"
    typography: "{typography.button}"
  button-secondary-hover:
    backgroundColor: "{colors.near-black-hover}"
    textColor: "#ffffff"
  button-ghost:
    backgroundColor: "rgba(0,0,0,0.05)"
    textColor: "{colors.deep-ink}"
    rounded: "{rounded.component}"
    padding: "16px 24px"
    typography: "{typography.button}"
  button-ghost-hover:
    backgroundColor: "rgba(0,0,0,0.09)"
    textColor: "{colors.deep-ink}"
  input-default:
    backgroundColor: "transparent"
    textColor: "{colors.deep-ink}"
    rounded: "{rounded.component}"
    padding: "16px 13px"
  input-focus:
    backgroundColor: "transparent"
    textColor: "{colors.deep-ink}"
    rounded: "{rounded.component}"
  message-error:
    backgroundColor: "{colors.blush-alert}"
    textColor: "{colors.alarm-red}"
    rounded: "{rounded.message}"
    padding: "10px 16px"
  message-success:
    backgroundColor: "{colors.sage-wash}"
    textColor: "{colors.forest-affirmation}"
    rounded: "{rounded.message}"
    padding: "10px 16px"
  message-neutral:
    backgroundColor: "rgba(0,0,0,0.05)"
    textColor: "{colors.deep-ink}"
    rounded: "{rounded.message}"
    padding: "10px 16px"
---

# Design System: ZynC

## 1. Overview

**Creative North Star: "The Warm Instruction Card"**

ZynC feels like the well-considered card that came folded inside a piece of hardware you actually wanted. The parchment background is not beige and it is not off-white; it is specifically this cream, warm and deliberate, chosen to make the terracotta orange feel like it belongs rather than shouts. Bricolage Grotesque at large weights does the structural work. Nothing decorates that doesn't earn its place in a 420-pixel-wide window.

The system is committed, not restrained. One saturated color, Terracotta Signal, carries 30-40% of interactive surface area. It appears on primary buttons and checked states only; its rarity is the point. The near-black ink handles secondary actions with quiet authority. Everything else is the parchment canvas and its translucent overlays.

This system explicitly rejects: developer tooling aesthetics (terminal dark, dense monospace, purple gradients); generic SaaS palettes (white background, blue primaries, card grids); and overwrought macOS chrome (translucency, vibrancy, excessive system-native metaphors). If it could belong on a Settings pane in macOS 14, the design has drifted. If it could belong in a fintech dashboard, same verdict.

**Key Characteristics:**
- Parchment canvas, not white
- One committed accent, used with restraint
- Typography at display sizes (24-32px) on a small fixed window
- Flat surfaces, no shadows, depth through tonal overlays
- Status communicated through colored message boxes, never inline inline text
- Two actions: Upload and Pull. The interface has nothing else to say.

## 2. Colors: The Terracotta & Parchment Palette

A two-pole palette: warm cream ground with a single terracotta accent. Every other color is either a functional overlay or a semantic signal (success/error).

### Primary
- **Terracotta Signal** (#f76f53 / oklch(65% 0.165 27)): The only saturated color in the system. Used on primary buttons (Upload, Save & Pair, Copy) and checked checkbox states. Appears nowhere decorative. Its rarity is structural.
- **Terracotta Deep** (#e5614a / oklch(60% 0.165 27)): The hover state for Terracotta Signal only. Never used at rest.

### Secondary
- **Near-Black Ink** (#28262f / oklch(20% 0.010 290)): Secondary actions (Pull, Forget buttons, app header background tint). This is the dark weight of the system, warm-leaning rather than pure black.
- **Near-Black Hover** (#38363f / oklch(26% 0.010 290)): Hover for Near-Black Ink surfaces.

### Neutral
- **Parchment** (#f2f0e3 / oklch(95% 0.008 85)): The entire canvas. Every screen background, active tab surface. Warm-tinted, not white, not beige.
- **Deep Ink** (#1a1914 / oklch(15% 0.008 85)): Primary text, focused input borders, icons. Tinted toward the parchment hue.
- **Aged Ochre** (#9b9781 / oklch(65% 0.012 85)): Muted text, placeholder text, inactive input borders, the generate-passphrase button.
- **Dusty Divider** (#d4d2ca / oklch(85% 0.006 85)): Horizontal rule dividers only. Never as a border on interactive elements.
- **Surface Overlay** (rgba(0,0,0,0.05)): Ghost button resting state, neutral message boxes, the app header, inactive tab hover. Tonal depth without a discrete color value.

### Semantic
- **Blush Alert / Alarm Red** (#fadbd1 / #b80000): Error message boxes. Background is the blush, text is the alarm. Never swap or use independently.
- **Sage Wash / Forest Affirmation** (#e7f9d9 / #336d3f): Success message boxes. Same pairing rule applies.

### Named Rules

**The One Voice Rule.** Terracotta Signal appears on interactive primary surfaces only: Upload, Save & Pair, Copy, and checked checkbox states. If a fourth use case is proposed, reconsider the design first. Its rarity is the point.

**The Paired Signal Rule.** Semantic colors are always used in pairs: blush background with alarm text, sage background with forest text. Applying either alone violates the pairing and produces unacceptable contrast ratios.

## 3. Typography

**Primary Font:** Bricolage Grotesque (variable; opsz 12-96, wght 200-800), with system-ui sans-serif fallback

**Character:** A contemporary variable grotesque with warmth and optically-correct weights at large sizes. At 24px (the button size), it reads as confident and friendly without straining. At 32px (display), it has enough presence to anchor a 420px window without a logomark. There is no secondary typeface; Bricolage carries the entire hierarchy by weight variation.

### Hierarchy

- **Display** (600 weight, 32px, line-height 1.2): Screen titles. "Sync Code", "Pull successful!" Each result screen has one. Used nowhere else.
- **Button / Tab** (600 weight, 24px, letter-spacing 0.04em): All button labels and tab items. The large size is intentional for a utility app where misclicks are costly.
- **Body** (500 weight, 14px, line-height 1.4): Message box content. Status text. The app's only prose-length copy.
- **Label** (400 weight, 11px, letter-spacing 0.05em, all-caps): Field labels above inputs ("PASSPHRASE"). Used sparsely.
- **Small** (400 weight, 12px): Supporting text. Divider labels ("or pull from another machine"), countdown text, file list.

### Named Rules

**The Two-Size Rule.** New screens may only introduce typography at sizes already in use: 32, 24, 14, 12, or 11px. Adding a new size requires removing an existing one. The small fixed window cannot sustain a larger scale.

**The Weight Signal Rule.** Bold (700-800) appears exclusively in the countdown timer's time value, to signal urgency by weight before color. It is the only place 700+ weight is used outside hover states.

## 4. Elevation

ZynC is flat. There are no shadows anywhere in the system. Depth is communicated entirely through tonal layering: the parchment canvas at the base, Surface Overlay (5% black) applied to header areas, inactive states, and ghost buttons, deepening to 9% on hover.

This is a structural choice, not a default. The fixed-size window has no scrollable depth. Overlapping surfaces don't exist. Shadows would imply affordances that aren't present.

### Named Rules

**The No Shadow Rule.** No `box-shadow` for depth or separation. Tonal overlays only. If a component needs to feel elevated above its siblings, reconsider its placement before reaching for shadows.

**The Tonal Hierarchy.** Three tones only: Parchment (base), 5% overlay (resting interactive surface), 9% overlay (hover / pressed). A fourth tonal level is a design error.

## 5. Components

### Tab Navigation (Signature Component)

The header is the app's only navigation chrome. Two tabs: Pull and Pair. The active tab carries the Parchment background, connecting it visually to the content below (same color, no visible seam). Inactive tabs are transparent against the 5% overlay header. On hover, inactive tabs pick up the 9% overlay. This creates the impression that the active tab is "lifted" from the header without any shadow.

- **Container:** 76px tall, 5% overlay background, tabs aligned to bottom-right
- **Tab shape:** Rounded top corners only (10px radius, 0 on bottom)
- **Active:** Background #f2f0e3 (matches canvas exactly)
- **Inactive:** Transparent (inherits header bg)
- **Inactive hover:** 9% overlay background
- **Typography:** 24px, 600 weight

### Buttons

Tactile and legible. Large padding, large text. The window is small; every tap target needs to be confident.

- **Shape:** Gently curved (10px radius)
- **Primary:** Terracotta Signal (#f76f53) background, white text, 16px/24px padding
  - Hover: Terracotta Deep (#e5614a), 0.12s transition
  - Active: 0.85 opacity
  - Disabled: 0.4 opacity, cursor not-allowed
- **Secondary:** Near-Black Ink (#28262f), white text, same padding
  - Hover: Near-Black Hover (#38363f)
- **Ghost:** 5% overlay background, Deep Ink text, same padding
  - Hover: 9% overlay
- **Width:** Full-width (100%) in action areas; auto in result screens

### Inputs / Fields

Large, centered, with the code-style input (ZEN-XXXX-XXXX) functioning as a display element as much as an input field.

- **Style:** Transparent background, 2px solid Aged Ochre border (inactive), 10px radius, 16px/13px padding
- **Focus:** Border shifts to Deep Ink (2px solid #1a1914). No glow, no shadow. The border change is the signal.
- **Typography:** 24px, 600 weight, centered, uppercase for sync codes
- **Passphrase input:** Same structure, left-aligned, mixed case, 20px size
- **Placeholder:** Aged Ochre (#9b9781), 500 weight, no uppercase override

### Message Boxes

The primary system for feedback. Three semantic variants; all use the same shape. They replace inline status text entirely. A message box appears when there is something to say; it disappears when there is not.

- **Shape:** 8px radius, 10px/16px padding, full width
- **Error:** Blush Alert background (#fadbd1), Alarm Red text (#b80000). Used for blocking conditions (Zen is running, invalid code, passphrase too short).
- **Success:** Sage Wash background (#e7f9d9), Forest Affirmation text (#336d3f). Used for confirmation states (Paired, Open Zen to see changes).
- **Neutral:** 5% overlay background, Deep Ink text. Used for persistent instructions (Pair screen default state, loading status).
- **Typography:** 14px, 500 weight, centered

### Checkboxes

Custom-styled to match the accent palette. The native checkbox is hidden; the visual state is drawn by the background property.

- **Size:** 22x22px, 4px radius border
- **Unchecked:** Transparent background, 2px solid Aged Ochre
  - Hover: Border shifts to Terracotta Signal
- **Checked:** Terracotta Signal background, white checkmark SVG (13px, inline data URI), border matches fill

## 6. Do's and Don'ts

### Do:
- **Do** use Terracotta Signal (#f76f53) on primary buttons, Save & Pair, Copy, and checked checkboxes only. Those are the four legal uses. Count them.
- **Do** keep the canvas Parchment (#f2f0e3) everywhere. Screens have one background.
- **Do** use message boxes (not inline text) for all status: loading, error, success, instruction. The message box IS the feedback pattern.
- **Do** pair semantic colors: Blush Alert + Alarm Red together, Sage Wash + Forest Affirmation together. Never one without the other.
- **Do** size button text at 24px. The window is small; tap targets must be confident.
- **Do** let the active tab bleed into the content by matching the canvas color exactly. The visual seam should disappear.
- **Do** communicate input focus by shifting the border color to Deep Ink. No glow, no outline-offset, no box-shadow.

### Don't:
- **Don't** use developer tooling aesthetics: terminal dark themes, dense monospace everywhere, purple accents, dark-first design. This is the explicit anti-reference from PRODUCT.md.
- **Don't** use generic SaaS patterns: white background, blue primary buttons, card grids with icon/heading/text. If it looks like any SaaS product, the design has failed.
- **Don't** apply overwrought macOS design: translucency, vibrancy, excessive system chrome, backdrop-filter. The parchment canvas is deliberately opaque.
- **Don't** add shadows. Not for hover states, not for active states, not for z-index depth. The system is flat.
- **Don't** use `border-left` or `border-right` greater than 1px as a colored stripe on any element. Rewrite using a background tint or full border.
- **Don't** use gradient text (`background-clip: text`). Emphasis is weight and size, not gradient decoration.
- **Don't** introduce a new type size without retiring an existing one. Five sizes is already the ceiling for this window.
- **Don't** show Terracotta Signal in a decorative context (dividers, background washes, section highlights). It must mean "do this" every time it appears.
- **Don't** leave a message box visible when there is no message. Empty feedback areas erode trust. Hide them.
