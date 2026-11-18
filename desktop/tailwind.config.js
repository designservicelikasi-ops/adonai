/** AEGIS design tokens: dark institutional terminal, gold accent of the shield. */
export default {
  content: ["./index.html", "./src/**/*.{ts,tsx}"],
  theme: {
    extend: {
      colors: {
        ink: { 950: "#06090D", 900: "#0A0E13", 850: "#0E131A", 800: "#121922", 750: "#16202B", 700: "#1C2734", 600: "#273545", 500: "#3A4B5E" },
        fog: { 100: "#E8EEF4", 300: "#B6C3D0", 400: "#8C9BAB", 500: "#66768A" },
        aegis: { 300: "#F1D27A", 400: "#E2B84B", 500: "#C9A227", 600: "#9C7C14" },
        up: { 400: "#3DD598", 500: "#22B47A" },
        down: { 400: "#FF6B7D", 500: "#E5485C" },
        warn: { 400: "#F5B544" },
        info: { 400: "#5AA9FF" },
      },
      fontFamily: {
        sans: ["Inter", "SF Pro Text", "-apple-system", "Segoe UI", "Roboto", "Ubuntu", "sans-serif"],
        mono: ["JetBrains Mono", "SF Mono", "Cascadia Mono", "Consolas", "DejaVu Sans Mono", "monospace"],
      },
      boxShadow: { panel: "0 1px 0 0 rgba(255,255,255,0.03) inset, 0 8px 24px -12px rgba(0,0,0,0.6)" },
      keyframes: { pulseDot: { "0%,100%": { opacity: 1 }, "50%": { opacity: 0.35 } }, rise: { from: { opacity: 0, transform: "translateY(4px)" }, to: { opacity: 1, transform: "none" } } },
      animation: { pulseDot: "pulseDot 1.6s ease-in-out infinite", rise: "rise .25s ease-out both" },
    },
  },
  plugins: [],
};
