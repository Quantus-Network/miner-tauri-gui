/** @type {import('tailwindcss').Config} */
export default {
  content: ["./index.html", "./src/**/*.{ts,tsx,js,jsx}"],
  darkMode: "class",
  theme: {
    extend: {
      colors: {
        // Ensure our status colors are available and consistent
        status: {
          mining: "#16a34a", // green-600
          syncing: "#f59e0b", // amber-500
          starting: "#2563eb", // blue-600
          repairing: "#7c3aed", // purple-600
          error: "#dc2626", // red-600
          idle: "#6b7280", // gray-500
        },
      },
      boxShadow: {
        card: "0 1px 2px rgba(0,0,0,0.06), 0 1px 3px rgba(0,0,0,0.1)",
      },
    },
  },
  plugins: [],
  safelist: [
    // Dynamic status/background classes we assemble at runtime
    "bg-green-600",
    "bg-amber-500",
    "bg-blue-600",
    "bg-purple-600",
    "bg-red-600",
    "bg-gray-500",
    "bg-gray-600",
    "text-white",
    "text-black",
    // Utility classes we rely on frequently
    "rounded-2xl",
    "rounded-xl",
    "rounded-md",
    "rounded-full",
    "shadow",
    "font-mono",
    "font-sans",
    "text-xs",
    "text-sm",
    "text-2xl",
    "font-bold",
    "opacity-70",
    "p-2",
    "p-3",
    "px-2",
    "px-3",
    "py-1",
    "py-2",
    "h-48",
    "w-80",
    "bg-black",
    "bg-black/80",
    "bg-black/20",
    "overflow-auto",
    "leading-tight",
    "ml-2",
    "mb-2",
    "mb-4",
    "mx-auto",
    "max-w-3xl",
    "border",
    "flex",
    "gap-2",
    "gap-3",
    "items-center",
    "items-end",
    "fixed",
    "top-4",
    "right-4",
    "z-40",
  ],
};
