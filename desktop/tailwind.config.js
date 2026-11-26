/** AEGIS design tokens — PURE WHITE & GOLD, premium fintech.
 *  The historical token names are kept (ink = surfaces, fog = text) so every page follows:
 *  ink-950 page cream → ink-850 white panels → ink-700 hairline borders; fog-100 is the main
 *  ink-black text. Gold is reserved for primary actions, selections, progress and key figures;
 *  the deep gold (#9A7839) is used for text on white (contrast ≥ 4.5:1). */
export default {
  content: ["./index.html", "./src/**/*.{ts,tsx}"],
  theme: {
    extend: {
      colors: {
        ink: { 950: "#F5F3ED", 900: "#FAF9F6", 850: "#FFFFFF", 800: "#FFFFFF", 750: "#F6F3EA", 700: "#E8E3D8", 600: "#DCD4C2", 500: "#BFB39A" },
        fog: { 100: "#18202B", 300: "#3B4553", 400: "#697386", 500: "#858E9D" },
        aegis: { 200: "#F3E7C9", 300: "#9A7839", 400: "#B08A45", 500: "#C6A15B", 600: "#9A7839", metal: "#D4AF37" },
        up: { 400: "#1F7A52", 500: "#2E8B62" },
        down: { 400: "#B43A30", 500: "#C4473C" },
        warn: { 400: "#A86A12" },
        info: { 400: "#2D63A6" },
      },
      fontFamily: {
        sans: ["Inter", "SF Pro Text", "-apple-system", "Segoe UI", "Roboto", "Ubuntu", "sans-serif"],
        mono: ["JetBrains Mono", "SF Mono", "Cascadia Mono", "Consolas", "DejaVu Sans Mono", "monospace"],
      },
      boxShadow: {
        panel: "0 1px 2px rgba(24,32,43,.04), 0 8px 24px -16px rgba(24,32,43,.18)",
        gold: "0 0 0 1px rgba(198,161,91,.45), 0 6px 18px -10px rgba(154,120,57,.45)",
      },
      keyframes: { pulseDot: { "0%,100%": { opacity: 1 }, "50%": { opacity: 0.35 } }, rise: { from: { opacity: 0, transform: "translateY(4px)" }, to: { opacity: 1, transform: "none" } } },
      animation: { pulseDot: "pulseDot 1.6s ease-in-out infinite", rise: "rise .25s ease-out both" },
    },
  },
  plugins: [],
};
