import { heroui } from "@heroui/react";

/** @type {import('tailwindcss').Config} */
export default {
  content: [
    "./index.html",
    "./src/**/*.{js,ts,jsx,tsx}",
    "./node_modules/@heroui/theme/dist/**/*.{js,ts,jsx,tsx}",
  ],
  darkMode: "class",
  theme: {
    extend: {
      fontFamily: {
        // Whole-UI monospace, matching bVisor.
        sans: [
          "'Roboto Mono Variable'",
          "ui-monospace",
          "SFMono-Regular",
          "'SF Mono'",
          "'JetBrains Mono'",
          "'Cascadia Code'",
          "monospace",
        ],
        mono: [
          "'Roboto Mono Variable'",
          "ui-monospace",
          "SFMono-Regular",
          "monospace",
        ],
      },
      keyframes: {
        // The sweep behind the loading skeletons. Only the highlight moves,
        // so the placeholder itself keeps whatever colour the active theme
        // gives it.
        shimmer: {
          "100%": { transform: "translateX(100%)" },
        },
      },
      animation: {
        shimmer: "shimmer 1.6s ease-in-out infinite",
      },
      colors: {
        // Synthwave accents not covered by HeroUI semantic slots.
        neon: {
          purple: "#B026FF",
          cyan: "#05D9E8",
          magenta: "#FF2A6D",
          green: "#05FFA1",
          yellow: "#FFD319",
        },
      },
    },
  },
  // Dark surfaces follow the "Night Rider" palette (deep indigo: editor
  // #1E1C3F, sidebar #1A1837, status bar #171530, tabs #222246) with flat
  // neon accents — same theme as bVisor.
  plugins: [
    heroui({
      defaultTheme: "dark",
      themes: {
        dark: {
          colors: {
            background: "#1E1C3F",
            foreground: "#F5F5FF",
            focus: "#FF2A6D",
            content1: "#1A1837",
            content2: "#27264D",
            content3: "#32305A",
            content4: "#403D6E",
            divider: "#32305A",
            default: {
              100: "#222246",
              200: "#2C2A52",
              300: "#3A3766",
              400: "#5B5890",
              500: "#8B87B3",
              600: "#B6B3D1",
              foreground: "#F5F5FF",
              DEFAULT: "#2C2A52",
            },
            primary: {
              100: "#3D0A20",
              200: "#66112F",
              300: "#99183F",
              400: "#E02460",
              500: "#FF2A6D",
              600: "#FF5C8D",
              foreground: "#FFFFFF",
              DEFAULT: "#FF2A6D",
            },
            secondary: {
              100: "#03323A",
              200: "#054C58",
              300: "#067687",
              400: "#05B4C6",
              500: "#05D9E8",
              600: "#4EE7F2",
              foreground: "#0D0221",
              DEFAULT: "#05D9E8",
            },
            success: {
              500: "#05FFA1",
              foreground: "#0D0221",
              DEFAULT: "#05FFA1",
            },
            warning: {
              500: "#FFD319",
              foreground: "#0D0221",
              DEFAULT: "#FFD319",
            },
            danger: {
              500: "#FF3864",
              foreground: "#FFFFFF",
              DEFAULT: "#FF3864",
            },
          },
        },
        light: {
          colors: {
            focus: "#E11D5F",
            primary: {
              500: "#E11D5F",
              foreground: "#FFFFFF",
              DEFAULT: "#E11D5F",
            },
            secondary: {
              500: "#0891B2",
              foreground: "#FFFFFF",
              DEFAULT: "#0891B2",
            },
          },
        },
      },
    }),
  ],
};
